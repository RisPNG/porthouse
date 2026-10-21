# Assemble dist\netusb-<version>-windows-x64.zip from a finished release build.
#   cargo build --release ; ./scripts/package-windows.ps1
param([string]$Bin = 'target\release')
$ErrorActionPreference = 'Stop'
$ver = (Select-String -Path Cargo.toml -Pattern '^version = "(.*)"').Matches[0].Groups[1].Value
$name = "netusb-$ver-windows-x64"
$out = "dist\$name"
Remove-Item -Recurse -Force $out -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $out | Out-Null
Copy-Item "$Bin\netusb-client.exe", "$Bin\netusb-server.exe" $out
Copy-Item 'packaging\windows\install-windows.ps1', 'packaging\windows\uninstall-windows.ps1', 'packaging\windows\README-WINDOWS.txt' $out
Copy-Item 'LICENSE-MIT', 'LICENSE-APACHE' $out
Push-Location $out
Get-ChildItem -File | ForEach-Object { "$((Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower())  $($_.Name)" } | Set-Content 'SHA256SUMS.txt' -Encoding ascii
Pop-Location
Compress-Archive -Path $out -DestinationPath "dist\$name.zip" -Force
Get-Item "dist\$name.zip" | Format-List Name, Length
