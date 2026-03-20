# AXIOM Test Vectors

**Protocol Version:** 1
**Status:** NORMATIVE (LOCKED)

## 1. Purpose

This document defines canonical test vectors for the AXIOM protocol.
An implementation is considered AXIOM-compliant if and only if it produces the exact expected outputs defined in this document when given the specified inputs.

These test vectors are normative.
Passing them is mandatory for protocol compliance.

## 2. Conventions

- All hashes are 64-character lowercase hexadecimal strings (SHA-256)
- All numeric values are unsigned 64-bit integers
- All state hashes are computed over the full canonical state representation per PROTOCOL.md Section 5.3
- Transaction and block ordering is strictly preserved
- Any deviation from expected results constitutes a failure
- Account and validator IDs use placeholder labels (account-A through account-D, validator-1 through validator-4) for readability; implementations must use hex-encoded Ed25519 public keys
- All blocks in v1 have epoch = 0

## 3. Test Key Material

The following Ed25519 key pairs are used across all test vectors.
Implementations must use these exact keys to produce matching hashes and signatures.

Keys are generated deterministically from fixed seed strings: `seed = SHA-256(seed_string)`, then `key_pair = Ed25519_from_seed(seed)`.

```
Validator-1 / Account-D:
  Seed String: "axiom-test-validator-1"
  Private Key: eed1444f431a29ddaba560d09559f7b3453cc1def5861ab51bcd3344dae18834
  Public Key:  e112358c36b47432dd832a97a3629c97275d9e74184149e50f46bccca2e49dd5

Validator-2 / Account-A:
  Seed String: "axiom-test-validator-2"
  Private Key: 9bd3bf36c5da99993f250e5b2e558e6768583ed5bbbd24a39560fca381b3c369
  Public Key:  97bbcd06ce80fc383b1f03d2bc08344f4de8bb7559cbfb24a5531c44512202b3

Validator-3 / Account-C:
  Seed String: "axiom-test-validator-3"
  Private Key: 2a8e0ea62396cbe5821e10a3700ee4da1a96eea2bed02c6f28d16591e682e3cb
  Public Key:  b306eefdf57d6833438ebbde45b6645af8d1b66c000bf5d4a8e394dee062b9bf

Validator-4 / Account-B:
  Seed String: "axiom-test-validator-4"
  Private Key: 139a29f05f0426440423e577fe65810d96d8dd4418f4f4d2226b04f2b5a40712
  Public Key:  9c4132d3263292f48262d61f64f4d878ad6144741204290f14c0bccffff1dda0
```

**Sorted Validator Order** (by public key, lexicographic ascending):

| Index | Validator | Public Key Prefix  |
|-------|-----------|--------------------|
| 0     | validator-2 | `97bbcd06...`    |
| 1     | validator-4 | `9c4132d3...`    |
| 2     | validator-3 | `b306eefd...`    |
| 3     | validator-1 | `e112358c...`    |

Account labels (account-A through account-D) are assigned by sorted key order, not by validator generation order.

**Additional Test Account** (used in Transaction Vector 4, auto-created):

```
Account-E:
  Seed String: "axiom-test-account-5"
  Seed (SHA-256): f6108668b63449ba233f99c0428b9de6b8e12ea72d149dee418294ced427da35
  Public Key:  b09bcc8b365f5df9d6829ecfb1aa4b524b723138eacdf002b7e73602f19d9fb0
```

Account-E is not a validator and does not exist in genesis state. It is auto-created when first receiving a transfer.

## 4. Genesis State

### 4.1 Genesis Parameters

| Parameter    | Value     |
|--------------|-----------|
| Height       | 0         |
| Epoch        | 0         |
| Block Reward | 10        |
| Total Supply | 4,000,000 |

### 4.2 Accounts

| Account   | Balance   | Nonce |
|-----------|-----------|-------|
| account-A | 1,000,000 | 0     |
| account-B | 1,000,000 | 0     |
| account-C | 1,000,000 | 0     |
| account-D | 1,000,000 | 0     |

### 4.3 Validators

| Validator   | Voting Power | Account   | Active |
|-------------|--------------|-----------|--------|
| validator-1 | 10           | account-D | true   |
| validator-2 | 10           | account-A | true   |
| validator-3 | 10           | account-C | true   |
| validator-4 | 10           | account-B | true   |

