# AXIOM Protocol Specification

**Protocol Version:** 1
**Status:** NORMATIVE (FROZEN)

::: warning Protocol Freeze
Protocol Version 1 is **FROZEN**.
No normative changes are permitted.
Any behavioral change requires a protocol version increment.
:::

## 1. Purpose and Scope

This document defines the AXIOM protocol.
It specifies the data structures, cryptographic primitives, serialization rules, state transition rules, and consensus guarantees that together define a valid AXIOM implementation.

**This document is normative.**
Any implementation that deviates from the rules defined here is not AXIOM-compliant.
User interfaces, tooling, networking libraries, and implementation details are explicitly out of scope unless referenced.

**Document Precedence:**
In case of conflict between specification documents, the following precedence applies (highest to lowest):

1. PROTOCOL.md (this document)
2. TEST_VECTORS.md
3. IMPLEMENTATION.md
4. All other specification documents

## 2. Fundamental Definitions

- **Node**: A software process that maintains AXIOM state and participates in block processing.
- **Validator**: A node that is authorized to propose and vote on blocks.
- **Epoch**: A monotonically increasing logical period tied to validator set changes.
- **Block**: An immutable data structure containing transactions and consensus metadata.
- **Transaction**: A signed instruction that requests a state change.
- **State**: The complete, deterministic representation of AXIOM at a given block height.
- **Commit**: The act of irreversibly accepting a block as the canonical block for a given height.
- **Finality**: The guarantee that a committed block cannot be reverted.
- **Mempool**: A non-consensus-critical holding area for unconfirmed transactions.

## 3. Determinism Guarantees

AXIOM is a fully deterministic state machine.
Given the same initial state and the same ordered sequence of blocks, all compliant nodes must produce the same final state.

The following inputs must not affect state transitions:

- Wall-clock time
- Randomness
- Network message order
- Hardware characteristics

Only the explicit contents of blocks and transactions may affect state.

## 4. Cryptographic Primitives

### 4.1 Hash Function

- Algorithm: SHA-256
- Output: 32 bytes
- Representation: 64-character lowercase hexadecimal string
- No `0x` prefix in protocol-level representations

All protocol hashes (block hash, state hash, genesis hash, transaction hash) use SHA-256.

### 4.2 Signature Scheme

- Algorithm: Ed25519 (RFC 8032)
- Private key: 32 bytes
- Public key: 32 bytes (Ed25519 public key)
- Signature: 64 bytes

Signatures are computed over the canonical serialization of the signed data.

### 4.3 Identity Model

- **Account ID**: 64-character lowercase hex encoding of the Ed25519 public key
- **Validator ID**: Same as the account ID of the validator's associated account

There is no separate address derivation. The public key is the identity.

### 4.4 Cryptographic Rules

- All signature verification must use constant-time comparison
- No custom cryptographic implementations are permitted
- Only audited, published Ed25519 and SHA-256 libraries may be used
- Key generation is out of scope for the protocol (keys are external inputs)

## 5. Canonical Serialization

### 5.1 Genesis Serialization

Genesis state is serialized as deterministic JSON:

- Keys sorted lexicographically (ascending, Unicode code point order)
- No whitespace between tokens
- No trailing commas
- UTF-8 encoding
- Integers as JSON numbers (no quotes)
- Strings as JSON strings (double-quoted)
- No null values
- No floating-point values

This format is used exclusively for the genesis file and genesis hash computation.

### 5.2 State Hash Serialization

State hashes are computed over a deterministic binary representation:

- Fields are serialized in a fixed, protocol-defined order
- Integers are encoded as unsigned 64-bit big-endian
- Strings are encoded as length-prefixed UTF-8 (4-byte big-endian length prefix, followed by UTF-8 bytes)
- Lists are encoded as count-prefixed sequences (4-byte big-endian count, followed by elements in order)
- Maps are encoded as count-prefixed sorted sequences (sorted by key, lexicographic byte order)

### 5.3 State Hash Field Order

The canonical state is serialized in the following field order:

1. `total_supply` (u64, big-endian)
2. `block_reward` (u64, big-endian)
3. `accounts` (map, sorted by account ID)
   - Each entry: account_id (length-prefixed string), balance (u64), nonce (u64)
4. `validators` (map, sorted by validator ID)
   - Each entry: validator_id (length-prefixed string), voting_power (u64), account_id (length-prefixed string), active (u8: 1 = true, 0 = false)

