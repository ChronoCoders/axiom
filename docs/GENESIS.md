# AXIOM Genesis Specification

**Protocol Version:** 1
**Status:** NORMATIVE (LOCKED)

## 1. Purpose

This document defines the canonical genesis configuration for the AXIOM network.

Genesis establishes:

- Initial state
- Initial validator set
- Initial economic distribution
- Genesis state hash

Any node starting with a different genesis is not part of the same network.

## 2. Genesis Invariants

The following invariants must hold:

- Genesis height is 0
- Genesis epoch is 0
- Genesis state is deterministic
- Genesis state hash is immutable once locked
- All balances, accounts, and validators are explicitly defined
- Implicit defaults are forbidden
- Sum of all initial balances equals total_supply

## 3. Genesis Parameters

| Parameter      | Value                                                          |
|----------------|----------------------------------------------------------------|
| Height         | 0                                                              |
| Epoch          | 0                                                              |
| Block Reward   | 10 AXM                                                         |
| Total Supply   | 4,000,000 AXM                                                  |
| Genesis Hash   | `c1b50f23e410fe99b7ec6e304165b18f1dfe723ad5417133a12cdf8517460761` |

## 4. Initial Accounts

All accounts must be explicitly listed.

| Account ID | Balance (AXM) | Nonce |
|------------|---------------|-------|
| account-A  | 1,000,000     | 0     |
| account-B  | 1,000,000     | 0     |
| account-C  | 1,000,000     | 0     |
| account-D  | 1,000,000     | 0     |

Rules:

- Balances are unsigned 64-bit integers
- Nonces start at 0
- Sum of all balances must equal total_supply (1,000,000 × 4 = 4,000,000)
- Account IDs listed here are placeholder labels; in the actual genesis file, account IDs are 64-character hex-encoded Ed25519 public keys as defined in PROTOCOL.md Section 4.3

### 4.1 Locked Account Keys

| Label     | Public Key (hex)                                                   |
|-----------|--------------------------------------------------------------------|
| account-A | `97bbcd06ce80fc383b1f03d2bc08344f4de8bb7559cbfb24a5531c44512202b3` |
| account-B | `9c4132d3263292f48262d61f64f4d878ad6144741204290f14c0bccffff1dda0` |
| account-C | `b306eefdf57d6833438ebbde45b6645af8d1b66c000bf5d4a8e394dee062b9bf` |
| account-D | `e112358c36b47432dd832a97a3629c97275d9e74184149e50f46bccca2e49dd5` |

Account labels map to validators as follows: account-A = validator-2's account, account-B = validator-4's account, account-C = validator-3's account, account-D = validator-1's account. The labels are assigned by sorted key order, not by validator generation order.

## 5. Initial Validators

All validators must be explicitly listed.

| Validator ID | Voting Power | Associated Account |
|--------------|--------------|--------------------|
| validator-1  | 10           | account-D          |
| validator-2  | 10           | account-A          |
| validator-3  | 10           | account-C          |
| validator-4  | 10           | account-B          |

- Total voting power: 40
- Quorum threshold: strictly greater than 2/3 of total voting power
- Required: 3 × collected > 2 × 40 = 80, so collected > 26.67
- Minimum signatures needed: 3 (3 × 10 = 30 > 26.67)
- The network tolerates 1 faulty or offline validator

### 5.1 Locked Validator Keys

| Label       | Seed String               | Public Key (hex)                                                   |
|-------------|---------------------------|--------------------------------------------------------------------|
| validator-1 | `axiom-test-validator-1`  | `e112358c36b47432dd832a97a3629c97275d9e74184149e50f46bccca2e49dd5` |
| validator-2 | `axiom-test-validator-2`  | `97bbcd06ce80fc383b1f03d2bc08344f4de8bb7559cbfb24a5531c44512202b3` |
| validator-3 | `axiom-test-validator-3`  | `b306eefdf57d6833438ebbde45b6645af8d1b66c000bf5d4a8e394dee062b9bf` |
| validator-4 | `axiom-test-validator-4`  | `9c4132d3263292f48262d61f64f4d878ad6144741204290f14c0bccffff1dda0` |

Keys are generated deterministically: `private_key = seed = SHA-256(seed_string)`, `public_key = Ed25519_public(private_key)`.

Sorted validator order (by public key, lexicographic ascending): validator-2, validator-4, validator-3, validator-1.

Rules:

