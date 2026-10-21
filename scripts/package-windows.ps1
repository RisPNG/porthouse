# Builds the Windows installer. Needs the release build of the Rust programs, the built program
# from gui/ (see packaging notes in docs/building.md), windeployqt on the PATH, and Inno Setup.
#
#   ./scripts/package-windows.ps1 -GuiBuild build\gui
param(
    [string]$Bin = "$PWD\target\release",
    [Parameter(Mandatory = $true)][string]$GuiBuild
)
$ErrorActionPreference = 'Stop'

$version = [regex]::Match((Get-Content Cargo.toml -Raw), '(?m)^version = "([0-9.]+)"').Groups[1].Value
$stage = "$PWD\dist\stage"
Remove-Item -Recurse -Force $stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path "$stage\gui" | Out-Null

foreach ($f in 'porthouse-server.exe', 'porthouse-clientd.exe', 'porthouse-client.exe') { Copy-Item (Join-Path $Bin $f) $stage }
Copy-Item 'LICENSE', 'THIRD-PARTY-LICENSES.md', 'gui\icons\porthouse.ico' $stage
Copy-Item -Recurse 'licenses' $stage

# The program with the Qt files it needs. Only the Basic style is used, so only that is shipped.
Copy-Item (Join-Path $GuiBuild 'porthouse.exe') "$stage\gui"
& windeployqt.exe --release --no-translations --no-system-d3d-compiler --no-opengl-sw --qmldir gui\qml "$stage\gui\porthouse.exe"
if ($LASTEXITCODE -ne 0) { throw 'windeployqt failed' }
$runtime = Join-Path $env:VCToolsRedistDir 'x64\Microsoft.VC143.CRT'
if (-not (Test-Path $runtime)) { throw "the Visual C++ runtime was not found at $runtime" }
Copy-Item "$runtime\*.dll" "$stage\gui"

# The driver installer from usbip-win2 (BSD-2-Clause), pinned by checksum.
$usbip = "$stage\USBip-0.9.8.1-x64.exe"
Invoke-WebRequest -Uri 'https://github.com/vadimgrn/usbip-win2/releases/download/v.0.9.8.1/USBip-0.9.8.1-x64.exe' -OutFile $usbip
$sha = '38cad6d4432b52d5bb9409d9ad03b72fdffc4ada4cd3a48fbeca1a2752a8518a'
if ((Get-FileHash $usbip -Algorithm SHA256).Hash.ToLower() -ne $sha) { throw 'usbip-win2 installer checksum mismatch' }

$iscc = (Get-Command ISCC.exe -ErrorAction SilentlyContinue).Source
if (-not $iscc) { $iscc = "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe" }
& $iscc "/DVersion=$version" "/DStage=$stage" packaging\windows\porthouse.iss
if ($LASTEXITCODE -ne 0) { throw 'Inno Setup failed' }
Get-ChildItem dist\*-setup.exe | Format-Table Name, Length -AutoSize | Out-String | Write-Host
