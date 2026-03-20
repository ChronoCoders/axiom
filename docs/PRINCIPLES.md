# AXIOM — Engineering Principles & Technology Decisions

**Protocol Version:** 1

## 1. Project Scope

AXIOM is a serious infrastructure project focused on correctness, determinism, and long-term maintainability.

- Design decisions favor clarity and reliability over convenience, trends, or rapid prototyping shortcuts
- The system is built in clearly separated layers, with strict boundaries between frontend, backend, and storage
- Document precedence is defined in PROTOCOL.md Section 1

## 2. Frontend Principles

### Technology

- Pure HTML, CSS, JavaScript, and SVG
- No frontend frameworks
- No build step
- No SPA frameworks (React, Vue, Svelte, etc.)

### Philosophy

- The frontend is read-only and stateless
- It never makes decisions
- It never computes consensus, state, or validation logic
- It only renders data provided by the backend API

### Responsibilities

- Display current node and network state
- Visualize blocks, validators, and consensus status
- Act as an operational console, not a control plane

### Design Style

The UI follows a minimalist, serious infrastructure aesthetic, inspired by Stripe, Linear, Railway, Vercel, GitHub.

Characteristics:

- Minimal color palette
- Clear typography
- Low visual noise
- Data-first layouts
- No decorative animations

## 3. Backend Principles

### Technology

- Rust (classic, deterministic style)
- Single native binary
- Explicit ownership and lifetimes
- Zero compiler warnings
- Async used only for I/O (outside deterministic core)

### Philosophy

- The backend is the single source of truth
- Deterministic behavior is non-negotiable
- Identical inputs must always produce identical outputs
- Forks are considered bugs, not edge cases

### Architectural Rules

- Consensus and state machine logic are fully synchronous
- No business logic in the frontend
- No implicit behavior or hidden side effects
- Clear module boundaries as defined in IMPLEMENTATION.md
- Explicit error handling at all boundaries

### Cryptographic Rules

- Ed25519 for all signatures
- SHA-256 for all hashing
- No custom cryptographic implementations
- Audited libraries only
- Constant-time comparisons for security-sensitive operations

## 4. Database Strategy

### Primary Database: SQLite

Used for all consensus-critical persistence:

- Chain state
- Blocks
- Validator registry
- State snapshots

Configuration:

- WAL mode
- Explicit transactions
- Atomic block+state persistence
- No ORMs

### Analytics Database (Optional)

Used for non-consensus-critical data:

- Analytics
- Structured logs
- Historical queries
- Read-heavy inspection

Rules:

- Entirely optional
- Not required for protocol compliance
- Failure must not affect node operation

### PostgreSQL Policy

Not used in v1. If strictly necessary in future versions, must be explicitly justified.

### General Rules

- Embedded databases are preferred
- Static schemas only
- Explicit transactions
- No ORMs. SQL is written and reviewed manually.

## 5. Separation of Concerns

| Layer        | Responsibility                                |
|--------------|-----------------------------------------------|
| Frontend     | Visualization only                            |
| Backend      | Consensus, state, validation, truth           |
| Database     | Persistence only, never decision-making       |
| Network      | Transport only, no logic                      |
| Mempool      | Transaction holding, non-consensus-critical   |
| Analytics    | Historical inspection, entirely optional      |

No layer may leak responsibilities or make assumptions about implementation details of another.

## 6. Design Ethos

AXIOM intentionally avoids:

- Trend-driven frameworks
- Over-abstraction
- Microservice sprawl
- Dynamic or reflective systems
- Premature optimization
- Custom cryptographic implementations

AXIOM prioritizes:

- Determinism
- Predictability
- Auditability
- Long-term stability
- Engineer confidence

The goal is not to move fast. The goal is to be correct and stay correct.

## 7. Non-Negotiable Constraints

- Deterministic execution
- Minimal dependencies
- Explicit behavior everywhere
- Infrastructure-grade seriousness
- No "temporary" hacks that become permanent
- Audited cryptography only
- Zero compiler warnings

## 8. Summary

- AXIOM is built like infrastructure, not a demo
- The frontend is simple, durable, and honest
- The backend is strict, deterministic, and intentionally boring
- Databases are embedded and controlled
- Cryptography uses audited libraries with constant-time operations
- The overall style is minimalist, serious, and professional

This document defines the baseline rules. Any deviation must be intentional, justified, and documented.
