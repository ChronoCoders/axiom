#!/bin/bash
set -e

HOST=$1
KEY_FILE=$2
GENESIS_FILE=$3
NODE_ID=$4
PEERS=$5

if [ -z "$HOST" ] || [ -z "$KEY_FILE" ] || [ -z "$GENESIS_FILE" ] || [ -z "$NODE_ID" ]; then
    echo "Usage: ./deploy.sh <user@host> <key_file> <genesis_file> <node_id> [peer1:port,peer2:port]"
    exit 1
fi

echo "Deploying to $HOST..."

# Build Linux binary using Docker
echo "Building Linux binary via Docker..."
docker build -t axiom-node-builder -f Dockerfile .
id=$(docker create axiom-node-builder)
docker cp $id:/usr/local/bin/axiom-node ./axiom-node-linux
docker rm -v $id

# Copy files
echo "Copying files..."
ssh $HOST "mkdir -p /tmp/axiom_install"
scp ./axiom-node-linux $HOST:/tmp/axiom_install/axiom-node
scp $KEY_FILE $HOST:/tmp/axiom_install/validator.secret
scp $GENESIS_FILE $HOST:/tmp/axiom_install/genesis.json
scp axiom.service $HOST:/tmp/axiom_install/
scp -r web $HOST:/tmp/axiom_install/web

# Prepare Peers TOML
if [ -n "$PEERS" ]; then
    # Wrap each peer in quotes
    PEERS_TOML="[\"$(echo $PEERS | sed 's/,/","/g')\"]"
else
    PEERS_TOML="[]"
fi

# Run setup on remote
echo "Running setup on remote..."
ssh $HOST "NODE_ID='$NODE_ID' PEERS_TOML='$PEERS_TOML' PEERS_RAW='$PEERS' bash -s" << 'EOF'
set -e

# Create user
if ! id -u axiom >/dev/null 2>&1; then
    sudo useradd -r -s /bin/false axiom
fi

# Install binary
sudo mv /tmp/axiom_install/axiom-node /usr/local/bin/
sudo chmod +x /usr/local/bin/axiom-node

# Setup directories
sudo mkdir -p /var/lib/axiom
sudo mkdir -p /etc/axiom
sudo chown -R axiom:axiom /var/lib/axiom

# Copy web console files
sudo rm -rf /var/lib/axiom/web
sudo mv /tmp/axiom_install/web /var/lib/axiom/
sudo chown -R axiom:axiom /var/lib/axiom/web

# Install genesis
sudo mv /tmp/axiom_install/genesis.json /etc/axiom/

# Install key
sudo mv /tmp/axiom_install/validator.secret /etc/axiom/
sudo chown axiom:axiom /etc/axiom/validator.secret
sudo chmod 600 /etc/axiom/validator.secret

# Generate Config
cat <<TOML | sudo tee /etc/axiom/axiom.toml
[node]
node_id = "${NODE_ID}"
data_dir = "/var/lib/axiom"

[network]
enabled = true
listen_address = "0.0.0.0:3000"
peers = ${PEERS_TOML}

[api]
enabled = true
bind_address = "0.0.0.0:8080"
tls_enabled = false

[storage]
sqlite_path = "/var/lib/axiom/axiom.db"

[genesis]
genesis_file = "/etc/axiom/genesis.json"

[mempool]
max_size = 10000
max_tx_bytes = 65536

[logging]
level = "info"
format = "json"

[validator]
private_key = "$(cat /etc/axiom/validator.secret)" 
TOML

sudo chmod 600 /etc/axiom/axiom.toml

# Setup Systemd
sudo mv /tmp/axiom_install/axiom.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable axiom
sudo systemctl restart axiom

# Firewall
if command -v ufw >/dev/null; then
    # Reset 3000 rules to ensure clean slate (optional, but safer to just add)
    sudo ufw allow 8080/tcp
    
    # Restrict 3000 to peers only
    if [ -n "$PEERS_RAW" ]; then
        IFS=',' read -ra ADDR <<< "$PEERS_RAW"
        for peer in "${ADDR[@]}"; do
            # Extract IP (remove :port)
            IP="${peer%%:*}"
            if [ -n "$IP" ]; then
                echo "Allowing P2P from $IP"
                sudo ufw allow from "$IP" to any port 3000 proto tcp
            fi
        done
    else
        # Fallback if no peers provided (e.g. first node), maybe allow all or none?
        # User requirement: "restricted via UFW to only allow connections from the other 3 validator IPs"
        # If no peers, we might strictly default to deny incoming on 3000 until added?
        # For now, we won't open 3000 globally.
        echo "No peers provided, port 3000 not opened globally."
    fi
fi

# Clean up
rm -rf /tmp/axiom_install

echo "Deployment complete on $(hostname)"
EOF

# Local Cleanup
rm -f ./axiom-node-linux
