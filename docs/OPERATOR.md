# AXIOM Operator Documentation

## 1. Deployment Runbook

### Prerequisites
- 4 Linux Servers (Ubuntu/Debian recommended)
- SSH Access with sudo privileges
- Ports 8080 (API) and 3000 (P2P) open (configured via script)
- Docker installed on the deployment machine (for building binaries)

### Step 1: Genesis Ceremony
Run the genesis ceremony script locally to generate validator keys and the initial `genesis.json`.

```bash
./scripts/genesis_ceremony.sh
```
This will produce `genesis_output/` containing:
- `genesis.json`
- `validator_1.secret`
- `validator_2.secret`
- `validator_3.secret`
- `validator_4.secret`

### Step 2: Deploy Validators
Use the deployment script to provision each node. The script accepts a comma-separated list of peers to configure the firewall and network.

**Example Setup:**
- Node 1: 192.168.1.10
- Node 2: 192.168.1.11
- Node 3: 192.168.1.12
- Node 4: 192.168.1.13

```bash
# Node 1 (Peers: 2, 3, 4)
./scripts/deploy.sh user@192.168.1.10 genesis_output/validator_1.secret genesis_output/genesis.json node-1 "192.168.1.11:3000,192.168.1.12:3000,192.168.1.13:3000"

# Node 2 (Peers: 1, 3, 4)
./scripts/deploy.sh user@192.168.1.11 genesis_output/validator_2.secret genesis_output/genesis.json node-2 "192.168.1.10:3000,192.168.1.12:3000,192.168.1.13:3000"

# Node 3 (Peers: 1, 2, 4)
./scripts/deploy.sh user@192.168.1.12 genesis_output/validator_3.secret genesis_output/genesis.json node-3 "192.168.1.10:3000,192.168.1.11:3000,192.168.1.13:3000"

# Node 4 (Peers: 1, 2, 3)
./scripts/deploy.sh user@192.168.1.13 genesis_output/validator_4.secret genesis_output/genesis.json node-4 "192.168.1.10:3000,192.168.1.11:3000,192.168.1.12:3000"
```

The script will:
1. Build the binary using Docker.
2. Copy files and set secure permissions (`chmod 600` for keys).
3. Configure `axiom.toml` with the provided peers.
4. Set up `ufw` firewall rules to ONLY allow port 3000 from the listed peer IPs.
5. Start the `axiom` systemd service.

## 2. Monitoring

### Logs
Check logs using systemd:
```bash
journalctl -u axiom -f
```

### Log Rotation
Systemd's `journald` handles log rotation automatically. You can configure retention policies in `/etc/systemd/journald.conf`:
```ini
[Journal]
SystemMaxUse=500M
MaxRetentionSec=1month
```
Restart journald to apply: `sudo systemctl restart systemd-journald`.

### Health Checks
- Liveness: `curl http://localhost:8080/health/live`
- Readiness: `curl http://localhost:8080/health/ready`

## 3. Backup & Restore

### Database Backup (SQLite)
AXIOM uses SQLite for persistence. To back up the node state:

1. Stop the node to ensure consistency (WAL mode checkpointing):
   ```bash
   sudo systemctl stop axiom
   ```
2. Backup the data directory:
   ```bash
   # Backup main DB and WAL files
   sudo cp /var/lib/axiom/axiom.db /backup/axiom.db.bak
   sudo cp /var/lib/axiom/axiom.db-wal /backup/axiom.db-wal.bak
   sudo cp /var/lib/axiom/axiom.db-shm /backup/axiom.db-shm.bak
   ```
3. Start the node:
   ```bash
   sudo systemctl start axiom
   ```


### Restore
1. Stop the node.
2. Replace files in `/var/lib/axiom/` with backup files.
3. Ensure permissions are correct:
   ```bash
   sudo chown axiom:axiom /var/lib/axiom/*
   ```
4. Start the node.

## 4. Troubleshooting

### Lost Validator Key
**Recovery is NOT possible.** If a validator key is lost:
1. The key cannot be regenerated or recovered.
2. You must perform a new **Genesis Ceremony** to generate new keys for all validators.
3. Redeploy the network from scratch (new genesis file, new keys) to all nodes.

### Node fails to start
- Check logs: `journalctl -u axiom -n 50`
- Verify genesis hash matches the hardcoded locked hash.
- Verify config validity.

### Consensus stalled
- Ensure at least 2/3 (3 of 4) validators are online.
- Check network connectivity between nodes (Port 3000).
- Check logs for "ViewChange" or "Timeout" messages.
