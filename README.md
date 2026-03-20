# Axiom

Axiom is a compact, deterministic, quorum-finality ledger designed as a commercial notarization and audit infrastructure foundation.

## What’s In This Repo

- **Node**: boots from a genesis file, maintains state in SQLite (WAL), runs P2P + HTTP API + web console ([node](file:///c:/axiom/node))
- **Protocol primitives**: canonical serialization, IDs, hashes, types ([primitives](file:///c:/axiom/primitives))
- **Crypto**: Ed25519 signatures, vote/tx signing, SHA-256 hashing ([crypto](file:///c:/axiom/crypto))
- **Execution/State**: state transition rules and invariants ([execution](file:///c:/axiom/execution), [state](file:///c:/axiom/state))
- **Storage**: SQLite-backed persistence ([storage](file:///c:/axiom/storage))
- **Network**: TCP transport for node-to-node messaging ([network](file:///c:/axiom/network))
- **API**: Axum HTTP API + static web console ([api](file:///c:/axiom/api), [web](file:///c:/axiom/web))
- **Docs**: protocol + locked test vectors + configuration spec ([docs](file:///c:/axiom/docs))

## Quick Start (Windows)

Run a local 4-node testnet (API on `8081-8084`, P2P on `3001-3004`):

```powershell
pwsh -ExecutionPolicy Bypass -File .\scripts\run_local_testnet.ps1
```

Open the console:

- http://127.0.0.1:8081/

Default console credentials:

- `operator` / `axiom`

Stop the testnet:

```powershell
Stop-Process -Id (Get-Content .\testnet_data\pids.txt)
```

## Quick Start (Linux/macOS)

```bash
./scripts/run_local_testnet.sh
```

Console:

- http://127.0.0.1:8081/

Stop:

```bash
kill $(cat ./testnet_data/pids.txt)
```

## Build & Test

```bash
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

## API

Core endpoints (served under `/api`):

- `GET /api/status`
- `GET /api/blocks`
- `GET /api/blocks/{height}`
- `GET /api/blocks/by-hash/{hash}`
- `GET /api/accounts/{account_id}`
- `GET /api/validators`
- `GET /api/network/peers`
- `POST /api/transactions`

Health:

- `GET /health/live`
- `GET /health/ready`

See [API.md](file:///c:/axiom/docs/API.md).

## Configuration

Nodes are configured via `axiom.toml`, environment overrides, and CLI flags.

Key sections:

- `[node]`, `[network]`, `[api]`, `[storage]`, `[genesis]`, `[mempool]`, `[logging]`, `[console]`

See [CONFIG.md](file:///c:/axiom/docs/CONFIG.md).

## Protocol & Compliance

- v1 protocol spec: [PROTOCOL_v1.md](file:///c:/axiom/docs/PROTOCOL_v1.md)
- Locked test vectors (normative): [TEST_VECTORS.md](file:///c:/axiom/docs/TEST_VECTORS.md)
- Locked reference genesis: [reference_genesis.json](file:///c:/axiom/docs/reference_genesis.json)

The node binary enforces a locked genesis hash at startup. If the configured `genesis_file` does not match the locked hash, the node exits.

## Security Notes

- Console authentication is a lightweight UI gate (session token) intended for local/operator use; do not treat it as a complete security perimeter.
- P2P transport is not authenticated/encrypted in v1; run behind network controls.