- Total voting power: 40
- Quorum rule: `3 × collected > 2 × total` (strictly greater than 2/3)
- Required: `3 × collected > 80`, so `collected > 26.67`
- Minimum collected power: 30 (3 validators × 10 power each)
- Signatures required: 3 of 4
- The network tolerates 1 faulty or offline validator

### 4.4 Expected Genesis State Hash

```
STATE_HASH_GENESIS = c1b50f23e410fe99b7ec6e304165b18f1dfe723ad5417133a12cdf8517460761
```

### 4.5 Genesis Invariant Checks

- Sum of balances: 1,000,000 × 4 = 4,000,000 = total_supply (PASS)
- All nonces = 0 (PASS)
- All validators reference existing accounts (PASS)
- Epoch = 0 (PASS)

## 5. Block Vector 1 — Single Valid Empty Block

### 5.1 Proposer Determination

Per PROTOCOL.md Section 8.3:

- Height 1, validator count = 4
- Proposer index = 1 % 4 = 1
- Sorted validators: [validator-2, validator-4, validator-3, validator-1] (lexicographic by public key)
- Proposer = validator-4 (sorted index 1)

### 5.2 Input Block

| Field        | Value                                                    |
|--------------|----------------------------------------------------------|
| Height       | 1                                                        |
| Parent       | `0000000000000000000000000000000000000000000000000000000000000000` |
| Epoch        | 0                                                        |
| Proposer     | validator-4                                              |
| Transactions | none                                                     |
| Signatures   | 3 of 4 validators (quorum)                               |

- Voting power of signatures: 30
- Quorum check: 3 × 30 = 90 > 2 × 40 = 80 (PASS)

### 5.3 Expected Result

- Block is valid
- Block is committed
- State transition succeeds
- Block reward of 10 AXM credited to validator-4's account (account-B)
- account-B balance: 1,000,000 + 10 = 1,000,010
- total_supply: 4,000,000 + 10 = 4,000,010

### 5.4 Expected State Hash

```
STATE_HASH_BLOCK_1 = 3803627a326ce03883ca996f9b8bcfd41ff3f5cf51ae21a6726207e9221b9514
```

### 5.5 Expected Block Hash

```
BLOCK_HASH_1 = 1fa4adaedc4ff6776c22aba6185966736d031a4e981791d7b711833e06838cfe
```

## 6. Block Vector 2 — Invalid Quorum (2 of 4)

### 6.1 Input Block

| Field        | Value                       |
|--------------|-----------------------------|
| Height       | 2                           |
| Parent       | BLOCK_HASH_1                |
| Epoch        | 0                           |
| Proposer     | validator-3                 |
| Transactions | none                        |
| Signatures   | validator-3, validator-1    |

- Voting power of signatures: 20 (quorum NOT satisfied: 3 × 20 = 60 is NOT > 80)

### 6.2 Expected Result

- Block is rejected
- State remains unchanged at STATE_HASH_BLOCK_1

## 7. Block Vector 3 — Duplicate Height

### 7.1 Input Block

| Field        | Value                                    |
|--------------|------------------------------------------|
| Height       | 1                                        |
| Parent       | Genesis block hash                       |
| Epoch        | 0                                        |
| Proposer     | validator-4                              |
| Transactions | none                                     |
| Signatures   | 3 of 4 validators                        |

### 7.2 Expected Result

- Block is rejected
- Reason: height 1 already committed
- State remains unchanged

## 8. Block Vector 4 — Fallback Proposer Accepted

### 8.1 Scenario

The primary proposer (validator-3) is assumed to have timed out.
A fallback proposer (validator-2) proposes a block and collects quorum.
Per PROTOCOL.md Section 8.3 (Fallback Mechanism), a fallback proposer is accepted
if the proposer is an active validator and quorum is satisfied.

### 8.2 Input Block

| Field        | Value                        |
|--------------|------------------------------|
| Height       | 2                            |
| Parent       | BLOCK_HASH_1                 |
| Epoch        | 0                            |
| Proposer     | validator-2                  |
| Transactions | none                         |
| Signatures   | 3 of 4 validators            |

### 8.3 Proposer Determination

- Height 2, validator count = 4
- Primary proposer index = 2 % 4 = 2
- Primary proposer = validator-3 (sorted index 2)
- Block proposer = validator-2 (sorted index 0) — NOT the primary proposer
- validator-2 is an active validator with voting power 10

