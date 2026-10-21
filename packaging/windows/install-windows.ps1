<#
.SYNOPSIS
    Install netusb on Windows (client, server, or both). Run from an elevated PowerShell:

        powershell -ExecutionPolicy Bypass -File .\install-windows.ps1 -Role client

.NOTES
    UNTESTED: this script was written without access to a Windows machine. Read it before running.
    It copies two programs to Program Files, registers scheduled tasks, and (for the server) adds
    firewall rules. It does not install the USB/IP driver; see README-WINDOWS.txt.
#>
#Requires -RunAsAdministrator
param(
    [ValidateSet('client', 'server', 'both')]
    [string]$Role = 'client'
)
$ErrorActionPreference = 'Stop'

$src  = Split-Path -Parent $MyInvocation.MyCommand.Path
$dest = Join-Path $env:ProgramFiles 'netusb'
New-Item -ItemType Directory -Force -Path $dest | Out-Null
foreach ($f in 'netusb-client.exe', 'netusb-server.exe') {
    Copy-Item -Force -Path (Join-Path $src $f) -Destination $dest
}
Write-Host "Installed programs to $dest"

if ($Role -eq 'server' -or $Role -eq 'both') {
    $exe  = Join-Path $dest 'netusb-server.exe'
    $data = Join-Path $env:ProgramData 'netusb'
    New-Item -ItemType Directory -Force -Path $data | Out-Null
    # The server's private key and pairing PIN live here: only SYSTEM and Administrators may read it.
    & icacls.exe $data /inheritance:r /grant:r '*S-1-5-18:(OI)(CI)F' '*S-1-5-32-544:(OI)(CI)F' | Out-Null

    $action    = New-ScheduledTaskAction -Execute $exe -Argument 'run'
    $trigger   = New-ScheduledTaskTrigger -AtStartup
    $principal = New-ScheduledTaskPrincipal -UserId 'SYSTEM' -LogonType ServiceAccount -RunLevel Highest
    $settings  = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries `
                     -RestartCount 5 -RestartInterval (New-TimeSpan -Minutes 1) -ExecutionTimeLimit ([TimeSpan]::Zero)
    Register-ScheduledTask -TaskName 'netusb server' -Action $action -Trigger $trigger `
        -Principal $principal -Settings $settings -Force | Out-Null

    foreach ($name in 'netusb server (TCP)', 'netusb server (mDNS)') {
        Remove-NetFirewallRule -DisplayName $name -ErrorAction SilentlyContinue
    }
    New-NetFirewallRule -DisplayName 'netusb server (TCP)' -Direction Inbound -Protocol TCP -LocalPort 7600 `
        -Action Allow -Profile Domain, Private | Out-Null
    New-NetFirewallRule -DisplayName 'netusb server (mDNS)' -Direction Inbound -Protocol UDP -LocalPort 5353 `
        -Program $exe -Action Allow -Profile Domain, Private | Out-Null

    Start-ScheduledTask -TaskName 'netusb server'
    Write-Host ''
    Write-Host 'Server started. To authorize a client, run in an elevated prompt:'
    Write-Host "    & '$exe' pair"
}

if ($Role -eq 'client' -or $Role -eq 'both') {
    $exe   = Join-Path $dest 'netusb-client.exe'
    $usbip = Join-Path $env:ProgramFiles 'USBip\usbip.exe'
    if (-not (Test-Path $usbip) -and -not (Get-Command 'usbip.exe' -ErrorAction SilentlyContinue)) {
        Write-Warning ('usbip-win2 is not installed, so devices cannot be attached yet. Install it from ' +
                       'https://github.com/vadimgrn/usbip-win2/releases and run this script again, ' +
                       'or set NETUSB_USBIP_EXE to the full path of usbip.exe.')
    }

    # Attaching a device needs administrator rights, so the interface runs elevated at logon.
    $user      = "$env:USERDOMAIN\$env:USERNAME"
    $action    = New-ScheduledTaskAction -Execute $exe -Argument 'gui'
    $trigger   = New-ScheduledTaskTrigger -AtLogOn -User $user
    $principal = New-ScheduledTaskPrincipal -UserId $user -LogonType Interactive -RunLevel Highest
    $settings  = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries `
                     -ExecutionTimeLimit ([TimeSpan]::Zero)
    Register-ScheduledTask -TaskName 'netusb client' -Action $action -Trigger $trigger `
        -Principal $principal -Settings $settings -Force | Out-Null
    Start-ScheduledTask -TaskName 'netusb client'

    # A Start Menu entry that opens the interface in the browser.
    $lnk = Join-Path $env:ProgramData 'Microsoft\Windows\Start Menu\Programs\netusb.lnk'
    $sc  = (New-Object -ComObject WScript.Shell).CreateShortcut($lnk)
    $sc.TargetPath  = $exe
    $sc.Arguments   = 'open'
    $sc.WindowStyle = 7   # minimized
    $sc.Save()

    Write-Host ''
    Write-Host 'Client started. Open "netusb" from the Start Menu (or run: netusb-client open).'
}
