# Tests the WINDOWS server engine (nusb over WinUSB) against a real Windows USB device that needs
# no hardware:
#
#   server A (virtual loopback device) --TLS--> client --usbip-win2 driver--> Windows enumerates a
#   USB device 1209:0002 and, thanks to its Microsoft OS descriptors, binds WinUSB to it
#   server B (netusb-server.exe, native engine) shares that WinUSB device
#   fake driver --TLS--> server B: control, bulk echo, cancel, ordering
param([string]$Bin = "$PWD\target\release")
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\windows-lib.ps1"
$work = Join-Path $env:RUNNER_TEMP 'nu-chain'
New-Item -ItemType Directory -Force -Path "$work\out" | Out-Null

Write-Host '== install usbip-win2 (pinned by checksum)'
Install-UsbipWin2 $work

Write-Host '== server A shares the virtual loopback device; client 1 plugs it into Windows'
$env:NETUSB_TRACE = '1'
$sa = Start-Nu $srv @('run', '--no-mdns', '--port', 7600, '--test-device', 'loopback') "$work\a" 'server'
Remove-Item Env:\NETUSB_TRACE
$c1d = Start-Nu $clid @('run') "$work\c1" 'clientd'
Check 'server A answers' (Wait-ServerReady "$work\a")
Check 'client service 1 answers' (Wait-ClientReady "$work\c1")
Check 'server A is listening' (Wait-For 20 { Get-NetTCPConnection -LocalPort 7600 -State Listen -ErrorAction SilentlyContinue })
$pin = Get-Pin "$work\a"
Use-Instance "$work\c1"
Check 'client 1 paired with A' ((& $cli pair 127.0.0.1:7600 --pin $pin 2>&1 | Out-String) -match 'paired with')
$c1Out = & $cli use 127.0.0.1:7600 99-2 --seconds 600 2>&1 | Out-String
Set-Content "$work\c1.out" $c1Out
Check 'client 1 attached the virtual device' ($c1Out -match 'attached')

$dev = $null
$found = Wait-For 60 { $script:dev = Get-PnpDevice -PresentOnly -ErrorAction SilentlyContinue | Where-Object { $_.InstanceId -like 'USB\VID_1209&PID_0002*' }; $null -ne $script:dev }
Check 'Windows enumerated the loopback device (VID_1209 PID_0002)' $found
if ($found) {
    $id = $dev[0].InstanceId
    $bound = Wait-For 60 {
        $svc = (Get-PnpDeviceProperty -InstanceId $id -KeyName 'DEVPKEY_Device_Service' -ErrorAction SilentlyContinue).Data
        $svc -eq 'WINUSB'
    }
    $svc = (Get-PnpDeviceProperty -InstanceId $id -KeyName 'DEVPKEY_Device_Service' -ErrorAction SilentlyContinue).Data
    Check 'Windows bound WinUSB to it automatically (from the Microsoft OS descriptors)' $bound "(service is '$svc')"
    Get-PnpDevice -InstanceId $id | Format-List Status, Class, FriendlyName, InstanceId | Out-String | Write-Host
    if (-not $bound) {
        Write-Host '--- problem details'
        (Get-PnpDeviceProperty -InstanceId $id -KeyName 'DEVPKEY_Device_ProblemCode' -ErrorAction SilentlyContinue).Data
        pnputil.exe /enum-devices /instanceid $id /drivers | Write-Host
    }
}

Write-Host '== server B (native Windows engine) shares that WinUSB device'
$env:NETUSB_TRACE = '1'   # log every URB the Windows engine handles
$sb = Start-Nu $srv @('run', '--no-mdns', '--port', 7601) "$work\b" 'server'
Remove-Item Env:\NETUSB_TRACE
Check 'server B answers' (Wait-ServerReady "$work\b")
Check 'server B is listening' (Wait-For 20 { Get-NetTCPConnection -LocalPort 7601 -State Listen -ErrorAction SilentlyContinue })
$pin = Get-Pin "$work\b"
Write-Host $list
$busid = [regex]::Match($list, '(\S+)\s+1209:0002').Groups[1].Value
Check 'server B lists the WinUSB device' ($busid -ne '') $list
Check 'the server reports it with its real name' ($list -match 'netusb loopback device')

Write-Host '== fake driver exercises server B''s Windows engine'
$py = (Get-Command python -ErrorAction Stop).Source
Write-Host "python: $py"
$cmd = "$work\fake-usbip.cmd"
Set-Content -Path $cmd -Value "@echo off`r`n`"$py`" `"$PWD\scripts\ci\fake-usbip.py`" %*" -Encoding ascii
$env:FAKE_OUT = "$work\out"
$c2d = Start-Nu $clid @('run', '--importer', $cmd) "$work\c2" 'clientd'
Check 'client service 2 answers' (Wait-ClientReady "$work\c2")
Use-Instance "$work\c2"
Check 'client 2 paired with B' ((& $cli pair 127.0.0.1:7601 --pin $pin 2>&1 | Out-String) -match 'paired with')
$list = & $cli list 2>&1 | Out-String
Write-Host $list
$busid = [regex]::Match($list, '(\S+)\s+1209:0002').Groups[1].Value
Check 'server B lists the WinUSB device' ($busid -ne '') $list
Check 'the server reports it with its real name' ($list -match 'netusb loopback device')
$c2Out = & $cli use 127.0.0.1:7601 $busid --seconds 150 2>&1 | Out-String
Set-Content "$work\c2.out" $c2Out
Check 'the exercise finished' (Wait-For 60 { Test-Path "$work\out\done" })
$results = Get-Content "$work\out\results.txt" -ErrorAction SilentlyContinue
$results | ForEach-Object { Write-Host "   $_" }
$nPass = ($results | Where-Object { $_ -like 'PASS*' }).Count
$nFail = ($results | Where-Object { $_ -like 'FAIL*' }).Count
Check 'all 14 engine checks passed' (($nPass -eq 14) -and ($nFail -eq 0)) "($nPass passed, $nFail failed)"

Write-Host '--- client 1'; Get-Content "$work\c1.out", "$work\c1\clientd.err" -ErrorAction SilentlyContinue | Write-Host
Write-Host '--- client 2'; Get-Content "$work\c2.out", "$work\c2\clientd.err" -ErrorAction SilentlyContinue | Write-Host
Write-Host '--- server A (last 40 lines, with URB trace)'; Get-Content "$work\a\server.err" -Tail 40 -ErrorAction SilentlyContinue | Write-Host
Write-Host '--- server B (last 60 lines, with URB trace)'; Get-Content "$work\b\server.err" -Tail 60 -ErrorAction SilentlyContinue | Write-Host
Stop-Process -Id $sa.Id, $sb.Id, $c1d.Id, $c2d.Id -Force -ErrorAction SilentlyContinue
if ($fails -gt 0) { Write-Host "$fails check(s) failed"; exit 1 }
Write-Host 'windows chain: all checks passed'
