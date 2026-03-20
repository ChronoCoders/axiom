# AXIOM Implementation Specification

**Protocol Version:** 1
**Status:** NORMATIVE (subordinate to PROTOCOL.md)

## 1. Purpose

This document defines how the AXIOM protocol is implemented in code.

While PROTOCOL.md defines what is correct, this document defines:

- Code structure
- Module boundaries
- Dependency rules
- Execution model

Any implementation that violates the constraints in this document is considered architecturally invalid, even if it appears to follow the protocol.

**Document Precedence:** If this document conflicts with PROTOCOL.md, PROTOCOL.md takes precedence. See PROTOCOL.md Section 1 for the full precedence hierarchy.

## 2. Implementation Language and Style

### 2.1 Language

- AXIOM is implemented in Rust
- Rust 2021 edition or later
- Stable Rust only
- Single native binary

### 2.2 Style Rules

- Deterministic, explicit, classical Rust
- No hidden control flow
- No dynamic dispatch in consensus or state logic
- No macros that obscure logic
- Zero compiler warnings
- Readability and predictability are prioritized over brevity

## 3. Workspace and Crate Structure

AXIOM is implemented as a Rust workspace with strict crate boundaries.

### 3.1 Required Crates

```
axiom/
├── primitives/      # Core data types, serialization, hashing
├── crypto/          # Ed25519 signatures, SHA-256, key handling
├── state/           # State representation
├── execution/       # State transition logic
├── consensus/       # Consensus logic (proposer selection, quorum, voting)
├── mempool/         # Transaction pool (non-consensus-critical)
├── storage/         # Persistence (SQLite)
├── analytics/       # Historical inspection (Optional)
├── network/         # Networking (transport only)
├── api/             # Read-only query endpoints + transaction submission
└── node/            # Binary entry point, wiring, lifecycle
```

### 3.2 Dependency Rules

The following dependency rules are mandatory:

```
primitives
  → depends on nothing

crypto
  → depends only on primitives

state
  → depends only on primitives

execution
  → depends only on primitives, crypto, and state

consensus
  → depends only on primitives, crypto, state, and execution

mempool
  → depends only on primitives and crypto (for validation)

storage
  → depends on primitives and state

analytics
  → depends on primitives, state, and storage (read-only, no protocol logic)

network
  → depends on primitives only

api
  → depends on primitives, state, storage, and mempool

node
  → wires all crates together
```

No crate may depend "upwards" in this hierarchy.
Circular dependencies are forbidden.

### 3.3 Crate Responsibilities

**primitives**: Core data types shared across the system. Block, Transaction, StateHash, ValidatorId, AccountId, all serialization formats (canonical binary, deterministic JSON). No logic beyond serialization and type conversion.

**crypto**: Ed25519 signing via `ed25519-dalek`, vote signing/verification, SHA-256 block hashing, transaction hashing. Wraps audited external libraries. No custom crypto implementations. Constant-time operations where security-relevant.

**state**: State representation. Account balances, nonces, validator registry, economic parameters. Immutable state snapshots. State diff computation if needed.

**execution**: The apply_block function and all state transition logic. Transaction validation and execution. Block reward application. Economic invariant verification. This is the most critical crate in the system.

**consensus**: Proposer selection (round-robin by sorted validator ID). Quorum calculation. Vote validation. Block signature verification. Does not inspect transaction internals.

**mempool**: In-memory transaction pool with configurable capacity, batch retrieval, and batch removal after block commit. Basic validation on submission (signature, nonce, balance). Eviction policy. Not consensus-critical.

**storage**: SQLite persistence. Blocks, state snapshots, validator registry. Atomic commits (block + state in single transaction). WAL mode. Crash safety.

**analytics**: Logs, historical queries, analytics. Entirely optional. Not required for protocol compliance. Read-only relationship with protocol state. Never affects consensus or execution.

**network**: TCP transport. Peer management. Message framing. No protocol logic. No consensus decisions.

**api**: HTTP server. API port is configurable via `ApiConfig.bind_address`. Endpoints: `/api/transactions` (POST, submit transactions), `/api/accounts/{id}` (GET, account lookup), blocks, validators, status. Transaction submission writes to mempool.

