#!/usr/bin/env python3
"""Fail-closed wrapper for named/filtered `cargo test` invocations (#2861).

`cargo test` exits 0 when a name filter matches nothing::

    test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 1747 filtered out

and when a test binary contains no tests at all (a feature-gated module that
compiled empty). Both are VOID results wearing a green exit code: a lane or a
workflow step that names such a target logs a false green, and no reviewer can
catch it from the evidence, because the evidence looks like a pass. This class
has bitten twice in one night (#2851 merged over a red payout test behind
`cut-composition-harness`; the `test-support` emission tests have the same
shape), and each prior escape was patched with a per-step `grep` — see
`ci-deploy-features.yml` — which protects exactly one invocation each time.

This wrapper is the one enforcement point. It runs the given `cargo` command,
parses every libtest summary line, and FAILS when the run executed zero tests,
without anyone having to know in advance which features exist:

    python3 scripts/cargo_test_nonvacuous.py -- test --locked -p <pkg> \
        [--features <feats>] [--test <target> | <filter>] [-- <libtest args>]

Everything after ``--`` is passed to ``cargo`` verbatim. A cargo failure still
fails (this wrapper can only ADD a failure mode, never mask one). ``--no-run``
and ``--list`` invocations are refused: they legitimately execute nothing, so
"did anything run" is not a question this wrapper can answer about them.

``--self-test`` drives the verdict function over canned zero-match, passing and
failing outputs, proving the gate can reach RED — a gate that cannot
demonstrate its own red is not evidence (docs/ci/VACUOUS_GATES.md check 1).
"""

from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

SUMMARY = re.compile(
    r"^test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored; "
    r"(\d+) measured; (\d+) filtered out",
    re.MULTILINE,
)

# Flags whose presence makes "zero tests executed" the CORRECT outcome. The
# wrapper refuses them rather than special-casing them: an invocation that
# cannot be vacuity-checked should not claim the wrapper's protection.
NON_EXECUTING_FLAGS = ("--no-run", "--list")


def parse_summaries(output: str) -> list[tuple[str, int, int, int, int, int]]:
    """All libtest summary lines as (verdict, passed, failed, ignored, measured, filtered)."""
    return [
        (m.group(1), *(int(m.group(i)) for i in range(2, 7)))
        for m in SUMMARY.finditer(output)
    ]


def parse_sectioned_summaries(
    output: str,
) -> list[tuple[str, str, str, int, int, int, int, int]]:
    """Summary lines with their harness section, in output order.

    Each entry is (kind, label, verdict, passed, failed, ignored, measured,
    filtered) where kind is "itest" for an integration-test section
    (`Running tests/<file> (...)`), "unit" for a lib/bin section (`Running
    unittests <path> (...)`), "doc" for a `Doc-tests <crate>` section, and
    "unknown" when no header preceded the summary. The distinctions matter in
    evaluate: a crate with no doc-tests and a lib/bin with no unit tests both
    legitimately print all-zero summaries, while an all-zero integration-test
    BINARY is a file someone wrote to BE tests executing none of them — the
    fake-green shape.
    """
    events: list[tuple[int, str, object]] = []
    for m in re.finditer(r"^\s*Running (unittests )?(\S+)", output, re.MULTILINE):
        events.append((m.start(), "unit" if m.group(1) else "itest", m.group(2)))
    for m in re.finditer(r"^\s*Doc-tests (\S+)", output, re.MULTILINE):
        events.append((m.start(), "doc", m.group(1)))
    for m in SUMMARY.finditer(output):
        events.append((m.start(), "sum", m))
    events.sort(key=lambda e: e[0])
    sectioned = []
    kind, label = "unknown", "?"
    for _pos, event_kind, value in events:
        if event_kind in ("unit", "itest", "doc"):
            kind, label = event_kind, value  # type: ignore[assignment]
        else:
            m = value  # type: ignore[assignment]
            sectioned.append(
                (kind, label, m.group(1), *(int(m.group(i)) for i in range(2, 7)))
            )
    return sectioned