The state hash is: `SHA-256(canonical_binary_state)`

### 5.4 Block Hash Computation

Block hash is computed over the following fields in order:

1. `parent_hash` (32 bytes, raw)
2. `height` (u64, big-endian)
3. `epoch` (u64, big-endian)
4. `proposer_id` (length-prefixed string)
5. `transactions` (count-prefixed, each transaction as its canonical binary form)
6. `state_hash` (32 bytes, raw — the resulting state hash after applying this block)

Block hash: `SHA-256(canonical_binary_block_fields)`

### 5.5 Transaction Serialization

Transaction canonical form (for hashing and signing):

1. `sender` (length-prefixed string)
2. `recipient` (length-prefixed string)
3. `amount` (u64, big-endian)
4. `nonce` (u64, big-endian)

Transaction hash: `SHA-256(canonical_binary_transaction_fields)`
Signature: `Ed25519_Sign(private_key, canonical_binary_transaction_fields)`

The signature is not included in the signed data.

## 6. Data Structures

### 6.1 Block

A block consists of the following fields:

- `parent_hash`: SHA-256 hash of the immediately preceding block (hex string)
- `height`: Sequential block number (u64, starting at 0 for genesis)
- `epoch`: Current epoch number (u64)
- `proposer_id`: Identifier of the proposing validator (hex string)
- `transactions`: Ordered list of transactions
- `signatures`: List of validator signatures approving this block
- `state_hash`: Resulting state hash after block application (hex string)

Blocks are immutable once committed.

### 6.2 Transaction

A transaction consists of:

- `sender`: Account ID of the transaction originator (hex string)
- `recipient`: Account ID of the recipient (hex string)
- `amount`: Transfer amount in AXM (u64)
- `nonce`: Monotonically increasing counter preventing replay (u64)
- `signature`: Ed25519 signature over the canonical transaction bytes (hex string, 128 characters)

Transactions are processed strictly in the order they appear in a block.

### 6.3 State

State is a deterministic data structure containing:

- Account balances and nonces
- Validator registry
- Economic parameters (total_supply, block_reward)

State is not a Merkle tree; the state hash is a canonical hash over the full state.

State does not contain:

- Cached computations
- UI-related data
- Network metadata
- Mempool contents

State is updated only via block application.

### 6.4 Validator Registry

The validator registry is part of state and defines:

- Validator identifiers (account IDs)
- Voting power (u64)
- Associated account ID
- Active status (boolean)

Only validators present in the registry with active status may participate in consensus.

### 6.5 Block Limits

- Maximum transactions per block: 1000
- Maximum serialized block size: 1,048,576 bytes (1 MB)

Blocks exceeding either limit are invalid and must be rejected.

## 7. State Transition Function

### 7.1 apply_block Definition

State transitions are defined by the function:

```
apply_block(previous_state, block) -> new_state | error
```

This function:

- Is deterministic
- Has no side effects
- Performs no I/O
- Does not access external data

If `apply_block` returns an error, the block is invalid and must not be committed.

### 7.2 apply_block Procedure

The following steps are executed in exact order:

1. Validate block height equals previous height + 1
2. Validate parent_hash matches hash of previous block
3. Validate epoch matches expected epoch (see Section 8.2)
4. Validate proposer_id matches expected proposer (see Section 8.3)
5. Validate block does not exceed block limits (Section 6.5)
6. Validate quorum (see Section 8.5)
7. For each transaction in order:
   a. Validate transaction (see Section 7.3)
   b. Apply transaction (see Section 7.4)
8. Apply block reward (see Section 9.7)
9. Verify economic invariants (see Section 9.8)
10. Compute and verify state_hash

If any step fails, the entire block is rejected. No partial state updates occur.

### 7.3 Transaction Validation Rules

A transaction is valid if and only if:

- Sender account exists
- Sender nonce equals the transaction nonce
- Sender balance >= amount
- amount > 0
- Ed25519 signature is valid over the canonical transaction bytes, verified against sender's public key
- Recipient account exists OR recipient account will be auto-created (see Section 9.6)

Failure of any rule invalidates the transaction and the containing block.

### 7.4 Transaction Execution Rules

When a valid transfer is applied:

1. If recipient account does not exist, create it with balance 0 and nonce 0
2. Sender balance is decreased by amount
3. Recipient balance is increased by amount
4. Sender nonce is incremented by 1

