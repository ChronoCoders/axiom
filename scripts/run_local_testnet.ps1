[CmdletBinding()]
param(
    [switch]$FastForward,
    [UInt64]$FastForwardHeight = 9999
)

$ErrorActionPreference = "Stop"

# Resolve workspace root (parent of 'scripts' directory)
$ScriptDir = $PSScriptRoot
$Root = (Resolve-Path "$ScriptDir/..").Path
$BinDir = "$Root/target/debug"
$TestnetDir = "$Root/testnet_data"

# 1. Build Node
Write-Host "Building Node..."
cargo build -p axiom-node
if ($LASTEXITCODE -ne 0) { exit 1 }

if ($FastForward) {
    Write-Host "Building fast-forward..."
    cargo build -p fast-forward
    if ($LASTEXITCODE -ne 0) { exit 1 }
}

# Kill any existing testnet nodes
$PidFile = "$TestnetDir/pids.txt"
if (Test-Path $PidFile) {
    $PidText = Get-Content $PidFile -Raw
    ($PidText -split '\s+') | Where-Object { $_ -match '^\d+$' } | ForEach-Object {
        Stop-Process -Id ([int]$_) -Force -ErrorAction SilentlyContinue
    }
    Start-Sleep -Seconds 2
}
Get-Process axiom-node -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 1

# 2. Clean
if (Test-Path $TestnetDir) {
    Write-Host "Cleaning previous testnet data..."
    Remove-Item -Recurse -Force $TestnetDir
}
New-Item -ItemType Directory -Force -Path $TestnetDir | Out-Null

# 3. Use Locked Reference Genesis + Locked Test Validator Keys
Copy-Item "$Root/fixtures/reference_genesis.json" "$TestnetDir/genesis.json"

$ValidatorSecrets = @(
    "61864c755c13af4bbed639740e37c957562682ae6d4b734e2fc4e51e1d9b45cc",
    "487108a4a56afdac12740bd20b2e9932aff5e8b85988dd15b0cebd53b3f2401c",
    "42074f160e543917a985c8b52fcdc7590481ff9f6926fc6843263e2cf39e060b",
    "1b8e7559a62e4c39bbbb03d0920d0a48e97c72c83ecd65e33d6e414dbb46447d"
)
for ($i = 1; $i -le 4; $i++) {
    $ValidatorSecrets[$i - 1] | Set-Content -NoNewline -Path "$TestnetDir/validator_$i.secret"
}

# 6. Setup Nodes
$Nodes = 1..4
$BaseP2P = 13000
$BaseAPI = 8080

foreach ($i in $Nodes) {
    $NodeDir = "$TestnetDir/node$i"
    New-Item -ItemType Directory -Force -Path $NodeDir | Out-Null
    
    # Copy Genesis and Key
    Copy-Item "$TestnetDir/genesis.json" "$NodeDir/genesis.json"
    Copy-Item -Recurse "$Root/web" "$NodeDir/web"
    Copy-Item "$TestnetDir/validator_$i.secret" "$NodeDir/validator_key"

    if ($FastForward) {
        $DbPath = "$NodeDir/axiom.db"
        if (Test-Path $DbPath) { Remove-Item -Force $DbPath }
        & "$Root/target/debug/fast-forward.exe" $DbPath $FastForwardHeight "$NodeDir/genesis.json"
        if ($LASTEXITCODE -ne 0) { exit 1 }
    }
    
    # Generate Peers List and API Map (exclude self)
    $Peers = @()
    $PeerApiMapLines = @()
    foreach ($j in $Nodes) {
        if ($i -ne $j) {
            $Peers += "127.0.0.1:$($BaseP2P + $j)"
            $PeerApiMapLines += "`"127.0.0.1:$($BaseP2P + $j)`" = `"127.0.0.1:$($BaseAPI + $j)`""
        }
    }
    $PeersString = $Peers -join '", "'
    $PeerApiMapString = $PeerApiMapLines -join "`n"
    
    # Create Config
    $ConfigContent = @"
[node]
node_id = "node-$i"
data_dir = "."

[network]
enabled = true
listen_address = "127.0.0.1:$($BaseP2P + $i)"
peers = ["$PeersString"]

[network.peer_api_map]
$PeerApiMapString

[api]
enabled = true
bind_address = "127.0.0.1:$($BaseAPI + $i)"
tls_enabled = false

[storage]
sqlite_path = "axiom.db"

[genesis]
genesis_file = "genesis.json"

[mempool]
max_size = 10000
max_tx_bytes = 65536

[logging]
level = "info"
format = "json"

[console]
user = "operator"
password = "axiom"

[validator]
"@
    Set-Content -Path "$NodeDir/axiom.toml" -Value $ConfigContent
}

# 7. Start Nodes (with Liveness Gate)
Write-Host "Starting 4 Validator Nodes..."
$Pids = @()

foreach ($i in $Nodes) {
    $NodeDir = "$TestnetDir/node$i"
    $LogOut = "$NodeDir/node.log"
    $LogErr = "$NodeDir/node.err"
    
    # Load validator key into environment for the child process
    $KeyContent = Get-Content "$NodeDir/validator_key" -Raw
    $env:AXIOM_VALIDATOR_PRIVATE_KEY = $KeyContent.Trim()
    
    # Start process in background
    $p = Start-Process -FilePath "$BinDir/axiom-node.exe" `
        -ArgumentList "--config=axiom.toml" `
        -WorkingDirectory $NodeDir `
        -RedirectStandardOutput $LogOut `
        -RedirectStandardError $LogErr `
        -PassThru `
        -NoNewWindow
        
    # Liveness Gate: Wait and Check
    Start-Sleep -Seconds 2
    if ($p.HasExited) {
        Write-Host "CRITICAL FAILURE: Node $i failed to start (Exit Code: $($p.ExitCode))"
        Write-Host "--- Stderr Log Tail ---"
        if (Test-Path $LogErr) {
            Get-Content $LogErr -Tail 20
        } else {
            Write-Host "No stderr log found."
        }
        Write-Host "-----------------------"
        
        # Kill any previously started nodes
        if ($Pids.Count -gt 0) {
            Write-Host "Stopping previously started nodes..."
            $Pids | ForEach-Object { Stop-Process -Id $_ -Force -ErrorAction SilentlyContinue }
        }
        exit 1
    }

    $Pids += $p.Id
    Write-Host "Node $i started (PID: $($p.Id)) API: 127.0.0.1:$($BaseAPI + $i)"
}

# Clear key from environment
Remove-Item Env:AXIOM_VALIDATOR_PRIVATE_KEY -ErrorAction SilentlyContinue

# Save PIDs for cleanup (only if all started successfully)
$Pids | Set-Content "$TestnetDir/pids.txt"

# Operator Guidance
Write-Host "Testnet running successfully."
Write-Host "Health Check:  curl http://127.0.0.1:8081/health/live"
Write-Host "Manual Attach: cargo run -p axiom-node -- --config $TestnetDir/node1/axiom.toml"
Write-Host "To stop:       Stop-Process -Id (Get-Content $TestnetDir/pids.txt)"
