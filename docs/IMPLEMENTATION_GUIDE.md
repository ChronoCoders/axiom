# AXIOM Implementation Guide

**Purpose:** This document is the master task list for implementing AXIOM Protocol v1.
It is ordered by crate dependency. Each crate must be completed and tested before moving to the next.

**Rules:**
- Read PROTOCOL.md, IMPLEMENTATION.md, and CODING_RULES.md before writing any code
- Zero compiler warnings at all times
- No unwrap() or expect() outside of tests
- All arithmetic must use checked operations (checked_add, checked_sub, etc.)
- All cryptographic operations use audited crates (ed25519-dalek, sha2)
- No ORMs, no frameworks, no dynamic dispatch in protocol code
- Every public function must have a doc comment
- Every error type must be an explicit enum with descriptive variants
- Rust 2021 edition, stable toolchain only

---

## Phase 1: axiom-primitives

**Dependencies:** None (only std and serde for serialization)

### Types to Define

```rust
// Protocol constants
pub const PROTOCOL_VERSION: u64 = 1;
pub const MAX_TRANSACTIONS_PER_BLOCK: usize = 1000;
pub const MAX_BLOCK_SIZE_BYTES: usize = 1_048_576; // 1 MB

// Core identifiers
pub struct AccountId(pub [u8; 32]); // Ed25519 public key
pub struct ValidatorId(pub [u8; 32]); // Same as AccountId
pub struct BlockHash(pub [u8; 32]); // SHA-256 output
pub struct StateHash(pub [u8; 32]); // SHA-256 output
pub struct TransactionHash(pub [u8; 32]); // SHA-256 output
pub struct Signature(pub [u8; 64]); // Ed25519 signature
pub struct PublicKey(pub [u8; 32]); // Ed25519 public key

// Display: all hashes and keys display as 64-char lowercase hex
// AccountId and ValidatorId are interchangeable at the type level

// Block structure
pub struct Block {
    pub parent_hash: BlockHash,
    pub height: u64,
    pub epoch: u64,
    pub proposer_id: ValidatorId,
    pub transactions: Vec<Transaction>,
    pub signatures: Vec<ValidatorSignature>,
    pub state_hash: StateHash,
}

pub struct ValidatorSignature {
    pub validator_id: ValidatorId,
    pub signature: Signature,
}

// Transaction structure
pub struct Transaction {
    pub sender: AccountId,
    pub recipient: AccountId,
    pub amount: u64,
    pub nonce: u64,
    pub signature: Signature,
}

// Genesis structure (for JSON deserialization)
pub struct GenesisConfig {
    pub total_supply: u64,
    pub block_reward: u64,
    pub accounts: Vec<GenesisAccount>,
    pub validators: Vec<GenesisValidator>,
}

pub struct GenesisAccount {
    pub id: AccountId,
    pub balance: u64,
    pub nonce: u64,
}

pub struct GenesisValidator {
    pub id: ValidatorId,
    pub voting_power: u64,
    pub account_id: AccountId,
    pub active: bool,
}
```

### Serialization Functions to Implement

```rust
// Canonical binary serialization (PROTOCOL.md Section 5.2)
pub fn serialize_state_canonical(state: &State) -> Vec<u8>;
pub fn serialize_block_canonical(block: &Block) -> Vec<u8>;
pub fn serialize_transaction_canonical(tx: &Transaction) -> Vec<u8>;

// Deterministic JSON serialization for genesis (PROTOCOL.md Section 5.1)
pub fn serialize_genesis_json(genesis: &GenesisConfig) -> String;
pub fn deserialize_genesis_json(json: &str) -> Result<GenesisConfig, PrimitivesError>;

// Hex encoding/decoding
pub fn to_hex(bytes: &[u8]) -> String; // lowercase, no 0x prefix
pub fn from_hex(hex: &str) -> Result<Vec<u8>, PrimitivesError>;
```

