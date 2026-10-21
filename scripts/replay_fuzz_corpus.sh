#!/usr/bin/env bash
# Replay every committed fuzz-regression corpus entry through its target once and fail on
# any panic (#2110). This is a fast regression gate, NOT a fuzzing campaign — each input is
# run exactly once. Wired into PR CI so a reintroduced crasher is caught on the PR.
#
#   scripts/replay_fuzz_corpus.sh --check   # validate targets + entries + notes (no build)
#   scripts/replay_fuzz_corpus.sh           # real replay (needs nightly + cargo-fuzz)
#
# A gate that cannot tell "nothing to replay" from "everything replayed cleanly" is worse
# than no gate — the check name asserts coverage that does not exist. So an EMPTY corpus, or
# a target directory with no replayable entries, is a FAILURE, not a pass.
#
# Corpus layout (see fuzz-corpus/README.md):
#   fuzz-corpus/<target>/crash-<sha256[:12]>       # a reproducer for a fixed crash
#   fuzz-corpus/<target>/crash-<sha256[:12]>.md    # REQUIRED note naming the issue it fixes
#   fuzz-corpus/<target>/seed-<name>               # a known-good seed (no note required)

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CORPUS_DIR="${REPO_ROOT}/fuzz-corpus"
FUZZ_CARGO="${REPO_ROOT}/fuzz/Cargo.toml"
CHECK_ONLY=0
[[ "${1:-}" == "--check" ]] && CHECK_ONLY=1

if [[ ! -f "${FUZZ_CARGO}" ]]; then
  echo "ERROR: ${FUZZ_CARGO} not found" >&2
  exit 2
fi

# Known targets = [[bin]] names in fuzz/Cargo.toml. Parsed with a real TOML reader so a blank
# line or a reordered key cannot silently empty the list (the grep/sed form did — #2251 review).
read_targets() {
  python3 - "$FUZZ_CARGO" <<'PY'
import sys, tomllib
with open(sys.argv[1], "rb") as f:
    data = tomllib.load(f)
for b in data.get("bin", []):
    name = b.get("name")
    if name:
        print(name)
PY
}
mapfile -t KNOWN_TARGETS < <(read_targets)
if [[ "${#KNOWN_TARGETS[@]}" -eq 0 ]]; then
  echo "ERROR: parsed 0 [[bin]] targets from ${FUZZ_CARGO} — target list is empty" >&2
  exit 2
fi

is_known_target() {
  local t="$1"
  for k in "${KNOWN_TARGETS[@]}"; do [[ "$k" == "$t" ]] && return 0; done
  return 1
}

if [[ ! -d "${CORPUS_DIR}" ]]; then
  echo "ERROR: ${CORPUS_DIR} does not exist — the replay gate has nothing to replay." >&2
  exit 1
fi

total_entries=0
target_dirs=0
fail=0
covered_targets=()

shopt -s nullglob
for target_dir in "${CORPUS_DIR}"/*/; do
  target="$(basename "${target_dir}")"
  target_dirs=$((target_dirs + 1))
  covered_targets+=("${target}")
  if ! is_known_target "${target}"; then
    echo "ERROR: corpus dir '${target}' does not match any [[bin]] in fuzz/Cargo.toml" >&2
    fail=1
    continue
  fi

  # Collect replayable entries (everything that is not a .md note).
  dir_entries=0
  for entry in "${target_dir}"*; do
    base="$(basename "${entry}")"
    [[ "${base}" == *.md ]] && continue
    [[ -f "${entry}" ]] || continue
    dir_entries=$((dir_entries + 1))
    total_entries=$((total_entries + 1))

    # Policy enforcement: a crash reproducer must carry a paired note naming its issue
    # (fuzz-corpus/README.md §3). Without this the note requirement is aspirational and drifts.
    if [[ "${base}" == crash-* && ! -f "${entry}.md" ]]; then
      echo "ERROR: reproducer '${target}/${base}' has no paired note '${base}.md' naming its issue" >&2
      fail=1
    fi

    if [[ "${CHECK_ONLY}" == "1" ]]; then
      echo "  [check] ${target}/${base}"
      continue
    fi
    echo "==> replay ${target} < ${base}"
    # Single-input replay: cargo-fuzz runs the target once on the given file and exits
    # non-zero (and prints the panic) if it crashes.
    if ! ( cd "${REPO_ROOT}/fuzz" && cargo +nightly fuzz run "${target}" "${entry}" -- -runs=1 ); then
      echo "CRASH: ${target} panicked on committed corpus entry ${base}" >&2
      fail=1
    fi
  done

  # A committed target directory with no replayable entry is a defect: it looks like coverage
  # but replays nothing. Fail rather than silently skip it.
  if [[ "${dir_entries}" -eq 0 ]]; then
    echo "ERROR: corpus dir '${target}' has no replayable entries (only notes or empty)" >&2
    fail=1
  fi
done

# Reconcile the other direction: a KNOWN target with no committed input is uncovered, and
# until now the script printed "(N known targets)" next to a smaller covered count and exited
# 0 anyway (#2251 review). Observing a violation and declining to act on it is the same
# fail-open shape as an empty corpus reporting green — the check name asserts per-target
# replay, so a target with nothing to replay must fail.
uncovered=()
for known in "${KNOWN_TARGETS[@]}"; do
  found=0
  for covered in ${covered_targets[@]+"${covered_targets[@]}"}; do
    [[ "${covered}" == "${known}" ]] && { found=1; break; }
  done
  [[ "${found}" -eq 0 ]] && uncovered+=("${known}")
done
if [[ "${#uncovered[@]}" -gt 0 ]]; then
  echo "ERROR: ${#uncovered[@]} known fuzz target(s) have no committed corpus input:" >&2
  for t in "${uncovered[@]}"; do echo "         ${t}" >&2; done
  echo "       Seed each with at least one known-good input under fuzz-corpus/<target>/," >&2
  echo "       or remove the [[bin]] from fuzz/Cargo.toml. A target the gate never replays" >&2
  echo "       is coverage this check claims and does not have." >&2
  fail=1
fi

# The core anti-vacuity guard: an empty corpus must FAIL, not report a green "replayed nothing".
if [[ "${target_dirs}" -eq 0 || "${total_entries}" -eq 0 ]]; then
  echo "ERROR: fuzz-corpus is empty (${target_dirs} target dirs, ${total_entries} entries)." >&2
  echo "       An empty corpus makes the replay gate vacuous and green forever. Seed at least" >&2
  echo "       one known-good input per active target before this gate means anything." >&2
  exit 1
fi

echo "Replayed ${total_entries} committed corpus entries across ${target_dirs} corpus target dirs (${#KNOWN_TARGETS[@]} known targets)."
if [[ "${fail}" != "0" ]]; then
  echo "FAIL: a committed corpus entry crashed, lacked a required note, or an unknown/empty target dir exists." >&2
  exit 1
fi
echo "OK: no committed corpus entry crashed."
