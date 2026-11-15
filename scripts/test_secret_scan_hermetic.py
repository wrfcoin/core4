#!/usr/bin/env python3
"""Regression guard: the secret scan must not depend on Slack's availability (#2292).

THE DEFECT

`detect-secrets scan` (the CLI, unlike the library's `default_settings()`)
registers a filter the library does not:

    detect_secrets.filters.common.is_ignored_due_to_verification_policies

For `SlackDetector` candidates that filter issues a LIVE NETWORK REQUEST to the
candidate webhook URL. Templated webhooks get a definitive negative from Slack,
are marked VERIFIED_FALSE, and are silently dropped from the scan output. The
gate's result therefore became a function of Slack's reachability rather than of
repository content:

                             network up   network blocked
      plain scan                      0                 3
      scan --no-verify                3                 3

That is how #2242's dropped acknowledgements passed two green badges while a
reviewer running the same pinned scanner locally saw all three findings.

WHY THIS FILE EXISTS

The fix is one flag. A flag is exactly the kind of thing that gets dropped by a
future edit, a scanner upgrade, or a well-meaning simplification — and the
failure is SILENT, because the gate stays green while scanning less. So the
flag is asserted, not trusted:

  * the workflow must actually pass --no-verify (drift guard);
  * the templated webhooks in a real, committed repository file must appear in
    the scan output run the way the workflow runs it;
  * and the failure mode is demonstrated, not just described — the same fixture
    is scanned with the verification filter RE-ENABLED and must come back empty.
    A fixture you cannot watch fail proves nothing, which is the same defect one
    layer up.

Run: python3 scripts/test_secret_scan_hermetic.py

Network note: the filter-re-enabled demonstration needs to reach Slack to get a
definitive negative. Without network the filter cannot mark anything
VERIFIED_FALSE and the case is inapplicable — it is then reported SKIPPED, never
silently passed.
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
BASELINE = REPO / ".secrets.baseline"

# The three templated webhooks. Pinned by hash so a change to the file that
# removes them is a visible test failure rather than a silent loss of coverage.
# Read from the SAME file the workflow's sentinel check reads. Keeping a second
# copy here is how the two drift, and a drifted sentinel list silently stops
# sentinelling — the failure mode this whole file exists to prevent (#2315).
SENTINELS = REPO / "scripts/secret_scan_sentinels.py"
sys.path.insert(0, str(REPO / "scripts"))
import secret_scan_sentinels as _sentinels  # noqa: E402

EXPECTED_HASHES = set(_sentinels.HASHED_SECRETS)

failures: list[str] = []
skipped: list[str] = []


def check(name: str, ok: bool, detail: str = "") -> None:
    print(f"  {'PASS' if ok else 'FAIL'}  {name}" + (f": {detail}" if detail and not ok else ""))
    if not ok:
        failures.append(f"{name}{(': ' + detail) if detail else ''}")


def scan(path: Path, *flags: str, cwd: Path | None = None) -> dict:
    """Scan one file.

    `cwd` matters: detect-secrets keys its results on paths RELATIVE to the
    working directory, so scanning an absolute path from an unrelated cwd
    returns an empty result set rather than an error. That silently-empty
    behaviour is the same shape as the bug this file guards, so the caller
    always passes the directory the file lives in.
    """
    cwd = cwd or REPO
    try:
        rel = path.relative_to(cwd)
    except ValueError:
        rel = path
    r = subprocess.run(
        ["detect-secrets", "scan", *flags, str(rel)],
        capture_output=True,
        text=True,
        cwd=cwd,
    )
    if r.returncode != 0:
        return {}
    return json.loads(r.stdout or "{}")


def hashes(result: dict) -> set[str]:
    return {
        f["hashed_secret"] for fs in result.get("results", {}).values() for f in fs
    }


def test_workflow_passes_no_verify() -> None:
    """The drift guard. This is the durable half of the fix."""
    text = WORKFLOW.read_text(encoding="utf-8")
    scan_lines = [ln for ln in text.splitlines() if re.search(r"^\s*detect-secrets scan", ln)]
    check(
        "workflow invokes detect-secrets scan exactly once",
        len(scan_lines) == 1,
        f"found {len(scan_lines)}: {scan_lines}",
    )
    if not scan_lines:
        return
    line = scan_lines[0]
    hermetic_var = "HERMETIC" in line and re.search(
        r"^\s*HERMETIC=\(.*--no-verify.*\)", text, re.MULTILINE
    )
    check(
        "the scan is hermetic (--no-verify, directly or via HERMETIC)",
        "--no-verify" in line or bool(hermetic_var),
        f"scan line does not disable verification: {line.strip()}",
    )
    # The regeneration instructions must use the same flags, or a developer
    # following them produces a baseline the gate then rejects.
    check(
        "regeneration help renders the same flags",
        "scanflags.txt" in text,
        "printed instructions would omit --no-verify and drift from the gate",
    )


def test_workflow_executes_a_sentinel_check() -> None:
    """The #2315 blocker: a test nobody runs is not a guard.

    The first revision of this file was never invoked by any workflow or test
    target, while a comment in secret-scanning.yml claimed the flag was
    "guarded by" it. That is prose asserting a property the mechanism did not
    deliver. The guard now lives IN the gate; this asserts it is still there.
    """
    text = WORKFLOW.read_text(encoding="utf-8")
    check(
        "the workflow reads the shared sentinel list",
        "secret_scan_sentinels" in text,
        "the gate does not load the sentinels, so nothing asserts they are present",
    )
    check(
        "the workflow exits non-zero when a sentinel is absent",
        "is NOT hermetic" in text and "sys.exit(1)" in text,
        "a sentinel check that warns instead of failing is not a gate",
    )
    # Order matters: asserting presence AFTER the difference would let the
    # green path be taken first.
    sentinel_at = text.find("SENTINEL CHECK")
    diff_at = text.find("new_findings = []")
    check(
        "the sentinel check runs BEFORE the unacknowledged comparison",
        -1 < sentinel_at < diff_at,
        f"sentinel at {sentinel_at}, comparison at {diff_at}",
    )


def test_sentinels_are_generated_not_committed() -> None:
    """The sentinel list must describe findings the generator really emits."""
    check(
        "the sentinel list is non-empty",
        bool(_sentinels.HASHED_SECRETS),
        "an empty list makes the gate's presence check pass vacuously",
    )
    tracked = subprocess.run(
        ["git", "grep", "-l", "-e", "hooks.slack.com/services/T0", "--", ".", ":!*.py"],
        capture_output=True, text=True, cwd=REPO,
    )
    check(
        "no committed file carries a literal webhook-shaped string",
        tracked.stdout.strip() == "",
        f"tracked: {tracked.stdout.split()}",
    )


def test_templated_webhooks_are_surfaced() -> None:
    """The generated fixture: all sentinels must appear in the hermetic scan."""
    missing = _sentinels.absent_sentinels(["--no-verify"])
    check(
        "all three generated webhooks appear in the hermetic scan",
        not missing,
        f"missing {missing}",
    )


def test_empty_generator_is_red() -> None:
    """RED capability: a generator that emits nothing must fail the self-test."""
    missing = _sentinels.absent_sentinels(["--no-verify"], generator=lambda: "")
    check(
        "an empty generator leaves all three sentinels absent (self-test goes RED)",
        set(missing) == EXPECTED_HASHES,
        f"absent: {missing}",
    )


def test_filter_reenabled_drops_them() -> None:
    """Watch the fixture FAIL: with verification on, the same file yields nothing.

    This is the case that made the gate lie. If it ever stops reproducing, the
    upstream behaviour changed and the --no-verify guard should be re-justified
    rather than assumed still necessary.
    """
    found = _sentinels.scan_generated([])
    if EXPECTED_HASHES <= found:
        # Verification could not reach Slack, so nothing was marked
        # VERIFIED_FALSE and the case is inapplicable here.
        skipped.append(
            "filter-re-enabled demonstration (no route to Slack; the filter "
            "cannot mark anything VERIFIED_FALSE without one)"
        )
        print("  SKIP  verification filter drops them — no route to Slack")
        return
    check(
        "with verification ENABLED the same webhooks vanish (the defect)",
        not (EXPECTED_HASHES & found),
        f"expected all three dropped, still saw {sorted(EXPECTED_HASHES & found)}",
    )


def test_synthetic_webhook_round_trip() -> None:
    """A freshly written templated webhook is surfaced too.

    Guards the case where the committed fixture is edited away: the RULE, not
    just the current file, is what must keep working.
    """
    with tempfile.TemporaryDirectory() as td:
        p = Path(td) / "probe.md"
        p.write_text(_sentinels.render_fixture().splitlines()[0] + "\n", encoding="utf-8")
        found = hashes(scan(p, "--no-verify", cwd=Path(td)))
        check(
            "a newly written templated webhook is surfaced by the hermetic scan",
            len(found) == 1,
            f"expected 1 finding, got {len(found)}",
        )


def main() -> int:
    if subprocess.run(["which", "detect-secrets"], capture_output=True).returncode != 0:
        print("detect-secrets not installed; cannot verify anything", file=sys.stderr)
        return 1

    print("#2292 — the secret scan must not depend on Slack's availability")
    test_workflow_passes_no_verify()
    test_workflow_executes_a_sentinel_check()
    test_sentinels_are_generated_not_committed()
    test_templated_webhooks_are_surfaced()
    test_empty_generator_is_red()
    test_filter_reenabled_drops_them()
    test_synthetic_webhook_round_trip()

    print()
    if skipped:
        print(f"SKIPPED ({len(skipped)}):")
        for s in skipped:
            print(f"  {s}")
    if failures:
        print(f"FAILED ({len(failures)}):", file=sys.stderr)
        for f in failures:
            print(f"  {f}", file=sys.stderr)
        return 1
    print("secret scan is hermetic: generated templated webhooks are surfaced.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