### Binary Serialization Format

Follow PROTOCOL.md Section 5.2 exactly:
- u64: 8 bytes, big-endian
- String: 4-byte big-endian length prefix + UTF-8 bytes
- List: 4-byte big-endian count + elements in order
- Map: 4-byte big-endian count + entries sorted by key (lexicographic byte order)
- Bool: 1 byte (0x01 = true, 0x00 = false)

### Tests

- Round-trip serialization for all types
- Canonical ordering verification (maps must be sorted)
- Hex encoding/decoding
- Genesis JSON serialization produces deterministic output
- Verify serialization is deterministic (same input -> same bytes across runs)

---

## Phase 2: axiom-crypto

**Dependencies:** axiom-primitives, ed25519-dalek, sha2

### Functions to Implement

```rust
// Hashing
pub fn sha256(data: &[u8]) -> [u8; 32];
pub fn compute_state_hash(state: &State) -> StateHash;
pub fn compute_block_hash(block: &Block) -> BlockHash;
pub fn compute_transaction_hash(tx: &Transaction) -> TransactionHash;
pub fn compute_genesis_hash(genesis: &GenesisConfig) -> StateHash;

// Signing
pub fn sign_transaction(private_key: &PrivateKey, tx: &Transaction) -> Signature;
pub fn verify_transaction_signature(tx: &Transaction) -> Result<(), CryptoError>;

// Vote signing (PROTOCOL.md Section 8.4)
// Vote message = SHA-256(block_hash || height as u64 big-endian)
pub fn sign_vote(private_key: &PrivateKey, block_hash: &BlockHash, height: u64) -> Signature;
pub fn verify_vote(public_key: &PublicKey, block_hash: &BlockHash, height: u64, signature: &Signature) -> Result<(), CryptoError>;

// Constant-time comparison
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool;

// Key handling (for tests)
pub fn generate_keypair_from_seed(seed: &[u8; 32]) -> (PrivateKey, PublicKey);

// Test key generation (IMPLEMENTATION.md Section 11.3)
pub fn test_keypair(identity: &str) -> (PrivateKey, PublicKey);
// Implementation: seed = SHA-256(identity), then generate_keypair_from_seed(seed)
```

### Error Type

```rust
pub enum CryptoError {
    InvalidSignature,
    InvalidPublicKey,
    InvalidPrivateKey,
    HashMismatch { expected: String, got: String },
}
```

### Tests

- Sign and verify round-trip
- Invalid signature rejected
- Wrong key rejected
- Constant-time comparison works
- Vote sign/verify round-trip
- Test key generation is deterministic (same identity -> same keypair)
- Hash computation is deterministic

---

## Phase 3: axiom-state

**Dependencies:** axiom-primitives

### Types to Define

```rust
pub struct State {
    pub total_supply: u64,
    pub block_reward: u64,
    pub accounts: BTreeMap<AccountId, Account>,
    pub validators: BTreeMap<ValidatorId, Validator>,
}

pub struct Account {
    pub balance: u64,
    pub nonce: u64,
}

pub struct Validator {
    pub voting_power: u64,
    pub account_id: AccountId,
    pub active: bool,
}
```

### Functions to Implement

```rust
// State construction
pub fn from_genesis(genesis: &GenesisConfig) -> Result<State, StateError>;

// State queries
pub fn get_account(&self, id: &AccountId) -> Option<&Account>;
pub fn get_validator(&self, id: &ValidatorId) -> Option<&Validator>;
pub fn active_validators(&self) -> Vec<(&ValidatorId, &Validator)>; // sorted by ID
pub fn total_voting_power(&self) -> u64;

// State validation
pub fn verify_invariants(&self) -> Result<(), StateError>;
// Checks: sum of balances == total_supply, no negative balances, no overflow
```

### Error Type

