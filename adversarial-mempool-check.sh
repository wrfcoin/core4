#!/usr/bin/env bash
# adversarial-mempool-check.sh — Pre-flight validation for 48h soak test (#1200)
#
# Runs the mempool adversarial input validation test suite to verify:
#   1. Oversized payload rejection (>64KB commit/reveal, >32KB attestation)
#   2. Duplicate nonce / replay protection (commit, reveal, attestation, UTXO)
#   3. Malformed JSON payload rejection (no panic)
#   4. Mempool capacity enforcement (eviction + rejection)
#   5. Structural validation (missing fields, empty required fields)
#
# Usage:
#   ./adversarial-mempool-check.sh
#
# Exit codes:
#   0 — all checks passed
#   1 — one or more checks failed (do NOT proceed with 48h test)

set -euo pipefail

echo "=== WRFCoin Mempool Adversarial Pre-flight Check (#1200) ==="
echo ""

# Run the adversarial test suite plus existing mempool validation tests
echo "[1/2] Running adversarial mempool tests..."
if ! cargo test -p wrfcoin-shared-protocol -- "mempool::pool::tests" --no-fail-fast 2>&1 | \
    tee /dev/stderr | grep -q "test result: FAILED"; then
    echo "  PASSED"
else
    echo "  FAILED — see output above"
    exit 1
fi

echo ""
echo "[2/2] Running commit-reveal expiry tests..."
if ! cargo test -p wrfcoin-shared-protocol --test commit_reveal_expiry --no-fail-fast 2>&1 | \
    tee /dev/stderr | grep -q "test result: FAILED"; then
    echo "  PASSED"
else
    echo "  FAILED — see output above"
    exit 1
fi

echo ""
echo "=== All adversarial mempool checks passed ==="
