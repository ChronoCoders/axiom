# AXIOM Coding Rules

**Protocol Version:** 1
**Status:** NORMATIVE

## 1. Purpose

This document defines mandatory coding rules for all AXIOM implementations.

Its goal is to ensure:

- Determinism
- Predictability
- Long-term maintainability
- Strict adherence to protocol and implementation specifications

Any code that violates these rules is invalid, regardless of whether it appears to function correctly.

## 2. General Principles

- Correctness is more important than performance
- Explicit code is preferred over clever code
- Readability is preferred over brevity
- Boring code is good code
- Assumptions must be encoded explicitly

## 3. Language Rules (Rust)

### 3.1 Allowed Rust Version

- Rust 2021 edition or later
- Stable Rust only
- No nightly features

### 3.2 Forbidden Language Features

The following are explicitly forbidden in protocol-critical code:

- `unsafe`
- `unwrap()` / `expect()` (outside of tests)
- Global mutable state
- Reflection or runtime type inspection
- Macros that obscure control flow
- Floating-point types (`f32`, `f64`)
- Random number generators (except deterministic test key generation)
- Dynamic dispatch (`dyn Trait`) in consensus or execution paths
- `std::time` in deterministic core
- `std::env` in deterministic core
- `std::thread` in deterministic core

### 3.3 Allowed Features With Restrictions

- Traits: allowed, but must be simple and explicit
- Generics: allowed, but must not obscure logic
- Enums: preferred over booleans for state representation
- Pattern matching: encouraged when exhaustive
- `#[must_use]`: encouraged on Result-returning functions
- `#[non_exhaustive]`: allowed on public enums

## 4. Deterministic Core Rules

The deterministic core includes:

- primitives
- crypto (hashing and verification only)
- state
- execution
- consensus

### Mandatory Rules

- No async/await
- No I/O
- No system clock access
- No environment variables
- No thread spawning
- No shared mutable state
- No logging that affects behavior (logging calls are allowed but must be side-effect-free)

All functions must be pure relative to their inputs.

## 5. Cryptographic Code Rules

### 5.1 No Custom Implementations

- All cryptographic operations must use audited, published libraries
- Ed25519: use ed25519-dalek or equivalent audited crate
- SHA-256: use sha2 crate or equivalent audited crate
- No hand-rolled hash functions, signature schemes, or key derivation

### 5.2 Constant-Time Operations

- All signature verification must use constant-time comparison
- All hash comparison (e.g., comparing state hashes) must use constant-time equality
- No early-return on byte-by-byte comparison of security-sensitive data

### 5.3 Key Handling

- Private keys must never be logged
- Private keys must never appear in error messages
- Private keys must never be serialized to persistent storage in plaintext (key storage format is implementation-defined but must not be plaintext)
- Public keys may be freely logged and serialized

### 5.4 Signature Verification

- Always verify signatures before processing transaction contents
- Reject transactions with invalid signatures before checking nonce or balance
- Signature verification failure must not leak timing information about the expected key

## 6. Error Handling

### 6.1 Errors Are Data

- All recoverable errors must be returned as `Result<T, E>`
- Error types must be explicit enums with descriptive variants
- Errors must be handled at boundaries, not ignored
- `?` operator is the preferred propagation method

### 6.2 Panics

- Panics are forbidden in deterministic core
- Panics are allowed only for unrecoverable programmer errors (e.g., invariant violations that indicate a bug)
- Panics must never be used for control flow
- If a panic occurs in production, it indicates a bug that must be fixed

### 6.3 Error Enums

Error types must be structured by module:

- `ExecutionError`: Errors from apply_block and transaction processing
- `ConsensusError`: Errors from quorum verification, proposer validation
- `StorageError`: Errors from persistence operations
- `ApiError`: Errors from API request handling
- `ConfigError`: Errors from configuration parsing and validation

Error variants must be descriptive:

```rust
// Good
enum ExecutionError {
    InvalidNonce { expected: u64, got: u64 },
    InsufficientBalance { account: AccountId, required: u64, available: u64 },
    InvalidSignature { sender: AccountId },
}

// Bad
enum ExecutionError {
    Error(String),
    Failed,
}
```

## 7. State Handling