def evaluate(
    output: str,
    returncode: int,
    min_executed: int,
    min_summaries: int = 0,
) -> list[str]:
    """Errors that make this run inadmissible as green evidence.

    Pure over (output, returncode) so the self-test and the unit tests drive
    the production verdict, not a re-typed copy of it (the #2602 rule).
    """
    errors: list[str] = []
    summaries = parse_summaries(output)
    executed = sum(passed + failed for _, passed, failed, *_ in summaries)
    filtered = sum(f for *_, f in summaries)

    if returncode != 0:
        # A real red is evidence; propagate it untouched. The wrapper never
        # converts a failure into anything else.
        errors.append(f"cargo exited {returncode}")
        return errors

    if not summaries:
        errors.append(
            "VACUOUS_TEST_RUN no libtest summary found in the output — the "
            "command did not run any test harness, so its exit 0 is not "
            "evidence (#2861)"
        )
        return errors

    # An INTEGRATION-TEST binary whose summary is all-zero (nothing passed,
    # failed, ignored, or filtered) ran nothing and declared nothing — fake
    # green, indistinguishable from a binary passing everything, and
    # invisible behind its siblings' totals in any count floor (the shape
    # found on shared-protocol's yubikey_signer target: 2,120 sibling tests
    # green, one target empty, package gate success; file-level
    # `#![cfg(feature = …)]` compiles the whole file away). Feature-gated
    # targets must expose a `#[cfg(not(feature = "…"))] #[test] #[ignore]`
    # sentinel so the absence is COUNTABLE (the #2748 principle extended to
    # the cfg dimension — see shared-protocol/tests/yubikey_signer.rs).
    # Exempt: Doc-tests sections and lib/bin "unittests" sections — a crate
    # with no doc-tests and a bin with no unit tests print all-zero
    # summaries legitimately; a tests/*.rs file exists only to hold tests.
    # Filter-miss runs (filtered > 0) are the separate, pre-existing
    # min-executed class (#2861). Headerless summaries stay covered
    # (fail closed).
    for kind, label, _verdict, s_passed, s_failed, s_ignored, _m, s_filtered in (
        parse_sectioned_summaries(output)
    ):
        if kind in ("doc", "unit"):
            continue
        if s_passed == 0 and s_failed == 0 and s_ignored == 0 and s_filtered == 0:
            errors.append(
                f"EMPTY_TEST_BINARY {label}: executed=0 ignored=0 filtered=0 — "
                "this target ran nothing and declared nothing, so its green is "
                "not evidence. Give it a #[cfg(not(feature))] ignored sentinel "
                "(countable absence, #2748), enable the feature, or drop the "
                "target from the invocation"
            )

    if executed < min_executed:
        errors.append(
            f"VACUOUS_TEST_RUN executed={executed} (< required {min_executed}) "
            f"with filtered_out={filtered} across {len(summaries)} summary "
            "line(s). A named target or filter that matches nothing exits 0 — "
            "this is a VOID result wearing a green exit code, usually a "
            "misspelled test name or a test behind a cargo feature this "
            "invocation did not enable (#2861)"
        )

    # #2889: a whole test BINARY disappearing is invisible to a count floor on
    # a broad suite. `cargo test -p wrfcoin-p2p` executes 486 tests across ten
    # binaries, but nine of them contribute 16 tests between them — so any
    # count floor loose enough to tolerate ordinary churn is also loose enough
    # to miss an entire integration target silently dropping out. The binary
    # count is the dimension that catches that, and it is stable in a way the
    # test count is not: tests are added and removed constantly, binaries
    # almost never.
    if min_summaries and len(summaries) < min_summaries:
        errors.append(
            f"VACUOUS_TEST_RUN summaries={len(summaries)} (< required "
            f"{min_summaries}) with executed={executed}. A test binary that "
            "stops being built or run contributes no libtest summary at all, "
            "so the suite silently covers less while the executed count can "
            "still clear its floor (#2889)"
        )
    return errors


# #2889: how far above its floor a run may drift before the floor is reported
# as decayed. A floor set with the ~10% margin this repo uses sits at a ratio
# near 1.1; the p2p gate that prompted this was at 486. Two is comfortably
# clear of honest churn and still catches a floor that has stopped meaning
# anything.
STALE_FLOOR_RATIO = 2


