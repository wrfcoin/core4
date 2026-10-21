#!/usr/bin/env bash
#
# Consensus-Critical Dependency Budget Enforcement — core4 workspace
#
# Checks direct runtime dependencies of wrfcoin-shared-protocol
# (the consensus-critical crate).  Dev-dependencies are excluded.
#
# Exit codes: 0 = pass, 1 = budget exceeded
#
# Budgets:
#   wrfcoin-shared-protocol runtime deps: max 30 direct
#   Crypto deps (sha2/sha3/ed25519/blake3/k256): max 8
#
# Usage: ./scripts/check-dep-budget.sh [--verbose]
#
# Note: The original version of this script (infra/scripts/check-dep-budget.sh)
# targeted blockchain-core and was extracted to wrfcoin/infra in PR #374.
# This version is adapted for the core4 Rust workspace.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

BUDGET_SHARED_PROTOCOL=30   # direct runtime deps for consensus crate
BUDGET_CRYPTO_DEPS=8        # crypto primitives in shared-protocol

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

VERBOSE=false
if [[ "${1:-}" == "--verbose" ]]; then
    VERBOSE=true
fi

log_info()    { echo -e "${GREEN}[INFO]${NC} $1"; }
log_warn()    { echo -e "${YELLOW}[WARN]${NC} $1"; }
log_error()   { echo -e "${RED}[ERROR]${NC} $1"; }
log_verbose() { [[ "$VERBOSE" == true ]] && echo -e "       $1" || true; }

cd "$PROJECT_ROOT"

echo "=============================================="
echo "  Consensus-Critical Dependency Budget Check"
echo "  Crate: wrfcoin-shared-protocol"
echo "=============================================="
echo ""

FAILED=false

# ── Check 1: Direct runtime deps of shared-protocol ────────────────────────
log_info "Checking wrfcoin-shared-protocol direct runtime dependencies..."

DEP_LIST=$(cargo tree -p wrfcoin-shared-protocol \
    --depth 1 --edges normal --prefix none 2>/dev/null \
    | grep -v "^wrfcoin-shared-protocol" | grep -v "^$" | sort -u)

DEP_COUNT=$(echo "$DEP_LIST" | grep -c . || echo 0)

log_info "  Direct runtime dependencies: $DEP_COUNT (budget: $BUDGET_SHARED_PROTOCOL)"

if [[ "$VERBOSE" == true ]]; then
    echo ""
    log_verbose "Dependencies:"
    echo "$DEP_LIST" | while read -r dep; do
        log_verbose "  - $dep"
    done
    echo ""
fi

if [[ "$DEP_COUNT" -gt "$BUDGET_SHARED_PROTOCOL" ]]; then
    log_error "  BUDGET EXCEEDED! ($DEP_COUNT > $BUDGET_SHARED_PROTOCOL)"
    log_error "  Review Cargo.toml — move optional deps behind feature flags or remove unused ones."
    FAILED=true
else
    log_info "  PASS"
fi

echo ""

# ── Check 2: Crypto primitive count ─────────────────────────────────────────
log_info "Checking crypto primitive dependencies..."

CRYPTO_DEPS="sha2 sha3 ed25519-dalek blake3 k256 aes-gcm secp256k1 p256"
CRYPTO_COUNT=0
FOUND_CRYPTO=()

for dep in $CRYPTO_DEPS; do
    if echo "$DEP_LIST" | grep -q "^${dep} "; then
        ((CRYPTO_COUNT++)) || true
        FOUND_CRYPTO+=("$dep")
        log_verbose "  - $dep: present"
    fi
done

log_info "  Crypto dependencies found: $CRYPTO_COUNT (budget: $BUDGET_CRYPTO_DEPS)"

if [[ "$VERBOSE" == true ]] && [[ "${#FOUND_CRYPTO[@]}" -gt 0 ]]; then
    for dep in "${FOUND_CRYPTO[@]}"; do
        log_verbose "  - $dep"
    done
fi

if [[ "$CRYPTO_COUNT" -gt "$BUDGET_CRYPTO_DEPS" ]]; then
    log_warn "  Crypto dep count approaching budget ($CRYPTO_COUNT > $BUDGET_CRYPTO_DEPS)"
    log_warn "  Audit for duplicate functionality before adding more crypto primitives."
else
    log_info "  PASS"
fi

echo ""

# ── Check 3: EVM / CosmWasm / ink! isolation ────────────────────────────────
log_info "Checking optional VM isolation (should not appear without feature flags)..."

ALL_DEPS=$(cargo tree -p wrfcoin-shared-protocol \
    --prefix none 2>/dev/null | sort -u)

EVM_DEPS=$(echo "$ALL_DEPS" | { grep -E "^ethers|^ethereum-types|^evm v" || true; } | wc -l | tr -d ' ')
COSMWASM_DEPS=$(echo "$ALL_DEPS" | { grep -E "^cosmwasm|^cw-storage|^schemars" || true; } | wc -l | tr -d ' ')
INK_DEPS=$(echo "$ALL_DEPS" | { grep "^ink_" || true; } | wc -l | tr -d ' ')

for label in "EVM:$EVM_DEPS" "CosmWasm:$COSMWASM_DEPS" "ink!:$INK_DEPS"; do
    name="${label%%:*}"
    count="${label##*:}"
    if [[ "$count" -gt 0 ]]; then
        log_error "  $name deps present without feature flag! ($count found)"
        FAILED=true
    else
        log_info "  $name isolation: PASS"
    fi
done

echo ""

# ── Check 4: Deprecated crates ───────────────────────────────────────────────
log_info "Checking for deprecated crates in dependency tree..."

DEPRECATED=$(cargo tree -p wrfcoin-shared-protocol \
    --prefix none 2>/dev/null | { grep "+deprecated" || true; } | sort -u)
DEPRECATED_COUNT=$(echo "$DEPRECATED" | { grep -c '[^[:space:]]' || true; })
DEPRECATED_COUNT="${DEPRECATED_COUNT:-0}"

if [[ "$DEPRECATED_COUNT" -gt 0 ]]; then
    log_warn "  Found $DEPRECATED_COUNT deprecated dependency entry(ies):"
    echo "$DEPRECATED" | while read -r dep; do
        log_warn "    - $dep"
    done
else
    log_info "  No deprecated dependencies found. PASS"
fi

echo ""
echo "=============================================="

if [[ "$FAILED" == true ]]; then
    log_error "DEPENDENCY BUDGET CHECK FAILED"
    echo ""
    echo "To fix:"
    echo "  1. Move optional dependencies behind feature flags"
    echo "  2. Remove unused dependencies"
    echo "  3. Replace deprecated crates"
    echo "  4. See docs/perf/ for optimization history"
    exit 1
else
    log_info "ALL DEPENDENCY BUDGET CHECKS PASSED"
    exit 0
fi