```rust
pub enum StateError {
    BalanceMismatch { expected_supply: u64, actual_sum: u64 },
    DuplicateAccount { id: AccountId },
    DuplicateValidator { id: ValidatorId },
    ValidatorAccountMissing { validator_id: ValidatorId, account_id: AccountId },
    GenesisSupplyMismatch { declared: u64, actual: u64 },
}
```

### Tests

- Genesis construction with valid data
- Genesis construction fails on supply mismatch
- Genesis construction fails on duplicate accounts
- Invariant verification passes on valid state
- Invariant verification fails on corrupted state
- BTreeMap ensures deterministic ordering

---

## Phase 4: axiom-execution

**Dependencies:** axiom-primitives, axiom-crypto, axiom-state

This is the most critical crate. It contains apply_block.

### Functions to Implement

```rust
// The central function (PROTOCOL.md Section 7.1, 7.2)
pub fn apply_block(previous_state: &State, block: &Block) -> Result<State, ExecutionError>;

// Internal steps (all called within apply_block):
fn validate_block_header(state: &State, block: &Block, expected_height: u64, expected_parent: &BlockHash) -> Result<(), ExecutionError>;
fn validate_proposer(state: &State, block: &Block) -> Result<(), ExecutionError>;
fn validate_quorum(state: &State, block: &Block) -> Result<(), ExecutionError>;
fn validate_block_limits(block: &Block) -> Result<(), ExecutionError>;
fn validate_transaction(state: &State, tx: &Transaction) -> Result<(), ExecutionError>;
fn apply_transaction(state: &mut State, tx: &Transaction) -> Result<(), ExecutionError>;
fn apply_block_reward(state: &mut State, proposer_id: &ValidatorId) -> Result<(), ExecutionError>;
```

### apply_block Procedure (PROTOCOL.md Section 7.2)

```
1. Validate block height == previous height + 1
2. Validate parent_hash matches hash of previous block
3. Validate epoch == 0 (v1 only)
4. Validate proposer matches expected proposer for this height
5. Validate block limits (tx count <= 1000, size <= 1MB)
6. Validate quorum (signatures represent > 2/3 total voting power)
7. For each transaction in order:
   a. Validate signature (Ed25519)
   b. Validate sender exists
   c. Validate nonce matches
   d. Validate amount > 0
   e. Validate sender balance >= amount
   f. If recipient doesn't exist, auto-create with balance 0, nonce 0
   g. Decrease sender balance by amount (checked_sub)
   h. Increase recipient balance by amount (checked_add)
   i. Increment sender nonce (checked_add)
8. Apply block reward to proposer account (checked_add on balance, checked_add on total_supply)
9. Verify economic invariants (sum of balances == total_supply)
10. Compute state hash and verify it matches block.state_hash
11. Return new state
```

If any step fails, return ExecutionError. No partial state changes.

### Proposer Selection (PROTOCOL.md Section 8.3)

```rust
pub fn select_proposer(validators: &BTreeMap<ValidatorId, Validator>, height: u64) -> Result<ValidatorId, ExecutionError> {
    let active: Vec<_> = validators.iter()
        .filter(|(_, v)| v.active)
        .collect(); // BTreeMap iteration is sorted by key
    if active.is_empty() {
        return Err(ExecutionError::NoActiveValidators);
    }
    let index = (height as usize) % active.len();
    Ok(active[index].0.clone())
}
```

### Quorum Verification

