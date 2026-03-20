# AXIOM Protocol Specification v2 (DRAFT)

**Status:** DRAFT
**Previous Version:** v1 (FROZEN)
**Activation:** Height-based (activation height TBD)

## 1. Scope

Protocol v2 extends v1 with:

- Round-based BFT consensus (replacing v1 single-proposer model)
- Staking mechanism (new economic primitive)
- Slashing conditions (safety enforcement)
- Validator set changes (dynamic validator management)

All v1 rules remain in effect unless explicitly superseded by this document.
v2 rules apply only after the activation height.

## 2. Activation Mechanism

### 2.1 Activation Height

- v2 activates at a specific block height defined in the genesis or upgrade configuration
- Blocks below the activation height follow v1 rules exclusively
- The block at the activation height is the first block processed under v2 rules
- Activation height is a protocol constant, not a configuration parameter

### 2.2 Block Header Extension

Starting at the activation height, blocks must include:

- `protocol_version`: 2 (u64)
- `round`: Round number within the height (u64, starting at 0)

Nodes running v1-only must reject blocks with protocol_version = 2.
Nodes running v2 must reject blocks below activation height that claim protocol_version = 2.

## 3. Staking

### 3.1 Staking as a New Economic Primitive

v1 has no staking. v2 introduces staking as the mechanism by which validators post collateral to participate in consensus.

### 3.2 Staking State Extensions

The protocol state is extended with:

- `stakes`: Mapping from validator ID to staked amount (u64)
- `minimum_stake`: Minimum AXM required to be an active validator (u64, defined at activation)
- `unbonding_period`: Number of blocks a validator must wait after unstaking before funds are released (u64)
- `unbonding_queue`: List of pending unbonding entries (validator_id, amount, release_height)

### 3.3 Staking Transaction Types

#### 3.3.1 Stake

