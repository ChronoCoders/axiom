$ErrorActionPreference = "Stop"

$Root = (Resolve-Path "$PSScriptRoot/..").Path
$TestnetDir = "$Root/testnet_data"
$BinDir = "$Root/target/debug"

& "$Root/scripts/run_local_testnet.ps1" -FastForward -FastForwardHeight 9999
if ($LASTEXITCODE -ne 0) { exit 1 }

function Wait-Height([UInt64]$Target, [Int32]$TimeoutSec) {
    $Deadline = [DateTimeOffset]::UtcNow.AddSeconds($TimeoutSec)
    while ([DateTimeOffset]::UtcNow -lt $Deadline) {
        try {
            $s = Invoke-RestMethod -Uri "http://127.0.0.1:8081/api/status" -Method Get -TimeoutSec 5
            if ($s.height -ge $Target) { return $true }
        } catch {}
        Start-Sleep -Seconds 2
    }
    return $false
}

if (-not (Wait-Height 10005 180)) {
    Write-Host "Did not reach v2 height in time"
    exit 1
}

$PidsPath = "$TestnetDir/pids.txt"
$Pids = (Get-Content $PidsPath -Raw) -split '\s+' | Where-Object { $_ -match '^\d+$' } | ForEach-Object { [int]$_ }
if ($Pids.Count -lt 2) {
    Write-Host "Missing node PIDs"
    exit 1
}

$Node2Pid = $Pids[1]
Stop-Process -Id $Node2Pid -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 2

$Node2Dir = "$TestnetDir/node2"
$Node2LogOut = "$Node2Dir/stdout.log"
$Node2LogErr = "$Node2Dir/stderr.log"
if (Test-Path $Node2LogOut) { Remove-Item -Force $Node2LogOut }
if (Test-Path $Node2LogErr) { Remove-Item -Force $Node2LogErr }

$p = Start-Process "$BinDir/axiom-node.exe" -ArgumentList @("--config", "$Node2Dir/axiom.toml") -PassThru -NoNewWindow -RedirectStandardOutput $Node2LogOut -RedirectStandardError $Node2LogErr
$Pids[1] = $p.Id
$Pids | Set-Content $PidsPath

if (-not (Wait-Height 10010 180)) {
    Write-Host "Chain did not keep progressing after restart"
    exit 1
}

Write-Host "ok"
