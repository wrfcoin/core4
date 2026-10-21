#!/bin/bash
# Run cargo-fuzz targets for a specified duration.
# Usage: ./scripts/run-fuzz.sh [seconds]  (default: 60)
set -euo pipefail

DURATION="${1:-60}"

for target in fuzz_transaction_deserialize fuzz_range_proof_verify fuzz_block_validation; do
    echo "=== Fuzzing $target for ${DURATION}s ==="
    cargo +nightly fuzz run "$target" -- -max_total_time="$DURATION" || true
done