Execution is atomic. Steps 1-4 are applied as a unit.

### 7.5 State Update Rules

- State updates are atomic
- Either the entire block is applied, or no changes occur
- State rollback after commit is forbidden

## 8. Consensus Algorithm

### 8.1 Validator Set

- The validator set is explicitly defined in state
- A validator's voting power is deterministic
- Quorum is defined as strictly greater than two-thirds of total voting power
- In v1, the validator set is fixed at genesis and does not change

### 8.2 Epoch Rules

- Epoch is a monotonically increasing counter
- Epoch increments when the active validator set changes
- In Protocol v1, the validator set is fixed. Epoch remains 0 for all v1 blocks
- All blocks in v1 must have epoch = 0

### 8.3 Proposer Selection

Proposers are selected deterministically:

1. Collect all active validators
2. Sort by validator ID (lexicographic, ascending)
3. Primary Proposer index = `height % validator_count`
4. Primary Proposer = `sorted_validators[primary_index]`

**Fallback Mechanism:**

To ensure liveness, a fallback proposer is accepted if the primary proposer is offline.
A validator is a valid fallback proposer if:
- Fallback index = `(height + attempt) % validator_count`
- Fallback Proposer = `sorted_validators[fallback_index]`

**Fallback:** Deterministic proposer rotation after timeout, without relaxing proposer validity or quorum rules.

> **Note:** The `attempt` counter is ephemeral consensus-local state and **MUST NOT** be persisted, hashed, or exposed to execution.

This function is deterministic and requires no external input.

### 8.4 Voting

- Validators may vote for at most one block per height
- Votes are Ed25519 signatures over the block hash bound to the block height
- Vote message: `SHA-256(block_hash || height as u64 big-endian)`
- Votes for invalid blocks are ignored
- Votes from non-active validators are ignored

### 8.5 Commit Rules

A block is committed if and only if:

- Valid signatures representing strictly greater than two-thirds of total voting power are collected
- The block passes all state transition checks
- Once committed, a block becomes the canonical block for its height

**Quorum Calculation Examples:**

| Total Power | Required (> 2/3) | Formula (floor(2N/3) + 1) |
|-------------|------------------|---------------------------|
| 1           | 1                | floor(2/3) + 1 = 1        |
| 3           | 3                | floor(6/3) + 1 = 3        |
| 4           | 3                | floor(8/3) + 1 = 3        |
| 40          | 27               | floor(80/3) + 1 = 27      |
| 100         | 67               | floor(200/3) + 1 = 67     |

> **Reference genesis (4 validators, power 10 each):** Total power = 40. Quorum requires collected > 26.67, minimum 27. With 3 of 4 validators signing (power 30), quorum is satisfied (30 > 26.67). The network tolerates 1 faulty or offline validator.

> **Note:** With 3 validators of equal power, quorum requires unanimous agreement (0 fault tolerance). The canonical genesis uses 4 validators specifically to provide 1-fault-tolerant BFT.

### 8.6 Finality

AXIOM provides immediate finality once quorum is reached.
Once a block is committed:

- It cannot be reverted
- No alternative block at the same height may ever be committed

## 9. Economics

### 9.1 Native Coin

- Name: AXIOM
- Symbol: AXM
- Decimals: 0 (integer-only)
- All balances are unsigned 64-bit integers

### 9.2 Economic State

The protocol state includes the following economic fields:

- `accounts`: Mapping from account ID to (balance: u64, nonce: u64)
- `total_supply`: Total AXM currently in existence (u64)
- `block_reward`: Fixed reward amount per committed block (u64)

No other economic fields are permitted.

### 9.3 Genesis Economic Rules

At genesis:

- total_supply is initialized to GENESIS_SUPPLY
- All AXM are allocated to explicitly defined accounts
- All account nonces are initialized to 0
- Sum of all initial balances must equal GENESIS_SUPPLY

Implicit balances are forbidden.

### 9.4 Transaction Type: Transfer

A transfer transaction includes:

- sender (account ID)
- recipient (account ID)
- amount (u64)
- nonce (u64)
- signature (Ed25519)

Transfer is the only transaction type in Protocol v1.

### 9.5 Transfer Validation Rules

See Section 7.3.

### 9.6 Account Auto-Creation

When a transfer specifies a recipient that does not exist in state:

