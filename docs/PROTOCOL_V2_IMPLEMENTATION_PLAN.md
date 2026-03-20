# AXIOM Protocol v2.0 — Implementation Plan

**Date:** 2026-02-10
**Status:** PLANNING
**Prerequisite:** Protocol v1.0 is FROZEN and fully backed up at `backups/v1.0.0/`

## Overview

Protocol v2 extends v1 with round-based BFT consensus, staking, slashing, and dynamic validator sets. Implementation is organized into 6 phases, each keeping the system buildable, testable, and v1-compatible below the activation height.

## Dependency Graph

```
Phase 1: Types & Versioning (primitives)
    │
    ├──► Phase 2: State & Storage Extensions (state, storage)
    │        │
    │        ├──► Phase 3: Execution & Transaction Types (execution, mempool)
    │        │        │
    │        │        ├──► Phase 4: Round-Based Consensus (consensus, network, node)
    │        │        │        │
    │        │        │        ├──► Phase 5: Slashing & Validator Set Changes (execution, consensus, state)
    │        │        │        │
    │        │        │        └──► Phase 6: API, Dashboard & Integration Testing (api, web, node)
    │        │        │
    │        │        └──► Phase 3b: V1 Migration Logic (execution, state)
```

---

## Phase 1: Types & Protocol Versioning

**Goal:** Add all v2 data structures and constants with backward-compatible defaults. All existing v1 tests must continue to pass.

**Crates modified:** `primitives`

### Tasks

1.1. **Protocol version constant and activation height**
   - Add `PROTOCOL_VERSION_V2: u64 = 2` constant
   - Add `V2_ACTIVATION_HEIGHT: u64` constant (value TBD, but defined in primitives)
   - Add `PROTOCOL_VERSION_V1: u64 = 1` constant for clarity

1.2. **Block header extension**
   - Add `protocol_version: u64` field to `Block` struct (default = 1 for v1 blocks)
   - Add `round: u64` field to `Block` struct (default = 0 for v1 blocks)
   - Update canonical serialization to include new fields
   - Gate: blocks below activation height must have protocol_version = 1

1.3. **Transaction type enum**
   - Create `TransactionType` enum: `Transfer = 0`, `Stake = 1`, `Unstake = 2`, `SlashEvidence = 3`
   - Add `tx_type: TransactionType` field to `Transaction` struct
   - Default to `Transfer` for backward compatibility
   - Add u8 prefix to canonical transaction serialization

1.4. **Vote and evidence types**
   - `VotePhase` enum: `Prevote`, `Precommit`
   - `Vote` struct: `height`, `round`, `phase`, `block_hash` (Option), `validator_id`, `signature`
   - `Proposal` struct: `height`, `round`, `block`, `proposer_id`, `signature`
   - `Evidence` enum: `DoublePropose { proposal_a, proposal_b }`, `DoubleVote { vote_a, vote_b }`

1.5. **Staking constants**
   - `MINIMUM_STAKE: u64` (protocol constant)
   - `UNBONDING_PERIOD: u64` (number of blocks)
   - `SLASH_PERCENTAGE: u64` (e.g., 10, meaning 10%)

**Tests:** Serialization round-trip tests for new types. Verify existing v1 block/tx hashes are unchanged.

**Risk:** LOW — additive changes only, no behavior changes.

---

## Phase 2: State & Storage Extensions

**Goal:** Extend state representation and SQLite schema to support staking, unbonding, and lock persistence.

**Crates modified:** `state`, `storage`

### Tasks

2.1. **Staking state extensions** (`state`)
   - Add `stakes: BTreeMap<ValidatorId, u64>` to state
   - Add `minimum_stake: u64` to state
   - Add `unbonding_period: u64` to state
   - Add `UnbondingEntry` struct: `validator_id`, `amount`, `release_height`
   - Add `unbonding_queue: Vec<UnbondingEntry>` to state
   - Add `active` flag to validator info (derived from stake >= minimum_stake)

2.2. **State hash computation update**
   - Include staking fields in canonical state hash computation
   - Gate: below activation height, state hash computed using v1 rules only
   - Above activation height, include stakes/unbonding in hash

2.3. **Lock state struct** (`state` or `consensus`)
   - `LockState`: `height`, `round`, `block_hash`
   - Must be serializable for persistence