| Field     | Value                        |
|-----------|------------------------------|
| Type      | stake                        |
| Sender    | Account ID (must be a validator's associated account) |
| Amount    | AXM to stake (u64, > 0)     |
| Nonce     | Sender nonce                 |
| Signature | Ed25519                      |

Validation:

- Sender account exists
- Sender balance >= amount
- Nonce matches
- Signature valid
- Sender is a registered validator's associated account

Execution:

- Sender balance decreased by amount
- Validator's staked amount increased by amount
- If staked amount >= minimum_stake and validator was inactive, validator becomes active

#### 3.3.2 Unstake

| Field     | Value                        |
|-----------|------------------------------|
| Type      | unstake                      |
| Sender    | Account ID (validator's associated account) |
| Amount    | AXM to unstake (u64, > 0)   |
| Nonce     | Sender nonce                 |
| Signature | Ed25519                      |

Validation:

- Sender account exists
- Validator's staked amount >= amount
- Nonce matches
- Signature valid

Execution:

- Validator's staked amount decreased by amount
- Entry added to unbonding_queue: (validator_id, amount, current_height + unbonding_period)
- If staked amount < minimum_stake, validator becomes inactive
- Inactive validators are excluded from proposer selection and voting

#### 3.3.3 Unbonding Release

At each block, before transaction processing:

- Scan unbonding_queue for entries where release_height <= current_height
- For each matured entry: credit amount to the validator's associated account
- Remove matured entries from the queue

This is automatic and does not require a transaction.

### 3.4 Staking Invariants

- Sum of all balances + sum of all staked amounts + sum of all unbonding amounts = total_supply
- No staked amount may be negative
- No unbonding amount may be negative
- Validators below minimum_stake are inactive and excluded from consensus

### 3.5 Migration from v1

At the activation height:

- All existing v1 validators receive an initial stake equal to their voting power (or a protocol-defined migration amount)
- minimum_stake is set to the protocol-defined constant
- unbonding_period is set to the protocol-defined constant
- The migration is applied as part of the activation block's state transition

## 4. Consensus: Round-Based BFT

### 4.1 Overview

v2 replaces v1's single-proposer model with a round-based BFT mechanism consisting of four phases:

1. Proposal
2. Prevote
3. Precommit
4. Commit

### 4.2 Proposer Selection

- Proposers are selected deterministically from the active validator set
- Mechanism: weighted round-robin based on staked amount
- Proposer is a function of (height, round)
- Selection algorithm: validators sorted by ID, weighted by stake, cycled deterministically

The exact weighted round-robin algorithm:

1. Compute cumulative stake weights for active validators (sorted by ID)
2. Seed = SHA-256(height || round) interpreted as u64 (first 8 bytes, big-endian)
3. Index = seed % total_active_stake
4. Proposer = validator whose cumulative weight range contains the index

### 4.3 Proposal Phase

- Leader for (height, round) constructs a block
- Block includes round number and proposer ID
- Block is broadcast as a Proposal message
- Only the designated proposer may propose for a given (height, round)
- If no proposal is received within the proposal timeout, validators proceed to prevote nil

### 4.4 Prevote Phase

- Validators receive the Proposal
- Validate: correct proposer, valid transactions, valid state transition
- If valid: broadcast Prevote(block_hash)
- If invalid or no proposal received: broadcast Prevote(nil)

Locking:

- If a validator observes a polka (>2/3 prevotes for a block_hash), it locks on that block
- A locked validator must prevote for its locked block in subsequent rounds at the same height
- A lock is released only if the validator observes a polka for a different block (re-lock) or for nil

### 4.5 Precommit Phase

- Validators wait for >2/3 voting power worth of Prevote messages
- If >2/3 Prevote(block_hash): broadcast Precommit(block_hash)
- If >2/3 Prevote(nil): broadcast Precommit(nil)
- If timeout expires without >2/3 agreement: broadcast Precommit(nil)

### 4.6 Commit Phase

- Validators wait for >2/3 voting power worth of Precommit messages
- If >2/3 Precommit(block_hash):
  - Block is finalized
  - Apply block to state
  - Increment height, reset round to 0
- If >2/3 Precommit(nil) or timeout:
  - Increment round
  - Return to Proposal phase with new proposer

### 4.7 Voting Rules

- One Prevote per (height, round) per validator
- One Precommit per (height, round) per validator
- Votes are Ed25519 signed
- Vote message includes: block_hash (or nil indicator), height, round, phase

### 4.8 Timeout Rules

- Proposal timeout: T_propose (implementation-defined, not consensus-critical)
- Prevote timeout: T_prevote (triggered after receiving >2/3 prevotes but no quorum for a single value)
- Precommit timeout: T_precommit (triggered after receiving >2/3 precommits but no quorum for a single value)
- Timeouts increase with each round to ensure eventual progress

Timeouts are non-normative. They affect liveness but not safety.

## 5. Locking Rules (Safety-Critical)

### 5.1 Lock Definition

A validator is locked on a block B at round R if:

- The validator observed >2/3 prevotes for B in round R
- No subsequent polka for a different block or nil has been observed at a later round

### 5.2 Lock Enforcement

A locked validator:

- Must prevote for its locked block in all subsequent rounds at the same height
- Must not prevote for any other block
- May prevote nil only if it has not locked on any block

### 5.3 Lock Release

A lock is released when:

- A polka for nil is observed at a round > locked round (validator may then prevote freely)
- A polka for a different block is observed at a round > locked round (validator re-locks on new block)

### 5.4 Lock Persistence

- Locks must survive node restarts within the same height
- Lock state is persisted before any prevote is broadcast
- A restarting node must recover its lock state before participating in consensus

### 5.5 Safety Proof Sketch

Given these locking rules:

- If block B is committed at height H, then >2/3 validators precommitted B
- Those validators must have observed >2/3 prevotes for B (polka)
- Any subsequent round at height H: locked validators will prevote B, preventing quorum for any other block
- Therefore no other block can achieve a polka or commit at height H

## 6. Slashing

### 6.1 Slashable Offenses

1. **Double Proposing**: Proposing two different blocks for the same (height, round)
2. **Double Voting**: Casting two different Prevotes or two different Precommits for the same (height, round)

### 6.2 Evidence

Slashing requires cryptographic evidence:

- Two signed proposals for the same (height, round) with different block hashes
- Two signed votes for the same (height, round, phase) with different values

Evidence must include the conflicting signed messages.

### 6.3 Evidence Submission

- Evidence is submitted as a special transaction type: SlashEvidence
- Evidence transactions are validated during block processing
- Evidence can be submitted by any account
- Evidence has no expiration in v2 (may be bounded in future versions)

### 6.4 Penalties

On confirmed slashing:

- Slashed validator's staked amount is reduced by slash_percentage (protocol constant, e.g., 10%)
- Slashed amount is burned (removed from total_supply)
- Slashed validator is set to inactive (jailed)
- Jailed validators cannot participate in consensus
- Unjailing mechanism: TBD (out of scope for initial v2)

### 6.5 Slashing Invariants

- Slashing must be deterministic (same evidence produces same penalty on all nodes)
- Slashing is applied during block execution, inside apply_block
- Burned amounts are subtracted from total_supply
- Economic invariants must hold after slashing

## 7. Validator Set Changes

### 7.1 Epoch Increment

In v2, epoch increments whenever the active validator set changes:

- A validator's stake crosses the minimum_stake threshold (activation or deactivation)
- A validator is jailed via slashing

### 7.2 Validator Set Update Timing

- Validator set changes take effect at the start of the next epoch (next block after the change)
- The block that causes the change is processed with the old validator set
- The next block uses the new validator set

## 8. Extended Transaction Types

v2 adds the following transaction types to the v1 Transfer type:

| Type          | Description                                    |
|---------------|------------------------------------------------|
| Transfer      | Same as v1                                     |
| Stake         | Stake AXM to become/remain active validator    |
| Unstake       | Begin unbonding staked AXM                     |
| SlashEvidence | Submit evidence of a slashable offense         |

Transaction type is encoded as a u8 prefix in the canonical transaction serialization:

- 0: Transfer
- 1: Stake
- 2: Unstake
- 3: SlashEvidence

## 9. Finality and Quorum

- Quorum: strictly greater than 2/3 of total active voting power
- In v2, voting power is proportional to staked amount
- Finality: instant finality once a block is committed
- No forks allowed
- A committed block is immutable

## 10. Non-Goals for v2

- Delegation (staking by proxy)
- Governance voting
- Smart contracts
- Fee markets
- Light client protocol
- Cross-chain communication

## 11. Compliance

An implementation is v2-compliant if and only if:

- All v1 rules are enforced below the activation height
- All v2 rules are enforced at and above the activation height
- Staking invariants hold at all times
- Locking rules are correctly implemented and persisted
- Slashing is deterministic and evidence-based
- Epoch transitions are correctly triggered
- v2 test vectors pass (to be defined in TEST_VECTORS_v2.md)
