# Real attach on Windows: installs the usbip-win2 driver, then attaches netusb's virtual mouse
# through it and checks that Windows enumerates the device and that the pointer moves.
param([string]$Bin = "$PWD\target\release")
$ErrorActionPreference = 'Stop'
$srv = Join-Path $Bin 'netusb-server.exe'
$cli = Join-Path $Bin 'netusb-client.exe'
$work = Join-Path $env:RUNNER_TEMP 'nu-attach'
New-Item -ItemType Directory -Force -Path "$work\s", "$work\c" | Out-Null
$port = 7600
$fails = 0
function Check($name, $ok, $detail = '') {
    if ($ok) { Write-Host "  PASS  $name" } else { Write-Host "  FAIL  $name $detail"; $script:fails++ }
}
function Show-Usb {
    Get-PnpDevice -PresentOnly -ErrorAction SilentlyContinue |
        Where-Object { $_.InstanceId -like 'USB\VID_1209*' -or $_.InstanceId -like 'ROOT\USBIP*' -or $_.FriendlyName -like '*USBip*' -or $_.FriendlyName -like '*netusb*' } |
        Format-List Status, Class, FriendlyName, InstanceId | Out-String | Write-Host
}

Write-Host '== install usbip-win2 (pinned by checksum)'
$url = 'https://github.com/vadimgrn/usbip-win2/releases/download/v.0.9.8.1/USBip-0.9.8.1-x64.exe'
$sha = '38cad6d4432b52d5bb9409d9ad03b72fdffc4ada4cd3a48fbeca1a2752a8518a'
$inst = "$work\USBip-x64.exe"
Invoke-WebRequest -Uri $url -OutFile $inst
$got = (Get-FileHash $inst -Algorithm SHA256).Hash.ToLower()
if ($got -ne $sha) { throw "installer checksum mismatch: $got" }
$p = Start-Process -FilePath $inst -ArgumentList '/VERYSILENT', '/NORESTART', '/SUPPRESSMSGBOXES', '/SP-', "/LOG=$work\usbip-install.log" -Wait -PassThru
Write-Host "installer exit code: $($p.ExitCode)"
Get-Content "$work\usbip-install.log" -Tail 40 -ErrorAction SilentlyContinue | Write-Host
$usbip = Join-Path $env:ProgramFiles 'USBip\usbip.exe'
Check 'usbip.exe is installed' (Test-Path $usbip)
Get-ChildItem (Join-Path $env:ProgramFiles 'USBip') -ErrorAction SilentlyContinue | Format-Table Name, Length -AutoSize | Out-String | Write-Host
Write-Host '--- usbip drivers'
pnputil.exe /enum-drivers | Select-String -Context 1, 5 -Pattern 'usbip' | Out-String | Write-Host
Write-Host '--- usbip devices'
Show-Usb
if (-not (Test-Path $usbip)) { throw 'usbip-win2 did not install' }

Write-Host '== server and pairing'
$env:NETUSB_STATE_DIR = "$work\s"
$sp = Start-Process -FilePath $srv -ArgumentList 'run', '--no-mdns', '--port', $port, '--test-device', 'mouse' `
    -RedirectStandardError "$work\server.err" -RedirectStandardOutput "$work\server.out" -PassThru -NoNewWindow
$up = $false
for ($i = 0; $i -lt 40; $i++) {
    if (Get-NetTCPConnection -LocalPort $port -State Listen -ErrorAction SilentlyContinue) { $up = $true; break }
    Start-Sleep -Milliseconds 250
}
Check 'server is listening' $up
$pin = [regex]::Match((& $srv pair | Out-String), 'PIN:\s+(\d{4}-\d{4})').Groups[1].Value
$env:NETUSB_STATE_DIR = "$work\c"
Check 'paired' ((& $cli pair "127.0.0.1:$port" --pin $pin 2>&1 | Out-String) -match 'paired with')

Write-Host '== attach the virtual mouse for 40 seconds'
Add-Type @'
using System; using System.Runtime.InteropServices;
public struct NuPoint { public int X; public int Y; }
public class NuCursor { [DllImport("user32.dll")] public static extern bool GetCursorPos(out NuPoint p); }
'@
function Cursor { $p = New-Object NuPoint; [void][NuCursor]::GetCursorPos([ref]$p); return "$($p.X),$($p.Y)" }
$before = Cursor
Write-Host "cursor before: $before"
# Control: with nothing attached the pointer must be perfectly still, or movement proves nothing.
$still = @{}
for ($i = 0; $i -lt 8; $i++) { $still[(Cursor)] = $true; Start-Sleep -Milliseconds 400 }
Check 'control: the pointer is still before attach' ($still.Count -eq 1) "(saw $($still.Keys -join ' | '))"

$cp = Start-Process -FilePath $cli -ArgumentList 'use', "127.0.0.1:$port", '99-1', '--seconds', '40' `
    -RedirectStandardOutput "$work\use.out" -RedirectStandardError "$work\use.err" -PassThru -NoNewWindow
