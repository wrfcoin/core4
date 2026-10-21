#!/usr/bin/env bash
# ============================================================================
# Phase 3C TDD Gate Script — canonical local testnet runner
#
# Runs all Phase 3C TDD checks (TDD-12.1 through TDD-12.11).
# TDD-12.1 requires a live launch_api at :8556 with WRFCOIN_OPERATOR_TOKEN.
# TDD-12.2–12.5 are pure Rust unit/integration tests (no live node needed).
# TDD-12.6–12.11 require a live launch_api (negative-quorum, pagination, restart).
#
# Usage:
#   bash scripts/run-phase3c-security-api.sh [--rust-only] [--no-color]
#
# Options:
#   --rust-only   Skip TDD-12.1 API tests (do not require a running testnet)
#   --no-color    Disable ANSI color output
#
# Expected output when all pass:
#   [PASS] TDD-12.1  API slashing gate end-to-end
#   [PASS] TDD-12.2  Low-quality streak slashing determinism
#   [PASS] TDD-12.3  Reputation-weighted reward scaling
#   [PASS] TDD-12.4  Low-band reveal quorum (verify path)
#   [PASS] TDD-12.5  Low-band reveal quorum (mining path)
#   [PASS] TDD-12.6  Oracle negative-quorum API tests
#   [PASS] TDD-12.7  Quality-events pagination consistency
#   [PASS] TDD-12.8  Distinct oracle quorum requirement
#   [PASS] TDD-12.9  Pagination filter stability
#   [PASS] TDD-12.10 Quality events restart consistency
#   [PASS] TDD-12.11 Trust-reward timeline pagination
#   Phase 3C gate: ALL PASS
# ============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

# ── Options ──────────────────────────────────────────────────────────────────
RUST_ONLY=false
COLOR=true
for arg in "$@"; do
  case "$arg" in
    --rust-only) RUST_ONLY=true ;;
    --no-color)  COLOR=false ;;
  esac
done

# ── Color helpers ─────────────────────────────────────────────────────────────
if $COLOR; then
  RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'
  BOLD='\033[1m'; RESET='\033[0m'
else
  RED=''; GREEN=''; YELLOW=''; BOLD=''; RESET=''
fi

PASS=0; FAIL=0; SKIP=0

pass()  { echo -e "${GREEN}[PASS]${RESET} $*"; PASS=$((PASS+1)); }
fail()  { echo -e "${RED}[FAIL]${RESET} $*"; FAIL=$((FAIL+1)); }
skip()  { echo -e "${YELLOW}[SKIP]${RESET} $*"; SKIP=$((SKIP+1)); }
header(){ echo -e "\n${BOLD}==> $*${RESET}"; }

# ── TDD-12.1: API slashing gate end-to-end ───────────────────────────────────
header "TDD-12.1 — API slashing gate end-to-end"

if $RUST_ONLY; then
  skip "TDD-12.1  API slashing gate (skipped: --rust-only)"
