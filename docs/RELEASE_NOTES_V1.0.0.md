# AXIOM v1.0.0 — Release Notes

Status: Stable (Phase 1 complete)

This release focuses on deterministic protocol compliance, operator ergonomics, and strict configuration behavior.

## Highlights

- Locked v1 test vectors are enforced by automated tests.
- Locked genesis hash is enforced at node startup.
- P2P network identity is enforced as `(protocol_version, genesis_state_hash)`.
- Configuration parsing is strict (unknown keys rejected; startup fails on invalid/missing required configuration).
- HTTP console authentication is served by the node (`/auth/login`, `/auth/verify`, `/auth/logout`).
- Logging is JSON-only for deterministic structured logs.

## Operational Notes

- Local testnet scripts start a 4-node testnet:
  - API: `8081-8084`
  - P2P: `3001-3004`
- Console URL: `http://127.0.0.1:8081/`
- Default console credentials in testnet scripts: `operator / axiom`

## Documentation

- Protocol: [PROTOCOL_v1.md](file:///c:/axiom/docs/PROTOCOL_v1.md)
- Genesis: [GENESIS.md](file:///c:/axiom/docs/GENESIS.md)
- Test vectors: [TEST_VECTORS.md](file:///c:/axiom/docs/TEST_VECTORS.md)
- Config: [CONFIG.md](file:///c:/axiom/docs/CONFIG.md)
- API: [API.md](file:///c:/axiom/docs/API.md)