### 8.4 Expected Result

- Block is **accepted**
- Justification: The `attempt` counter is ephemeral consensus-local state (PROTOCOL.md Section 8.3) and is not encoded in the block. The execution layer accepts any active validator as proposer when quorum is satisfied, because fallback proposer selection is a consensus-layer concern.
- Quorum check: 3 × 30 = 90 > 2 × 40 = 80 (PASS)
- State transition proceeds normally with block reward credited to validator-2's account (account-A)

## 9. Block Vector 5 — Wrong Epoch

### 9.1 Input Block

| Field        | Value                        |
|--------------|------------------------------|
| Height       | 2                            |
| Parent       | BLOCK_HASH_1                 |
| Epoch        | 1                            |
| Proposer     | validator-3                  |
| Transactions | none                         |
| Signatures   | 3 of 4 validators            |

### 9.2 Expected Result

- Block is rejected
- Reason: epoch must be 0 for all v1 blocks (no validator set changes)
- State remains unchanged

## 10. Transaction Vector 1 — Valid Transfer

### 10.1 Precondition

State is at STATE_HASH_BLOCK_1.

### 10.2 Proposer Determination

- Height 2, proposer index = 2 % 4 = 2
- Proposer = validator-3 (sorted index 2)

### 10.3 Input Transaction

| Field     | Value     |
|-----------|-----------|
| Sender    | account-D |
| Recipient | account-A |
| Amount    | 100,000   |
| Nonce     | 0         |
| Signature | valid     |

### 10.4 Input Block

| Field        | Value                        |
|--------------|------------------------------|
| Height       | 2                            |
| Parent       | BLOCK_HASH_1                 |
| Epoch        | 0                            |
| Proposer     | validator-3                  |
| Transactions | [transfer above]             |
| Signatures   | 3 of 4 validators            |

### 10.5 Expected Result

- Transaction is applied
- Block is committed
- Block reward of 10 AXM credited to validator-3's account (account-C)
- Final balances:
  - account-A: 1,000,000 + 100,000 = 1,100,000
  - account-B: 1,000,010 (unchanged from block 1)
  - account-C: 1,000,000 + 10 = 1,000,010
  - account-D: 1,000,000 - 100,000 = 900,000
- account-D nonce: 1
- total_supply: 4,000,010 + 10 = 4,000,020

### 10.6 Expected State Hash

```
STATE_HASH_BLOCK_2 = 9febb4ee5ce09acf044e8d34238c3e2ec6315382dc1008bc985ac403201b5287
```

## 11. Transaction Vector 2 — Invalid Nonce (Duplicate)

### 11.1 Precondition

State is at STATE_HASH_BLOCK_2 (account-D nonce = 1).

### 11.2 Proposer Determination

- Height 3, proposer index = 3 % 4 = 3
- Proposer = validator-1 (sorted index 3)

### 11.3 Input Transaction

| Field     | Value     |
|-----------|-----------|
| Sender    | account-D |
| Recipient | account-A |
| Amount    | 1         |
| Nonce     | 0         |
| Signature | valid     |

### 11.4 Input Block

| Field        | Value                        |
|--------------|------------------------------|
| Height       | 3                            |
| Parent       | Block 2 hash                 |
| Epoch        | 0                            |
| Proposer     | validator-1                  |
| Transactions | [transfer above]             |
| Signatures   | 3 of 4 validators            |

### 11.5 Expected Result

- Transaction is invalid (nonce 0, expected 1)
- Entire block is rejected
- State remains at STATE_HASH_BLOCK_2

## 12. Transaction Vector 3 — Invalid Signature

### 12.1 Input Transaction

| Field     | Value                                |
|-----------|--------------------------------------|
| Sender    | account-D                            |
| Recipient | account-A                            |
| Amount    | 1,000                                |
| Nonce     | 1                                    |
| Signature | invalid (signed with wrong key)      |

### 12.2 Input Block

| Field        | Value                        |
|--------------|------------------------------|
| Height       | 3                            |
| Parent       | Block 2 hash                 |
| Epoch        | 0                            |
| Proposer     | validator-1                  |
| Transactions | [transfer above]             |
| Signatures   | 3 of 4 validators            |

### 12.3 Expected Result

- Transaction is invalid (signature verification fails)
- Entire block is rejected
- State remains at STATE_HASH_BLOCK_2

