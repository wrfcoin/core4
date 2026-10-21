#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_FILE="${OUT_FILE:-/tmp/wrfcoin-mempool-bench-latest.txt}"

if ! command -v cargo >/dev/null 2>&1; then
  echo "cargo is required to run mempool benchmarks" >&2
  exit 1
fi

if command -v cargo-flamegraph >/dev/null 2>&1; then
  echo "cargo-flamegraph detected; flamegraph capture can be run separately if needed."
else
  echo "cargo-flamegraph not found; running Criterion benchmark only." >&2
fi

mkdir -p "$(dirname "$OUT_FILE")"

cd "$ROOT_DIR"
cargo bench -p wrfcoin-shared-protocol --bench mempool_bench 2>&1 | tee "$OUT_FILE"

echo "Benchmark log written to $OUT_FILE"
