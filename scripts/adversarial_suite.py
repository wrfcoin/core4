#!/usr/bin/env python3
"""Adversarial-suite runner, corpus guard, and determinism comparator (#2311).

`.github/workflows/adversarial-tests.yml` was decorative. It ran only the 258
`launch_api` **unit** tests (`cargo test --bin launch_api`) — never the adversarial
integration test targets; its "determinism" step compared pass COUNTS, not test
identities; and its regression script did not exist, so it printed "skipping". The
#2311 negative control proved a deliberately failing adversarial integration test
left the gate green, because it was never compiled or run.

This module is the load-bearing part the workflow lacked:

- an explicit MANIFEST of adversarial `--test` targets (replay, partition, reorg,
  oracle, churn, bridge). `--check-corpus` FAILS CLOSED if any target is absent —
  the exact regression #2311 found, where the dedicated `--test adversarial` target
  was removed by 27ac70c0 and nothing noticed;
- `--run` executes each target with `cargo test --locked -p <crate> --test <target>`,
  so the adversarial tests actually run and a failing one turns the gate red;
- `--compare` diffs a real deterministic artifact — the ORDERED list of executed
  test identities and their results between two runs — so two outputs with the same
  count but different test identities are caught (the #2311 determinism defect),
  which a `grep -c` count never was.

    python3 scripts/adversarial_suite.py --check-corpus       # targets exist? (fail-closed)
    python3 scripts/adversarial_suite.py --run                # cargo test each target
    python3 scripts/adversarial_suite.py --compare a.txt b.txt  # ordered-identity diff
    python3 scripts/adversarial_suite.py --self-test          # hermetic acceptance probe

Exit 0 = the requested check passed. Exit 1 = a target is missing / a test failed /
the two runs diverged. Exit 2 = usage error.
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]

# The adversarial corpus, by attack class. Each target is a real integration test
# under `<crate-dir>/tests/<target>.rs`. Removing a target (as 27ac70c0 did to the
# original `--test adversarial`) makes `--check-corpus` fail, so the gate can no
# longer go green while its advertised coverage silently disappears.
#   (crate, cargo target name, path relative to repo root, attack class)
MANIFEST: list[tuple[str, str, str, str]] = [
    ("wrfcoin-native-chain", "security_attacks",
     "native-chain/tests/security_attacks.rs", "forgery/tamper"),
    ("wrfcoin-native-chain", "partition_mode_block_production",
     "native-chain/tests/partition_mode_block_production.rs", "partition"),
    ("wrfcoin-native-chain", "partition_mode_routes",
     "native-chain/tests/partition_mode_routes.rs", "partition"),
    ("wrfcoin-native-chain", "wal_replay_finalization_determinism_1980",
     "native-chain/tests/wal_replay_finalization_determinism_1980.rs", "replay-determinism"),
    ("wrfcoin-native-chain", "oracle_registration_wal_replay_2055",
     "native-chain/tests/oracle_registration_wal_replay_2055.rs", "replay"),
    ("wrfcoin-native-chain", "reorg_finalization_parity_1810",
     "native-chain/tests/reorg_finalization_parity_1810.rs", "reorg"),
    ("wrfcoin-native-chain", "reorg_persist_batch_recalc_1977",
     "native-chain/tests/reorg_persist_batch_recalc_1977.rs", "reorg"),
    ("wrfcoin-native-chain", "peer_churn_e2e",
     "native-chain/tests/peer_churn_e2e.rs", "churn"),
    ("wrfcoin-consensus-engines", "security_bridge",
     "consensus-engines/tests/security_bridge.rs", "bridge-security"),
]

# The determinism artifact is produced by re-running this target: it asserts
# producer-vs-WAL-replay `state_root` parity, and its per-test identities are what
# `--compare` diffs across two runs.
DETERMINISM_TARGET = ("wrfcoin-native-chain", "wal_replay_finalization_determinism_1980")

# `test <path::name> ... ok|FAILED|ignored` — the libtest per-test result line.
RESULT_RE = re.compile(r"^test (?P<name>\S+) \.\.\. (?P<result>ok|FAILED|ignored)\s*$", re.M)
SUMMARY_RE = re.compile(
    r"test result: (?:ok|FAILED)\. (?P<passed>\d+) passed; "
    r"(?P<failed>\d+) failed; (?P<ignored>\d+) ignored",
)

# A target may compile to an empty integration crate when its test body is
# feature-gated.  The corpus must declare that profile explicitly; otherwise a
# successful Cargo exit would be a green-shaped skipped adversarial target.
TARGET_FEATURES: dict[str, tuple[str, ...]] = {
    "peer_churn_e2e": ("p2p",),
}


def executed_test_count(output: str) -> int:
    """Count non-ignored tests from libtest summaries in one target run."""
    return sum(int(m["passed"]) + int(m["failed"]) for m in SUMMARY_RE.finditer(output))


def missing_targets(manifest: list[tuple[str, str, str, str]], root: Path) -> list[str]:
    """Manifest entries whose test file is absent — a fail-closed corpus gap."""
    return [f"{crate}::{target} ({path})"
            for crate, target, path, _ in manifest
            if not (root / path).is_file()]


def check_corpus() -> int:
    missing = missing_targets(MANIFEST, REPO)
    if missing:
        print("FAIL: adversarial corpus is incomplete — these targets are absent (#2311):",
              file=sys.stderr)
        for m in missing:
            print(f"  - {m}", file=sys.stderr)
        print("  The gate must not run green while its advertised coverage is missing.",
              file=sys.stderr)
        return 1
    classes = sorted({cls for *_, cls in MANIFEST})
    print(f"adversarial corpus OK: {len(MANIFEST)} targets present, "
          f"covering {len(classes)} attack classes ({', '.join(classes)})")
    return 0


def identities(output: str) -> list[tuple[str, str]]:
    """Ordered (test-name, result) pairs — the deterministic artifact to compare.

    Ordered and identity-bearing on purpose: the #2311 defect was that two outputs
    with completely different passing tests but the same COUNT were called
    'Determinism verified'. Comparing identities and order catches that.
    """
    return [(m.group("name"), m.group("result")) for m in RESULT_RE.finditer(output)]


ANCHOR = REPO / "tests/integration/fixtures/determinism/expected_roots.json"

# The oracle is KNOWN-RED at every height right now, and that is a real signal we
# are deliberately not silencing: it is stale drift from the post-oracle
# `gamification_claims_seen` root material, not nondeterminism (proven on #2311).
#
# Regenerating it now would be doubly wrong. It would convert a real signal into a
# green light, and #2398 bumps the state root v8 -> v9, which changes the roots
# again — so the regenerated file would be stale on arrival. Reconciliation is
# therefore deferred until after #2398 merges, tracked as a follow-up.
#
# Until then the anchor is reported LOUDLY but does not gate. `--require-anchor`
# is the switch the follow-up flips once the oracle is regenerated; it exists now
# so that flipping it is a one-word change rather than a rediscovery.
ANCHOR_DEFERRED_NOTE = (
    "oracle reconciliation is a TRACKED FOLLOW-UP gated on #2398 (state root v8 -> v9). "
    "Not regenerated here: that would turn a real signal green, and #2398 changes the "
    "roots again. Re-enable gating with --require-anchor once the oracle is regenerated."
)


def _load_roots_module():
    """Reuse scripts/compare_state_roots.py rather than reimplementing its checks."""
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    import compare_state_roots  # noqa: E402

    return compare_state_roots


def compare_roots(roots1: Path, roots2: Path) -> list[str]:
    """Byte-for-byte comparison of the two legs' raw root files.

    Byte-for-byte is deliberate and is the whole point of this check. The
    per-height comparison below is for the human reading the failure; the verdict
    is taken on the raw bytes, because anything that normalises before comparing
    is a place a difference can hide.
    """
    errors: list[str] = []
    for f in (roots1, roots2):
        if not f.is_file():
            errors.append(f"MISSING ROOTS: {f} does not exist (fail-closed)")
    if errors:
        return errors

    a, b = roots1.read_bytes(), roots2.read_bytes()
    if not a.strip():
        return [f"EMPTY ROOTS: {roots1} is empty (fail-closed)"]
    if a == b:
        return []

    errors.append("STATE-ROOT DIVERGENCE: the two runs produced different roots (#2311).")
    errors.append(f"  {roots1.name}: {len(a)} bytes")
    errors.append(f"  {roots2.name}: {len(b)} bytes")
    # Per-height detail, via the existing tool, so the operator sees WHICH height.
    try:
        csr = _load_roots_module()
        for line in csr.compare([roots1, roots2], None):
            errors.append(f"  {line}")
    except Exception as exc:  # diagnostics must never mask the verdict
        errors.append(f"  (per-height diagnostic unavailable: {exc})")
    return errors


def check_anchor(roots: list[Path]) -> list[str]:
    """Compare the legs against the committed oracle. Reported, not gated (see above)."""
    if not ANCHOR.is_file():
        return [f"anchor {ANCHOR} not found"]
    try:
        csr = _load_roots_module()
        return csr.compare(roots, ANCHOR)
    except Exception as exc:
        return [f"anchor check unavailable: {exc}"]


def compare(run1: Path, run2: Path, roots1: Path | None = None,
            roots2: Path | None = None, require_anchor: bool = False,
            allow_no_roots: bool = False, skip_anchor: bool = False) -> int:
    """Determinism verdict across two runs of the replay-determinism target.

    TWO artifacts are compared, because the first one alone was vacuous (#2311,
    Frank's finding):

    1. **Ordered libtest identities/results.** Catches a different SET of tests
       running between the legs.
    2. **Raw per-height state roots, byte-for-byte.** This is the artifact that
       actually carries determinism. Comparing only (1) meant two runs whose
       `STATE_ROOT` genuinely differed still compared EQUAL and printed
       "determinism verified", exit 0 — proven with two artifacts differing only
       in STATE_ROOT. A determinism gate that cannot observe the state root is
       not a determinism gate.
    """
    a = identities(run1.read_text(encoding="utf-8", errors="replace"))
    b = identities(run2.read_text(encoding="utf-8", errors="replace"))
    failures: list[str] = []

    if not a:
        print(f"FAIL: {run1} contains no libtest results — nothing to compare (fail-closed)",
              file=sys.stderr)
        return 1

    # --- artifact 1: ordered identities -----------------------------------
    if a != b:
        failures.append("NON-DETERMINISTIC TEST IDENTITIES — the two runs differ (#2311):")
        sa, sb = dict(a), dict(b)
        for name in sorted(set(sa) | set(sb)):
            if sa.get(name) != sb.get(name):
                failures.append(
                    f"  {name}: run1={sa.get(name, '<absent>')} run2={sb.get(name, '<absent>')}")
        if [n for n, _ in a] != [n for n, _ in b] and sa == sb:
            failures.append("  (same identities/results but different ORDER)")

    # --- artifact 2: raw state roots --------------------------------------
    if roots1 is None or roots2 is None:
        if not allow_no_roots:
            print("FAIL: no root files given. The identity artifact alone is VACUOUS for "
                  "determinism — two runs with different state roots compare equal (#2311).\n"
                  "  Pass --roots ROOTS1 ROOTS2, each produced by:\n"
                  "    cargo run -p test_server --example emit_roots --release > roots-N.json\n"
                  "  or --no-roots for identity-only diffing (never in CI).",
                  file=sys.stderr)
            return 1
        print("WARNING: --no-roots given. Only test identities were compared, which does "
              "NOT verify determinism: two runs with different state roots would pass here.",
              file=sys.stderr)
    else:
        failures.extend(compare_roots(roots1, roots2))

    if failures:
        print("FAIL: determinism check failed (#2311):", file=sys.stderr)
        for line in failures:
            print(line, file=sys.stderr)
        return 1

    # --- anchor: reported loudly, gated only under --require-anchor --------
    anchor_errors: list[str] = []
    if roots1 is not None and roots2 is not None and not skip_anchor:
        anchor_errors = check_anchor([roots1, roots2])

    if anchor_errors:
        banner = "=" * 72
        stream = sys.stderr if require_anchor else sys.stdout
        print(f"\n{banner}", file=stream)
        print("ORACLE DRIFT: the legs agree with each other but NOT with the committed",
              file=stream)
        print(f"expected_roots.json anchor. {len(anchor_errors)} finding(s):", file=stream)
        for line in anchor_errors:
            print(f"  {line}", file=stream)
        print("", file=stream)
        print(f"{ANCHOR_DEFERRED_NOTE}", file=stream)
        print(f"{banner}\n", file=stream)
        if require_anchor:
            print("FAIL: --require-anchor was given and the anchor does not match.",
                  file=sys.stderr)
            return 1

    roots_compared = roots1 is not None and roots2 is not None
    if roots_compared:
        print(f"determinism verified: both runs executed the identical {len(a)} test "
              "identities, in the same order, with the same results, AND produced "
              "byte-identical per-height state roots"
              + (" (anchor drift reported above — does not gate)" if anchor_errors else ""))
    else:
        # Must NOT claim the roots matched: none were compared. Overstating what
        # was checked is how the vacuous gate read as trustworthy in the first place.
        print(f"identities only: both runs executed the identical {len(a)} test "
              "identities, in the same order, with the same results. State roots were "
              "NOT compared, so this does NOT verify determinism (#2311).")
    return 0


def run() -> int:
    if check_corpus() != 0:
        return 1
    failed: list[str] = []
    for crate, target, _path, cls in MANIFEST:
        print(f"\n=== [{cls}] cargo test -p {crate} --test {target} ===", flush=True)
        command = ["cargo", "test", "--locked", "-p", crate]
        features = TARGET_FEATURES.get(target, ())
        if features:
            command.extend(["--features", ",".join(features)])
        command.extend(["--test", target, "--", "--nocapture"])
        proc = subprocess.run(
            command, cwd=REPO, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
        )
        print(proc.stdout, end="")
        if proc.returncode != 0:
            failed.append(f"{crate}::{target}")
        elif executed_test_count(proc.stdout) == 0:
            failed.append(f"{crate}::{target} (zero executed tests)")
    if failed:
        print(f"\nFAIL: {len(failed)} adversarial target(s) failed: {', '.join(failed)}",
              file=sys.stderr)
        return 1
    print(f"\nAll {len(MANIFEST)} adversarial targets passed.")
    return 0


def self_test() -> int:
    """Hermetic acceptance probe. Proves the gate is load-bearing without compiling:
    reverting either fail-closed rule below turns it RED."""
    # 1. corpus guard: a manifest naming a target that does not exist must fail.
    bogus = [("wrfcoin-native-chain", "no_such_adversarial_target",
              "native-chain/tests/no_such_adversarial_target.rs", "replay")]
    assert missing_targets(bogus, REPO), \
        "SELF-TEST FAILED: a missing adversarial target was not detected (corpus guard is open)"
    # and the real manifest must be complete.
    assert not missing_targets(MANIFEST, REPO), \
        f"SELF-TEST FAILED: real corpus is incomplete: {missing_targets(MANIFEST, REPO)}"

    # 2. determinism comparator: the #2311 defect — SAME count, DIFFERENT identities.
    good = ("test a::one ... ok\ntest a::two ... ok\n"
            "test result: ok. 2 passed; 0 failed\n")
    same_count_diff_ids = ("test a::three ... ok\ntest a::four ... ok\n"
                           "test result: ok. 2 passed; 0 failed\n")
    with tempfile.TemporaryDirectory() as d:
        root = Path(d)
        g1, g2 = root / "g1.txt", root / "g2.txt"
        g1.write_text(good, encoding="utf-8")
        g2.write_text(good, encoding="utf-8")
        assert compare(g1, g2, allow_no_roots=True) == 0, \
            "identical runs must verify (identity artifact alone)"

        b2 = root / "b2.txt"
        b2.write_text(same_count_diff_ids, encoding="utf-8")
        assert compare(g1, b2, allow_no_roots=True) == 1, (
            "SELF-TEST FAILED: two runs with the same pass count but different test "
            "identities were treated as deterministic — this is the #2311 defect a "
            "grep -c count never caught")

        # a result flip (ok -> FAILED) at the same identity must also fail.
        flipped = root / "flip.txt"
        flipped.write_text("test a::one ... ok\ntest a::two ... FAILED\n", encoding="utf-8")
        assert compare(g1, flipped, allow_no_roots=True) == 1, "a result flip must fail"

        # empty output must fail closed, not pass vacuously.
        empty = root / "empty.txt"
        empty.write_text("", encoding="utf-8")
        assert compare(empty, empty, allow_no_roots=True) == 1, "empty output must fail closed"

        # 3. THE #2311 DEFECT ITSELF (Frank's finding): two runs whose libtest
        #    output is identical but whose STATE ROOTS differ must FAIL. Before
        #    the fix these compared EQUAL and printed "determinism verified".
        r1, r2 = root / "roots1.json", root / "roots2.json"
        roots_a = '{\n  "0": "aa00",\n  "1": "bb11"\n}\n'
        r1.write_text(roots_a, encoding="utf-8")
        # differs in exactly ONE byte of one root
        r2.write_text(roots_a.replace('"bb11"', '"bb12"'), encoding="utf-8")
        assert compare(g1, g2, r1, r2, skip_anchor=True) == 1, (
            "SELF-TEST FAILED: two runs with identical test identities but DIFFERENT "
            "state roots were called deterministic — this is the exact #2311 defect: "
            "the comparator only looked at libtest identities, so a genuine state_root "
            "divergence compared EQUAL and exited 0")

        # identical roots + identical identities must PASS.
        r2.write_text(roots_a, encoding="utf-8")
        assert compare(g1, g2, r1, r2, skip_anchor=True) == 0, \
            "identical runs and identical roots must verify"

        # roots omitted must FAIL CLOSED rather than silently degrade to the
        # vacuous identity-only comparison.
        assert compare(g1, g2) == 1, (
            "SELF-TEST FAILED: omitting the root files silently degraded to the "
            "identity-only comparison, which is the vacuous check #2311 found")

        # a missing / empty root file is fail-closed too.
        gone = root / "absent.json"
        assert compare(g1, g2, r1, gone, skip_anchor=True) == 1, \
            "a missing root file must fail closed"
        blank = root / "blank.json"
        blank.write_text("", encoding="utf-8")
        assert compare(g1, g2, blank, blank, skip_anchor=True) == 1, \
            "an empty root file must fail closed"

    assert executed_test_count("test result: ok. 0 passed; 0 failed; 1 ignored\n") == 0, \
        "SELF-TEST FAILED: all-ignored target was counted as executed"
    assert executed_test_count("test result: ok. 2 passed; 1 failed; 0 ignored\n") == 3, \
        "SELF-TEST FAILED: non-ignored libtest results were not counted"

    print("adversarial_suite.py self-test passed "
          "(corpus guard catches a missing target; determinism compare catches "
          "same-count/different-identity, result flips, empty output, AND — the "
          "#2311 defect — identical test identities with divergent state roots)")
    return 0


def main(argv: list[str]) -> int:
    p = argparse.ArgumentParser(description=__doc__,
                                formatter_class=argparse.RawDescriptionHelpFormatter)
    g = p.add_mutually_exclusive_group(required=True)
    g.add_argument("--check-corpus", action="store_true",
                   help="fail closed if any adversarial target is absent")
    g.add_argument("--run", action="store_true", help="cargo test every adversarial target")
    g.add_argument("--compare", nargs=2, metavar=("RUN1", "RUN2"), type=Path,
                   help="diff ordered test identities/results between two run outputs")
    g.add_argument("--self-test", action="store_true", help="hermetic acceptance probe")
    p.add_argument("--roots", nargs=2, metavar=("ROOTS1", "ROOTS2"), type=Path,
                   help="per-leg state-root JSON from `cargo run -p test_server "
                        "--example emit_roots` — REQUIRED with --compare; without it the "
                        "comparison is vacuous for determinism (#2311)")
    p.add_argument("--no-roots", action="store_true",
                   help="compare test identities only — ad-hoc local use, NEVER in CI")
    p.add_argument("--require-anchor", action="store_true",
                   help="also GATE on the committed expected_roots.json anchor. Off by "
                        "default while the oracle is known-red pending #2398; the "
                        "follow-up flips this on after regenerating it")
    args = p.parse_args(argv[1:])

    if args.check_corpus:
        return check_corpus()
    if args.run:
        return run()
    if args.compare:
        if args.roots and args.no_roots:
            print("error: --roots and --no-roots are mutually exclusive", file=sys.stderr)
            return 2
        r1, r2 = (args.roots[0], args.roots[1]) if args.roots else (None, None)
        return compare(args.compare[0], args.compare[1], r1, r2,
                       require_anchor=args.require_anchor,
                       allow_no_roots=args.no_roots)
    if args.self_test:
        return self_test()
    return 2


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