**node**: Binary entry point. Configuration loading and validation. Genesis initialization. Crate wiring. Graceful shutdown. Process lifecycle.

## 4. Deterministic Core vs I/O Boundary

### 4.1 Deterministic Core

The following crates form the deterministic core:

- primitives
- crypto (hashing and verification only; key generation is non-deterministic but only used in tests)
- state
- execution
- consensus

Rules:

- No async
- No I/O
- No system time
- No randomness (except test key generation, isolated to test modules)
- No global state
- No logging that affects behavior

These crates must be pure, replayable, and testable in isolation.

### 4.2 I/O and Side Effects

The following crates may perform I/O:

- storage
- analytics
- network
- api
- mempool (may perform I/O for persistence, but pool logic itself is synchronous)
- node

Rules:

- Async allowed only here
- I/O must not affect protocol semantics
- Errors must be propagated explicitly
- No panics in normal operation

## 5. State Transition Implementation

### 5.1 Central Function

The single most important function in the codebase is:

```
apply_block(previous_state, block) -> Result<new_state, ExecutionError>
```

Rules:

- Must be deterministic
- Must not mutate input state
- Must either fully succeed or fully fail
- Must not perform I/O
- Must follow the exact procedure defined in PROTOCOL.md Section 7.2

All state changes must flow through this function.

### 5.2 Transaction Processing

1. Validate each transaction per PROTOCOL.md Section 7.3
2. Apply each transaction per PROTOCOL.md Section 7.4
3. If any transaction fails, return error (entire block rejected)
4. Apply block reward per PROTOCOL.md Section 9.7
5. Verify economic invariants per PROTOCOL.md Section 9.8
6. Compute state hash per PROTOCOL.md Section 5.3

### 5.3 Account Auto-Creation

Per PROTOCOL.md Section 9.6, transfers to non-existent accounts auto-create the recipient account with balance 0 and nonce 0 before the transfer is applied.

## 6. Consensus Implementation

### 6.1 Consensus Model & Separation

**Model**: Single-round propose-vote-commit BFT with proposal timeout fallback (5s default).
**Cleanup**: Stale pending blocks are cleared if quorum isn't reached within 2x timeout; fallback proposer takes over.

- Consensus decides which block is committed
- Execution decides what the block does
- Consensus must not inspect transaction internals

### 6.2 Proposer Selection

Implemented per PROTOCOL.md Section 8.3:

1. Collect active validators
2. Sort by validator ID (lexicographic, ascending, byte-level comparison)
3. **Primary**: `select_proposer` uses `height % active_validator_count`
4. **Fallback**: `select_fallback_proposer` uses `(height + attempt) % active_validator_count` (for fault tolerance when primary is offline)

**Note**: `apply_block` accepts any active validator as proposer (not just the primary), since safety comes from >2/3 quorum, not proposer identity.

### 6.3 Quorum Verification

- Collect signatures on the block
- Map each signature to a validator and voting power
- Reject signatures from non-active validators
- Sum voting power of valid signatures
- Quorum satisfied if sum > (total_voting_power * 2 / 3)
- Integer arithmetic: quorum satisfied if sum * 3 > total_voting_power * 2

### 6.4 Vote Verification

- Votes are Ed25519 signatures over: SHA-256(block_hash || height as u64 big-endian)
- Verify signature against the validator's public key
- Reject votes from unknown or inactive validators

## 7. Persistence Strategy

### 7.1 Primary Database: SQLite

Used for all consensus-critical persistence:

- Blocks (full block data)
- State snapshots (at each committed height)
- Validator registry
- Genesis data

Configuration:

- WAL mode enabled
- Explicit transactions only
- No ORMs
- SQL written and reviewed manually
- Prepared statements for all queries

### 7.2 Analytics Database (Optional)

Used for non-consensus-critical historical data:

- Structured logs
- Analytics queries
- Historical inspection
- Performance metrics

Rules:

- Analytics is entirely optional
- Not required for protocol compliance
- Never affects execution, consensus, or state
- Read-only relationship with protocol data (may import snapshots)
- Failure or absence of analytics database must not affect node operation

### 7.3 PostgreSQL Policy

Not used in AXIOM v1. If strictly necessary in future versions, must be explicitly justified and documented.

### 7.4 Persistence Rules

