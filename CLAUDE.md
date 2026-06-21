# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What This Is

AXIOM is a permissioned BFT blockchain ledger for commercial notarization and audit infrastructure. It is not a public cryptocurrency. The validator set is fixed and known; blocks achieve instant finality at 2/3+ quorum.

## Commands

```bash
# Build
cargo build --workspace
cargo build --release -p axiom-node   # production binary (embeds GIT_SHA)

# Test
cargo test --workspace
cargo test -p axiom-execution         # single crate
cargo test test_apply_valid_transfer  # single test by name

# Lint (matches CI — zero warnings allowed)
cargo clippy-strict                   # alias defined in .cargo/config.toml
# expands to: cargo clippy --workspace --all-targets --all-features -- -D warnings

# Release build + checksums
GIT_SHA=$(git rev-parse HEAD) bash scripts/build_release.sh   # Linux/macOS
# output lands in dist/

# Local 4-node testnet (Windows)
.\scripts\run_local_testnet.ps1
.\scripts\run_local_testnet.ps1 -FastForward              # start at height 9999 (pre-Staking)
.\scripts\run_local_testnet.ps1 -FastForward -FastForwardHeight 10001  # start in Staking
# Logs: testnet_data/nodeN/node.log and node.err  (one dir per validator, wiped on each run)
# On Windows, kill any running node before rebuilding: Get-Process axiom-node | Stop-Process -Force

# Fuzzing (requires cargo-fuzz, nightly)
cargo fuzz run network_message -- -max_total_time=30
cargo fuzz run transaction_json -- -max_total_time=30
```

## Workspace Layout and Dependency Order

Crates must only import downward in this chain — never upward:

```
primitives → crypto → state → execution
                                  ↓
                    storage   network   mempool
                                  ↓
                              consensus
                                  ↓
                           api  ←  node  (binary)
```

`tools/` binaries (genesis-tool, fast-forward, test-vector-gen) are standalone and import wherever needed.

## Key Architectural Facts

**Protocol versioning** is purely height-derived at runtime — never stored or configured:
- `height < 10_000` → Transfer (Transfer only, epoch=0)
- `height >= 10_000` → Staking (Stake, Unstake, SlashEvidence, staking state)

Changing `STAKING_ACTIVATION_HEIGHT` in `primitives/src/lib.rs` requires rebuilding all crates.

**Locked genesis hash** — `node/src/main.rs` hardcodes `LOCKED_GENESIS_HASH`. The binary refuses to start if the loaded genesis file does not hash to this value. Tests bypass this (it is only enforced in `main`). Any genesis change requires updating this constant.

**Canonical serialization** — all hashing (block, transaction, state) uses deterministic length-prefixed binary encoding in `primitives/src/lib.rs`, never JSON. Do not use `serde_json` for anything that feeds into a hash.

**Storage** — single `Arc<Mutex<Connection>>` in `storage/src/lib.rs`. All reads and writes serialize through one lock. SQLite WAL mode is enabled. The schema is initialized and migrated in `Storage::initialize()`.

**BFT engine** (`consensus/src/bft.rs`) — `Engine` is instantiated per height and reset on commit. Entry points: `make_proposal`, `on_proposal`, `make_prevote`, `on_vote`, `make_precommit`. Supermajority threshold: `power * 3 > total * 2`.

**Two distinct signature schemes** — do not confuse them:
- `verify_vote` / `sign_vote`: v1 format, `sha256(block_hash || height_be)`. Used for the proposer signature inside `construct_block`.
- `verify_precommit` / `sign_consensus_vote`: BFT precommit format, `serialize_vote_canonical({height, round, phase, block_hash, validator_id})`. Used for ALL validator signatures in a `CommittedBlock` regardless of protocol version.

`verify_quorum_v2` (and `verify_quorum`) call `verify_precommit`, not `verify_vote`. Passing a `sign_vote`-signed value to `verify_precommit` always fails with `InvalidSignature`.

**Validator private key** — loaded exclusively from the `AXIOM_VALIDATOR_PRIVATE_KEY` environment variable. Never in config files or CLI args. The console password must also not be passed as a CLI arg (`--console-password` does not exist); use the TOML config or `AXIOM__CONSOLE__PASSWORD` env var.

## Running a Single Node Manually

```bash
AXIOM_VALIDATOR_PRIVATE_KEY=<hex> cargo run -p axiom-node -- --config axiom.toml
```

Config keys can be overridden via env vars prefixed `AXIOM__` with `__` as separator (e.g. `AXIOM__LOGGING__LEVEL=debug`).

## API Endpoints

