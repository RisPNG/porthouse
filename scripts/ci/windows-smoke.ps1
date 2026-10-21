# Native Windows smoke test: the real Windows programs talking to each other over TLS, pairing,
# listing the virtual test device. No driver involved.
param([string]$Bin = "$PWD\target\release")
$ErrorActionPreference = 'Stop'
$srv = Join-Path $Bin 'netusb-server.exe'
$cli = Join-Path $Bin 'netusb-client.exe'
$work = Join-Path $env:RUNNER_TEMP 'nu-smoke'
New-Item -ItemType Directory -Force -Path "$work\s", "$work\c" | Out-Null
$port = 7600
$fails = 0
function Check($name, $ok, $detail = '') {
    if ($ok) { Write-Host "  PASS  $name" } else { Write-Host "  FAIL  $name $detail"; $script:fails++ }
}

Write-Host '== programs start'
Check 'server --help runs' ((& $srv --help 2>&1 | Out-String) -match 'netusb-server')
Check 'client --help runs' ((& $cli --help 2>&1 | Out-String) -match 'netusb-client')

Write-Host '== server'
$env:NETUSB_STATE_DIR = "$work\s"
$sp = Start-Process -FilePath $srv -ArgumentList 'run', '--no-mdns', '--port', $port, '--test-device', 'mouse' `
    -RedirectStandardError "$work\server.err" -RedirectStandardOutput "$work\server.out" -PassThru -NoNewWindow
try {
    $up = $false
    for ($i = 0; $i -lt 40; $i++) {
        if (Get-NetTCPConnection -LocalPort $port -State Listen -ErrorAction SilentlyContinue) { $up = $true; break }
        Start-Sleep -Milliseconds 250
    }
    Check 'server is listening' $up (Get-Content "$work\server.err" -Raw)

    $pairOut = & $srv pair | Out-String
    $pin = [regex]::Match($pairOut, 'PIN:\s+(\d{4}-\d{4})').Groups[1].Value
    Check 'server prints a pairing PIN' ($pin -ne '') $pairOut

    Write-Host '== client'
    $env:NETUSB_STATE_DIR = "$work\c"
    $unpaired = & $cli list --host "127.0.0.1:$port" 2>&1 | Out-String
    Check 'unpaired client is refused' ($unpaired -match 'not a paired server') $unpaired
    $wrong = & $cli pair "127.0.0.1:$port" --pin 00000000 2>&1 | Out-String
    Check 'wrong PIN is rejected' ($wrong -match 'wrong PIN') $wrong
    $paired = & $cli pair "127.0.0.1:$port" --pin $pin 2>&1 | Out-String
    Check 'correct PIN pairs' ($paired -match 'paired with') $paired
    $list = & $cli list --host "127.0.0.1:$port" 2>&1 | Out-String
    Check 'paired client lists the virtual mouse' ($list -match '99-1' -and $list -match '1209:0001') $list
    Write-Host $list

    Write-Host '== state files'
    Get-ChildItem "$work\s", "$work\c" | Format-Table Name, Length -AutoSize | Out-String | Write-Host
} finally {
    Stop-Process -Id $sp.Id -Force -ErrorAction SilentlyContinue
    Write-Host '--- server log'
    Get-Content "$work\server.err" -ErrorAction SilentlyContinue
}
if ($fails -gt 0) { Write-Host "$fails check(s) failed"; exit 1 }
Write-Host 'windows smoke: all checks passed'
