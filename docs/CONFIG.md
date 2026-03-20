# AXIOM Node Configuration Specification

**Protocol Version:** 1
**Status:** NORMATIVE

## 1. Purpose

This document defines the canonical configuration model for an AXIOM node.

It specifies:

- Configuration sources
- Required and optional parameters
- Validation rules
- Deployment guarantees

Configuration controls runtime behavior only.
It must never affect protocol semantics, consensus rules, or determinism.

## 2. Configuration Principles

- Configuration is explicit
- Configuration is file-based
- No implicit defaults for critical fields
- Invalid configuration causes startup failure
- Configuration does not alter protocol rules
- Environment-based behavior without explicit config is forbidden

## 3. Configuration Sources and Precedence

Configuration is loaded in the following order:

1. Configuration file (axiom.toml)
2. Environment variables (override file values)
3. Command-line flags (override all)

Precedence (highest to lowest):

CLI flags > Environment variables > Config file

## 4. Configuration File Format

- Format: TOML
- File name: axiom.toml
- File path: explicitly provided via CLI flag or defaulted to ./axiom.toml

The file must be readable at startup.

## 5. Global Configuration

```toml
[node]
node_id = "node-1"
data_dir = "./data"
```

| Field    | Description                           | Required |
|----------|---------------------------------------|----------|
| node_id  | Unique identifier for the node        | Yes      |
| data_dir | Base directory for all persisted data  | Yes      |

## 6. Network Configuration

```toml
[network]
enabled = true
listen_address = "0.0.0.0:7000"
peers = [
  "127.0.0.1:7001",
  "127.0.0.1:7002"
]
```

| Field          | Description                    | Required                  |
|----------------|--------------------------------|---------------------------|
| enabled        | Enables or disables networking | Yes                       |
| listen_address | TCP address to bind            | Yes (if enabled = true)   |
| peers          | Static peer list               | No (empty list if omitted)|

Rules:

- Disabling networking must preserve single-node behavior
- Network configuration must not affect consensus logic

## 7. API Configuration

```toml
[api]
enabled = true
bind_address = "127.0.0.1:8000"
```

| Field        | Description                  | Required                |
|--------------|------------------------------|-------------------------|
| enabled      | Enables or disables the API  | Yes                     |
| bind_address | Address to serve the API     | Yes (if enabled = true) |

Rules:

- API query endpoints are read-only
- Transaction submission endpoint writes to mempool only
- API configuration must not affect protocol behavior

## 8. Storage Configuration

```toml
[storage]
sqlite_path = "./data/axiom.db"
```

| Field       | Description                    | Required |
|-------------|--------------------------------|----------|
| sqlite_path | Path to SQLite database file   | Yes      |

Rules:

- Storage must be initialized before node startup
- Path must be writable
- WAL mode is enabled by the implementation (not configurable)
- Storage configuration must not affect determinism

## 9. Genesis Configuration

```toml
[genesis]
genesis_file = "./genesis.json"
```

| Field        | Description                         | Required |
|--------------|-------------------------------------|----------|
| genesis_file | Path to canonical genesis definition| Yes      |

Rules:

- Genesis file must conform to GENESIS.md
- Genesis hash must match the locked value
- Hash mismatch causes immediate startup failure

## 10. Console Configuration

The AXIOM web console uses a session token gate for UI access.

```toml
[console]
user = "operator"
password = "axiom"
```

| Field    | Description                 | Required                 |
|----------|-----------------------------|--------------------------|
| user     | Console login username      | Yes (if api.enabled=true)|
| password | Console login password      | Yes (if api.enabled=true)|

Rules:

- Console authentication is enforced by the node HTTP server via `/auth/login`, `/auth/verify`, `/auth/logout`.
- Console credentials affect access control only and must not affect protocol execution or determinism.

## 11. Mempool Configuration

```toml
[mempool]
max_size = 10000
max_tx_bytes = 65536
```

| Field        | Description                                     | Required |
|--------------|-------------------------------------------------|----------|
| max_size     | Maximum number of transactions in the mempool   | Yes      |
| max_tx_bytes | Maximum size of a single transaction in bytes    | Yes      |

Rules:

- Mempool configuration does not affect consensus
- Transactions exceeding max_tx_bytes are rejected at submission
- When mempool is full, eviction policy is implementation-defined
- Mempool is non-normative; configuration variations do not affect protocol compliance

## 11. Logging Configuration

```toml
[logging]
level = "info"
format = "json"
```

| Field  | Description                                    | Required |
|--------|------------------------------------------------|----------|
| level  | Log verbosity (error, warn, info, debug, trace)| Yes |
| format | Output format (json)                           | Yes |

Rules:

- Logging must be structured
- Logging must not affect protocol execution
- Logging format and level must be explicitly configured

## 12. Environment Variable Overrides

Environment variables may override configuration values.

Format:

```
AXIOM__<SECTION>__<KEY>
```

Double underscore separator between section and key.

Examples:

```
AXIOM__NETWORK__ENABLED=false
AXIOM__API__BIND_ADDRESS=0.0.0.0:9000
AXIOM__NODE__NODE_ID=node-2
AXIOM__MEMPOOL__MAX_SIZE=5000
AXIOM__CONSOLE__USER=operator
AXIOM__CONSOLE__PASSWORD=axiom
```

## 13. Command-Line Flags

CLI flags override all other configuration sources.

Example:

```
axiom-node \
  --config=./axiom.toml \
  --network_enabled=false \
  --api_bind_address=127.0.0.1:9000 \
  --mempool_max_size=10000 \
  --mempool_max_tx_bytes=65536 \
  --console_user=operator \
  --console_password=axiom
```

Rules:

- Flags must be explicit
- Unknown flags cause startup failure
- `--config` specifies the configuration file path

## 14. Validation Rules

On startup, the node must:

1. Parse the configuration file
2. Apply environment variable overrides
3. Apply CLI flag overrides
4. Validate all required fields are present
5. Reject unknown configuration keys
6. Reject invalid values (wrong types, out-of-range, malformed addresses)
7. Abort startup on any validation error

Partial startup is forbidden.

## 15. Determinism Guarantee

Configuration must not:

- Change protocol rules
- Change consensus behavior
- Introduce nondeterminism
- Affect state transition logic

All nodes with the same (protocol_version, genesis_state_hash) are part of the same network regardless of configuration differences.

## 16. Non-Goals

Configuration does not include:

- Protocol upgrades
- Consensus tuning
- Economic parameter changes
- Feature negotiation
- Block limits (defined in protocol, not configuration)

These require protocol-level changes.

## 17. Compliance

An implementation is AXIOM-compliant if and only if:

- Configuration is parsed and validated as defined in this document
- Invalid configuration prevents startup
- Configuration does not affect protocol semantics
- Unknown keys are rejected
- Missing required fields cause startup failure
