# AXIOM v2 Local Testnet (Fast-Forward)

This repo’s v2 activation height is `10000`. To exercise v2 locally without producing 10,000 blocks in real time, you can pre-fill a node’s SQLite DB up to height `9999` using the `fast-forward` tool.

## 1. Generate a Fast-Forward DB

From the repo root:

```bash
cargo run -p fast-forward -- ./test_data_ff/axiom.db
```

Optional arguments:

```bash
cargo run -p fast-forward -- <sqlite_path> [target_height] [genesis_json_path]
```

Defaults:

- `target_height = 9999`
- `genesis_json_path = docs/reference_genesis.json`

## 2. Start Nodes Using the Pre-Filled DB

Create per-node configs with:

- `genesis.genesis_file = docs/reference_genesis.json`
- `storage.sqlite_path` pointing to a DB that was fast-forwarded to height `9999`
- distinct `network.listen_address` and `api.bind_address` per node

Each node should start with a latest committed height of `9999` and then propose/commit v2 blocks starting at height `10000`.

## 3. Verify v2 is Active

Once running:

- `GET /api/status` shows current height (≥ 9999)
- `GET /api/consensus` shows `protocol_version` for `next_height` as `2` once `next_height >= 10000`
- `GET /api/staking` shows `enabled: true` once current height is at/above activation and a v2 block has been executed

