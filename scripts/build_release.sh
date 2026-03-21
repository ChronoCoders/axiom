#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/dist"

rm -rf "$OUT"
mkdir -p "$OUT"

export GIT_SHA="$(git rev-parse HEAD)"

cargo build --release -p axiom-node
cargo build --release -p fast-forward
cargo build --release -p genesis-tool
cargo build --release -p test-vector-gen

cp "$ROOT/target/release/axiom-node" "$OUT/"
cp "$ROOT/target/release/fast-forward" "$OUT/"
cp "$ROOT/target/release/genesis-tool" "$OUT/"
cp "$ROOT/target/release/test-vector-gen" "$OUT/"

cd "$OUT"
sha256sum axiom-node fast-forward genesis-tool test-vector-gen > SHA256SUMS.txt
