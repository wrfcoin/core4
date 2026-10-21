#!/usr/bin/env bash
# flamegraph.sh — Generate a flamegraph SVG for a named bench binary
#
# Usage:
#   ./scripts/flamegraph.sh [crate] [bench]
#
# Examples:
#   ./scripts/flamegraph.sh wrfcoin-shared-protocol crypto_bench
#   ./scripts/flamegraph.sh wrfcoin-shared-protocol consensus_bench
#
# Requires: cargo install flamegraph
# On WSL2 without perf, use: cargo install samply  (swap the final command)
set -euo pipefail

CRATE="${1:-wrfcoin-shared-protocol}"
BENCH="${2:-crypto_bench}"
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUTDIR="$REPO_ROOT/docs/perf"

mkdir -p "$OUTDIR"

echo "Generating flamegraph for $CRATE::$BENCH ..."
echo "Output → $OUTDIR/flamegraph-${BENCH}.svg"

cd "$REPO_ROOT"

# Attempt perf-based flamegraph first (native Linux), fall back to dtrace (macOS)
if command -v perf &>/dev/null; then
    cargo flamegraph \
        --profile bench \
        --bench "$BENCH" \
        --package "$CRATE" \
        --output "$OUTDIR/flamegraph-${BENCH}.svg" \
        -- --bench
elif command -v samply &>/dev/null; then
    # WSL2 / systems without perf
    cargo build --profile bench --bench "$BENCH" --package "$CRATE"
    BENCH_BIN=$(find target/bench/deps -name "${BENCH}-*" -type f -perm /111 | head -1)
    samply record -o "$OUTDIR/flamegraph-${BENCH}.json" "$BENCH_BIN" --bench
    echo "samply profile saved to $OUTDIR/flamegraph-${BENCH}.json"
    echo "Open with: samply load $OUTDIR/flamegraph-${BENCH}.json"
else
    echo "ERROR: Neither 'perf' nor 'samply' found."
    echo "Install one of:"
    echo "  sudo apt-get install linux-tools-common linux-tools-\$(uname -r)"
    echo "  cargo install samply"
    exit 1
fi

echo "Done."
