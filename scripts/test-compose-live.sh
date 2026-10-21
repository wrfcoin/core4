#!/usr/bin/env bash
# test-compose-live.sh — Live Docker Compose E2E smoke test
#
# Starts the Docker Compose stack, waits for services to become healthy,
# validates the health endpoint, and tears down on exit.
#
# Usage:
#   ./scripts/test-compose-live.sh [compose-file]
#
# Environment:
#   WRFCOIN_HEALTH_URL   Override the health check URL
#                        (default: http://localhost:4001/health)
#   WRFCOIN_MAX_WAIT     Maximum seconds to wait for health (default: 300)
#
# Exits 0 on success, 1 on failure.
#
# Closes #1041

set -euo pipefail

COMPOSE_FILE="${1:-docker-compose.mvp.yml}"
HEALTH_URL="${WRFCOIN_HEALTH_URL:-http://localhost:4001/health}"
MAX_WAIT="${WRFCOIN_MAX_WAIT:-300}"

# ── Locate compose file relative to repo root ──────────────────────
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

if [[ ! -f "$REPO_ROOT/$COMPOSE_FILE" ]]; then
  echo "ERROR: Compose file not found: $REPO_ROOT/$COMPOSE_FILE" >&2
  exit 1
fi

# ── Teardown trap — always clean up containers ─────────────────────
cleanup() {
  echo ">>> Tearing down Docker Compose stack..."
  docker compose -f "$REPO_ROOT/$COMPOSE_FILE" down --volumes --remove-orphans 2>/dev/null || true
}
trap cleanup EXIT INT TERM

# ── Start the stack ────────────────────────────────────────────────
echo ">>> Starting Docker Compose stack: $COMPOSE_FILE"
docker compose -f "$REPO_ROOT/$COMPOSE_FILE" up -d --build

# ── Poll health endpoint with exponential backoff ──────────────────
echo ">>> Waiting for health endpoint: $HEALTH_URL (max ${MAX_WAIT}s)"

elapsed=0
delay=2

while (( elapsed < MAX_WAIT )); do
  # Attempt the health check
  if response=$(curl -sf --max-time 5 "$HEALTH_URL" 2>/dev/null); then
    # Validate that the response contains a "status" field
    if echo "$response" | grep -q '"status"'; then
      echo ">>> Health check PASSED after ${elapsed}s"
      echo "    Response: $response"
      exit 0
    else
      echo "    Response missing 'status' field: $response"
    fi
  fi

  echo "    Waiting ${delay}s... (${elapsed}s / ${MAX_WAIT}s elapsed)"
  sleep "$delay"
  elapsed=$(( elapsed + delay ))

  # Exponential backoff: 2, 4, 8, 16, cap at 30
  delay=$(( delay * 2 ))
  if (( delay > 30 )); then
    delay=30
  fi
done

echo "ERROR: Health check did not pass within ${MAX_WAIT}s" >&2
echo ">>> Docker Compose logs (last 50 lines):"
docker compose -f "$REPO_ROOT/$COMPOSE_FILE" logs --tail=50 2>/dev/null || true
exit 1
