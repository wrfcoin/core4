#!/usr/bin/env python3
"""Regression guard: the determinism-fixture exclusion must be path-anchored (#2886).

THE DEFECT

`detect-secrets==1.5.0` applies every `--exclude-files <pattern>` with
`regex.search`, not a full-path match. `secret-scanning.yml` excludes the
determinism golden fixture by a bare suffix pattern:

    tests/integration/fixtures/determinism/expected_roots\\.json$

`regex.search` has no notion of "the start of the path" unless the pattern
supplies one, so this also matches — and silently drops from scanning — any
other path ending the same way, e.g.:

    shadow/tests/integration/fixtures/determinism/expected_roots.json

A reviewer (Bob) proved this in a scratch repo with a tracked fake AWS key at
that nested path: the unanchored pattern excluded it from the scan entirely.
The fix is a single `^` anchor. A single character is exactly the kind of fix
that regresses silently on the next edit of this exclusion list — the gate
stays green while scanning less, the same failure shape as #2292 one file
over. So the anchor is asserted here, not trusted:

  * the workflow's exclusion pattern for this fixture must still be anchored
    (drift guard, read from the live workflow text — not a copy that could
    drift from it);
  * a real `detect-secrets scan` using that exact extracted pattern must
    still surface a secret planted at a suffix-collision path;
  * and the failure mode is demonstrated, not just described — the same scan
    with the anchor stripped must make that same secret vanish. A fixture you
    cannot watch fail proves nothing, which is the same defect one layer up.

Run: python3 scripts/test_secret_scan_exclude_anchoring.py
"""

from __future__ import annotations

import json
import re
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
WORKFLOW = REPO / ".github/workflows/secret-scanning.yml"

CANONICAL = "tests/integration/fixtures/determinism/expected_roots.json"
COLLISION = "shadow/tests/integration/fixtures/determinism/expected_roots.json"

failures: list[str] = []


def check(name: str, ok: bool, detail: str = "") -> None:
    print(f"  {'PASS' if ok else 'FAIL'}  {name}" + (f": {detail}" if detail and not ok else ""))
    if not ok:
        failures.append(f"{name}{(': ' + detail) if detail else ''}")


def extract_pattern() -> str:
    """Pull the determinism-fixture exclude pattern straight out of the workflow.

    Reading it out of the live file (rather than hardcoding a copy here) is
    the whole point: a copy can drift from the real pattern and stop
    guarding anything without ever failing. Mirrors how
    scripts/integration_test_ratchet.py's validate_manifest() reads
    ci-readiness.yml's own text instead of trusting a description of it.
    """
    text = WORKFLOW.read_text(encoding="utf-8")
    m = re.search(
        r"--exclude-files '([^']*expected_roots\\\.json\$)'",
        text,
    )
    if not m:
        return ""
    return m.group(1)


def scan(root: Path, exclude_pattern: str) -> dict:
    r = subprocess.run(
        ["detect-secrets", "scan", "--no-verify", "--exclude-files", exclude_pattern, "."],
        capture_output=True,
        text=True,
        cwd=root,
    )
    if r.returncode != 0:
        return {}
    return json.loads(r.stdout or "{}")


def build_fixture(root: Path) -> None:
    canonical = root / CANONICAL
    collision = root / COLLISION
    canonical.parent.mkdir(parents=True, exist_ok=True)
    collision.parent.mkdir(parents=True, exist_ok=True)

    # Both planted values below are assembled from fragments at runtime
    # rather than written as contiguous literals. This test's own purpose is
    # to plant secret-shaped content and prove the scanner still finds it at
    # the collision path -- so its source MUST NOT itself contain a
    # contiguous string a scanner would flag, or this file becomes exactly
    # the kind of finding it exists to catch (confirmed: it did, on the
    # first version of this file -- core4#2886 remediation round 2). An
    # allowlist pragma was considered and rejected: it would suppress
    # detection at this exact source location permanently, which is a worse
    # footgun in a file whose entire job is planting secret-shaped strings
    # than fragmenting the literals is.

    # Canonical path: a real-shaped, 64-char lowercase-hex state root -- no
    # secret, must never be a finding regardless of exclusion, so it is not
    # itself part of the RED/GREEN assertion below. A 64-char run trips
    # HexHighEntropyString even split in half (32 chars is still enough);
    # joining 8-char pieces is below the threshold (verified empirically: 8
    # chars never trips it, 12+ always does, against both a synthetic string
    # and this exact value).
    state_root_pieces = [
        "cba327d2", "11e0992f", "746dd71e", "dbfbf4f0",
        "1669181f", "b202a7e6", "387812fc", "d77adeba",
    ]
    state_root = "".join(state_root_pieces)
    canonical.write_text(json.dumps({"0": state_root}), encoding="utf-8")

    # Collision path: a fake-but-real-shaped AWS access key. This is the
    # planted secret the anchor must keep visible to the scanner. Split so
    # the AKIA prefix and the following 16 characters never appear
    # contiguously in this file.
    fake_aws_key = "AKIA" + "3H7QZPX9WKMR2VBD"
    collision.write_text(json.dumps({"0": fake_aws_key}), encoding="utf-8")
    # detect-secrets enumerates candidate files via git, so an un-tracked
    # scratch tree scans as empty regardless of exclusion — that would make
    # every assertion below pass vacuously. Commit it for real.
    subprocess.run(["git", "init", "-q"], cwd=root, check=True)
    subprocess.run(["git", "add", "-A"], cwd=root, check=True)
    subprocess.run(
        ["git", "-c", "user.email=t@t", "-c", "user.name=t", "commit", "-q", "-m", "init"],
        cwd=root,
        check=True,
    )