```rust
pub fn verify_quorum(state: &State, block: &Block) -> Result<(), ExecutionError> {
    let total_power = state.total_voting_power();
    let mut collected_power: u64 = 0;
    let mut seen_validators = HashSet::new();

    for sig in &block.signatures {
        // Reject duplicate signatures
        if !seen_validators.insert(&sig.validator_id) {
            return Err(ExecutionError::DuplicateSignature { validator: sig.validator_id.clone() });
        }
        // Verify validator is active
        let validator = state.get_validator(&sig.validator_id)
            .ok_or(ExecutionError::UnknownValidator { id: sig.validator_id.clone() })?;
        if !validator.active {
            return Err(ExecutionError::InactiveValidator { id: sig.validator_id.clone() });
        }
        // Verify vote signature
        let block_hash = compute_block_hash(block); // or passed in
        verify_vote(&sig.validator_id.as_public_key(), &block_hash, block.height, &sig.signature)?;
        // Accumulate power
        collected_power = collected_power.checked_add(validator.voting_power)
            .ok_or(ExecutionError::Overflow)?;
    }

    // Quorum check: collected * 3 > total * 2 (integer arithmetic for > 2/3)
    let lhs = collected_power.checked_mul(3).ok_or(ExecutionError::Overflow)?;
    let rhs = total_power.checked_mul(2).ok_or(ExecutionError::Overflow)?;
    if lhs <= rhs {
        return Err(ExecutionError::QuorumNotMet { collected: collected_power, total: total_power });
    }

    Ok(())
}
```

### Error Type

```rust
pub enum ExecutionError {
    // Block validation
    InvalidHeight { expected: u64, got: u64 },
    InvalidParentHash { expected: BlockHash, got: BlockHash },
    InvalidEpoch { expected: u64, got: u64 },
    InvalidProposer { expected: ValidatorId, got: ValidatorId },
    BlockTooManyTransactions { count: usize, max: usize },
    BlockTooLarge { size: usize, max: usize },

    // Quorum
    QuorumNotMet { collected: u64, total: u64 },
    DuplicateSignature { validator: ValidatorId },
    UnknownValidator { id: ValidatorId },
    InactiveValidator { id: ValidatorId },
    NoActiveValidators,

    // Transaction validation
    InvalidSignature { sender: AccountId },
    SenderNotFound { sender: AccountId },
    InvalidNonce { expected: u64, got: u64 },
    InsufficientBalance { account: AccountId, required: u64, available: u64 },
    ZeroAmount,

    // Economic
    Overflow,
    Underflow,
    InvariantViolation { expected_supply: u64, actual_sum: u64 },

    // State hash
    StateHashMismatch { expected: StateHash, computed: StateHash },

    // Crypto
    CryptoError(CryptoError),
}
```

### Tests

This crate needs the most comprehensive tests:

- Valid empty block (reward only)
- Valid block with single transfer
- Valid block with multiple transfers
- Transfer to new account (auto-creation)
- Invalid nonce rejection
- Insufficient balance rejection
- Zero amount rejection
- Invalid signature rejection
- Wrong proposer rejection
- Wrong epoch rejection
- Insufficient quorum rejection
- Duplicate height rejection
- Block too many transactions rejection
- Mixed valid/invalid transactions (entire block rejected)
- Block reward applied to proposer
- Economic invariants verified after every block
- Replay test: genesis through N blocks produces deterministic state
- All TEST_VECTORS.md vectors

---

## Phase 5: axiom-consensus

**Dependencies:** axiom-primitives, axiom-crypto, axiom-state, axiom-execution

### Functions to Implement

```rust
// Re-export proposer selection from execution (or define here)
pub fn select_proposer(state: &State, height: u64) -> Result<ValidatorId, ConsensusError>;

// Block validation (wraps execution)
pub fn validate_and_commit_block(state: &State, block: &Block) -> Result<State, ConsensusError>;

// For proposer nodes: construct a block
pub fn construct_block(
    state: &State,
    height: u64,
    parent_hash: BlockHash,
    transactions: Vec<Transaction>,
    proposer_key: &PrivateKey,
) -> Result<Block, ConsensusError>;
```

### Tests

- Proposer rotation across heights
- Block construction and self-validation
- Quorum edge cases

---

## Phase 6: axiom-mempool

**Dependencies:** axiom-primitives, axiom-crypto

### Functions to Implement

