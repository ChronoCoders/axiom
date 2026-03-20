# AXIOM Protocol — Release Notes V1.0.0

**Release Date:** February 10, 2026
**Protocol Version:** 1 (FROZEN)
**Crate Version:** 0.1.0
**Status:** Production-ready initial release

---

## What is AXIOM?

AXIOM is a purpose-built proof layer and cryptographic trust anchor. It provides cryptographic guarantees that data existed at a specific time, in a specific order, and has not been altered since.

AXIOM is optimized for timestamping, ordering, and immutability — not computation. It intentionally excludes a virtual machine, gas model, or smart contract execution environment.

**Target use cases:** notarization, audit logs, supply chain provenance, event anchoring, and compliance records.

---

## Protocol v1 Highlights

- **Fully deterministic state machine** — given the same genesis and block sequence, all compliant nodes produce identical state. Wall-clock time, randomness, network order, and hardware have zero influence on state transitions.
- **Ed25519 digital signatures** (RFC 8032) for all identity and transaction authentication.
- **SHA-256 hashing** for block hashes, state hashes, genesis hash, and transaction hashes.
- **Constant-time cryptographic comparisons** using the `subtle` crate to prevent timing side-channel attacks.
- **BFT consensus** with deterministic round-robin proposer selection and 2/3+ quorum requirement.
- **Checked arithmetic everywhere** — no overflow, no wrapping, no saturating arithmetic in protocol-critical paths.
- **Canonical binary serialization** ensuring deterministic hashing across all implementations.

## Genesis Parameters

| Parameter | Value |
|---|---|
| Initial validators | 4 |
| Voting power per validator | 10 |
| Total voting power | 40 |
| Quorum (signatures required) | 3 of 4 |
| Stake per validator | 1,000,000 AXM |
| Total initial supply | 4,000,000 AXM |
| Block reward | 10 AXM per committed block |
| Reward recipient | Block proposer |
| Genesis height | 0 |
| Genesis epoch | 0 |
| Genesis state hash | `c1b50f23e410fe99b7ec6e304165b18f1dfe723ad5417133a12cdf8517460761` |

## Architecture

AXIOM is implemented as a Rust workspace with 12 crates organized by strict dependency boundaries:

| Crate | Purpose |
|---|---|
| `primitives` | Core types — BlockHash, StateHash, AccountId, ValidatorId, Signature, PublicKey |
| `crypto` | Ed25519 signatures, SHA-256 hashing, key handling |
| `state` | Accounts, balances, nonces, validator sets, epoch tracking |
| `execution` | State transition logic, block reward distribution |
| `consensus` | Block validation, quorum verification, proposer selection |
| `mempool` | Transaction holding area (non-consensus-critical) |
| `storage` | Persistent block and state storage (SQLite) |
| `network` | P2P networking layer (TCP, bincode, tokio) |
| `api` | HTTP REST API (axum) |
| `node` | Main binary — wires all crates together |
| `genesis-tool` | CLI tool for genesis ceremony and key generation |
| `test-vector-gen` | CLI tool for deterministic test vector generation and hash locking |

**Codebase:** ~6,200 lines of Rust across 21 source files, plus HTML/CSS/JS dashboard.

## API Endpoints

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/status` | Node status (version, height, validator count) |
| `GET` | `/api/blocks?limit=N&cursor=H` | Paginated block listing |
| `GET` | `/api/network/peers` | Connected peer information |
| `POST` | `/api/transactions` | Submit a signed transaction |
| `GET` | `/health/live` | Liveness probe |
| `GET` | `/health/ready` | Readiness probe |

## AXIOM Console (Web Dashboard)

- Session-based authentication (default: `operator` / `axiom`, configurable via environment variables)
- Real-time block explorer with block detail views
- Validator list with power and status
- Account viewer with balance and nonce
- Network peer monitor with connection timestamps
- Block activity sparkline graphs
- Global search across blocks, hashes, and accounts
- Responsive layout, no frontend framework dependencies

## Local Testnet

- 4 validator nodes on API ports 8081–8084 and P2P ports 3001–3004
- Cross-platform scripts: `run_local_testnet.sh` (Linux/macOS), `run_local_testnet.ps1` (Windows)
- Fresh genesis ceremony on each run (new keys, new genesis hash)
- Validator keys passed exclusively via `AXIOM_VALIDATOR_PRIVATE_KEY` environment variable

## Security

- **No `unwrap()` or `expect()` outside of test code** — all error paths handled explicitly
- **No `unsafe` code** anywhere in the codebase
- **No floating-point types** — integer-only economics and state
- **Constant-time hash and signature comparisons** via `subtle::ConstantTimeEq`
- **Validator private keys never stored in config files** — environment variable only
- **Consensus livelock protection** — 30-second hard timeout prevents indefinite stalls from split votes
- **Zero compiler warnings, zero clippy warnings** with strict `cargo clippy -- -D warnings`
- **Checked arithmetic** on all protocol-critical calculations (no overflow possible)
- **No `std::time`, `std::env`, `std::thread`** in deterministic core crates

## Audited Cryptographic Dependencies

| Crate | Purpose |
|---|---|
| `ed25519-dalek` | Ed25519 digital signatures |
| `sha2` | SHA-256 hashing |
| `subtle` | Constant-time operations |

## Quality

- 92 automated tests passing across the workspace
- Full protocol v1 compliance audit against `docs/PROTOCOL_v1.md` — all normative sections verified
- Strict clippy enforcement: `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- Rust 2021 edition, stable toolchain only

## Configuration

- TOML-based configuration (`axiom.toml`)
- Precedence: CLI flags > Environment variables > Config file
- Configuration controls runtime behavior only, never protocol semantics
- Invalid configuration causes immediate startup failure

## What AXIOM Is Not

AXIOM intentionally excludes:

- Virtual machine or bytecode execution
- Smart contracts or programmable transactions
- Gas model or fee market
- General-purpose computation

These exclusions are by design. Smart contracts would add complexity and attack surface without improving the core proof guarantees that AXIOM provides.

## Looking Ahead

Protocol v2 specification is drafted (`docs/PROTOCOL_v2.md`) and introduces:

- Round-based BFT consensus
- Staking and delegation
- Slashing for misbehavior
- Dynamic validator set changes

Protocol v2 is not yet implemented. Protocol v1 remains frozen — no behavioral changes without a version increment.

---

**Protocol Specification:** `docs/PROTOCOL_v1.md` (NORMATIVE, FROZEN)