- Persistence must not introduce nondeterminism
- Stored state must be verifiable via state hash
- Replay from genesis must always produce the same result
- Block and state are committed in a single SQLite transaction (atomic persistence)
- Either both persist or neither persists
- No phantom blocks (block without corresponding state)

## 8. Transaction Ingress

### 8.1 Submission Endpoint

The API server exposes a transaction submission endpoint (see API.md).
This is the only external entry point for transactions.

### 8.2 Mempool Integration

Submitted transactions are validated and placed in the mempool.
The mempool is a local, non-consensus-critical data structure.

### 8.3 Proposer Flow

When a validator is selected as proposer:

1. Select transactions from the local mempool
2. Order transactions (implementation-defined ordering)
3. Validate that the set does not exceed block limits
4. Construct block with selected transactions
5. Execute apply_block to verify the block is valid before proposing
6. Broadcast proposal

## 9. API and UI Integration

### 9.1 API Scope

- Read-only query endpoints expose observable state only
- Transaction submission endpoint writes to mempool (not to state)
- API must not trigger consensus or state changes directly
- API serves only committed, canonical state

### 9.2 UI Contract

- UI is a passive observer
- UI never computes protocol logic
- UI never makes decisions
- The backend must not assume the presence of any UI
- UI is pure HTML/CSS/JS with no frameworks

## 10. Error Handling and Failure Modes

### 10.1 Error Principles

- Errors are explicit values (Result types with enum errors)
- No panics in deterministic core
- Panics allowed only for unrecoverable programmer errors (e.g., logic bugs that should never occur)
- No unwrap() or expect() outside of tests

### 10.2 Invalid Input Handling

- Invalid blocks are rejected (apply_block returns error)
- Invalid transactions reject the entire block
- Invalid votes are ignored
- Invalid API requests return appropriate error responses
- Invalid mempool submissions are rejected with reason

The node must remain in a well-defined state at all times.

## 11. Testing Strategy

### 11.1 Mandatory Tests

- All test vectors in TEST_VECTORS.md must pass
- Determinism tests must pass (same input -> same output across runs)
- Replay tests must pass (genesis through all blocks reproduces same state)
- Account auto-creation tests
- Block limit tests
- Proposer selection tests
- Quorum verification tests
- Economic invariant tests

### 11.2 Test Placement

- Protocol tests live alongside deterministic core crates
- No tests depend on network or real I/O
- Network and API tests are isolated
- Test key material is deterministically generated from fixed seeds

### 11.3 Test Key Generation

For test vectors, Ed25519 key pairs are generated deterministically:

- Use a fixed seed per test identity (e.g., SHA-256("axiom-test-validator-1") as the 32-byte seed)
- Generate Ed25519 keypair from the seed
- These keys are used across all test vectors
- Once generated, keys are locked and published in TEST_VECTORS.md

## 12. Logging and Observability

- Logging must be structured (key-value pairs, not free text)
- Logging must not affect protocol behavior
- Logging is non-normative and optional for compliance
- Log levels: error, warn, info, debug, trace
- Structured log output format: JSON

## 13. Versioning and Upgrades

- Protocol version is explicit (integer, stored in primitives)
- Implementation versioning follows semantic versioning
- Backward compatibility rules are enforced by protocol version checks
- Nodes must reject blocks with unsupported protocol versions

## 14. Prohibited Practices

The following are explicitly forbidden:

- Business logic in the UI
- Async logic in deterministic core
- Dynamic configuration affecting consensus
- Hidden global state
- "Temporary" hacks
- ORMs
- Custom cryptographic implementations
- Floating-point arithmetic in any protocol-critical code
- Global mutable state
- Dynamic dispatch in consensus or execution paths
- Implicit behavior or hidden side effects

Violation of any of these rules invalidates the implementation.

## 15. Completion Criteria

The implementation is considered complete when:

1. All protocol rules from PROTOCOL.md are implemented
2. All test vectors from TEST_VECTORS.md pass
3. Determinism is preserved under replay
4. Restart and replay produce identical state
5. Economic invariants hold at every committed height
6. Zero compiler warnings
7. All coding rules from CODING_RULES.md are followed

Only after this point may additional system layers (analytics, advanced networking, UI enhancements) be considered.