- Validator IDs are unique
- Voting power is a positive u64
- Associated accounts must exist in the initial accounts list
- Validator IDs are the hex-encoded public keys of their associated accounts

## 6. Genesis State Construction

Genesis state is constructed by:

1. Initialize empty state
2. Set total_supply = 4,000,000
3. Set block_reward = 10
4. Insert all 4 accounts with balances (1,000,000 each) and nonces (0)
5. Insert validator registry with voting power (10 each) and account associations
6. Compute canonical state hash per PROTOCOL.md Section 5.3

No transactions are applied at genesis.
No block reward is applied at genesis.

## 7. Genesis File Format

The genesis file is a deterministic JSON file following PROTOCOL.md Section 5.1:

- Keys sorted lexicographically
- No whitespace
- No trailing commas
- UTF-8 encoding
- Integers as JSON numbers
- Strings as JSON strings

### Genesis File Structure

The canonical reference genesis is stored at `docs/reference_genesis.json`.

```json
{"accounts":[{"balance":1000000,"id":"97bbcd06ce80fc383b1f03d2bc08344f4de8bb7559cbfb24a5531c44512202b3","nonce":0},{"balance":1000000,"id":"9c4132d3263292f48262d61f64f4d878ad6144741204290f14c0bccffff1dda0","nonce":0},{"balance":1000000,"id":"b306eefdf57d6833438ebbde45b6645af8d1b66c000bf5d4a8e394dee062b9bf","nonce":0},{"balance":1000000,"id":"e112358c36b47432dd832a97a3629c97275d9e74184149e50f46bccca2e49dd5","nonce":0}],"block_reward":10,"total_supply":4000000,"validators":[{"account_id":"97bbcd06ce80fc383b1f03d2bc08344f4de8bb7559cbfb24a5531c44512202b3","active":true,"id":"97bbcd06ce80fc383b1f03d2bc08344f4de8bb7559cbfb24a5531c44512202b3","voting_power":10},{"account_id":"9c4132d3263292f48262d61f64f4d878ad6144741204290f14c0bccffff1dda0","active":true,"id":"9c4132d3263292f48262d61f64f4d878ad6144741204290f14c0bccffff1dda0","voting_power":10},{"account_id":"b306eefdf57d6833438ebbde45b6645af8d1b66c000bf5d4a8e394dee062b9bf","active":true,"id":"b306eefdf57d6833438ebbde45b6645af8d1b66c000bf5d4a8e394dee062b9bf","voting_power":10},{"account_id":"e112358c36b47432dd832a97a3629c97275d9e74184149e50f46bccca2e49dd5","active":true,"id":"e112358c36b47432dd832a97a3629c97275d9e74184149e50f46bccca2e49dd5","voting_power":10}]}
```

Arrays within the JSON are sorted by the `id` field (lexicographic, ascending).

## 8. Genesis State Hash

The genesis state hash is computed as:

```
GENESIS_STATE_HASH = SHA-256(canonical_binary_state)
```

Where `canonical_binary_state` follows the field order defined in PROTOCOL.md Section 5.3.

**Locked Value:**

```
GENESIS_STATE_HASH = c1b50f23e410fe99b7ec6e304165b18f1dfe723ad5417133a12cdf8517460761
```

This hash:

- Is computed once from the genesis file
- Is treated as a constant after computation
- Must be identical across all nodes
- Is locked in the reference implementation source code

## 9. Node Startup Rules

On startup:

1. Load genesis file from configured path
2. Parse and validate the genesis file
3. Construct genesis state per Section 6
4. Compute genesis state hash per Section 8

If no stored state exists:

- Node initializes state from genesis
- Stores the genesis state and hash

If stored state exists:

- Genesis hash must match the stored genesis hash
- If mismatch: immediate process termination with explicit error

## 10. Network Identity

The tuple:

```
(protocol_version, genesis_state_hash)
```

defines the AXIOM network identity.

Nodes with differing values must not communicate.

## 11. Non-Goals

Genesis does not include:

- Governance parameters
- Dynamic configuration
- Migration logic
- Upgrade rules
- Fee parameters

## 12. Compliance

An implementation is AXIOM-compliant if and only if:

- Genesis state is constructed exactly as defined in this document
- Genesis hash matches the locked value (`c1b50f23e410fe99b7ec6e304165b18f1dfe723ad5417133a12cdf8517460761`)
- Startup enforces genesis verification
- Sum of initial balances equals total_supply
- All validators reference existing accounts
