#!/usr/bin/env bash
# Mutation controls for the current-main determinism-debt repair.
#
# This probe is intentionally separate from the general detector probe: it proves that
# removing a production allow exposes the repaired finding, reintroducing a stale baseline
# entry fails closed, and accepting a new unbaselined violation still turns the gate red.
# Every mutation is restored byte-for-byte before the final clean-gate assertion.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

LINT="scripts/check_determinism.py"
BASELINE="scripts/determinism_baseline.json"
TARGET="shared-protocol/src/blockchain/state.rs"
PLANT="shared-protocol/src/blockchain/zz_determinism_repair_probe.rs"
TMP_DIR="$(mktemp -d)"

cp "$BASELINE" "$TMP_DIR/baseline"
cp "$TARGET" "$TMP_DIR/target"

cleanup() {
  cp "$TMP_DIR/baseline" "$BASELINE"
  cp "$TMP_DIR/target" "$TARGET"
  rm -f "$PLANT"
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT INT TERM

gate() {
  python3 "$LINT" --baseline "$BASELINE"
}

expect_finding() {
  local output="$1" path="$2" rule="$3"
  grep -F "$path" <<<"$output" | grep -F "$rule" >/dev/null
}

echo "=== clean repaired gate ==="
gate >/dev/null
echo "OK: repaired tree passes before mutation."

echo "=== remove one repaired allow — expect attributed RED ==="
python3 - "$TARGET" <<'PY'
from pathlib import Path
import sys

path = Path(sys.argv[1])
text = path.read_text(encoding="utf-8")
needle = "// determinism-ok: HashMap entries are collected only as an intermediate"
if text.count(needle) != 1:
    raise SystemExit("expected exactly one emission-window allow")
path.write_text(text.replace(needle, "// removed by determinism_repair_probe", 1), encoding="utf-8")
PY
ALLOW_OUT="$(python3 "$LINT" --baseline "$BASELINE" 2>&1 || true)"
expect_finding "$ALLOW_OUT" "shared-protocol/src/blockchain/state.rs" "unordered-iteration" \
  || { echo "allow removal did not expose the repaired unordered iteration" >&2; exit 1; }
echo "OK: removing the allow exposes the state-root iteration finding."
cp "$TMP_DIR/target" "$TARGET"

echo "=== reintroduce one stale baseline entry — expect RED ==="
python3 - "$BASELINE" <<'PY'
import hashlib
import json
from pathlib import Path
import sys

path = Path(sys.argv[1])
raw = json.loads(path.read_text(encoding="utf-8"))
stale = "consensus-engines/src/ffi.rs|float-literal|avg_block_time: 10.0,"
raw["findings"][stale] = 1
raw["totals"] = {
    "fingerprints": len(raw["findings"]),
    "occurrences": sum(raw["findings"].values()),
}
canonical = json.dumps(
    {"findings": raw["findings"], "authorizations": raw.get("authorizations") or []},
    sort_keys=True,
    separators=(",", ":"),
)
raw["digest"] = "sha256:" + hashlib.sha256(canonical.encode()).hexdigest()
path.write_text(json.dumps(raw, indent=2) + "\n", encoding="utf-8")
PY
STALE_OUT="$(python3 "$LINT" --baseline "$BASELINE" 2>&1 || true)"
grep -F "STALE consensus-engines/src/ffi.rs|float-literal|avg_block_time: 10.0," <<<"$STALE_OUT" >/dev/null 2>&1 || {
  echo "reintroduced stale baseline entry was not rejected" >&2
  exit 1
}
echo "OK: reintroduced stale entry is rejected."
cp "$TMP_DIR/baseline" "$BASELINE"

echo "=== plant an unjustified new wall-clock read — expect attributed RED ==="
cat > "$PLANT" <<'RS'
pub fn probe_consensus_clock() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
RS
NEW_OUT="$(python3 "$LINT" --baseline "$BASELINE" 2>&1 || true)"
expect_finding "$NEW_OUT" "shared-protocol/src/blockchain/zz_determinism_repair_probe.rs" "wall-clock" \
  || { echo "new unjustified finding was not attributed" >&2; exit 1; }
echo "OK: new unjustified finding is rejected and attributed."
rm -f "$PLANT"

cmp -s "$BASELINE" "$TMP_DIR/baseline"
cmp -s "$TARGET" "$TMP_DIR/target"
gate >/dev/null
python3 "$LINT" --check-workflow-paths >/dev/null
echo "OK: all mutations restored byte-for-byte; clean gate and path check pass."