```rust
pub struct Mempool {
    transactions: BTreeMap<TransactionHash, Transaction>,
    max_size: usize,
}

pub fn new(max_size: usize) -> Self;
pub fn submit(&mut self, tx: Transaction, current_state: &State) -> Result<TransactionHash, MempoolError>;
pub fn remove(&mut self, hash: &TransactionHash);
pub fn select(&self, max_count: usize) -> Vec<Transaction>;
pub fn len(&self) -> usize;
pub fn is_empty(&self) -> bool;
```

### Submission Validation

- Verify Ed25519 signature
- Check nonce >= sender's current nonce
- Check sender balance >= amount
- Check amount > 0

### Tests

- Submit valid transaction
- Reject invalid signature
- Reject bad nonce
- Reject insufficient balance
- Eviction when full
- Select returns transactions

---

## Phase 7: axiom-storage

**Dependencies:** axiom-primitives, axiom-state, rusqlite

### Schema

```sql
CREATE TABLE blocks (
    height INTEGER PRIMARY KEY,
    hash TEXT NOT NULL UNIQUE,
    parent_hash TEXT NOT NULL,
    epoch INTEGER NOT NULL,
    proposer_id TEXT NOT NULL,
    state_hash TEXT NOT NULL,
    block_data BLOB NOT NULL  -- canonical binary serialization
);

CREATE TABLE accounts (
    account_id TEXT PRIMARY KEY,
    balance INTEGER NOT NULL,
    nonce INTEGER NOT NULL
);

CREATE TABLE validators (
    validator_id TEXT PRIMARY KEY,
    voting_power INTEGER NOT NULL,
    account_id TEXT NOT NULL,
    active INTEGER NOT NULL  -- 0 or 1
);

CREATE TABLE meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
-- meta stores: genesis_hash, latest_height, protocol_version
```

### Functions to Implement

```rust
pub fn initialize(path: &str) -> Result<Storage, StorageError>;
pub fn store_genesis(&self, state: &State, genesis_hash: &StateHash) -> Result<(), StorageError>;
pub fn commit_block(&self, block: &Block, state: &State) -> Result<(), StorageError>;
// ^ This must be a single SQLite transaction (atomic persistence)

pub fn get_block_by_height(&self, height: u64) -> Result<Option<Block>, StorageError>;
pub fn get_block_by_hash(&self, hash: &BlockHash) -> Result<Option<Block>, StorageError>;
pub fn get_latest_height(&self) -> Result<u64, StorageError>;
pub fn get_account(&self, id: &AccountId) -> Result<Option<Account>, StorageError>;
pub fn get_validators(&self) -> Result<Vec<(ValidatorId, Validator)>, StorageError>;
pub fn get_genesis_hash(&self) -> Result<StateHash, StorageError>;
pub fn get_state(&self) -> Result<State, StorageError>;
```

### Rules

- WAL mode enabled at initialization
- All writes in explicit transactions
- commit_block writes block AND updates all account/validator state atomically
- Prepared statements for all queries
- No ORMs

### Tests

- Store and retrieve genesis
- Store and retrieve blocks
- Atomic persistence (block + state)
- Account queries
- Validator queries
- Crash safety (simulate interruption)

---

## Phase 8: axiom-api

**Dependencies:** axiom-primitives, axiom-state, axiom-storage, axiom-mempool, hyper (or minimal HTTP)

### Endpoints to Implement

Per API.md:

```
GET  /health/live
GET  /health/ready
GET  /api/status
GET  /api/blocks?limit={n}&cursor={height}
GET  /api/blocks/{height}
GET  /api/blocks/by-hash/{hash}
GET  /api/accounts/{account_id}
GET  /api/validators
GET  /api/network/peers
POST /api/transactions
```

### Rules

- All responses are JSON
- GET endpoints are read-only, serve committed state only
- POST /api/transactions writes to mempool, returns 202 on success
- Proper HTTP status codes (200, 202, 400, 404, 500, 503)
- Consistent error response format

### Tests