$attached = $false
for ($i = 0; $i -lt 60; $i++) {
    if ((Get-Content "$work\use.out" -Raw -ErrorAction SilentlyContinue) -match 'attached') { $attached = $true; break }
    if ($cp.HasExited) { break }
    Start-Sleep -Seconds 1
}
Write-Host '--- client output'
Get-Content "$work\use.out", "$work\use.err" -ErrorAction SilentlyContinue | Write-Host
Check 'client reports the device attached' $attached

if ($attached) {
    Start-Sleep -Seconds 6
    Write-Host '--- devices after attach'
    Show-Usb
    $dev = Get-PnpDevice -PresentOnly -ErrorAction SilentlyContinue | Where-Object { $_.InstanceId -like 'USB\VID_1209&PID_0001*' }
    Check 'Windows enumerated the virtual device (VID_1209 PID_0001)' ($null -ne $dev) 
    Check 'the device is healthy (Status OK)' (($dev | Where-Object { $_.Status -eq 'OK' }) -ne $null)
    $hid = Get-PnpDevice -PresentOnly -ErrorAction SilentlyContinue | Where-Object { $_.InstanceId -like 'HID\VID_1209&PID_0001*' }
    Check 'Windows created a HID device for it' ($null -ne $hid)
    $mouse = Get-PnpDevice -Class Mouse -PresentOnly -ErrorAction SilentlyContinue | Where-Object { $_.InstanceId -like '*VID_1209*' }
    Check 'it is recognised as a mouse' ($null -ne $mouse)

    $seen = @{}
    for ($i = 0; $i -lt 8; $i++) { $seen[(Cursor)] = $true; Start-Sleep -Milliseconds 400 }
    Write-Host "cursor positions seen: $($seen.Keys -join ' | ')"
    Check 'the pointer moved (reports flow through the whole chain)' ($seen.Count -gt 1)
}

Write-Host '== release'
$null = $cp.WaitForExit(60000)
Check 'client exited after its time limit' $cp.HasExited
Start-Sleep -Seconds 3
$gone = -not (Get-PnpDevice -PresentOnly -ErrorAction SilentlyContinue | Where-Object { $_.InstanceId -like 'USB\VID_1209&PID_0001*' })
Check 'the virtual device is gone after release' $gone
$after = @{}
for ($i = 0; $i -lt 8; $i++) { $after[(Cursor)] = $true; Start-Sleep -Milliseconds 400 }
Check 'control: the pointer is still again after release' ($after.Count -eq 1) "(saw $($after.Keys -join ' | '))"
Write-Host '--- usbip port'
& $usbip port 2>&1 | Write-Host

Stop-Process -Id $sp.Id -Force -ErrorAction SilentlyContinue
Write-Host '--- server log'
Get-Content "$work\server.err" -ErrorAction SilentlyContinue | Write-Host
if ($fails -gt 0) { Write-Host "$fails check(s) failed"; exit 1 }
Write-Host 'windows attach: all checks passed'
