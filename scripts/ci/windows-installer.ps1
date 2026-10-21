# Runs the real installer on a Windows machine and uses what it installed: services, firewall rules,
# protected key directory, driver, shortcuts, the program against the live services, and removal.
param([Parameter(Mandatory = $true)][string]$Setup)
$ErrorActionPreference = 'Stop'
$Bin = Join-Path $env:ProgramFiles 'netusb'
. "$PSScriptRoot\windows-lib.ps1"
$work = Join-Path $env:RUNNER_TEMP 'nu-installer'
New-Item -ItemType Directory -Force -Path $work | Out-Null
$data = Join-Path $env:ProgramData 'netusb'
Remove-Item Env:\NETUSB_STATE_DIR, Env:\NETUSB_IPC_DIR -ErrorAction SilentlyContinue

Write-Host '== install'
$p = Start-Process -FilePath $Setup -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/TASKS=autostart', "/LOG=$work\setup.log" -Wait -PassThru
Write-Host "installer exit code: $($p.ExitCode)"
Get-Content "$work\setup.log" -Tail 40 -ErrorAction SilentlyContinue | Write-Host
Check 'the installer finished successfully' ($p.ExitCode -eq 0)
foreach ($f in 'netusb.exe', 'netusb-server.exe', 'netusb-clientd.exe', 'netusb-client.exe', 'Qt6Core.dll', 'Qt6Quick.dll', 'vcruntime140.dll') {
    Check "$f is installed" (Test-Path (Join-Path $Bin $f))
}
Check 'the QML runtime is installed' (Test-Path (Join-Path $Bin 'qml\QtQuick\Controls\Basic'))
Check 'the Windows platform plugin is installed' (Test-Path (Join-Path $Bin 'platforms\qwindows.dll'))

Write-Host '== services'
foreach ($name in 'netusb-server', 'netusb-client') {
    $svc = Get-Service $name -ErrorAction SilentlyContinue
    Check "$name service exists and is running" (($null -ne $svc) -and ($svc.Status -eq 'Running')) "($($svc.Status))"
    Check "$name starts automatically" ($svc.StartType -eq 'Automatic')
    $failure = & sc.exe qfailure $name | Out-String
    Check "$name restarts after a failure" ($failure -match 'RESTART') $failure
}
Check 'the server is listening on 7600' (Wait-For 30 { Get-NetTCPConnection -LocalPort 7600 -State Listen -ErrorAction SilentlyContinue })
Check 'firewall rule for the server port exists' ($null -ne (Get-NetFirewallRule -DisplayName 'netusb server (TCP)' -ErrorAction SilentlyContinue))
Check 'firewall rule for discovery exists' ($null -ne (Get-NetFirewallRule -DisplayName 'netusb server (mDNS)' -ErrorAction SilentlyContinue))
$acl = (& icacls.exe $data | Out-String)
Write-Host $acl
Check 'the key directory is limited to SYSTEM and Administrators' (($acl -match 'SYSTEM') -and ($acl -match 'Administrators') -and -not ($acl -match 'Users:') -and -not ($acl -match 'Everyone'))
Check 'the server created its identity' (Wait-For 20 { Test-Path "$data\server.pem" })
Check 'the client service created its identity' (Wait-For 20 { Test-Path "$data\client.pem" })

Write-Host '== the driver'
Check 'usbip-win2 is installed (usbip.exe present)' (Test-Path (Join-Path $env:ProgramFiles 'USBip\usbip.exe'))
$hc = Get-PnpDevice -PresentOnly -ErrorAction SilentlyContinue | Where-Object { $_.FriendlyName -like '*USBip*' -and $_.Status -eq 'OK' }
Check 'the virtual host controller is present' ($null -ne $hc)

Write-Host '== shortcuts and start-up'
Check 'Start Menu entry exists' (Test-Path (Join-Path $env:ProgramData 'Microsoft\Windows\Start Menu\Programs\netusb\netusb.lnk'))
$run = (Get-ItemProperty 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Run' -Name netusb -ErrorAction SilentlyContinue).netusb
Check 'the program starts with Windows, minimised' ($run -match 'netusb\.exe" --minimized') "($run)"

Write-Host '== pairing between the installed services, through their control pipes'
$srv = Join-Path $Bin 'netusb-server.exe'
$cli = Join-Path $Bin 'netusb-client.exe'
$pairOut = & $srv pair | Out-String
$pin = [regex]::Match($pairOut, 'PIN:\s+(\d{4}-\d{4})').Groups[1].Value
Check 'the server service opens pairing and shows a PIN' ($pin -ne '') $pairOut
$out = & $cli pair 127.0.0.1 --pin $pin 2>&1 | Out-String
Check 'the client service pairs with the server service' ($out -match 'paired with') $out
$out = & $cli list 2>&1 | Out-String
Check 'the paired server is listed' ($out -match '127\.0\.0\.1:7600\)\s+paired') $out

Write-Host '== the program against the live services'
New-Item -ItemType Directory -Force -Path "$work\shots" | Out-Null
Copy-Item (Join-Path $env:QT_ROOT_DIR 'plugins\platforms\qoffscreen.dll') (Join-Path $Bin 'platforms') -ErrorAction Stop
$env:QT_QPA_PLATFORM = 'offscreen'
foreach ($tab in 'use', 'share', 'activity') {
    & (Join-Path $Bin 'netusb.exe') --screenshot "$work\shots\$tab.png" --tab $tab
    Check "the $tab tab was drawn" ((Test-Path "$work\shots\$tab.png") -and ((Get-Item "$work\shots\$tab.png").Length -gt 5000))
}
Remove-Item Env:\QT_QPA_PLATFORM
Remove-Item (Join-Path $Bin 'platforms\qoffscreen.dll')

Write-Host '== remove'
$unins = Get-ChildItem $Bin -Filter 'unins*.exe' | Select-Object -First 1
$p = Start-Process -FilePath $unins.FullName -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/PURGE=1', "/LOG=$work\uninstall.log" -Wait -PassThru
Write-Host "uninstaller exit code: $($p.ExitCode)"
Check 'the uninstaller finished successfully' ($p.ExitCode -eq 0)
Start-Sleep -Seconds 3
Check 'the services are gone' (($null -eq (Get-Service netusb-server -ErrorAction SilentlyContinue)) -and ($null -eq (Get-Service netusb-client -ErrorAction SilentlyContinue)))
Check 'the firewall rules are gone' (($null -eq (Get-NetFirewallRule -DisplayName 'netusb server (TCP)' -ErrorAction SilentlyContinue)) -and ($null -eq (Get-NetFirewallRule -DisplayName 'netusb server (mDNS)' -ErrorAction SilentlyContinue)))
Check 'the program files are gone' (-not (Test-Path (Join-Path $Bin 'netusb.exe')))
Check 'nothing listens on 7600' (-not (Get-NetTCPConnection -LocalPort 7600 -State Listen -ErrorAction SilentlyContinue))
Check 'the start-up entry is gone' ($null -eq (Get-ItemProperty 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Run' -Name netusb -ErrorAction SilentlyContinue))
Check 'purging removed the keys and pairings' (-not (Test-Path $data))

if ($fails -gt 0) { Write-Host "$fails check(s) failed"; exit 1 }
Write-Host 'windows installer: all checks passed'