- Each endpoint returns correct data
- Transaction submission validation
- Error responses for invalid requests
- 404 for missing resources

---

## Phase 9: axiom-network

**Dependencies:** axiom-primitives, tokio

### Responsibilities

- TCP peer connections
- Message framing (length-prefixed messages)
- Peer discovery (static peer list from config)
- Message types: Block proposal, Vote, Transaction gossip

### Message Types

```rust
pub enum NetworkMessage {
    BlockProposal(Block),
    Vote(ValidatorSignature, BlockHash, u64), // sig, block_hash, height
    TransactionGossip(Transaction),
    StatusRequest,
    StatusResponse { height: u64, genesis_hash: StateHash },
}
```

### Rules

- No protocol logic in network crate
- Network failures must not crash the node
- All messages are serialized using the canonical binary format
- Bounded channels for message passing

---

## Phase 10: axiom-node

**Dependencies:** All crates

### Responsibilities

1. Parse and validate configuration (CONFIG.md)
2. Load genesis file
3. Construct genesis state, verify hash
4. Initialize storage
5. If fresh start: store genesis state
6. If existing state: verify genesis hash matches
7. Start API server
8. Start network layer
9. Start consensus loop
10. Handle graceful shutdown (SIGTERM, SIGINT)

### Consensus Loop (Simplified v1)

```
loop {
    if i_am_proposer(current_height) {
        txs = mempool.select(MAX_TRANSACTIONS_PER_BLOCK)
        block = construct_block(state, height, parent_hash, txs, my_key)
        broadcast(block)
    }

    block = receive_proposed_block()
    new_state = apply_block(state, block)?
    sign_vote(block_hash, height)
    broadcast_vote()

    if quorum_reached(block) {
        storage.commit_block(block, new_state)
        state = new_state
        height += 1
    }
}
```

### Tests

- Full integration test: genesis -> produce blocks -> verify state
- Configuration validation
- Genesis hash verification
- Graceful shutdown

---

## Phase 11: Console UI

**Dependencies:** None (pure HTML/CSS/JS, served by API)

### Pages

- Dashboard: current height, validator status, recent blocks
- Block explorer: list blocks, view block details
- Account viewer: look up account balance and nonce
- Validator list: active validators with voting power

### Rules

- No frameworks
- No build step
- No business logic
- Read-only, fetches from API endpoints
- Minimalist, data-first design

---

## Phase 12: Test Key Lock and Hash Computation

After all crates are implemented:

1. Run test_keypair("axiom-test-validator-1") to generate validator-1 keys
2. Run test_keypair("axiom-test-validator-2") to generate validator-2 keys
3. Run test_keypair("axiom-test-validator-3") to generate validator-3 keys
4. Construct genesis state with these keys
5. Compute genesis state hash
6. Compute all test vector state hashes
7. Lock all values in TEST_VECTORS.md
8. Lock genesis hash in GENESIS.md and node source code
9. Run full test suite
10. Tag release

---

## Execution Order Summary

| Phase | Crate           | Depends On                          | Test Before Proceeding |
|-------|-----------------|-------------------------------------|------------------------|
| 1     | primitives      | —                                   | Yes                    |
| 2     | crypto          | primitives                          | Yes                    |
| 3     | state           | primitives                          | Yes                    |
| 4     | execution       | primitives, crypto, state           | Yes (most critical)    |
| 5     | consensus       | primitives, crypto, state, execution| Yes                    |
| 6     | mempool         | primitives, crypto                  | Yes                    |
| 7     | storage         | primitives, state                   | Yes                    |
| 8     | api             | primitives, state, storage, mempool | Yes                    |
| 9     | network         | primitives                          | Yes                    |
| 10    | node            | all                                 | Yes (integration)      |
| 11    | console UI      | —                                   | Manual verification    |
| 12    | key lock        | all                                 | Final validation       |

Every phase must pass all tests before the next phase begins.
No exceptions.