## 13. Transaction Vector 4 — Transfer to New Account (Auto-Create)

### 13.1 Precondition

State is at STATE_HASH_BLOCK_2.

### 13.2 Input Transaction

| Field     | Value                              |
|-----------|------------------------------------|
| Sender    | account-D                          |
| Recipient | account-E (`b09bcc8b...`, does not exist in state)|
| Amount    | 50,000                             |
| Nonce     | 1                                  |
| Signature | valid                              |

### 13.3 Input Block

| Field        | Value                        |
|--------------|------------------------------|
| Height       | 3                            |
| Parent       | Block 2 hash                 |
| Epoch        | 0                            |
| Proposer     | validator-1                  |
| Transactions | [transfer above]             |
| Signatures   | 3 of 4 validators            |

### 13.4 Expected Result

- account-E is auto-created with balance 0, nonce 0
- Transfer is applied: account-E balance = 50,000
- Block reward of 10 AXM credited to validator-1's account (account-D)
- Final balances:
  - account-A: 1,100,000
  - account-B: 1,000,010
  - account-C: 1,000,010
  - account-D: 900,000 - 50,000 + 10 = 850,010
  - account-E: 50,000
- account-D nonce: 2
- total_supply: 4,000,020 + 10 = 4,000,030

### 13.5 Expected State Hash

```
STATE_HASH_BLOCK_3 = d8f1fb0f42dfcb895d87c3c46c8203615061b312123bb4aa9e6c97630af4c181
```

## 14. Replay Test

### 14.1 Input

Replay all committed blocks from genesis through Block 2 (or Block 3 if Vector 4 is used).
Apply blocks sequentially to an empty initial state constructed from genesis.

### 14.2 Expected Result

- Final state hash equals the state hash after the last committed block
- No divergence allowed
- All intermediate state hashes match

## 15. Determinism Test

### 15.1 Input

- Two independent nodes
- Same genesis state (same keys, same allocations)
- Same ordered block sequence

### 15.2 Expected Result

- Final state hashes are identical
- All intermediate state hashes are identical
- Any mismatch constitutes a protocol violation

## 16. Invalid Vote Vector

### 16.1 Input

- Vote signed by a key not in the validator registry
- Vote references a valid block hash

### 16.2 Expected Result

- Vote is ignored
- Consensus outcome is unaffected
- Block cannot reach quorum using this vote

## 17. Failure Handling Vector

### 17.1 Input

Block containing:

- Transaction 1: valid transfer (account-D to account-A, amount 1,000, nonce correct)
- Transaction 2: invalid transfer (account-D to account-C, amount 1, nonce incorrect)

### 17.2 Expected Result

- Entire block is rejected
- No balances or nonces are modified
- No block reward is applied
- total_supply unchanged
- State remains at previous committed state

## 18. Block Limit Vectors

### 18.1 Transaction Count Limit

- Block contains 1,001 transactions (exceeds max 1,000)
- All transactions are individually valid

**Expected Result**: Block is rejected before transaction processing.

### 18.2 Block Size Limit

- Block serialized size exceeds 1,048,576 bytes
- All transactions are individually valid

**Expected Result**: Block is rejected before transaction processing.

## 19. Economic Test Vectors

All vectors in this section are mandatory.
Failure of any vector indicates non-compliance.

### 19.1 Genesis Allocation Vector

**Input**: Genesis state per Section 4.

**Expected Result**:

- Sum of balances = 4,000,000 = total_supply
- All nonces = 0
- State hash is deterministic and matches STATE_HASH_GENESIS

### 19.2 Valid Transfer Vector

**Precondition**: Genesis state.

**Input Transaction**:

| Field     | Value     |
|-----------|-----------|
| Sender    | account-D |
| Recipient | account-A |
| Amount    | 100,000   |
| Nonce     | 0         |
| Signature | valid     |

**Input Block**: Valid block at height 1 with correct proposer and quorum (3 of 4 signatures).

**Expected Result**:

- Block is committed
- Balances after transfer (before reward):
  - account-D: 900,000
  - account-A: 1,100,000
  - account-B: 1,000,000
  - account-C: 1,000,000
- After block reward (10 AXM to proposer):
  - Proposer's account balance increased by 10
- account-D nonce = 1
- total_supply = 4,000,010

### 19.3 Invalid Transfer — Insufficient Balance

**Precondition**: State after Vector 19.2.

