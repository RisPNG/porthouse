# Tests the WINDOWS server engine (nusb over WinUSB) against a real Windows USB device that needs
# no hardware:
#
#   server A (virtual loopback device) --TLS--> client --usbip-win2 driver--> Windows enumerates a
#   USB device 1209:0002 and, thanks to its Microsoft OS descriptors, binds WinUSB to it
#   server B (netusb-server.exe, native engine) shares that WinUSB device
#   fake driver --TLS--> server B: control, bulk echo, cancel, ordering
param([string]$Bin = "$PWD\target\release")
$ErrorActionPreference = 'Stop'
$srv = Join-Path $Bin 'netusb-server.exe'
$cli = Join-Path $Bin 'netusb-client.exe'
$work = Join-Path $env:RUNNER_TEMP 'nu-chain'
New-Item -ItemType Directory -Force -Path "$work\a", "$work\b", "$work\c1", "$work\c2", "$work\out" | Out-Null
$fails = 0
function Check($name, $ok, $detail = '') {
    if ($ok) { Write-Host "  PASS  $name" } else { Write-Host "  FAIL  $name $detail"; $script:fails++ }
}
function Wait-For($seconds, [scriptblock]$cond) {
    for ($i = 0; $i -lt ($seconds * 2); $i++) { if (& $cond) { return $true }; Start-Sleep -Milliseconds 500 }
    return $false
}
function Start-NetusbServer($stateDir, $port, $extra) {
    $env:NETUSB_STATE_DIR = $stateDir
    $args = @('run', '--no-mdns', '--port', $port) + $extra
    Start-Process -FilePath $srv -ArgumentList $args -RedirectStandardError "$stateDir\server.err" -RedirectStandardOutput "$stateDir\server.out" -PassThru -NoNewWindow
}
function Get-Pin($stateDir) {
    $env:NETUSB_STATE_DIR = $stateDir
    [regex]::Match((& $srv pair | Out-String), 'PIN:\s+(\d{4}-\d{4})').Groups[1].Value
}

Write-Host '== install usbip-win2 (pinned by checksum)'
$url = 'https://github.com/vadimgrn/usbip-win2/releases/download/v.0.9.8.1/USBip-0.9.8.1-x64.exe'
$sha = '38cad6d4432b52d5bb9409d9ad03b72fdffc4ada4cd3a48fbeca1a2752a8518a'
$inst = "$work\USBip-x64.exe"
Invoke-WebRequest -Uri $url -OutFile $inst
if ((Get-FileHash $inst -Algorithm SHA256).Hash.ToLower() -ne $sha) { throw 'installer checksum mismatch' }
$p = Start-Process -FilePath $inst -ArgumentList '/VERYSILENT', '/NORESTART', '/SUPPRESSMSGBOXES', '/SP-', "/LOG=$work\usbip-install.log" -Wait -PassThru
Check 'usbip-win2 installed' ($p.ExitCode -eq 0 -and (Test-Path "$env:ProgramFiles\USBip\usbip.exe")) "exit=$($p.ExitCode)"

Write-Host '== server A shares the virtual loopback device; client 1 plugs it into Windows'
$env:NETUSB_TRACE = '1'
$sa = Start-NetusbServer "$work\a" 7600 @('--test-device', 'loopback')
Remove-Item Env:\NETUSB_TRACE
Check 'server A is listening' (Wait-For 20 { Get-NetTCPConnection -LocalPort 7600 -State Listen -ErrorAction SilentlyContinue })
$pin = Get-Pin "$work\a"
$env:NETUSB_STATE_DIR = "$work\c1"
Check 'client 1 paired with A' ((& $cli pair 127.0.0.1:7600 --pin $pin 2>&1 | Out-String) -match 'paired with')
$c1 = Start-Process -FilePath $cli -ArgumentList 'use', '127.0.0.1:7600', '99-2', '--seconds', '240' `
    -RedirectStandardOutput "$work\c1.out" -RedirectStandardError "$work\c1.err" -PassThru -NoNewWindow
Check 'client 1 attached the virtual device' (Wait-For 60 { (Get-Content "$work\c1.out" -Raw -ErrorAction SilentlyContinue) -match 'attached' })

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
$sb = Start-NetusbServer "$work\b" 7601 @()
Remove-Item Env:\NETUSB_TRACE
Check 'server B is listening' (Wait-For 20 { Get-NetTCPConnection -LocalPort 7601 -State Listen -ErrorAction SilentlyContinue })
$pin = Get-Pin "$work\b"
$env:NETUSB_STATE_DIR = "$work\c2"
Check 'client 2 paired with B' ((& $cli pair 127.0.0.1:7601 --pin $pin 2>&1 | Out-String) -match 'paired with')
$list = & $cli list --host 127.0.0.1:7601 2>&1 | Out-String
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
$c2 = Start-Process -FilePath $cli -ArgumentList 'use', '127.0.0.1:7601', $busid, '--importer', $cmd, '--seconds', '150' `
    -RedirectStandardOutput "$work\c2.out" -RedirectStandardError "$work\c2.err" -PassThru -NoNewWindow
Check 'the exercise finished' (Wait-For 60 { Test-Path "$work\out\done" })
$results = Get-Content "$work\out\results.txt" -ErrorAction SilentlyContinue
$results | ForEach-Object { Write-Host "   $_" }
$nPass = ($results | Where-Object { $_ -like 'PASS*' }).Count
$nFail = ($results | Where-Object { $_ -like 'FAIL*' }).Count
Check 'all 14 engine checks passed' (($nPass -eq 14) -and ($nFail -eq 0)) "($nPass passed, $nFail failed)"
Stop-Process -Id $c2.Id -Force -ErrorAction SilentlyContinue   # the exercise is over; do not wait out the timer

Write-Host '--- client 1'; Get-Content "$work\c1.out", "$work\c1.err" -ErrorAction SilentlyContinue | Write-Host
Write-Host '--- client 2'; Get-Content "$work\c2.out", "$work\c2.err" -ErrorAction SilentlyContinue | Write-Host
Write-Host '--- server A (last 40 lines, with URB trace)'; Get-Content "$work\a\server.err" -Tail 40 -ErrorAction SilentlyContinue | Write-Host
Write-Host '--- server B (last 60 lines, with URB trace)'; Get-Content "$work\b\server.err" -Tail 60 -ErrorAction SilentlyContinue | Write-Host
Stop-Process -Id $sa.Id, $sb.Id -Force -ErrorAction SilentlyContinue
if ($fails -gt 0) { Write-Host "$fails check(s) failed"; exit 1 }
Write-Host 'windows chain: all checks passed'