def stale_floor_note(
    actual: int, floor: int, dimension: str = "executed"
) -> str | None:
    """Warn when a floor has decayed far below what the suite actually runs.

    THE RECURRENCE ANSWER (#2889). The floors that decayed did so silently
    because nothing ever compared them to reality again — re-measuring is a
    deliberate act nobody was scheduled to perform. No new job is needed for
    it: this wrapper already holds both numbers on every single run, so the
    check costs nothing and happens everywhere a floor is used.

    It warns rather than fails. A run far above its floor means the suite
    GREW, which is good news; failing the build for it would train people to
    lower floors to silence the gate, which is precisely the decay this is
    meant to stop.

    #2899 review: this originally took only the executed dimension, so a
    suite growing from 10 test binaries to 20 could later lose ten whole
    targets with no signal — #2889 recreated for the summaries dimension by
    the change that fixes #2889. `dimension` makes it apply to either, and
    main() now calls it for both.
    """
    if floor < 1 or actual < floor * STALE_FLOOR_RATIO:
        return None
    unit = "tests" if dimension == "executed" else "test binaries"
    return (
        f"FLOOR_STALE {dimension}={actual} is {actual // floor}x the declared "
        f"floor of {floor}. This gate would still pass with {actual - floor} "
        f"of {actual} {unit} gone. Recalibrate the floor and record the "
        f"measurement date beside it (#2889)."
    )


def nonnegative_int(value: str) -> int:
    """argparse type for --min-summaries: zero (disabled) or positive.

    #2899 review, Alice: this parameter shipped as a plain `type=int`, so
    `--min-summaries -1` was accepted and then silently disabled the check —
    a negative floor is truthy, and `len(summaries) < -1` is never true, so
    the guard read as configured while enforcing nothing. rc=0 with
    `summaries=9` against a declared floor.

    That is the same fail-open Iris blocked on #2867 for `--min-executed`,
    whose cure (`positive_int`, directly below) was already in this file. The
    new parameter reintroduced the defect one argument over from its own fix,
    which is why both types now live together and are tested together.

    Zero is accepted here, unlike `--min-executed`: it is this flag's
    documented "disabled" value and its default. Negative is not a weaker
    floor, it is no floor at all wearing the shape of one.
    """
    try:
        number = int(value)
    except ValueError as error:
        raise argparse.ArgumentTypeError(f"not an integer: {value!r}") from error
    if number < 0:
        raise argparse.ArgumentTypeError(
            f"--min-summaries must be >= 0, got {number}: a negative floor is "
            "silently unenforceable (len(summaries) < negative is never true), "
            "so it disables the guard while looking configured. Use 0 to "
            "disable it deliberately."
        )
    return number


def positive_int(value: str) -> int:
    """argparse type for --min-executed: strictly positive, rejected otherwise.

    A floor of 0 (or below) would make the wrapper print an
    authoritative-looking NONVACUOUS_TEST_OK on a genuine zero-match run —
    the guard failing open at its own boundary, reproducing the exact defect
    it exists to prevent one layer up (#2867 review finding). There is no
    legitimate use for it: an invocation that expects nothing to run should
    not claim this wrapper's protection at all.
    """
    try:
        number = int(value)
    except ValueError as error:
        raise argparse.ArgumentTypeError(f"not an integer: {value!r}") from error
    if number < 1:
        raise argparse.ArgumentTypeError(
            f"--min-executed must be >= 1, got {number}: a nonpositive floor "
            "would accept a zero-match run as green, which is the false-green "
            "class this wrapper exists to eliminate (#2861)"
        )
    return number


