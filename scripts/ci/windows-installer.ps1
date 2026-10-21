# Exercises packaging/windows/install-windows.ps1 and uninstall-windows.ps1 for real: scheduled
# tasks, firewall rules, protected state directory, Start Menu entry, and the installed programs
# talking to each other (server as SYSTEM, client as the user).
param([string]$Bin = "$PWD\target\release")
$ErrorActionPreference = 'Stop'
$fails = 0
function Check($name, $ok, $detail = '') {
    if ($ok) { Write-Host "  PASS  $name" } else { Write-Host "  FAIL  $name $detail"; $script:fails++ }
}
function Wait-For($seconds, [scriptblock]$cond) {
    for ($i = 0; $i -lt ($seconds * 2); $i++) { if (& $cond) { return $true }; Start-Sleep -Milliseconds 500 }
    return $false
}

$stage = Join-Path $env:RUNNER_TEMP 'pkg'
New-Item -ItemType Directory -Force -Path $stage | Out-Null
Copy-Item (Join-Path $Bin 'netusb-client.exe'), (Join-Path $Bin 'netusb-server.exe') $stage
Copy-Item 'packaging\windows\install-windows.ps1', 'packaging\windows\uninstall-windows.ps1' $stage

Write-Host '== install'
& (Join-Path $stage 'install-windows.ps1') -Role both
$dest = Join-Path $env:ProgramFiles 'netusb'
$data = Join-Path $env:ProgramData 'netusb'
Check 'programs copied to Program Files' ((Test-Path "$dest\netusb-client.exe") -and (Test-Path "$dest\netusb-server.exe"))
Check 'server task is registered' ($null -ne (Get-ScheduledTask -TaskName 'netusb server' -ErrorAction SilentlyContinue))
Check 'client task is registered' ($null -ne (Get-ScheduledTask -TaskName 'netusb client' -ErrorAction SilentlyContinue))
Check 'server task runs as SYSTEM' ((Get-ScheduledTask -TaskName 'netusb server').Principal.UserId -match 'SYSTEM')
Check 'firewall rule for the server port exists' ($null -ne (Get-NetFirewallRule -DisplayName 'netusb server (TCP)' -ErrorAction SilentlyContinue))
Check 'Start Menu shortcut exists' (Test-Path (Join-Path $env:ProgramData 'Microsoft\Windows\Start Menu\Programs\netusb.lnk'))

Write-Host '== server (started by the scheduled task, as SYSTEM)'
Check 'server is listening on 7600' (Wait-For 30 { Get-NetTCPConnection -LocalPort 7600 -State Listen -ErrorAction SilentlyContinue })
Check 'server created its identity in ProgramData' (Wait-For 10 { Test-Path "$data\server.pem" })
$acl = (& icacls.exe $data | Out-String)
Write-Host $acl
Check 'state directory is limited to SYSTEM and Administrators' (($acl -match 'SYSTEM') -and ($acl -match 'Administrators') -and -not ($acl -match 'Users:') -and -not ($acl -match 'Everyone'))
Get-Process netusb-server -ErrorAction SilentlyContinue | Format-Table Id, ProcessName, SI -AutoSize | Out-String | Write-Host

Write-Host '== pairing between the installed programs'
$pairOut = & "$dest\netusb-server.exe" pair | Out-String
$pin = [regex]::Match($pairOut, 'PIN:\s+(\d{4}-\d{4})').Groups[1].Value
Check 'an administrator can open pairing and sees the PIN' ($pin -ne '') $pairOut
$cli = "$dest\netusb-client.exe"
$out = & $cli pair 127.0.0.1:7600 --pin $pin 2>&1 | Out-String
Check 'the user client pairs with the SYSTEM server' ($out -match 'paired with') $out
$out = & $cli list --host 127.0.0.1:7600 2>&1 | Out-String
Check 'the paired client can list' ($out -match '127\.0\.0\.1:7600') $out

Write-Host '== client interface (started by the scheduled task, as the user)'
$urlFile = Join-Path $env:APPDATA 'netusb\ui.url'
Check 'the interface wrote its sign-in address' (Wait-For 40 { Test-Path $urlFile }) "(looked for $urlFile)"
if (Test-Path $urlFile) {
    $url = (Get-Content $urlFile -Raw).Trim()
    $jar = Join-Path $env:RUNNER_TEMP 'cookies.txt'
    $page = & curl.exe -s -L -c $jar -b $jar $url | Out-String
    Check 'the web interface serves its page after sign-in' ($page -match '<title>netusb</title>')
    $unauth = & curl.exe -s -o NUL -w '%{http_code}' ($url -replace '\?token=.*', 'api/state')
    Check 'the API refuses requests without the cookie' ($unauth -eq '401') "(got $unauth)"
}

Write-Host '== uninstall'
& (Join-Path $stage 'uninstall-windows.ps1') -Purge
Start-Sleep -Seconds 2
Check 'tasks are gone' (($null -eq (Get-ScheduledTask -TaskName 'netusb server' -ErrorAction SilentlyContinue)) -and ($null -eq (Get-ScheduledTask -TaskName 'netusb client' -ErrorAction SilentlyContinue)))
Check 'firewall rules are gone' ($null -eq (Get-NetFirewallRule -DisplayName 'netusb server (TCP)' -ErrorAction SilentlyContinue))
Check 'programs are gone' (-not (Test-Path $dest))
Check 'server stopped' (-not (Get-NetTCPConnection -LocalPort 7600 -State Listen -ErrorAction SilentlyContinue))
Check 'purge removed the saved identities' ((-not (Test-Path $data)) -and (-not (Test-Path (Join-Path $env:APPDATA 'netusb'))))

if ($fails -gt 0) { Write-Host "$fails check(s) failed"; exit 1 }
Write-Host 'windows installer: all checks passed'
