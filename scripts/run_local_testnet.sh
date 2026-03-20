#!/usr/bin/env bash
set -e

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN_DIR="$ROOT/target/debug"
TESTNET="$ROOT/testnet_data"

# 1. Build Node
echo "Building Node..."
cargo build -p axiom-node

# 2. Kill existing nodes
pkill -f axiom-node 2>/dev/null || true
sleep 1

# 3. Clean previous testnet data
if [ -d "$TESTNET" ]; then
  echo "Cleaning previous testnet data..."
  rm -rf "$TESTNET"
fi
mkdir -p "$TESTNET"

# 4. Use Locked Reference Genesis + Locked Test Validator Keys
cp "$ROOT/docs/reference_genesis.json" "$TESTNET/genesis.json"
printf "%s" "eed1444f431a29ddaba560d09559f7b3453cc1def5861ab51bcd3344dae18834" > "$TESTNET/validator_1.secret"
printf "%s" "9bd3bf36c5da99993f250e5b2e558e6768583ed5bbbd24a39560fca381b3c369" > "$TESTNET/validator_2.secret"
printf "%s" "2a8e0ea62396cbe5821e10a3700ee4da1a96eea2bed02c6f28d16591e682e3cb" > "$TESTNET/validator_3.secret"
printf "%s" "139a29f05f0426440423e577fe65810d96d8dd4418f4f4d2226b04f2b5a40712" > "$TESTNET/validator_4.secret"

# 7. Setup Node Directories
BASE_P2P=3000
BASE_API=8080

for i in 1 2 3 4; do
  NODE_DIR="$TESTNET/node$i"
  mkdir -p "$NODE_DIR"

  cp "$TESTNET/genesis.json" "$NODE_DIR/genesis.json"
  cp -r "$ROOT/web" "$NODE_DIR/web"
  cp "$TESTNET/validator_$i.secret" "$NODE_DIR/validator_key"

  PEERS=""
  PEER_API_MAP=""
  for j in 1 2 3 4; do
    if [ "$i" != "$j" ]; then
      if [ -n "$PEERS" ]; then
        PEERS="$PEERS, "
      fi
      PEERS="${PEERS}\"127.0.0.1:$((BASE_P2P + j))\""
      PEER_API_MAP="${PEER_API_MAP}\"127.0.0.1:$((BASE_P2P + j))\" = \"127.0.0.1:$((BASE_API + j))\"
"
    fi
  done

  cat > "$NODE_DIR/axiom.toml" <<EOF
[node]
node_id = "node-$i"
data_dir = "."

[network]
enabled = true
listen_address = "127.0.0.1:$((BASE_P2P + i))"
peers = [$PEERS]

[network.peer_api_map]
${PEER_API_MAP}
[api]
enabled = true
bind_address = "127.0.0.1:$((BASE_API + i))"
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
format = "text"

[console]
user = "operator"
password = "axiom"

[validator]
EOF
done

# 8. Start Nodes
echo "Starting 4 Validator Nodes..."
PIDS=""
for i in 1 2 3 4; do
  DIR="$TESTNET/node$i"
  KEY=$(cat "$DIR/validator_key")
  (cd "$DIR" && AXIOM_VALIDATOR_PRIVATE_KEY="$KEY" exec "$BIN_DIR/axiom-node" --config=axiom.toml) \
    > "$DIR/node.log" 2> "$DIR/node.err" &
  PID=$!
  PIDS="$PIDS $PID"
  echo "Node $i started (PID $PID) API: 127.0.0.1:$((BASE_API + i))"

  sleep 1
  if ! kill -0 $PID 2>/dev/null; then
    echo "CRITICAL: Node $i failed to start"
    tail -20 "$DIR/node.err" 2>/dev/null
    for pid in $PIDS; do kill "$pid" 2>/dev/null || true; done
    exit 1
  fi
done

echo "$PIDS" > "$TESTNET/pids.txt"

cleanup() {
  echo "Stopping testnet..."
  for pid in $PIDS; do
    kill "$pid" 2>/dev/null || true
  done
  wait
}
trap cleanup EXIT INT TERM

sleep 8

for i in 1 2 3 4; do
  PORT=$((BASE_API + i))
  STATUS=$(curl -s -o /dev/null -w "%{http_code}" "http://127.0.0.1:$PORT/health/live" 2>/dev/null || echo "failed")
  echo "Node $i health (port $PORT): $STATUS"
done

echo "Testnet running."
echo "Health Check:  curl http://127.0.0.1:8081/health/live"
echo "Console UI:    http://127.0.0.1:8081/"
echo "To stop:       kill \$(cat $TESTNET/pids.txt)"
wait
