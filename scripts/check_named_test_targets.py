#!/usr/bin/env python3
"""Run the integration targets that no other workflow names, and classify each.

# Why this exists

Cargo integration targets are excluded by ``--workspace --lib``. In this repo a
target therefore runs in CI only if some step names it with ``--test``. The
mechanism that would otherwise cover them — ``ci-readiness.yml``'s
integration-target ratchet (#2490) — resolves to ``workflow_dispatch`` only,
because both ``schedule`` and ``pull_request`` are commented out (#2695), while
the banner above them claims ``pull_request`` is untouched.

The result: the tests that PROVE two shipped-by-default defects (#2712, #2713),
and the tests that guard their FIXES (#2718, #2719), are executed by nothing.
This script names them.

# The shape, inherited from #2711

Two phases, in this order:

* ``--self-test`` is the CONTROL. It exercises the classifier against synthetic
  cargo output and asserts it can produce each verdict — ok, mismatch and void.
  It must pass before the observation is worth reading, and it needs no cargo, so
  a broken toolchain cannot make it silently vacuous.
* the default run is the OBSERVATION, and it classifies three ways rather than
  two:

  ==========  ================================================================
  exit 0      every target matched its manifest expectation
  exit 1      REAL-RED, attributable: a target's outcome disagreed with its
              expectation. This is a finding about the code.
  exit 2      GATE BROKEN / VOID: a target is missing, compiled nothing, ran
              zero tests, or the manifest itself is wrong. **Not evidence about
              the code in either direction.**
  ==========  ================================================================

Collapsing 1 and 2 is how a broken gate gets read as a finding, and — worse — how
a real finding gets dismissed as "probably just the harness". A red must never be
a compile error wearing a finding's clothes.

# Expectations, and why one target expects RED

``chain_identity_split_2713`` is red by design: it asserts the property the system
should have (a node reports the identity it signs under) and #2713's divergent
literals are still present. A gate that demanded green from every target would be
permanently red for a reason that is not a defect, and would be silenced within a
week. So the manifest records the expected polarity per target, and an
expected-red target that starts PASSING is itself a mismatch — either the defect
was fixed (promote it) or the test was weakened (revert it). Both need a human.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_MANIFEST = Path(__file__).with_name("named_test_targets.json")

# `test result: FAILED. 2 passed; 4 failed; 0 ignored; ...`
RESULT_RE = re.compile(
    r"test result:\s+(?P<verdict>ok|FAILED)\.\s+(?P<passed>\d+)\s+passed;\s+(?P<failed>\d+)\s+failed"
)

# libtest prints the names of failing tests in a trailing block:
#
#     failures:
#         unset_chain_id_reports_and_signs_the_same_identity
#         empty_string_export_diverges_by_a_different_mechanism
#
#     test result: FAILED. 2 passed; 4 failed; ...
#
# The same word also heads the per-test stdout dumps above it, so only the block
# whose entries are bare indented identifiers is a name list.
FAILURE_BLOCK_RE = re.compile(r"^failures:\n((?:[ \t]+[\w:]+\n)+)", re.MULTILINE)

OK, MISMATCH, VOID = "ok", "mismatch", "void"


def failing_test_names(output: str) -> set[str]:
    """Extract the identities of the failing tests, not just how many there were.

    A count is substitutable: any one failure satisfies "something failed". The
    identity is not, which is the whole point of pinning it.
    """
    names: set[str] = set()
    for block in FAILURE_BLOCK_RE.findall(output):
        for line in block.splitlines():
            name = line.strip()
            if name:
                names.add(name)
    return names


def classify(
    expect: str,
    exit_code: int,
    output: str,
    expect_failures: list[str] | None = None,
) -> tuple[str, str]:
    """Classify one target's run. Pure, so `--self-test` can exercise it.

    Returns ``(verdict, reason)`` where verdict is one of OK / MISMATCH / VOID.

    For an ``expect: red`` target, ``expect_failures`` names the tests that are
    supposed to fail. Counting failures is not sufficient: a target that mixes
    controls with observations can keep its failure COUNT nonzero while the
    identity of the failure moves from an observation to a broken control, and a
    count-only check reads that substitution as the expected red. In
    ``chain_identity_split_2713`` that is not hypothetical — repairing #2713 means
    removing the very literals ``control_a_reporting_fallbacks_are_still_written_
    where_this_test_claims`` asserts are present, so the fix that should force a
    promote-to-green would instead be absorbed silently. Pinning the identities
    makes controls pass independently: a failing control is never in the expected
    set, so it is always a mismatch.
    """
    if expect not in ("green", "red"):
        return VOID, f"manifest expectation must be 'green' or 'red', got {expect!r}"

    matches = RESULT_RE.findall(output)
    if not matches:
        # No libtest summary at all: the target did not compile, the binary did
        # not start, or cargo failed before running anything. A nonzero exit here
        # looks exactly like a failing test and is not one.
        return VOID, (
            f"no libtest summary in output (exit {exit_code}) — the target did not run. "
            "This is not evidence that the code is right or wrong."
        )

    passed = sum(int(m[1]) for m in matches)
    failed = sum(int(m[2]) for m in matches)
    if passed + failed == 0:
        # `running 0 tests` exits 0. Green, and worth nothing.
        return VOID, "target executed ZERO tests — a green with no assertions is VOID"

    if expect == "green":
        if failed == 0 and exit_code == 0:
            return OK, f"{passed} passed, as expected"
        return MISMATCH, (
            f"expected GREEN but {failed} test(s) failed (exit {exit_code}). "
            "This is a finding about the code, not about the harness."
        )

    # expect == "red"
    if failed == 0 or exit_code == 0:
        return MISMATCH, (
            f"expected RED but the target PASSED ({passed} passed, {failed} failed). "
            "Either the defect was fixed — promote this target to expect 'green' — or the "
            "test was weakened. Do not silence it; both cases need a human."
        )

    # Nonzero failures. WHICH ones decides the verdict.
    if not expect_failures:
        return VOID, (
            "manifest declares expect 'red' but names no expect_failures. An "
            "unpinned red accepts ANY failure, so a broken control can stand in for "
            "the observation it was meant to guard. Name the failing tests."
        )

    observed = failing_test_names(output)
    if not observed:
        return VOID, (
            f"{failed} test(s) failed but libtest printed no failure-name block, so "
            "the identity of the failure cannot be checked against the manifest. "
            "Not evidence about the code in either direction."
        )

    expected = set(expect_failures)
    unexpected = sorted(observed - expected)
    missing = sorted(expected - observed)
    if not unexpected and not missing:
        return OK, f"{failed} failed / {passed} passed — RED as expected ({len(expected)} pinned)"

    parts = []
    if unexpected:
        parts.append(f"UNEXPECTED failure(s): {', '.join(unexpected)}")
    if missing:
        parts.append(f"expected failure(s) that PASSED: {', '.join(missing)}")
    return MISMATCH, (
        "expected RED, and the target is red, but by the wrong tests — "
        + "; ".join(parts)
        + ". A control that fails, or an observation that starts passing, must not be "
        "absorbed by the failure count. If the defect was fixed, promote this target "
        "to 'green'; if a control broke, repair it. Both need a human."
    )


def load_manifest(path: Path) -> dict:
    manifest = json.loads(path.read_text())
    if manifest.get("schema") != 1:
        raise SystemExit(f"FAIL-CLOSED: manifest schema must be 1, got {manifest.get('schema')!r}")
    if not manifest.get("targets"):
        raise SystemExit("FAIL-CLOSED: manifest lists no targets; a gate over nothing is VOID")
    return manifest


def preflight(manifest: dict) -> list[str]:
    """Structural checks that must hold before any target is run.

    Every one of these is a way the gate could report green while covering less
    than it claims — a silently shrunken list is the failure this whole file
    exists to prevent, so it must be loud rather than absent.
    """
    errors: list[str] = []
    seen: set[str] = set()

    for entry in manifest["targets"]:
        target = entry.get("target", "<unnamed>")
        if target in seen:
            errors.append(f"{target}: listed twice in the manifest")
        seen.add(target)
        for field in ("target", "package", "path", "expect", "why"):
            if not entry.get(field):
                errors.append(f"{target}: manifest entry is missing '{field}'")
        # An `expect: red` target must name WHICH tests fail. A failure count is
        # substitutable: a broken control can stand in for the observation it was
        # meant to guard, and the aggregate stays red while the exception it
        # encodes quietly becomes permanent. Only runnable targets are checked — a
        # `pending` entry is not executed here so its identities cannot be verified;
        # the promotion check below moves it into this list first, where it is.
        if entry.get("expect") == "red" and not entry.get("expect_failures"):
            errors.append(
                f"{target}: expect 'red' with no 'expect_failures'. An unpinned red "
                "accepts ANY failure, so a broken control can stand in for the "
                "observation it guards. Name the tests that are supposed to fail."
            )
        path = ROOT / entry.get("path", "")
        if not path.is_file():
            errors.append(
                f"{target}: {entry.get('path')} does not exist. Either the target was "
                "renamed or deleted, or this manifest is stale — both make the gate's "
                "coverage claim false."
            )

    # A pending entry whose file has landed must be promoted, not silently skipped.
    for entry in manifest.get("pending", []):
        path = ROOT / entry.get("path", "")
        if path.is_file():
            errors.append(
                f"{entry.get('target')}: listed as PENDING but its file now exists at "
                f"{entry.get('path')}. Promote it into 'targets' — a pending entry that "
                "quietly landed is how a coverage list rots."
            )
    return errors


def run_target(entry: dict) -> tuple[int, str]:
    cmd = [
        "cargo", "test", "--locked",
        "-p", entry["package"],
        "--test", entry["target"],
    ]
    proc = subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True)
    return proc.returncode, proc.stdout + proc.stderr


def self_test() -> int:
    """CONTROL: prove the classifier can produce all three verdicts.

    Runs no cargo, so it stays meaningful when the toolchain is broken — which is
    exactly when a gate is most likely to be misread.
    """
    green_ok = "test result: ok. 5 passed; 0 failed; 0 ignored"
    zero = "test result: ok. 0 passed; 0 failed; 0 ignored"

    def red_run(names: list[str], passed: int) -> str:
        """Synthesise libtest output that fails exactly `names`."""
        block = "failures:\n" + "".join(f"    {n}\n" for n in names)
        return (
            block
            + f"\ntest result: FAILED. {passed} passed; {len(names)} failed; 0 ignored\n"
        )

    OBS = [
        "unset_chain_id_reports_and_signs_the_same_identity",
        "unset_network_id_reports_and_signs_the_same_identity",
        "declared_signing_defaults_disagree_with_the_reporting_fallbacks",
        "empty_string_export_diverges_by_a_different_mechanism",
    ]
    CONTROL = "control_a_reporting_fallbacks_are_still_written_where_this_test_claims"

    red_out = red_run(OBS, 2)
    # The substitution this gate exists to refuse: #2713 is FIXED, so all four
    # observations now pass — but the control that asserts the old literals are
    # present has gone stale and fails. One failure, nonzero exit, and a
    # count-only classifier calls that the expected red.
    fixed_but_stale_control = red_run([CONTROL], 5)
    extra_failure = red_run(OBS + [CONTROL], 1)
    one_observation_fixed = red_run(OBS[:3], 3)
    unnamed_failure = "test result: FAILED. 2 passed; 4 failed; 0 ignored"

    cases = [
        ("green expectation, green run", ("green", 0, green_ok, None), OK),
        ("green expectation, failing run", ("green", 101, red_out, None), MISMATCH),
        ("red expectation, exactly the pinned failures", ("red", 101, red_out, OBS), OK),
        ("red expectation, PASSING run", ("red", 0, green_ok, OBS), MISMATCH),
        # --- failure-IDENTITY regressions (the #2854 review finding) ---
        (
            "red: defect FIXED but a stale control fails in its place",
            ("red", 101, fixed_but_stale_control, OBS),
            MISMATCH,
        ),
        ("red: pinned failures plus an unexpected control", ("red", 101, extra_failure, OBS), MISMATCH),
        ("red: one pinned observation started passing", ("red", 101, one_observation_fixed, OBS), MISMATCH),
        ("red: failures present but unnamed", ("red", 101, unnamed_failure, OBS), VOID),
        ("red: manifest pins no failure identities", ("red", 101, red_out, []), VOID),
        # --- structural ---
        ("no libtest summary at all", ("green", 101, "error: could not compile", None), VOID),
        ("zero tests executed", ("green", 0, zero, None), VOID),
        ("zero tests, red expectation", ("red", 0, zero, OBS), VOID),
        ("bogus expectation", ("banana", 0, green_ok, None), VOID),
    ]

    failures = 0
    for name, args, expected in cases:
        verdict, reason = classify(*args)
        status = "ok" if verdict == expected else "SELF-TEST FAILED"
        if verdict != expected:
            failures += 1
        print(f"  [{status}] {name}: got {verdict} (expected {expected}) — {reason[:60]}")

    produced = {classify(*a)[0] for _, a, _ in cases}
    if produced != {OK, MISMATCH, VOID}:
        print(
            f"FAIL-CLOSED: the classifier produced only {sorted(produced)} across the "
            "self-test corpus. A classifier that cannot reach all three verdicts cannot "
            "distinguish a finding from a broken gate."
        )
        failures += 1

    if failures:
        print(f"SELF-TEST FAILED ({failures} case(s)) — the observation below would be VOID")
        return 2
    print(f"SELF-TEST OK: {len(cases)} cases, all three verdicts reachable")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, default=DEFAULT_MANIFEST)
    parser.add_argument("--self-test", action="store_true", help="run the control and exit")
    parser.add_argument(
        "--preflight-only",
        action="store_true",
        help="run the structural checks without invoking cargo",
    )
    args = parser.parse_args()

    if args.self_test:
        return self_test()

    manifest = load_manifest(args.manifest)

    errors = preflight(manifest)
    if errors:
        for error in errors:
            print(f"FAIL-CLOSED: {error}")
        print(
            "\nGATE BROKEN — the manifest does not describe this tree, so nothing below "
            "would be evidence about the code. Exit 2."
        )
        return 2

    targets = manifest["targets"]
    print(f"Preflight OK: {len(targets)} target(s) present and well-formed.")
    for entry in manifest.get("pending", []):
        print(
            f"PENDING (not run here): {entry['target']} — {entry.get('lands_with', 'unscheduled')}"
        )
    if args.preflight_only:
        return 0

    mismatches: list[str] = []
    voids: list[str] = []
    for entry in targets:
        exit_code, output = run_target(entry)
        verdict, reason = classify(
            entry["expect"], exit_code, output, entry.get("expect_failures")
        )
        label = {OK: "OK", MISMATCH: "MISMATCH", VOID: "VOID"}[verdict]
        print(f"\n[{label}] {entry['target']} (expect {entry['expect']}): {reason}")
        if verdict == MISMATCH:
            mismatches.append(f"{entry['target']}: {reason}")
            print(f"  why this target is gated: {entry['why']}")
        elif verdict == VOID:
            voids.append(f"{entry['target']}: {reason}")
            print(output[-1500:])

    if voids:
        print("\nGATE BROKEN / VOID — these targets produced no usable result:")
        for void in voids:
            print(f"  {void}")
        print(
            "This run is NOT evidence that the code is right, and NOT evidence that it "
            "is wrong. Fix the gate, then re-read. Exit 2."
        )
        return 2

    if mismatches:
        print("\nREAL-RED, and attributable — these targets disagreed with the manifest:")
        for mismatch in mismatches:
            print(f"  {mismatch}")
        print(
            "The control passed and every target executed tests, so this is a finding "
            "about the code. Exit 1."
        )
        return 1

    print(f"\nAll {len(targets)} target(s) matched their expected polarity.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
