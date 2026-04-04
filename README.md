# AXIOM

A permissioned BFT blockchain ledger for commercial notarization and audit infrastructure.

AXIOM is not a public cryptocurrency. The validator set is fixed and known; blocks achieve instant finality at 2/3+ quorum. It is designed for regulated environments where auditability, determinism, and operator control matter more than open participation.

---

## Overview

- **Consensus** — Round-based BFT (PBFT-style) with prevote/precommit phases. Supermajority threshold: `votes * 3 > total_power * 2`.
- **Finality** — Instant. A committed block is final; no forks, no reorgs.
- **Validator set** — Fixed and configured at genesis. Rotation requires a protocol upgrade.
- **Transaction types** — Transfer (V1+), Stake, Unstake, SlashEvidence (V2+).
- **Protocol versioning** — Derived from block height at runtime. No stored version state.
  - `height < 10,000` → V1 (Transfer only)
  - `height ≥ 10,000` → V2 (staking, slashing, epochs)
- **Serialization** — Deterministic length-prefixed binary encoding for all hashes. MessagePack for P2P wire format.
- **Storage** — SQLite with WAL mode. Single serialized connection.

---

## Workspace Layout

```
primitives   Core types, constants, canonical serialization
crypto       Ed25519 signing, verification, constant-time comparison
state        In-memory ledger state (accounts, staking)
execution    Transaction validation and state transition
storage      SQLite persistence layer
mempool      Pending transaction pool
network      TCP P2P layer with framed MessagePack messaging
consensus    BFT engine (per-height Engine instances)
api          HTTP REST API + console UI (Axum, optional TLS)
node         Binary — wires all crates together
tools/
  genesis-tool       Generate genesis state and config
  fast-forward       Seed a database to a target height (for testing)
  test-vector-gen    Generate locked protocol test vectors
fuzz/                Fuzz targets (requires cargo-fuzz + nightly)
```

Dependency order (imports flow downward only):

```
primitives → crypto → state → execution
                                  ↓
                    storage   network   mempool
                                  ↓
                              consensus
                                  ↓
                           api  ←  node
```

---

## Building

```bash
# Development build
cargo build --workspace

# Production binary (embeds git SHA)
GIT_SHA=$(git rev-parse HEAD) cargo build --release -p axiom-node

# Release build + checksums (Linux/macOS)
GIT_SHA=$(git rev-parse HEAD) bash scripts/build_release.sh
# Output lands in dist/
```

---

## Running

### Single Node

```bash
AXIOM_VALIDATOR_PRIVATE_KEY=<hex> cargo run -p axiom-node -- --config axiom.toml
```

The validator private key must be supplied via the `AXIOM_VALIDATOR_PRIVATE_KEY` environment variable — never in config files or CLI arguments.

Config values can be overridden via environment variables prefixed `AXIOM__` using `__` as a separator:

```bash
AXIOM__LOGGING__LEVEL=debug AXIOM_VALIDATOR_PRIVATE_KEY=<hex> cargo run -p axiom-node -- --config axiom.toml
```

### Local 4-Node Testnet (Windows)

```powershell
.\scripts\run_local_testnet.ps1

# Start at height 9999 (one block before V2 activation)
.\scripts\run_local_testnet.ps1 -FastForward

# Start already in V2
.\scripts\run_local_testnet.ps1 -FastForward -FastForwardHeight 10001
```

Nodes will be available at:

| Node | API |
|------|-----|
| 1 | `http://127.0.0.1:8081` |
| 2 | `http://127.0.0.1:8082` |
| 3 | `http://127.0.0.1:8083` |
| 4 | `http://127.0.0.1:8084` |

Stop with:

```powershell
Stop-Process -Id (Get-Content testnet_data/pids.txt)
```

---

## Configuration

Logging must use JSON format (`logging.format = "json"` is enforced). The console password must not be passed as a CLI argument — use the TOML config or the `AXIOM__CONSOLE__PASSWORD` env var.

All config structs use `serde(deny_unknown_fields)`. Unknown keys are a hard error.

---

## API

| Method | Path | Description |
|--------|------|-------------|
| GET | `/status` | Node status and current height |
| GET | `/metrics` | Prometheus-compatible metrics |
| GET | `/blocks` | List recent blocks |
| GET | `/blocks/:height` | Block by height |
| GET | `/blocks/by-hash/:hash` | Block by hash |
| GET | `/accounts/:id` | Account balance and nonce |
| GET | `/validators` | Validator set |
| GET | `/staking` | Staking state (V2) |
| GET | `/consensus` | Current consensus round state |
| GET | `/network/peers` | Connected peers |
| POST | `/transactions` | Submit a transaction |
| GET | `/health/live` | Liveness probe |
| GET | `/health/ready` | Readiness probe |
| POST | `/auth/login` | Obtain a console auth token |
| POST | `/auth/verify` | Verify a token |
| POST | `/auth/logout` | Revoke a token |

Hex values are lowercase without `0x` prefix internally. The API accepts and strips `0x` on input.

Auth tokens have an 8-hour TTL. Credential comparison is constant-time.

---

## Testing

```bash
# Full test suite
cargo test --workspace

# Single crate
cargo test -p axiom-execution

# Single test
cargo test test_apply_valid_transfer

# Fuzz targets (requires cargo-fuzz + nightly)
cargo fuzz run network_message -- -max_total_time=30
cargo fuzz run transaction_json -- -max_total_time=30
```

The `storage/tests/vectors_replay.rs` suite replays locked protocol test vectors and must not be broken by any change.

`network/tests/network_test.rs::test_3_node_communication` is timing-sensitive and may flake under heavy parallel load — it passes reliably in isolation.

---

## Linting

Zero warnings are enforced (`#![deny(warnings)]` in every crate root):

```bash
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

---

## Security

- Validator private key loaded exclusively from `AXIOM_VALIDATOR_PRIVATE_KEY` env var.
- Genesis hash is locked in `node/src/main.rs` (`LOCKED_GENESIS_HASH`). The binary refuses to start if the loaded genesis does not match.
- P2P connections require a genesis hash and protocol version handshake before any messages are forwarded.
- All cryptographic operations use Ed25519 (ed25519-dalek). Auth credential comparison uses constant-time comparison via `axiom_crypto::ct_compare`.

---

## Protocol Constants

| Constant | Value |
|----------|-------|
| `V2_ACTIVATION_HEIGHT` | 10,000 |
| `MAX_TRANSACTIONS_PER_BLOCK` | 1,000 |
| `MAX_BLOCK_SIZE_BYTES` | 1 MB |
| `MIN_VALIDATOR_STAKE` | 100,000 |
| `UNBONDING_PERIOD` | 1,000 blocks |
| `SLASH_PERCENTAGE` | 10% |

---

## Author

Altug Tatlisu — [altug@bytus.io](mailto:altug@bytus.io)  
ChronoCoders