2.4. **Storage schema migration** (`storage`)
   - New SQLite tables: `stakes`, `unbonding_queue`, `consensus_locks`, `votes`, `evidence`
   - Schema version tracking (v1 = 1, v2 = 2)
   - Migration function: add new tables without altering existing ones

2.5. **Staking invariant checks** (`state`)
   - Function: `verify_staking_invariants(state) -> Result<()>`
   - Checks: balances + stakes + unbonding = total_supply
   - No negative values
   - All active validators have stake >= minimum_stake

**Tests:** State hash stability tests (v1 hashes unchanged). Staking invariant unit tests. Storage migration tests.

**Risk:** MEDIUM — state hash computation changes must be gated correctly to avoid breaking v1 consensus.

---

## Phase 3: Execution & Transaction Types

**Goal:** Implement new transaction validation and execution, unbonding release, and activation gating.

**Crates modified:** `execution`, `mempool`

### Tasks

3.1. **Activation gating in execution**
   - `apply_block` checks block height against activation height
   - Below activation: enforce v1 rules only (reject v2 tx types, reject protocol_version = 2)
   - At/above activation: enforce v2 rules

3.2. **Stake transaction execution**
   - Validate: sender exists, balance >= amount, nonce matches, signature valid, sender is validator account
   - Execute: decrease balance, increase staked amount
   - If stake crosses minimum_stake upward: mark validator active

3.3. **Unstake transaction execution**
   - Validate: sender exists, staked amount >= amount, nonce matches, signature valid
   - Execute: decrease staked amount, add to unbonding queue with release_height
   - If stake drops below minimum_stake: mark validator inactive

3.4. **Unbonding release (pre-block processing)**
   - At start of each block (v2+ only): scan unbonding queue
   - Release matured entries (release_height <= current_height)
   - Credit amounts to validator accounts
   - Remove from queue
   - All arithmetic uses checked operations

3.5. **Mempool updates**
   - Accept new transaction types
   - Basic validation before pool insertion (tx type field, format checks)

3.6. **V1-to-V2 migration** (at activation height)
   - At activation block: initialize staking state
   - Set initial stakes for existing validators (from voting power or migration constant)
   - Set minimum_stake and unbonding_period
   - Apply as part of activation block state transition
   - Verify invariants after migration

**Tests:** Unit tests for each tx type (valid/invalid). Migration test. Unbonding release tests. Activation boundary tests.

**Risk:** HIGH — migration is a one-shot operation that must be deterministic across all nodes. Arithmetic must use checked operations throughout.

---

## Phase 4: Round-Based BFT Consensus

**Goal:** Replace v1 single-proposer consensus with 4-phase round-based BFT.

**Crates modified:** `consensus`, `network`, `node`

### Tasks

4.1. **Weighted round-robin proposer selection**
   - Input: active validators sorted by ID, staked amounts, height, round
   - Algorithm: cumulative stake weights, SHA-256(height || round) → u64, index into cumulative weights
   - Must be deterministic across all implementations

4.2. **Consensus state machine**
   - States: `Proposal`, `Prevote`, `Precommit`, `Commit`
   - Per-height/round tracking: received proposals, prevotes, precommits
   - Vote aggregation by stake weight (not count)
   - Quorum: strictly > 2/3 of total active voting power

4.3. **Locking rules implementation**
   - Lock on polka (>2/3 prevotes for a block)
   - Lock enforcement: must prevote locked block in subsequent rounds
   - Lock release: on nil polka or different-block polka at higher round
   - Lock persistence: save to storage before broadcasting prevote

4.4. **Network message types**
   - Add message types: `ProposalMsg`, `PrevoteMsg`, `PrecommitMsg`, `EvidenceMsg`
   - Round and phase fields in all consensus messages
   - Bincode serialization for all new message types

4.5. **Node consensus loop update**
   - Replace v1 consensus loop with round-based state machine
   - Timeout management (proposal, prevote, precommit timeouts)
   - Timeouts increase with each round
   - Fallback to v1 loop below activation height

4.6. **Block validation update** (`consensus`)
   - Validate protocol_version matches expected for height
   - Validate round field
   - Validate proposer matches weighted round-robin for (height, round)
   - Validate vote signatures and weights

**Tests:** Proposer selection determinism tests. Consensus state machine tests (happy path, timeouts, re-elections). Lock persistence tests. Network message serialization tests.

