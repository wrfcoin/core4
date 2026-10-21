#!/usr/bin/env bash
#
# e2e-bench.sh — End-to-end HTTP benchmark using oha + hyperfine
#
# Usage:
#   ./scripts/e2e-bench.sh [label]      # label defaults to "run"
#   ./scripts/e2e-bench.sh baseline     # capture pre-optimisation baseline
#   ./scripts/e2e-bench.sh optimized    # capture post-optimisation numbers
#
# Prerequisites (install once):
#   cargo install oha
#   cargo install hyperfine
#
# The API server must be running before executing this script:
#   cargo run --release --bin launch_api
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname "$SCRIPT_DIR")"
LABEL="${1:-run}"
OUTDIR="$REPO_ROOT/docs/perf"
API_BASE="${WRFCOIN_API_BASE:-http://localhost:8556}"

mkdir -p "$OUTDIR"

echo "=============================================="
echo "  WRFCoin E2E Benchmark — label: $LABEL"
echo "  API: $API_BASE"
echo "=============================================="
echo ""

# ── Check prerequisites ──────────────────────────────────────────────────────

if ! command -v oha &>/dev/null; then
    echo "ERROR: 'oha' not found. Install with: cargo install oha"
    exit 1
fi

if ! command -v hyperfine &>/dev/null; then
    echo "ERROR: 'hyperfine' not found. Install with: cargo install hyperfine"
    exit 1
fi

# ── Health check ─────────────────────────────────────────────────────────────

echo "[1/3] Verifying server is reachable..."
if ! curl -sf "$API_BASE/health" >/dev/null 2>&1; then
    echo "ERROR: Server not responding at $API_BASE"
    echo "Start it with: cargo run --release --bin launch_api"
    exit 1
fi
echo "      Server OK"
echo ""

# ── oha: GET /api/chain/blocks/latest ────────────────────────────────────────

echo "[2/3] oha: GET /api/chain/blocks/latest (2000 reqs, 20 concurrent)..."
oha -n 2000 -c 20 \
    "$API_BASE/api/chain/blocks/latest" \
    --json > "$OUTDIR/oha-blocks-${LABEL}.json"

# Print summary line from oha JSON
python3 -c "
import json, sys
d = json.load(open('$OUTDIR/oha-blocks-${LABEL}.json'))
rps = d.get('summary', {}).get('requestsPerSec', 0)
lat_avg = d.get('summary', {}).get('average', 0) * 1000  # convert to ms
print(f'  Requests/sec: {rps:.1f}  |  Avg latency: {lat_avg:.2f}ms')
" 2>/dev/null || echo "  (install python3 for summary; raw data in $OUTDIR/oha-blocks-${LABEL}.json)"

echo ""

# ── hyperfine: single block latest call ──────────────────────────────────────

echo "[3/3] hyperfine: GET /api/chain/blocks/latest (50 runs, 5 warmup)..."
hyperfine \
    --warmup 5 \
    --runs 50 \
    --export-json "$OUTDIR/hyperfine-${LABEL}.json" \
    "curl -sf $API_BASE/api/chain/blocks/latest -o /dev/null"

echo ""
echo "Results saved to $OUTDIR/:"
echo "  oha-blocks-${LABEL}.json"
echo "  hyperfine-${LABEL}.json"
echo ""
echo "To compare baseline vs optimized:"
echo "  diff $OUTDIR/oha-blocks-baseline.json $OUTDIR/oha-blocks-optimized.json"