1. A new account is created with the recipient's ID
2. Initial balance: 0
3. Initial nonce: 0
4. The transfer then proceeds normally

Auto-created accounts are indistinguishable from genesis accounts.
This is the only mechanism for creating new accounts in Protocol v1.

### 9.7 Block Reward Rules

For every committed block:

1. A fixed reward of `block_reward` AXM is issued
2. The reward is credited to the proposer's associated account
3. The reward is applied after all transactions in the block
4. total_supply is increased by `block_reward`

If the proposer's account does not exist, it is created with balance 0 and nonce 0 before the reward is applied.

### 9.8 Economic Invariants

The following invariants must hold at all times:

- No account balance may be negative
- No overflow or underflow is permitted in any arithmetic operation
- Sum of all account balances equals total_supply
- total_supply increases only via block rewards
- total_supply must not exceed u64::MAX

Violation of any invariant constitutes a protocol failure.

### 9.9 Determinism Guarantee

Given the same initial state and block sequence, all compliant nodes must produce identical:

- Account balances
- Account nonces
- Total supply
- State hashes

## 10. Transaction Ingress

### 10.1 Transaction Submission

Nodes accept transaction submissions via a dedicated network endpoint.
Transaction submission is a request to include a transaction in a future block.
Submission does not guarantee inclusion.

### 10.2 Mempool

Nodes maintain a mempool: a non-consensus-critical holding area for unconfirmed transactions.

- The mempool is local to each node
- Mempool contents are not part of consensus state
- Mempool ordering and eviction policies are implementation-defined
- Proposers select transactions from their local mempool when constructing blocks

### 10.3 Mempool Validation

On submission, transactions must pass basic validation:

- Signature must be valid
- Nonce must be >= sender's current nonce
- Sender must have sufficient balance (at current state)

Invalid transactions are rejected at submission time. Transactions may become invalid by the time they are included in a block (due to state changes from other blocks).

### 10.4 Non-Normative Status

The mempool and transaction submission mechanism are non-normative.
Implementations may vary in:

- Mempool size limits
- Eviction policies
- Transaction propagation between nodes
- Submission API details

These do not affect consensus correctness.

## 11. Safety and Liveness Guarantees

### 11.1 Safety Invariants

The following invariants must always hold:

- No two different blocks may be committed at the same height
- All committed blocks form a single linear chain
- All compliant nodes agree on the committed block sequence
- Violation of any invariant constitutes a protocol failure

### 11.2 Liveness Assumptions

The protocol guarantees progress if:

- At least two-thirds of validators are online and responsive
- Network communication eventually delivers messages
- AXIOM makes no liveness guarantees under arbitrary network partitions

## 12. Persistence and Replay

- All committed blocks must be persisted
- State snapshots may be persisted for efficiency
- Replaying the full block sequence from genesis must always reproduce the same final state
- Persistence mechanisms must not alter protocol semantics

> **Warning:** Persistence ordering, durability, and atomicity **MUST NOT** affect protocol behavior. The storage engine is a non-normative implementation detail.

## 13. Network Model (Non-Normative)

The network layer:

- Is not trusted
- Does not guarantee message order
- Does not guarantee delivery time

Consensus correctness must not depend on network behavior.

## 14. Error Handling

- Invalid transactions invalidate the containing block
- Invalid blocks are discarded
- Invalid votes are ignored
- Nodes must not crash or enter undefined states due to invalid input

## 15. Versioning and Compatibility

- The protocol version is explicitly defined as an integer
- Protocol v1 is the current version
- Breaking changes require a protocol version increment
- Nodes must reject blocks using unsupported protocol versions

## 16. Non-Goals & Prohibited Extensions

AXIOM Protocol v1 explicitly does not define:

- Governance mechanisms
- Slashing rules
- Token economics beyond basic transfers and rewards
- Smart contracts
- Staking or delegation
- Fee markets
- Token burning

**Prohibited Extensions:**

Implementations **MUST NOT** add:
- Alternative proposer logic
- Optional quorum rules
- Configurable safety thresholds

Any such modification constitutes a hard fork and is non-compliant.

These concerns are outside the scope of Protocol v1.

## 17. Compliance

An implementation is AXIOM-compliant if and only if:

- It follows all rules defined in this document
- It preserves determinism and safety invariants
- It rejects all invalid blocks and transactions
- It passes all test vectors defined in TEST_VECTORS.md

Compliance may be verified through test vectors and state hash comparisons.