- `GET /health/live` — always returns 200; used by load balancers.
- `GET /health/ready` — returns 200 only if genesis loaded successfully; used by orchestrators to gate traffic.
- All other routes require a bearer token obtained via the login endpoint. Token TTL is 8 hours; tokens are pruned on every successful login.

**`/api/status` field semantics** — `protocol_version` is the binary's static `PROTOCOL_VERSION` constant (software version, not the chain's current block protocol). `next_protocol_version` is the height-derived protocol for the next block and is what you want to check against `STAKING_ACTIVATION_HEIGHT`. `syncing` is a heuristic: `height > 0 && (now − latest_block_timestamp) > 60s` — it becomes `true` 60 seconds after any chain stall, not immediately.

**SSE endpoint** (`GET /events`) — fires `block` events only when new blocks are committed. Silent during a stall; a stalled chain will not send heartbeats.

## Conventions

- All crate roots have `#![deny(warnings)]` — the build fails on any warning.
- The release profile sets `panic = "abort"`, `lto = true`, `codegen-units = 1` — production binaries are not unwind-safe and are not suitable for dynamic linking.
- Network enforces per-message-type size limits: block ≤ 2 MB, evidence ≤ 128 KB, transaction ≤ 64 KB. Exceeding these causes the connection to be dropped.
- `peer_api_map` in `NetworkConfig` (and in testnet TOML configs) maps a peer's P2P address to its HTTP API address. This is optional metadata used for inter-node queries; the node does not use it automatically.
- `serde(deny_unknown_fields)` on every config struct — maintain this on new structs.
- Error types use `thiserror` with structured `{ expected, got }` fields — follow this pattern.
- Logging is `tracing` in JSON format only (`logging.format = "json"` is required by `validate()`).
- Hex encoding is lowercase, no `0x` prefix internally; the API accepts and strips `0x` on input.
- Auth tokens are stored as `HashMap<String, Instant>` with an 8-hour TTL. Token generation uses `OsRng`. Credential comparison uses `axiom_crypto::ct_compare` (constant-time, hash-then-compare). Tokens are **in-memory only** — any node restart invalidates all tokens; browsers cache the stale token in `localStorage` under the key `axiom_token` and users must re-login.
- `Mempool::add()` is deprecated — always use `add_for_height(current_height, tx)`.
- `Storage::get_block_by_height` and `get_block_by_hash` return `(Block, String)` — the second element is the stored hash hex; do not recompute it.

## Block Sync Protocol

Added in Phase 1a. A lagging or restarting node catches up by requesting blocks from peers.

**New `NetworkMessage` variants** (`network/src/lib.rs`):
- `BlockRequest(u64)` — ask peers for the block at this height
- `BlockResponse(Option<Block>)` — peer's reply; `None` if not found

**Forwarding** — `StatusResponse` is now forwarded from `handle_incoming_connection` to `node_tx` so the node can detect height lag. Tests that read from `net_rx` must skip `StatusResponse` (use `recv_skip_status` in network tests).

**Sync flow** (`node/src/node.rs`):
1. The node broadcasts `StatusRequest` every 30 s. Peers reply with `StatusResponse { height, .. }`.
2. When a `StatusResponse` arrives with `peer_height > our_height`, a `BlockSyncSession` is started.
3. The sync path runs at the top of the consensus loop — it pre-empts BFT processing while active.
4. Each iteration: send `BlockRequest(next_to_request)`, wait for `BlockResponse`.
5. On `BlockResponse(Some(block))`: validate with `validate_and_commit_block`, commit with the appropriate storage call (`commit_block` Transfer / `commit_block_v2` Staking), increment `current_height`.
6. On `BlockResponse(None)` or timeout (500 ms): abort sync session; retry on next `StatusResponse`.
7. Both Transfer and Staking message arms also serve `BlockRequest` from other syncing peers.

The `BlockSyncSession` struct is local to `node::start`. No separate thread is used; sync is driven purely by the async event loop.

## Test Structure

- Unit tests are inline (`#[cfg(test)]` inside each `src/lib.rs`).
- Integration tests are in `crate/tests/*.rs`.
- `storage/tests/vectors_replay.rs` — replays locked protocol test vectors; must not be broken.
- `network/tests/network_test.rs::test_3_node_communication` is timing-sensitive and may flake under heavy parallel load; it passes reliably in isolation.
- Network tests that receive from `net_rx` must use `recv_skip_status` to drain `StatusResponse` messages, which are now forwarded to the node channel.
- The `fuzz/` crate is excluded from `--workspace` and requires `cargo-fuzz` + nightly.
