$ErrorActionPreference = "Stop"

$Root = (Resolve-Path "$PSScriptRoot/..").Path
$TestnetDir = "$Root/testnet_data"

& "$Root/scripts/run_local_testnet.ps1" -FastForward -FastForwardHeight 9999
if ($LASTEXITCODE -ne 0) { exit 1 }

$TargetHeight = 10000
$Deadline = [DateTimeOffset]::UtcNow.AddMinutes(3)
$Ok = $false

while ([DateTimeOffset]::UtcNow -lt $Deadline) {
    try {
        $s = Invoke-RestMethod -Uri "http://127.0.0.1:8081/api/status" -Method Get -TimeoutSec 5
        if ($s.height -ge $TargetHeight) { $Ok = $true; break }
    } catch {}
    Start-Sleep -Seconds 2
}

if (-not $Ok) {
    Write-Host "Did not reach height $TargetHeight"
    exit 1
}

try {
    $c = Invoke-RestMethod -Uri "http://127.0.0.1:8081/api/consensus" -Method Get -TimeoutSec 5
    if ($c.protocol_version -ne 2) { throw "protocol_version != 2" }
} catch {
    Write-Host "Consensus endpoint check failed: $_"
    exit 1
}

try {
    $st = Invoke-RestMethod -Uri "http://127.0.0.1:8081/api/staking" -Method Get -TimeoutSec 5
    if (-not $st.enabled) { throw "staking not enabled" }
} catch {
    Write-Host "Staking endpoint check failed: $_"
    exit 1
}

if (Test-Path "$TestnetDir/pids.txt") {
    $Pids = (Get-Content "$TestnetDir/pids.txt" -Raw) -split '\s+' | Where-Object { $_ -match '^\d+$' }
    foreach ($p in $Pids) { Stop-Process -Id ([int]$p) -Force -ErrorAction SilentlyContinue }
}