**Risk:** HIGHEST — consensus correctness is safety-critical. Locking rules must be implemented exactly per spec. Lock persistence across restarts is essential for safety.

---

## Phase 5: Slashing & Validator Set Changes

**Goal:** Implement slashing evidence detection, penalties, and dynamic validator set management.

**Crates modified:** `execution`, `consensus`, `state`

### Tasks

5.1. **Evidence validation**
   - Validate DoublePropose: two signed proposals for same (height, round) with different block hashes
   - Validate DoubleVote: two signed votes for same (height, round, phase) with different values
   - Verify both signatures are valid and from the same validator

5.2. **SlashEvidence transaction execution**
   - Accept evidence from any account
   - Validate evidence cryptographically
   - Apply penalty: reduce staked amount by slash_percentage
   - Burn slashed amount (subtract from total_supply)
   - Jail validator (set inactive, excluded from consensus)

5.3. **Epoch management**
   - Increment epoch when active validator set changes
   - Changes triggered by: stake crossing minimum, validator jailing
   - New validator set takes effect at start of next epoch (next block)
   - Block that causes change uses old validator set

5.4. **Evidence detection in consensus**
   - During consensus: detect conflicting proposals/votes
   - Automatically generate evidence transactions
   - Gossip evidence to network

**Tests:** Evidence validation tests. Slash penalty calculation tests. Epoch transition tests. Economic invariant tests after slashing.

**Risk:** HIGH — slashing must be deterministic. Burn mechanics affect total_supply invariant. Epoch transitions change the validator set mid-chain.

---

## Phase 6: API, Dashboard & Integration Testing

**Goal:** Expose v2 features via API, update dashboard, run full integration tests.

**Crates modified:** `api`, `web`, `node`

### Tasks

6.1. **New API endpoints**
   - `GET /api/staking` — current staking state (stakes, unbonding queue)
   - `GET /api/validators` — extended with stake, active status, jailed status
   - `POST /api/transactions` — accept new tx types (stake, unstake, evidence)
   - `GET /api/consensus` — current consensus round/phase info
   - `GET /api/status` — include protocol version

6.2. **Dashboard updates** (`web`)
   - Staking info panel on validator page
   - Consensus round/phase indicator
   - Slashing events display
   - Protocol version indicator

6.3. **Local testnet v2 scenario**
   - Update testnet scripts for v2 activation
   - Test full lifecycle: v1 blocks → activation → staking → consensus rounds → slashing

6.4. **Test vectors v2**
   - Create `docs/TEST_VECTORS_v2.md`
   - Activation boundary vectors
   - Staking/unstaking state hash vectors
   - Proposer selection vectors
   - Slashing evidence vectors

6.5. **Integration tests**
   - Multi-node consensus with round-based protocol
   - Validator set change across epoch boundary
   - Node restart with lock recovery

**Risk:** MEDIUM — integration complexity, but individual components are tested by this point.

---

## Risk Summary

| Phase | Risk Level | Primary Concern |
|-------|-----------|-----------------|
| 1     | LOW       | Serialization backward compatibility |
| 2     | MEDIUM    | State hash gating correctness |
| 3     | HIGH      | Migration determinism, checked arithmetic |
| 4     | HIGHEST   | Consensus safety, locking correctness |
| 5     | HIGH      | Slashing determinism, economic invariants |
| 6     | MEDIUM    | Integration complexity |

## Protocol Constants (To Be Decided)

| Constant | Description | Proposed Value |
|----------|-------------|---------------|
| V2_ACTIVATION_HEIGHT | Height at which v2 rules activate | TBD |
| MINIMUM_STAKE | Minimum AXM to be active validator | TBD |
| UNBONDING_PERIOD | Blocks before unbonded stake is released | TBD |
| SLASH_PERCENTAGE | Percentage of stake burned on slashing | 10% |
| T_PROPOSE | Proposal timeout (base) | 5s |
| T_PREVOTE | Prevote timeout (base) | 3s |
| T_PRECOMMIT | Precommit timeout (base) | 3s |

## Estimated Scope

- ~15-20 new source files across crates
- ~3,000-5,000 lines of new Rust code
- ~50-80 new unit tests
- ~10-15 new integration tests
- 1 new documentation file (TEST_VECTORS_v2.md)
