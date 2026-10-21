# Native Windows smoke test: the real Windows programs talking to each other over TLS, pairing,
# listing the virtual test device. No driver involved.
param([string]$Bin = "$PWD\target\release")
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\windows-lib.ps1"
$work = Join-Path $env:RUNNER_TEMP 'nu-smoke'
$port = 7600

Write-Host '== programs start'
Check 'server --help runs' ((& $srv --help 2>&1 | Out-String) -match 'porthouse-server')
Check 'client service --help runs' ((& $clid --help 2>&1 | Out-String) -match 'porthouse-clientd')
Check 'client --help runs' ((& $cli --help 2>&1 | Out-String) -match 'porthouse-client')

Write-Host '== services'
$sp = Start-Nu $srv @('run', '--no-mdns', '--port', $port, '--test-device', 'mouse') "$work\s" 'server'
$cp = Start-Nu $clid @('run') "$work\c" 'clientd'
try {
    Check 'server answers on its control pipe' (Wait-ServerReady "$work\s") (Get-Content "$work\s\server.err" -Raw)
    Check 'client service answers on its control pipe' (Wait-ClientReady "$work\c") (Get-Content "$work\c\clientd.err" -Raw)
    Check 'server is listening' (Wait-For 20 { Get-NetTCPConnection -LocalPort $port -State Listen -ErrorAction SilentlyContinue })

    $pairOut = & $srv pair | Out-String
    $pin = Get-Pin "$work\s"
    Check 'server prints a pairing PIN' ($pin -ne '') $pairOut

    Write-Host '== client'
    Use-Instance "$work\c"
    & $cli add "127.0.0.1:$port" | Out-Null
    $unpaired = & $cli list 2>&1 | Out-String
    Check 'unpaired client sees the server but not its devices' (($unpaired -match 'not paired') -and -not ($unpaired -match '99-1')) $unpaired
    $wrong = & $cli pair "127.0.0.1:$port" --pin 00000000 2>&1 | Out-String
    Check 'wrong PIN is rejected' ($wrong -match 'wrong PIN') $wrong
    $paired = & $cli pair "127.0.0.1:$port" --pin $pin 2>&1 | Out-String
    Check 'correct PIN pairs' ($paired -match 'paired with') $paired
    $list = & $cli list 2>&1 | Out-String
    Check 'paired client lists the virtual mouse' ($list -match '99-1' -and $list -match '1209:0001') $list
    Write-Host $list

    Write-Host '== state files'
    Get-ChildItem "$work\s", "$work\c" | Format-Table Name, Length -AutoSize | Out-String | Write-Host
} finally {
    Stop-Process -Id $sp.Id, $cp.Id -Force -ErrorAction SilentlyContinue
    Write-Host '--- server log'
    Get-Content "$work\s\server.err" -ErrorAction SilentlyContinue
    Write-Host '--- client service log'
    Get-Content "$work\c\clientd.err" -ErrorAction SilentlyContinue
}
if ($fails -gt 0) { Write-Host "$fails check(s) failed"; exit 1 }
Write-Host 'windows smoke: all checks passed'
