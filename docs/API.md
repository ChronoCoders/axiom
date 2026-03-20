# AXIOM v1 API Specification

**Protocol Version:** 1
**Status:** NORMATIVE

This document defines the AXIOM v1 API surface.
Query endpoints are read-only. The transaction submission endpoint writes to the mempool only and does not mutate consensus state.

## 1. Node Inspection

### 1.1 Status

**GET** `/api/status`

Returns the current node status.

Response fields:

- `protocol_version`: Protocol version (integer)
- `node_version`: Node software version (string)
- `height`: Current committed block height (integer)
- `latest_block_hash`: Hash of the latest committed block (hex string)
- `genesis_hash`: Genesis state hash (hex string)
- `validator_count`: Number of active validators (integer)
- `syncing`: Whether the node is currently syncing (boolean)

### 1.2 Health Checks

**GET** `/health/live`

Returns 200 OK if the node process is running.
No response body required.

**GET** `/health/ready`

Returns 200 OK if the node is ready to accept traffic (genesis verified, state loaded, storage accessible).
Returns 503 if the node is not ready.

## 2. Blockchain Data

### 2.1 List Blocks

**GET** `/api/blocks?limit={n}&cursor={height}`

Returns a list of blocks, ordered by height descending.

Parameters:

- `limit`: Max number of blocks to return (default 50, max 1000, integer)
- `cursor`: Block height to start listing from, exclusive (optional, integer)

Response: Array of block summaries.

Block summary fields:

- `height`: Block height (integer)
- `hash`: Block hash (hex string)
- `parent_hash`: Parent block hash (hex string)
- `epoch`: Epoch number (integer)
- `proposer_id`: Proposer validator ID (hex string)
- `transaction_count`: Number of transactions in the block (integer)
- `state_hash`: Resulting state hash (hex string)
- `timestamp`: Block commit timestamp (ISO 8601, non-normative, for display only)

### 2.2 Get Block by Height

**GET** `/api/blocks/{height}`

Returns full details for the block at the specified height.

Response fields:

- All block summary fields
- `transactions`: Array of transaction objects
- `signatures`: Array of validator signatures

Returns 404 if height does not exist.

### 2.3 Get Block by Hash

**GET** `/api/blocks/by-hash/{hash}`

Returns full details for the block with the specified hash.
Accepts hex string with or without `0x` prefix.

Returns 404 if hash does not match any committed block.

### 2.4 Get Account

**GET** `/api/accounts/{account_id}`

Returns the current state of an account.

Response fields:

- `account_id`: Account identifier (hex string)
- `balance`: Current balance in AXM (integer)
- `nonce`: Current transaction nonce (integer)

Returns 404 if account does not exist.

### 2.5 List Validators

**GET** `/api/validators`

Returns the current active validator set.

Response: Array of validator objects.

Validator fields:

- `validator_id`: Validator identifier (hex string)
- `voting_power`: Voting power (integer)
- `account_id`: Associated account identifier (hex string)
- `active`: Whether the validator is active (boolean)

## 3. Network

### 3.1 List Peers

**GET** `/api/network/peers`

Returns a list of connected peer addresses.

Response: Array of peer objects.

Peer fields:

- `address`: Peer network address (string)
- `connected_since`: Connection timestamp (ISO 8601, non-normative)

## 4. Transaction Submission

### 4.1 Submit Transaction

**POST** `/api/transactions`

Submits a transaction to the node's mempool.

Request body (JSON):

- `sender`: Sender account ID (hex string)
- `recipient`: Recipient account ID (hex string)
- `amount`: Transfer amount in AXM (integer)
- `nonce`: Transaction nonce (integer)
- `signature`: Ed25519 signature over canonical transaction bytes (hex string, 128 characters)

Response on success (202 Accepted):

- `tx_hash`: Transaction hash (hex string)
- `status`: "pending"

Response on validation failure (400 Bad Request):

- `error`: Error description (string)
- `code`: Error code (string)

Error codes:

- `invalid_signature`: Signature verification failed
- `invalid_nonce`: Nonce does not match sender's current nonce or is in the past
- `insufficient_balance`: Sender balance is less than amount
- `invalid_amount`: Amount is zero or negative
- `sender_not_found`: Sender account does not exist
- `tx_too_large`: Transaction exceeds max_tx_bytes
- `mempool_full`: Mempool is at capacity

### 4.2 Submission Guarantees

- Submission does not guarantee inclusion in a block
- A 202 response means the transaction passed basic validation and was added to the mempool
- Transactions may be evicted from the mempool before inclusion
- Transactions may become invalid by the time they are included (due to state changes from other blocks)
- The submission endpoint does not write to consensus state

## 5. Error Responses

All endpoints return errors in a consistent format:

```json
{
  "error": "Human-readable error message",
  "code": "machine_readable_error_code"
}
```

HTTP status codes:

- 200: Success
- 202: Accepted (transaction submitted)
- 400: Bad request (invalid parameters or transaction)
- 404: Not found
- 500: Internal server error
- 503: Service unavailable (node not ready)

## 6. API Guarantees

- All query endpoints (GET) are read-only and reflect committed, canonical state only
- No derived state or speculative computation in query responses
- Transaction submission (POST) writes to mempool only
- API responses must not leak internal implementation details
- API must not expose non-committed state

## 7. Content Type

- All request and response bodies are JSON
- Content-Type: application/json
- Character encoding: UTF-8

## 8. Rate Limiting and Security

Rate limiting and authentication are implementation-defined and non-normative.
Implementations may add rate limiting, API keys, or other security measures without affecting protocol compliance.

## 9. Freeze Policy

The v1 API surface defined in this document is locked.
Changes to existing endpoints or addition of new query endpoints require Protocol v2 activation.
The transaction submission endpoint (Section 4) is non-normative and may be refined without a protocol version increment, as it does not affect consensus.
