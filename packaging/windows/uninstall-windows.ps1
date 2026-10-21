<#
.SYNOPSIS
    Remove netusb from Windows. Add -Purge to also delete identities and pairings.
    UNTESTED: written without access to a Windows machine.
#>
#Requires -RunAsAdministrator
param([switch]$Purge)
$ErrorActionPreference = 'Continue'

foreach ($t in 'netusb server', 'netusb client') {
    Stop-ScheduledTask -TaskName $t -ErrorAction SilentlyContinue
    Unregister-ScheduledTask -TaskName $t -Confirm:$false -ErrorAction SilentlyContinue
}
Stop-Process -Name 'netusb-server', 'netusb-client' -Force -ErrorAction SilentlyContinue
foreach ($n in 'netusb server (TCP)', 'netusb server (mDNS)') {
    Remove-NetFirewallRule -DisplayName $n -ErrorAction SilentlyContinue
}
Remove-Item -Force -ErrorAction SilentlyContinue (Join-Path $env:ProgramData 'Microsoft\Windows\Start Menu\Programs\netusb.lnk')
Remove-Item -Recurse -Force -ErrorAction SilentlyContinue (Join-Path $env:ProgramFiles 'netusb')
if ($Purge) {
    Remove-Item -Recurse -Force -ErrorAction SilentlyContinue (Join-Path $env:ProgramData 'netusb')
    Remove-Item -Recurse -Force -ErrorAction SilentlyContinue (Join-Path $env:APPDATA 'netusb')
    Write-Host 'Removed netusb and its saved identities and pairings.'
} else {
    Write-Host 'Removed netusb. Identities and pairings were kept; use -Purge to delete them.'
}
