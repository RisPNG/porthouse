# Helpers shared by the Windows test scripts. Dot-source it from a script that has set $Bin.
#
# Every service in a test lives in its own directory: its state is there and its control pipe is
# named after it, so several can run side by side without touching the installed ones.

$script:fails = 0
$srv = Join-Path $Bin 'netusb-server.exe'
$cli = Join-Path $Bin 'netusb-client.exe'
$clid = Join-Path $Bin 'netusb-clientd.exe'

function Check($name, $ok, $detail = '') {
    if ($ok) { Write-Host "  PASS  $name" } else { Write-Host "  FAIL  $name $detail"; $script:fails++ }
}

function Wait-For($seconds, [scriptblock]$cond) {
    for ($i = 0; $i -lt ($seconds * 2); $i++) { if (& $cond) { return $true }; Start-Sleep -Milliseconds 500 }
    return $false
}

# Point the following netusb commands at the instance whose files are in $dir.
function Use-Instance($dir) {
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    $env:NETUSB_STATE_DIR = $dir
    $env:NETUSB_IPC_DIR = $dir
}

function Start-Nu($exe, $arguments, $dir, $logName) {
    Use-Instance $dir
    Start-Process -FilePath $exe -ArgumentList $arguments -RedirectStandardError "$dir\$logName.err" `
        -RedirectStandardOutput "$dir\$logName.out" -PassThru -NoNewWindow
}

function Wait-ServerReady($dir) {
    Use-Instance $dir
    Wait-For 30 { & $srv clients 2>&1 | Out-Null; $LASTEXITCODE -eq 0 }
}

function Wait-ClientReady($dir) {
    Use-Instance $dir
    Wait-For 30 { & $cli servers 2>&1 | Out-Null; $LASTEXITCODE -eq 0 }
}

function Get-Pin($dir) {
    Use-Instance $dir
    [regex]::Match((& $srv pair | Out-String), 'PIN:\s+(\d{4}-\d{4})').Groups[1].Value
}

function Install-UsbipWin2($work) {
    $url = 'https://github.com/vadimgrn/usbip-win2/releases/download/v.0.9.8.1/USBip-0.9.8.1-x64.exe'
    $sha = '38cad6d4432b52d5bb9409d9ad03b72fdffc4ada4cd3a48fbeca1a2752a8518a'
    $inst = "$work\USBip-x64.exe"
    Invoke-WebRequest -Uri $url -OutFile $inst
    if ((Get-FileHash $inst -Algorithm SHA256).Hash.ToLower() -ne $sha) { throw 'installer checksum mismatch' }
    $p = Start-Process -FilePath $inst -ArgumentList '/VERYSILENT', '/NORESTART', '/SUPPRESSMSGBOXES', '/SP-', "/LOG=$work\usbip-install.log" -Wait -PassThru
    # The third-party installer has been seen to crash on exit (0xC0000374) after finishing its work, so
    # judge the result by what matters: the importer exists and the virtual host controller is present.
    if ($p.ExitCode -ne 0) { Write-Host "NOTE: the installer exited with $($p.ExitCode); checking the result instead" }
    $hc = Get-PnpDevice -PresentOnly -ErrorAction SilentlyContinue | Where-Object { $_.FriendlyName -like '*USBip*' -and $_.Status -eq 'OK' }
    Check 'usbip-win2 is installed (usbip.exe and its virtual host controller are present)' ((Test-Path "$env:ProgramFiles\USBip\usbip.exe") -and ($null -ne $hc))
}
