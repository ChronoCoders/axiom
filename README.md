# AXIOM

A permissioned BFT blockchain ledger built for commercial notarization and audit infrastructure.

AXIOM is not a public cryptocurrency. The validator set is fixed and known at genesis, blocks achieve instant finality at 2/3+ quorum, and the system is designed for regulated environments where auditability, determinism, and operator control take precedence over open participation.

---

## Overview

| Property | Value |
|----------|-------|
| Consensus | Round-based BFT (PBFT-style) with prevote and precommit phases |
| Finality | Instant. A committed block is final with no forks and no reorgs |
| Validator set | Fixed at genesis. Rotation requires a protocol upgrade |
| Serialization | Deterministic length-prefixed binary encoding for all hashes; MessagePack for P2P wire format |
| Storage | SQLite with WAL mode, single serialized connection |

**Transaction types**

| Protocol | Available types |
|----------|----------------|
| Transfer (height < 10,000) | Transfer |
| Staking (height >= 10,000) | Transfer, Stake, Unstake, SlashEvidence |

Protocol version is derived from block height at runtime. No version state is stored or configured.

---

## Workspace Layout

```
primitives   Core types, constants, canonical serialization
crypto       Ed25519 signing, verification, constant-time comparison
state        In-memory ledger state (accounts, staking)
execution    Transaction validation and state transitions
storage      SQLite persistence layer
mempool      Pending transaction pool
network      TCP P2P layer with framed MessagePack messaging
consensus    BFT engine (per-height Engine instances)
api          HTTP REST API and operator console (Axum)
node         Binary that wires all crates together
tools/
  genesis-tool       Generate genesis state and config
  fast-forward       Seed a database to a target height for testing
  test-vector-gen    Generate locked protocol test vectors
fuzz/                Fuzz targets (requires cargo-fuzz and nightly)
```

Dependency order (imports flow downward only):

```
primitives -> crypto -> state -> execution
                                    |
                      storage   network   mempool
                                    |
                                consensus
                                    |
                             api <- node
```

---

## Building

```bash
# Development build
cargo build --workspace

# Production binary (embeds git SHA)
GIT_SHA=$(git rev-parse HEAD) cargo build --release -p axiom-node

# Release build with checksums (Linux/macOS)
GIT_SHA=$(git rev-parse HEAD) bash scripts/build_release.sh
# Output lands in dist/
```

---

## Running

### Single Node

```bash
AXIOM_VALIDATOR_PRIVATE_KEY=<hex> cargo run -p axiom-node -- --config axiom.toml
```

The validator private key must be supplied via the `AXIOM_VALIDATOR_PRIVATE_KEY` environment variable. It must never appear in config files or CLI arguments.

Config values can be overridden via environment variables prefixed with `AXIOM__`, using `__` as a separator:

```bash
AXIOM__LOGGING__LEVEL=debug AXIOM_VALIDATOR_PRIVATE_KEY=<hex> cargo run -p axiom-node -- --config axiom.toml
```

### Local 4-Node Testnet (Windows)

```powershell
.\scripts\run_local_testnet.ps1

# Start at height 9999 (one block before Staking activation)
.\scripts\run_local_testnet.ps1 -FastForward

# Start already in Staking era
.\scripts\run_local_testnet.ps1 -FastForward -FastForwardHeight 10001
```

Nodes are available at:

| Node | API |
|------|-----|
| 1 | http://127.0.0.1:8081 |
| 2 | http://127.0.0.1:8082 |
| 3 | http://127.0.0.1:8083 |
| 4 | http://127.0.0.1:8084 |

Stop all nodes:

```powershell
Stop-Process -Id (Get-Content testnet_data/pids.txt)
```

---

## Configuration

Logging must use JSON format (`logging.format = "json"` is enforced at startup). The console password must not be passed as a CLI argument; use the TOML config or the `AXIOM__CONSOLE__PASSWORD` environment variable.

All config structs use `serde(deny_unknown_fields)`. Unknown keys are a hard error at startup.

---

## API

All routes except health probes and auth endpoints require a bearer token obtained via `/auth/login`. Tokens have an 8-hour TTL and are pruned on every successful login.

| Method | Path | Description |
|--------|------|-------------|
| GET | `/health/live` | Liveness probe, always 200 |
| GET | `/health/ready` | Readiness probe, 200 once genesis is loaded |
| POST | `/auth/login` | Obtain a console auth token |
| POST | `/auth/verify` | Verify a token |
| POST | `/auth/logout` | Revoke a token |
| GET | `/api/status` | Node status and current height |
| GET | `/api/metrics` | Prometheus-compatible metrics |
| GET | `/api/blocks` | List recent blocks |
| GET | `/api/blocks/:height` | Block by height |
| GET | `/api/blocks/by-hash/:hash` | Block by hash |
| GET | `/api/accounts/:id` | Account balance and nonce |
| GET | `/api/validators` | Validator set |
| GET | `/api/staking` | Staking state (Staking protocol only) |
| GET | `/api/consensus` | Current consensus round state |
| GET | `/api/network/peers` | Connected peers |
| POST | `/api/transactions` | Submit a transaction |

Hex values are lowercase without a `0x` prefix internally. The API accepts and strips `0x` on input.

---

## Testing

```bash
# Full test suite
cargo test --workspace

# Single crate
cargo test -p axiom-execution

# Single test by name
cargo test test_apply_valid_transfer

# Fuzz targets (requires cargo-fuzz and nightly)
cargo fuzz run network_message -- -max_total_time=30
cargo fuzz run transaction_json -- -max_total_time=30
```

`storage/tests/vectors_replay.rs` replays locked protocol test vectors and must not be broken by any change.

`network/tests/network_test.rs::test_3_node_communication` is timing-sensitive and may flake under heavy parallel load. It passes reliably in isolation.

---

## Linting

Zero warnings are enforced (`#![deny(warnings)]` in every crate root):

```bash
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Or use the alias defined in `.cargo/config.toml`:

```bash
cargo clippy-strict
```

---

## Security

- The validator private key is loaded exclusively from the `AXIOM_VALIDATOR_PRIVATE_KEY` environment variable.
- The genesis hash is locked in `node/src/main.rs` as `LOCKED_GENESIS_HASH`. The binary refuses to start if the loaded genesis file does not match.
- P2P connections require a genesis hash and protocol version handshake before any messages are forwarded.
- All cryptographic operations use Ed25519 (ed25519-dalek). Auth credential comparison uses constant-time comparison via `axiom_crypto::ct_compare`.

---

## Protocol Constants

| Constant | Value |
|----------|-------|
| Staking activation height | 10,000 |
| Max transactions per block | 1,000 |
| Max block size | 2 MB |
| Max transaction size | 64 KB |
| Min validator stake | 100,000 AXM |
| Unbonding period | 1,000 blocks |
| Slash percentage | 10% |

---

## Author

Altug Tatlisu, [altug@bytus.io](mailto:altug@bytus.io)  
ChronoCoders