def self_test() -> int:
    zero_match = (
        "running 0 tests\n"
        "\n"
        "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; "
        "1747 filtered out; finished in 0.00s\n"
    )
    real_pass = (
        "running 1 test\n"
        "test one_real_test ... ok\n"
        "\n"
        "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; "
        "1746 filtered out; finished in 0.05s\n"
    )
    real_fail = (
        "running 1 test\n"
        "test one_real_test ... FAILED\n"
        "\n"
        "test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; "
        "0 filtered out; finished in 0.05s\n"
    )
    checks = [
        ("zero-match exit-0 run must be RED", evaluate(zero_match, 0, 1), True),
        ("summary-free exit-0 run must be RED", evaluate("Compiling foo\n", 0, 1), True),
        ("genuinely matching run must be GREEN", evaluate(real_pass, 0, 1), False),
        ("cargo failure must propagate as RED", evaluate(real_fail, 101, 1), True),
        ("min-executed floor must be enforceable", evaluate(real_pass, 0, 2), True),
        # #2889 — the binary-count dimension must also reach RED, and must
        # stay off when not asked for.
        (
            "min-summaries floor must be enforceable",
            evaluate(real_pass, 0, 1, 2),
            True,
        ),
        (
            "min-summaries 0 must not constrain anything",
            evaluate(real_pass, 0, 1, 0),
            False,
        ),
        (
            "a run meeting both dimensions must be GREEN",
            evaluate(real_pass + real_pass, 0, 2, 2),
            False,
        ),
    ]
    # EMPTY_TEST_BINARY: an all-zero summary from a test BINARY is fake green
    # even when siblings clear every count floor; an all-zero Doc-tests
    # summary is legitimate; an all-ignored binary is a countable absence
    # (the #2748 sentinel shape) and must stay green.
    empty_binary = (
        "     Running tests/yubikey_signer.rs (target/debug/deps/x)\n"
        "running 0 tests\n\n"
        "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; "
        "0 filtered out; finished in 0.00s\n"
    )
    empty_doctests = (
        "   Doc-tests wrfcoin_shared_protocol\n"
        "running 0 tests\n\n"
        "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; "
        "0 filtered out; finished in 0.00s\n"
    )
    ignored_sentinel = (
        "     Running tests/yubikey_signer.rs (target/debug/deps/x)\n"
        "running 1 test\n"
        "test yubikey_tests_require_the_hsm_feature ... ignored\n\n"
        "test result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; "
        "0 filtered out; finished in 0.00s\n"
    )
    checks += [
        (
            "an empty test binary hidden behind a green sibling must be RED",
            evaluate(real_pass + empty_binary, 0, 1),
            True,
        ),
        (
            "an all-zero Doc-tests summary must stay GREEN",
            evaluate(real_pass + empty_doctests, 0, 1),
            False,
        ),
        (
            "an all-ignored sentinel binary (countable absence) must stay GREEN",
            evaluate(real_pass + ignored_sentinel, 0, 1),
            False,
        ),
        (
            "a headerless all-zero summary must be RED (fail closed)",
            evaluate(
                "running 0 tests\n\n"
                "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; "
                "0 filtered out; finished in 0.00s\n" + real_pass,
                0,
                1,
            ),
            True,
        ),
        (
            "an all-zero lib/bin unittests summary must stay GREEN (a bin "
            "with no unit tests is normal, not vacuous)",
            evaluate(
                real_pass
                + "     Running unittests src/bin/tool.rs (target/debug/deps/t)\n"
                "running 0 tests\n\n"
                "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; "
                "0 filtered out; finished in 0.00s\n",
                0,
                1,
            ),
            False,
        ),
    ]
    for label, errors, expect_red in checks:
        if bool(errors) != expect_red:
            print(f"SELF_TEST_FAILED {label}: errors={errors}", file=sys.stderr)
            return 1
    # The floor itself must be fail-closed: nonpositive values are rejected
    # at the argument boundary, never accepted and compared — an accepted 0
    # would print NONVACUOUS_TEST_OK on a zero-match run (#2867 review).
    for bad in ("0", "-1"):
        try:
            positive_int(bad)
        except argparse.ArgumentTypeError:
            continue
        print(
            f"SELF_TEST_FAILED nonpositive floor {bad!r} was accepted",
            file=sys.stderr,
        )
        return 1
    # #2889 staleness detector: it must fire on a decayed floor and stay
    # silent on a healthy one, or it is decoration.
    if stale_floor_note(486, 1) is None:
        print("SELF_TEST_FAILED stale floor 1-vs-486 was not reported", file=sys.stderr)
        return 1
    if stale_floor_note(8, 8) is not None:
        print("SELF_TEST_FAILED exact floor 8-vs-8 was wrongly reported stale", file=sys.stderr)
        return 1
    if stale_floor_note(486, 430) is not None:
        print(
            "SELF_TEST_FAILED a floor with the house margin was reported stale",
            file=sys.stderr,
        )
        return 1
    # #2899 review: the summaries dimension must also be covered, or a suite
    # that grew from 10 binaries to 20 can lose ten with no signal.
    summaries_note = stale_floor_note(20, 10, "summaries")
    if summaries_note is None or "summaries=20" not in summaries_note:
        print(
            f"SELF_TEST_FAILED summaries staleness not reported: {summaries_note}",
            file=sys.stderr,
        )
        return 1
    if "test binaries" not in summaries_note:
        print(
            "SELF_TEST_FAILED summaries note must count binaries, not tests",
            file=sys.stderr,
        )
        return 1
    if stale_floor_note(10, 10, "summaries") is not None:
        print("SELF_TEST_FAILED exact summaries floor reported stale", file=sys.stderr)
        return 1
    # #2899 review: a negative --min-summaries must be refused, not silently
    # accepted as "no floor". Zero IS accepted — it is the documented default.
    for bad in ("-1", "-10"):
        try:
            nonnegative_int(bad)
        except argparse.ArgumentTypeError:
            continue
        print(
            f"SELF_TEST_FAILED negative --min-summaries {bad!r} was accepted",
            file=sys.stderr,
        )
        return 1
    for good in ("0", "6"):
        if nonnegative_int(good) != int(good):
            print(f"SELF_TEST_FAILED --min-summaries {good!r} was rejected", file=sys.stderr)
            return 1
    print("cargo_test_nonvacuous.py self-test passed (RED and GREEN both reachable)")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Run `cargo <args>` and fail loudly if zero tests executed."
    )
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument(
        "--min-executed",
        type=positive_int,
        default=1,
        help="Minimum tests that must have executed (passed+failed). "
        "Strictly positive; 0 and negatives are rejected because they would "
        "fail open on a zero-match run. Default 1.",
    )
    parser.add_argument(
        "--min-summaries",
        type=nonnegative_int,
        default=0,
        help="Minimum number of libtest summary lines (i.e. test binaries) the "
        "run must produce. Catches a whole test binary silently dropping out, "
        "which a count floor on a broad suite cannot see. 0 disables.",
    )
    parser.add_argument(
        "cargo_args",
        nargs="*",
        help="Arguments passed to `cargo` verbatim, after `--`.",
    )
    args = parser.parse_args()

    if args.self_test:
        return self_test()

    if not args.cargo_args:
        parser.error("no cargo arguments given (expected e.g. -- test -p <pkg> ...)")
    for flag in NON_EXECUTING_FLAGS:
        if flag in args.cargo_args:
            parser.error(
                f"{flag} legitimately executes nothing; this wrapper only "
                "guards invocations that are supposed to RUN tests"
            )

    command = ["cargo", *args.cargo_args]
    environment = os.environ.copy()
    environment["CARGO_TERM_COLOR"] = "never"
    print(f"NONVACUOUS_TEST_CMD {' '.join(command)}", flush=True)
    completed = subprocess.run(
        command,
        cwd=ROOT,
        env=environment,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )
    print(completed.stdout, end="")

    errors = evaluate(
        completed.stdout, completed.returncode, args.min_executed, args.min_summaries
    )
    summaries = parse_summaries(completed.stdout)
    executed = sum(passed + failed for _, passed, failed, *_ in summaries)
    if errors:
        for error in errors:
            print(f"NONVACUOUS_TEST_ERROR {error}", file=sys.stderr)
        return 1
    notes = [
        note
        for note in (
            stale_floor_note(executed, args.min_executed, "executed"),
            stale_floor_note(len(summaries), args.min_summaries, "summaries"),
        )
        if note
    ]
    for note in notes:
        print(note)
        # Surface it where a human actually looks: a line in a 3,000-line log
        # is not a signal, a run annotation is.
        if os.environ.get("GITHUB_ACTIONS") == "true":
            print(f"::warning::{note}")
    print(
        f"NONVACUOUS_TEST_OK executed={executed} "
        f"summaries={len(summaries)} min_executed={args.min_executed} "
        f"min_summaries={args.min_summaries}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