- State must be immutable by default
- State transitions must be explicit
- State mutation must occur in a single, clearly defined location (apply_block)
- Partial state updates are forbidden
- All state changes must flow through the protocol-defined state transition function
- State must be cloneable for safe snapshot creation

## 8. Integer Arithmetic

- All protocol arithmetic uses unsigned 64-bit integers (u64)
- Overflow must be checked explicitly (use checked_add, checked_sub, checked_mul)
- Underflow must be checked explicitly
- Wrapping arithmetic is forbidden in protocol-critical code
- Saturating arithmetic is forbidden in protocol-critical code (use checked operations and return errors)

## 9. Naming Conventions

### 9.1 Types

- Types use PascalCase
- Protocol-critical types must use descriptive names
- Examples: Block, ValidatorId, StateHash, AccountId, TransactionHash

### 9.2 Functions

- Functions use snake_case
- Verb-based naming is required
- Examples: apply_block, validate_transaction, commit_block, verify_signature, compute_state_hash

### 9.3 Modules

- Modules use snake_case
- Module names must reflect responsibility
- Examples: execution, consensus, storage, crypto

### 9.4 Constants

- Constants use SCREAMING_SNAKE_CASE
- Protocol constants must be defined in primitives
- Examples: PROTOCOL_VERSION, MAX_TRANSACTIONS_PER_BLOCK, MAX_BLOCK_SIZE_BYTES

## 10. Testing Rules

### 10.1 Mandatory Tests

- Every protocol rule must have at least one test
- All test vectors in TEST_VECTORS.md must be implemented
- Determinism tests are mandatory
- Replay tests are mandatory
- Economic invariant tests are mandatory
- Account auto-creation tests are mandatory
- Block limit tests are mandatory

### 10.2 Test Isolation

- Tests must not depend on network
- Tests must not depend on system time
- Tests must not depend on external databases
- Tests must not depend on execution order
- Each test must set up its own state from scratch

### 10.3 Test Key Generation

- Test key pairs are generated deterministically from fixed seeds
- Seed derivation: SHA-256 of a fixed string (e.g., "axiom-test-validator-1")
- This ensures reproducible test results across environments

## 11. Dependency Management

- Dependencies must be minimal
- Every dependency must be justified
- Transitive dependencies must be reviewed
- No dependency may introduce nondeterminism into the deterministic core
- Cryptographic dependencies must be audited crates from reputable sources
- Pin exact versions for cryptographic dependencies

## 12. Database Access Rules

- Database access is forbidden in deterministic core
- All persistence code lives in the storage crate
- SQL must be explicit (no ORMs, no query builders)
- Transactions must be explicit
- Prepared statements must be used for all queries
- Schema migrations must be versioned and explicit

## 13. Logging and Debugging

- Logging must be structured (key-value pairs)
- Logging must not affect behavior
- Logging must not exist in deterministic core (logging calls that are purely side-effect-free are permitted)
- Debug-only code must not affect release behavior
- Private keys must never be logged
- Log output must be JSON-formatted

## 14. Code Review Rules

A change must be rejected if it:

- Breaks determinism
- Introduces hidden state
- Blurs module boundaries
- Adds unnecessary complexity
- Violates any rule in this document
- Introduces a dependency without justification
- Uses custom cryptographic implementations
- Uses floating-point arithmetic in protocol code
- Uses unchecked arithmetic in protocol code

## 15. Refactoring Rules

- Refactoring must not change observable behavior
- All tests must pass before and after refactoring
- Protocol compliance must be preserved
- State hashes must remain identical before and after refactoring

## 16. Prohibited Justifications

The following phrases are not acceptable justifications for code quality compromises:

- "We can fix this later"
- "This is just a temporary workaround"
- "It probably won't matter"
- "It's faster this way"
- "Nobody will notice"
- "It works on my machine"
- "The tests pass so it must be correct"

## 17. Completion Criteria

AXIOM code is acceptable only if:

1. All coding rules in this document are followed
2. All protocol rules from PROTOCOL.md are implemented
3. All test vectors from TEST_VECTORS.md pass
4. Determinism is preserved under replay
5. Zero compiler warnings
6. No clippy warnings (with standard lints)

## 18. Final Rule

If there is any doubt about whether a piece of code is acceptable, assume it is not until proven otherwise.

Correctness is the default.
Deviation requires proof.
