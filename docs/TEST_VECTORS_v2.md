# AXIOM v2 Test Vectors

**Protocol Version:** 2  
**Status:** DRAFT

This document defines deterministic test vectors for v2 behavior. All vectors are expressed as:

- Input state (genesis + any prior blocks)
- Inputs (transactions / consensus messages where applicable)
- Expected canonical hashes (block hash, state hash)

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

