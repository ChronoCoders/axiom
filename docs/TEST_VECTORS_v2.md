# AXIOM v2 Test Vectors

**Protocol Version:** 2  
**Status:** NORMATIVE (LOCKED)

This document defines deterministic test vectors for v2 behavior. Passing them is mandatory for v2 compliance.

Conventions:

- Hashes are 64-character lowercase hex (SHA-256)
- Block hash is computed over canonical block serialization (signatures excluded)
- Transaction signatures are excluded from transaction canonical serialization (v1/v2)

## 1. Key Material and Genesis

These vectors reuse the locked v1 test keys from [TEST_VECTORS.md](file:///c:/axiom/docs/TEST_VECTORS.md) (seeded Ed25519).

Genesis input:

- File: [reference_genesis.json](file:///c:/axiom/docs/reference_genesis.json)
- Note: v2 activation occurs at height `10000`

## 2. Vector Set A — Activation, Epoch Changes, Slashing

This vector set constructs a deterministic 4-block v2 chain segment starting at activation height.

Block `A0` is the activation block (migration occurs because staking state is empty).
Blocks `A1..A3` demonstrate epoch changes (threshold crossing and jailing) and evidence slashing.

### 2.1 Block A0 — Activation / Migration

| Field | Value |
|---|---|
| Height | 10000 |
| Parent Hash | `0000000000000000000000000000000000000000000000000000000000000000` |
| Epoch (header) | 0 |
| Round | 0 |
| Proposer | `97bbcd06ce80fc383b1f03d2bc08344f4de8bb7559cbfb24a5531c44512202b3` |
| Transactions | none |

Expected:

- `STATE_HASH_A0 = 70fe2678ea11c47825a2eba0ecc0c36112e1cac651c69b0f73ff197dc2a73e76`
- `BLOCK_HASH_A0 = c0fe2b939193dc3d059b303f64e8cde8d109bb7a220a2208db2932f747bed341`

### 2.2 Block A1 — Unstake Crossing Below minimum_stake (Epoch Increments)

| Field | Value |
|---|---|
| Height | 10001 |
| Parent Hash | `BLOCK_HASH_A0` |
| Epoch (header) | 0 |
| Round | 0 |
| Proposer | `b306eefdf57d6833438ebbde45b6645af8d1b66c000bf5d4a8e394dee062b9bf` |
| Transactions | `Unstake` amount `1` from `97bbcd06...` |

Expected:

- Epoch after execution: 1
- `STATE_HASH_A1 = 6d2c245f9680e445033da3ca1e578abc24f073615c6574654c9dcb6c6f4341ae`
- `BLOCK_HASH_A1 = 4836e851c95e125214d1150234ab761fafbaaecee43ff80b8af7b09224587b4d`

### 2.3 Block A2 — Stake Crossing Above minimum_stake (Epoch Increments)

| Field | Value |
|---|---|
| Height | 10002 |
| Parent Hash | `BLOCK_HASH_A1` |
| Epoch (header) | 1 |
| Round | 0 |
| Proposer | `b306eefdf57d6833438ebbde45b6645af8d1b66c000bf5d4a8e394dee062b9bf` |
| Transactions | `Stake` amount `1` from `97bbcd06...` |

Expected:

- Epoch after execution: 2
- `STATE_HASH_A2 = 802cde7095913aeed3ace017f47fa38a6dadeb67609fbbe24c753f124ef065ad`
- `BLOCK_HASH_A2 = 3c7cb8167cbcfa1b412f44cb0e975e5d0dfcf708ef87c7def19dfca3a7515ce4`

### 2.4 Block A3 — SlashEvidence (DoubleVote)

Evidence: two signed `Prevote` votes for the same `(height=10001, round=0, phase=Prevote)` by validator `b306eef...`, with different `block_hash` values (`0x11..` vs `0x22..`).

| Field | Value |
|---|---|
| Height | 10003 |
| Parent Hash | `BLOCK_HASH_A2` |
| Epoch (header) | 2 |
| Round | 0 |
| Proposer | `b306eefdf57d6833438ebbde45b6645af8d1b66c000bf5d4a8e394dee062b9bf` |
| Transactions | `SlashEvidence` (DoubleVote) |

Expected:

- Epoch after execution: 3
- `STATE_HASH_A3 = fe30df7f408084c2c41127135868ab785a44852d9742534b3add261fd27cbf35`
- `BLOCK_HASH_A3 = 31d86321bd12d71f5ebd9dd5cea159c842ae9d711a919429f22c160be3e65067`

## 1. Activation Boundary

### 1.1 V1 block at height < activation

- Expectation: v1 canonical hashing and validation rules apply.
- Expectation: v2 transaction types are rejected.

### 1.2 Migration block at activation height

- Expectation: staking state initializes deterministically.
- Expectation: epoch starts at 0.

## 2. Staking

### 2.1 Stake crosses minimum_stake threshold

- Expectation: validator becomes active.
- Expectation: epoch increments by 1 after block execution.

### 2.2 Unstake crosses below minimum_stake threshold

- Expectation: validator becomes inactive.
- Expectation: epoch increments by 1 after block execution.

## 3. Proposer Selection (Weighted)

### 3.1 Deterministic proposer for (height, round)

- Expectation: proposer selection matches `select_proposer_v2`.

## 4. Slashing

### 4.1 DoubleVote evidence slashes and burns

- Expectation: `SLASH_PERCENTAGE` of stake is removed.
- Expectation: burned amount is subtracted from `total_supply`.
- Expectation: validator is jailed and excluded from the active set.
- Expectation: epoch increments by 1 after block execution.

## 5. Consensus Quorum

### 5.1 2/3+ weighted quorum required

- Expectation: commit requires strictly greater than 2/3 of total active stake.
