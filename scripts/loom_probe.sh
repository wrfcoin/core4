#!/usr/bin/env bash
# Acceptance probe for the loom interleaving models (#2109).
#
# A model whose assertions hold for every interleaving — including ones the real
# code cannot produce — proves nothing. Before trusting the models in
# shared-protocol/tests/loom_mempool.rs, prove they are capable of failing:
# plant a deliberate ordering bug, assert loom finds it, restore the file.
#
#   scripts/loom_probe.sh
#
# Exit 0 = loom caught the planted bug (models are load-bearing).
# Exit 1 = loom did NOT catch it, which means the harness is not exploring
#          interleavings and every green run below it is meaningless.
#
# The planted bug is a real check-then-act race on the PRODUCTION `Mempool`:
# two threads each read `size()`, then admit a transaction only if the pool
# looked empty — across two separate lock acquisitions. Both can observe 0 and
# both admit, so the `size() <= 1` assertion must fail on at least one
# interleaving. That is the same shape as the hazard
# `check_then_act_admission_cannot_corrupt_accounting` models, which is why
# catching it is evidence about those models specifically and not just about
# loom in general.
#
# Cleanup runs on EVERY exit path via trap. The fuzz corpus-replay self-test
# (#2251 review) shipped a version whose cleanup left a directory behind and
# turned the real gate red; the mechanism that proves a gate works must not be
# able to break it.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODEL_FILE="${REPO_ROOT}/shared-protocol/tests/loom_mempool.rs"
BACKUP="$(mktemp)"
PROBE_TEST="probe_planted_check_then_act_race"

if [[ ! -f "${MODEL_FILE}" ]]; then
  echo "ERROR: ${MODEL_FILE} not found" >&2
  exit 2
fi

cleanup() {
  # Restore unconditionally, even on Ctrl-C or a failed cargo invocation.
  if [[ -s "${BACKUP}" ]]; then
    cp "${BACKUP}" "${MODEL_FILE}"
  fi
  rm -f "${BACKUP}"
}
trap cleanup EXIT INT TERM

cp "${MODEL_FILE}" "${BACKUP}"

cat >> "${MODEL_FILE}" <<'PROBE'

// ---------------------------------------------------------------------------
// PLANTED BY scripts/loom_probe.sh — removed automatically. If you are reading
// this in a committed file, the probe's cleanup trap failed; delete this block.
// ---------------------------------------------------------------------------
#[test]
fn probe_planted_check_then_act_race() {
    loom::model(|| {
        let pool = LoomArc::new(Mutex::new(new_pool()));
        let spawn_one = |pool: LoomArc<Mutex<Mempool>>, tag: &'static str| {
            thread::spawn(move || {
                // Deliberate bug: decide on a stale read, act after releasing.
                let looked_empty = pool.lock().unwrap().size() == 0;
                if looked_empty {
                    let _ = pool.lock().unwrap().add_transaction(tx(tag));
                }
            })
        };
        let a = spawn_one(pool.clone(), "probe-a");
        let b = spawn_one(pool.clone(), "probe-b");
        a.join().unwrap();
        b.join().unwrap();
        // Must fail on the interleaving where both threads read 0 and both admit.
        assert!(
            pool.lock().unwrap().size() <= 1,
            "planted race admitted twice"
        );
    });
}
PROBE

echo "[loom-probe] planted deliberate ordering bug; expecting loom to FAIL it"

set +e
OUTPUT="$(cd "${REPO_ROOT}" && cargo test --locked -p wrfcoin-shared-protocol \
  --test loom_mempool "${PROBE_TEST}" 2>&1)"
STATUS=$?
set -e

if [[ ${STATUS} -eq 0 ]]; then
  echo "${OUTPUT}" | tail -20
  cat >&2 <<'MSG'

FAIL: loom did NOT catch the planted check-then-act race.

The models in shared-protocol/tests/loom_mempool.rs are therefore not exploring
interleavings, and a green run of them establishes nothing. Do not trust the
"Run loom interleaving models" step below this one until this probe fails as
designed (core4#2109).
MSG
  exit 1
fi

if ! grep -q "planted race admitted twice" <<<"${OUTPUT}"; then
  echo "${OUTPUT}" | tail -30
  cat >&2 <<'MSG'

FAIL: the probe test failed, but NOT on its planted assertion.

That means the run died for an unrelated reason (compile error, panic elsewhere)
and this probe proved nothing about loom's exploration. Investigate before
trusting the models (core4#2109).
MSG
  exit 1
fi

echo "[loom-probe] OK — loom found the planted race:"
grep -m1 "planted race admitted twice" <<<"${OUTPUT}" | sed 's/^/[loom-probe]   /'
echo "[loom-probe] models are capable of failing; restoring ${MODEL_FILE##*/}"