**Input Transaction**:

| Field     | Value     |
|-----------|-----------|
| Sender    | account-C |
| Recipient | account-A |
| Amount    | 1,500,000 |
| Nonce     | 0         |
| Signature | valid     |

**Expected Result**:

- Transaction is invalid (account-C balance is at most 1,000,010, insufficient for 1,500,000)
- Block is rejected
- State remains unchanged

### 19.4 Invalid Transfer — Bad Nonce

**Precondition**: State after Vector 19.2 (account-D nonce = 1).

**Input Transaction**:

| Field     | Value     |
|-----------|-----------|
| Sender    | account-D |
| Recipient | account-A |
| Amount    | 1         |
| Nonce     | 0         |
| Signature | valid     |

**Expected Result**:

- Transaction is invalid (nonce 0, expected 1)
- Block is rejected
- State remains unchanged

### 19.5 Invalid Transfer — Zero Amount

**Input Transaction**:

| Field     | Value     |
|-----------|-----------|
| Sender    | account-D |
| Recipient | account-A |
| Amount    | 0         |
| Nonce     | 1         |
| Signature | valid     |

**Expected Result**:

- Transaction is invalid (amount must be > 0)
- Block is rejected

### 19.6 Block Reward Vector

**Precondition**: State after a committed block with no transactions.

**Input Block**: Valid empty block at next height with correct proposer and quorum (3 of 4 signatures).

**Expected Result**:

- Block is committed
- Proposer account balance increased by 10 (block_reward)
- total_supply increased by 10
- No other balances changed

### 19.7 Supply Invariant Vector

**Input**: Replay all blocks from genesis through the latest committed block.

**Expected Result**:

- Sum of all account balances equals total_supply at every height
- No overflow or underflow at any step
- Deterministic final state hash

### 19.8 Atomicity Vector (Mixed Transactions)

**Input Block** containing:

- Transaction 1: valid transfer (account-D to account-A, amount 1,000, correct nonce)
- Transaction 2: invalid transfer (account-A to account-C, amount 1, wrong nonce)

**Expected Result**:

- Entire block is rejected
- No balances or nonces are modified
- total_supply unchanged
- No block reward applied

### 19.9 Determinism Vector (Economics)

**Input**: Two independent nodes process same genesis and same block sequence including transfers and rewards.

**Expected Result**:

- Final state hashes are identical
- All balances and nonces match exactly
- total_supply values match exactly

## 20. Compliance Criteria

An implementation is AXIOM-compliant if and only if:

- All valid vectors are accepted
- All invalid vectors are rejected
- All expected state hashes match exactly
- Replay and determinism tests succeed
- All economic invariants hold at every block height

Failure in any test vector indicates non-compliance.

## 21. Hash Computation Reference

This section defines the exact procedure for computing state hashes.
Implementations must follow this procedure to produce matching hashes.

### 21.1 State Hash Procedure

1. Serialize state in canonical binary format per PROTOCOL.md Section 5.3
2. Compute SHA-256 over the serialized bytes
3. Encode as 64-character lowercase hexadecimal string

### 21.2 Block Hash Procedure

1. Serialize block fields in canonical binary format per PROTOCOL.md Section 5.4
2. Compute SHA-256 over the serialized bytes
3. Encode as 64-character lowercase hexadecimal string

### 21.3 Transaction Hash Procedure

1. Serialize transaction fields in canonical binary format per PROTOCOL.md Section 5.5
2. Compute SHA-256 over the serialized bytes
3. Encode as 64-character lowercase hexadecimal string

### 21.4 Key Lock Status

Keys have been generated and locked. The following values are immutable:

- Test key material (Section 3)
- Genesis state hash: `c1b50f23e410fe99b7ec6e304165b18f1dfe723ad5417133a12cdf8517460761`
- Block 1 state hash: `3803627a326ce03883ca996f9b8bcfd41ff3f5cf51ae21a6726207e9221b9514`
- Block 1 hash: `1fa4adaedc4ff6776c22aba6185966736d031a4e981791d7b711833e06838cfe`

These values must never change without a protocol version increment.

## 22. Notes

- All test key material and locked hashes were generated by `tools/test-vector-gen`
- The reference genesis file is committed at `docs/reference_genesis.json`
- Once locked, these values must never change without a protocol version increment
- The `test-vector-gen` tool can be re-run to verify all locked values are reproducible
