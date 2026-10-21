# Real attach on Windows: installs the usbip-win2 driver, then attaches porthouse's virtual mouse
# through it and checks that Windows enumerates the device and that the pointer moves.
param([string]$Bin = "$PWD\target\release")
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\windows-lib.ps1"
$work = Join-Path $env:RUNNER_TEMP 'nu-attach'
New-Item -ItemType Directory -Force -Path $work | Out-Null
$port = 7600
function Show-Usb {
    Get-PnpDevice -PresentOnly -ErrorAction SilentlyContinue |
        Where-Object { $_.InstanceId -like 'USB\VID_1209*' -or $_.InstanceId -like 'ROOT\USBIP*' -or $_.FriendlyName -like '*USBip*' -or $_.FriendlyName -like '*porthouse*' } |
        Format-List Status, Class, FriendlyName, InstanceId | Out-String | Write-Host
}

Write-Host '== install usbip-win2 (pinned by checksum)'
Install-UsbipWin2 $work
$usbip = Join-Path $env:ProgramFiles 'USBip\usbip.exe'
Write-Host '--- usbip drivers'
pnputil.exe /enum-drivers | Select-String -Context 1, 5 -Pattern 'usbip' | Out-String | Write-Host
Write-Host '--- usbip devices'
Show-Usb
if (-not (Test-Path $usbip)) { throw 'usbip-win2 did not install' }

Write-Host '== services and pairing'
$sp = Start-Nu $srv @('run', '--no-mdns', '--port', $port, '--test-device', 'mouse') "$work\s" 'server'
$cp = Start-Nu $clid @('run') "$work\c" 'clientd'
Check 'server answers' (Wait-ServerReady "$work\s")
Check 'client service answers' (Wait-ClientReady "$work\c")
$pin = Get-Pin "$work\s"
Use-Instance "$work\c"
Check 'paired' ((& $cli pair "127.0.0.1:$port" --pin $pin 2>&1 | Out-String) -match 'paired with')

Write-Host '== attach the virtual mouse'
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

$useOut = & $cli use "127.0.0.1:$port" 99-1 --seconds 300 2>&1 | Out-String
Write-Host '--- client output'
Write-Host $useOut
$attached = $useOut -match 'attached'
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
Use-Instance "$work\c"
& $cli release "127.0.0.1:$port" 99-1 | Out-Null
$gone = Wait-For 30 { -not (Get-PnpDevice -PresentOnly -ErrorAction SilentlyContinue | Where-Object { $_.InstanceId -like 'USB\VID_1209&PID_0001*' }) }
Check 'the virtual device is gone after release' $gone
$after = @{}
for ($i = 0; $i -lt 8; $i++) { $after[(Cursor)] = $true; Start-Sleep -Milliseconds 400 }
Check 'control: the pointer is still again after release' ($after.Count -eq 1) "(saw $($after.Keys -join ' | '))"
Write-Host '--- usbip port'
& $usbip port 2>&1 | Write-Host

Stop-Process -Id $sp.Id, $cp.Id -Force -ErrorAction SilentlyContinue
Write-Host '--- server log'
Get-Content "$work\s\server.err" -ErrorAction SilentlyContinue | Write-Host
Write-Host '--- client service log'
Get-Content "$work\c\clientd.err" -ErrorAction SilentlyContinue | Write-Host
if ($fails -gt 0) { Write-Host "$fails check(s) failed"; exit 1 }
Write-Host 'windows attach: all checks passed'