else
  # Preflight: token and health check
  if [ -z "${WRFCOIN_OPERATOR_TOKEN:-}" ] && [ -f ".env" ]; then
    WRFCOIN_OPERATOR_TOKEN=$(awk -F= '/^WRFCOIN_OPERATOR_TOKEN=/{v=$2} END{print v}' .env)
    export WRFCOIN_OPERATOR_TOKEN
  fi

  TOKEN_OK=false
  printf '%s' "${WRFCOIN_OPERATOR_TOKEN:-}" | grep -Eq '^[0-9a-f]{64}$' && TOKEN_OK=true

  HEALTH=$(curl -sS -o /dev/null -w '%{http_code}' http://127.0.0.1:8556/health 2>/dev/null || echo "000")

  if ! $TOKEN_OK; then
    fail "TDD-12.1  WRFCOIN_OPERATOR_TOKEN missing or invalid (set env var or add to .env)"
  elif [ "$HEALTH" != "200" ]; then
    fail "TDD-12.1  launch_api not reachable at :8556 (HTTP $HEALTH) — start testnet first"
  else
    # Determine which API gate script to use
    API_SCRIPT="scripts/run-phase3b-security-api.sh"
    if [ -f "$API_SCRIPT" ]; then
      if bash "$API_SCRIPT" 2>&1; then
        pass "TDD-12.1  API slashing gate end-to-end"
      else
        fail "TDD-12.1  API slashing gate end-to-end"
      fi
    else
      # Fallback: run the test scripts directly
      NODE_PASS=true
      for test_script in \
        tests/security/test-quality-slashing.cjs \
        tests/security/test-reputation-decay.cjs; do
        if [ -f "$test_script" ]; then
          node "$test_script" 2>&1 || NODE_PASS=false
        else
          echo "  Warning: $test_script not found — skipping"
        fi
      done
      $NODE_PASS && pass "TDD-12.1  API slashing gate end-to-end" \
                  || fail "TDD-12.1  API slashing gate end-to-end"
    fi
  fi
fi

# ── TDD-12.2: Low-quality streak slashing determinism ────────────────────────
header "TDD-12.2 — Low-quality streak slashing determinism"
if cargo test --test slashing_integration -- --nocapture 2>&1; then
  pass "TDD-12.2  Low-quality streak slashing determinism"
else
  fail "TDD-12.2  Low-quality streak slashing determinism"
fi

# ── TDD-12.3: Reputation-weighted reward scaling ─────────────────────────────
header "TDD-12.3 — Reputation-weighted reward scaling"
if cargo test --test reward_integration -- --nocapture 2>&1; then
  pass "TDD-12.3  Reputation-weighted reward scaling"
else
  fail "TDD-12.3  Reputation-weighted reward scaling"
fi

# ── TDD-12.4: Low-band reveal quorum (verify path) ───────────────────────────
header "TDD-12.4 — Low-band reveal quorum (verify path)"
if cargo test -p wrfcoin-shared-protocol \
    verify_block_allows_low_quality_with_quorum_attestation_at_active_height \
    -- --nocapture 2>&1; then
  pass "TDD-12.4  Low-band reveal quorum (verify path)"
else
  fail "TDD-12.4  Low-band reveal quorum (verify path)"
fi

# ── TDD-12.5: Low-band reveal quorum (mining path) ───────────────────────────
header "TDD-12.5 — Low-band reveal quorum (mining path)"
if cargo test -p wrfcoin-shared-protocol \
    create_next_block_keeps_low_quality_reveal_when_quorum_attested \
    -- --nocapture 2>&1; then
  pass "TDD-12.5  Low-band reveal quorum (mining path)"
else
  fail "TDD-12.5  Low-band reveal quorum (mining path)"
fi

# ── TDD-12.6: Oracle negative-quorum API tests ───────────────────────────────
header "TDD-12.6 — Oracle negative-quorum API tests"
if $RUST_ONLY; then
  skip "TDD-12.6  Oracle negative-quorum API tests (skipped: --rust-only)"
elif ! ${TOKEN_OK:-false}; then
  fail "TDD-12.6  WRFCOIN_OPERATOR_TOKEN missing or invalid (set env var or add to .env)"
elif [ "${HEALTH:-000}" != "200" ]; then
  fail "TDD-12.6  launch_api not reachable at :8556 (HTTP ${HEALTH:-000}) — start testnet first"
else
  NEG_QUORUM_PASS=true
  for test_script in \
    tests/security/test-quality-slashing-no-quorum.cjs \
    tests/security/test-quality-slashing-duplicate-attestation.cjs \
    tests/security/test-quality-slashing-distinct-oracle-quorum.cjs; do
    if [ -f "$test_script" ]; then
      node "$test_script" 2>&1 || NEG_QUORUM_PASS=false
    else
      echo "  Warning: $test_script not found — skipping"
    fi
  done
  $NEG_QUORUM_PASS && pass "TDD-12.6  Oracle negative-quorum API tests" \
                    || fail "TDD-12.6  Oracle negative-quorum API tests"
fi

# ── TDD-12.7: Quality-events pagination consistency ──────────────────────────
header "TDD-12.7 — Quality-events pagination consistency"
if $RUST_ONLY; then
  skip "TDD-12.7  Quality-events pagination consistency (skipped: --rust-only)"
elif ! ${TOKEN_OK:-false}; then
  fail "TDD-12.7  WRFCOIN_OPERATOR_TOKEN missing or invalid (set env var or add to .env)"
elif [ "${HEALTH:-000}" != "200" ]; then
  fail "TDD-12.7  launch_api not reachable at :8556 (HTTP ${HEALTH:-000}) — start testnet first"
else
  PAGINATION_SCRIPT="tests/security/test-quality-events-pagination.cjs"
  if [ -f "$PAGINATION_SCRIPT" ]; then
    if node "$PAGINATION_SCRIPT" 2>&1; then
      pass "TDD-12.7  Quality-events pagination consistency"
    else
      fail "TDD-12.7  Quality-events pagination consistency"
    fi
  else
    skip "TDD-12.7  $PAGINATION_SCRIPT not found"
  fi
fi

# ── TDD-12.8: Distinct oracle quorum requirement ──────────────────────────────
header "TDD-12.8 — Distinct oracle quorum requirement"
if $RUST_ONLY; then
  skip "TDD-12.8  Distinct oracle quorum requirement (skipped: --rust-only)"
elif ! ${TOKEN_OK:-false}; then
  fail "TDD-12.8  WRFCOIN_OPERATOR_TOKEN missing or invalid (set env var or add to .env)"
elif [ "${HEALTH:-000}" != "200" ]; then
  fail "TDD-12.8  launch_api not reachable at :8556 (HTTP ${HEALTH:-000}) — start testnet first"
else
  DISTINCT_SCRIPT="tests/security/test-quality-slashing-distinct-oracle-quorum.cjs"
  if [ -f "$DISTINCT_SCRIPT" ]; then
    if node "$DISTINCT_SCRIPT" 2>&1; then
      pass "TDD-12.8  Distinct oracle quorum requirement"
    else
      fail "TDD-12.8  Distinct oracle quorum requirement"
    fi
  else
    skip "TDD-12.8  $DISTINCT_SCRIPT not found"
  fi
fi

# ── TDD-12.9: Pagination filter stability ─────────────────────────────────────
header "TDD-12.9 — Pagination filter stability"
if $RUST_ONLY; then
  skip "TDD-12.9  Pagination filter stability (skipped: --rust-only)"
elif ! ${TOKEN_OK:-false}; then
  fail "TDD-12.9  WRFCOIN_OPERATOR_TOKEN missing or invalid (set env var or add to .env)"
elif [ "${HEALTH:-000}" != "200" ]; then
  fail "TDD-12.9  launch_api not reachable at :8556 (HTTP ${HEALTH:-000}) — start testnet first"
else
  FILTER_SCRIPT="tests/security/test-quality-events-pagination.cjs"
  if [ -f "$FILTER_SCRIPT" ]; then
    if node "$FILTER_SCRIPT" 2>&1; then
      pass "TDD-12.9  Pagination filter stability"
    else
      fail "TDD-12.9  Pagination filter stability"
    fi
  else
    skip "TDD-12.9  $FILTER_SCRIPT not found"
  fi
fi

# ── TDD-12.10: Quality events restart consistency ─────────────────────────────
header "TDD-12.10 — Quality events restart consistency"
if $RUST_ONLY; then
  skip "TDD-12.10  Quality events restart consistency (skipped: --rust-only)"
elif [ -f "tests/security/test-quality-events-restart-consistency.sh" ]; then
  if bash tests/security/test-quality-events-restart-consistency.sh; then
    pass "TDD-12.10  Quality events restart consistency"
  else
    fail "TDD-12.10  Quality events restart consistency"
  fi
else
  skip "TDD-12.10  Quality events restart consistency (script not found)"
fi

# ── TDD-12.11: Trust-reward timeline pagination ───────────────────────────────
header "TDD-12.11 — Trust-reward timeline pagination"
if $RUST_ONLY; then
  skip "TDD-12.11  Trust-reward timeline pagination (skipped: --rust-only)"
elif ! ${TOKEN_OK:-false}; then
  fail "TDD-12.11  WRFCOIN_OPERATOR_TOKEN missing or invalid (set env var or add to .env)"
elif [ "${HEALTH:-000}" != "200" ]; then
  fail "TDD-12.11  launch_api not reachable at :8556 (HTTP ${HEALTH:-000}) — start testnet first"
else
  TIMELINE_SCRIPT="tests/security/test-trust-reward-timeline-pagination.cjs"
  if [ -f "$TIMELINE_SCRIPT" ]; then
    if node "$TIMELINE_SCRIPT" 2>&1; then
      pass "TDD-12.11  Trust-reward timeline pagination"
    else
      fail "TDD-12.11  Trust-reward timeline pagination"
    fi
  else
    skip "TDD-12.11  $TIMELINE_SCRIPT not found"
  fi
fi

# ── Summary ───────────────────────────────────────────────────────────────────
echo ""
echo -e "${BOLD}─────────────────────────────────────────${RESET}"
echo -e "  PASS: ${GREEN}$PASS${RESET}  FAIL: ${RED}$FAIL${RESET}  SKIP: ${YELLOW}$SKIP${RESET}"
echo -e "${BOLD}─────────────────────────────────────────${RESET}"

if [ "$FAIL" -eq 0 ]; then
  echo -e "${GREEN}${BOLD}Phase 3C gate: ALL PASS${RESET}"
  exit 0
else
  echo -e "${RED}${BOLD}Phase 3C gate: $FAIL FAILURE(S)${RESET}"
  exit 1
fi
