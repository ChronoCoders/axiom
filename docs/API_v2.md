# AXIOM v2 API Specification

**Protocol Version:** 2  
**Status:** NORMATIVE (for v2 endpoints), COMPATIBLE with v1 endpoints

All v1 endpoints remain available. This document defines v2 additions and v2-extended fields.

## 1. Node Inspection

### 1.1 Status

**GET** `/api/status`

Unchanged shape. `protocol_version` reflects the node implementation’s network protocol version.

### 1.2 Consensus

**GET** `/api/consensus`

Returns lightweight consensus information for the next height.

Response fields:

- `next_height`: Next block height (integer)
- `protocol_version`: Protocol version for `next_height` (integer)
- `lock`: Optional lock state
  - `height`: Lock height
  - `round`: Lock round
  - `block_hash`: Locked block hash (hex string) or null

## 2. Staking

### 2.1 Staking State

**GET** `/api/staking`

Response fields:

- `enabled`: Whether v2 staking is active at the current height (boolean)
- `epoch`: Current staking epoch (integer)
- `minimum_stake`: Minimum stake required for active validator status (integer)
- `unbonding_period`: Unbonding period in blocks (integer)
- `stakes`: Array of `{ validator_id, amount }`
- `unbonding_queue`: Array of `{ validator_id, amount, release_height }`
- `jailed_validators`: Array of validator IDs (hex strings)
- `processed_evidence_count`: Count of processed evidence items (integer)

## 3. Validators

### 3.1 List Validators

**GET** `/api/validators`

v2 extends each validator entry with:

- `stake_amount`: Current staked amount for the validator (integer, optional pre-activation)
- `jailed`: Whether the validator is jailed (boolean, optional pre-activation)