def test_pattern_is_anchored() -> None:
    """The drift guard. This is the durable half of the fix."""
    pattern = extract_pattern()
    check(
        "workflow still excludes the determinism fixture by this pattern shape",
        bool(pattern),
        "could not find an expected_roots.json exclude-files pattern in secret-scanning.yml",
    )
    if not pattern:
        return
    check(
        "the pattern is anchored to the start of the repo-relative path",
        pattern.startswith("^"),
        f"pattern is {pattern!r} — regex.search() will match this suffix anywhere in the tree",
    )


def test_harness_actually_detects_the_planted_secret() -> None:
    """Sanity canary: prove the fixture+detector work at all before trusting
    either RED or GREEN below.

    Without this, a broken harness (e.g. git enumeration silently returning
    nothing — the exact trap that produced a false pass on the first version
    of this file) makes the RED case ("collision path NOT found") pass for
    the wrong reason: nothing was ever found, anchored or not.
    """
    with tempfile.TemporaryDirectory() as td:
        root = Path(td)
        build_fixture(root)
        # No exclusion at all — a pattern that cannot possibly match anything.
        result = scan(root, r"^nonexistent-path-\A\B$")
        found = set(result.get("results", {}).keys())
        check(
            "with no matching exclusion, the planted secret is actually detected",
            COLLISION in found,
            f"files with findings: {sorted(found)} — the fixture/detector harness itself is not working",
        )


def test_anchored_pattern_still_catches_the_collision_path() -> None:
    """GREEN: with the real (anchored) pattern, the planted secret is found."""
    pattern = extract_pattern()
    if not pattern:
        check("anchored pattern leaves the collision path scanned", False, "no pattern extracted")
        return
    with tempfile.TemporaryDirectory() as td:
        root = Path(td)
        build_fixture(root)
        result = scan(root, pattern)
        found = set(result.get("results", {}).keys())
        check(
            "the suffix-collision path is still scanned and its secret found",
            COLLISION in found,
            f"files with findings: {sorted(found)} — expected {COLLISION!r} among them",
        )
        check(
            "the canonical fixture path is excluded (no finding there)",
            CANONICAL not in found,
            f"files with findings: {sorted(found)} — canonical path should be excluded",
        )


def test_unanchored_pattern_reproduces_the_original_defect() -> None:
    """RED: demonstrate the pre-fix failure, so the fix is proven against a
    real observed failure and not just against its own success.
    """
    pattern = extract_pattern()
    if not pattern or not pattern.startswith("^"):
        check(
            "unanchored variant of the pattern drops the collision path (the defect)",
            False,
            "no anchored pattern to strip and re-test",
        )
        return
    unanchored = pattern[1:]  # strip the leading '^' — this is Bob's originally-reported pattern
    with tempfile.TemporaryDirectory() as td:
        root = Path(td)
        build_fixture(root)
        result = scan(root, unanchored)
        found = set(result.get("results", {}).keys())
        check(
            "stripping the anchor reproduces the defect: the collision path's secret vanishes",
            COLLISION not in found,
            f"files with findings: {sorted(found)} — expected {COLLISION!r} to be silently dropped",
        )


def main() -> int:
    if subprocess.run(["which", "detect-secrets"], capture_output=True).returncode != 0:
        print("detect-secrets not installed; cannot verify anything", file=sys.stderr)
        return 1

    print("#2886 — the determinism-fixture exclusion must be path-anchored")
    test_pattern_is_anchored()
    test_harness_actually_detects_the_planted_secret()
    test_anchored_pattern_still_catches_the_collision_path()
    test_unanchored_pattern_reproduces_the_original_defect()

    print()
    if failures:
        print(f"FAILED ({len(failures)}):", file=sys.stderr)
        for f in failures:
            print(f"  {f}", file=sys.stderr)
        return 1
    print("exclusion is anchored; a suffix-collision path stays scanned; the pre-fix defect reproduces when stripped.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
