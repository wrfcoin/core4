#!/usr/bin/env python3
"""Package-wide `cargo test` gate that is aware of its own standing failures (#2911).

# The 27-day hole this closes

`cargo test -p wrfcoin-native-chain` has been REAL-RED on `main` since
2026-08-04 (`19af70d33`, "#2713 regression artifact, REAL-RED by design"). Four
tests in `chain_identity_split_2713` fail deliberately: they assert the property
the node SHOULD have and #2713's divergent literals are still there. The RED is
the proof, and #2713 is open, so it stays.

The cost was a coverage hole. A permanently-red package command reports exit
`101` whether the four known failures are the only ones or not, so for 27 days a
FRESH break anywhere else in native-chain produced the identical observable. Two
distinct mechanisms, both fatal:

1.  **Verdict masking.** exit 101 == exit 101. A reviewer must diff failure names
    by eye against a 27-day-old commit message to tell the two apart, and nobody
    did.
2.  **Execution masking.** Worse, and invisible in the verdict: without
    `--no-fail-fast`, cargo STOPS at the first failing test binary. Alphabetically
    `chain_identity_split_2713` is the third of ~47 native-chain integration
    targets, so the package command never even *built* most of the suite. The
    masked tests did not pass — they never ran.

This gate is the #2923 taxonomy applied to a red gate rather than a green one:
the observable must emit a positive, attributed statement of WHAT failed, not a
bare pass/fail. It does not make the package green. It makes the package's red
*legible*, so a fifth failure is a different observable from the four.

# What it will not do

It does not weaken the standing target. `expected_failures` names the four
observations by identity; the two `control_*` tests in the same file are
deliberately absent from it and must PASS. Re-targeting is the fix here;
weakening is the failure mode #2911 exists to record.

# Verdicts — three, never two

============  ==================================================================
exit 0        BASELINE. Every declared failure failed, by name, and nothing else
              did. The package is exactly as red as it is declared to be.
exit 1        REAL-RED, attributable. A finding about the code, of one of three
              named kinds:

              * ``NEW_FAILURE`` — a test failed that the baseline does not
                declare. This is the observable the 27-day hole did not have.
              * ``HEALED`` — a declared failure PASSED. #2713 may be fixed:
                promote it out of the baseline. Or the test was weakened. Both
                need a human; neither may be absorbed silently.
              * ``MISSING`` — a declared failure is no longer present in its
                target at all. Deleting the assertion is the other way to make a
                standing red go away, and it must not read as baseline.
exit 2        GATE BROKEN / VOID. The run says nothing about the code in either
              direction: nothing compiled, a declared target produced no libtest
              summary, an integration binary ran and declared nothing, the
              invocation lacked ``--no-fail-fast``, or a floor was not met.
              A failed doctest is an explicit ``DOCTEST_FAILURES`` VOID: cargo's
              doctest harness does not emit a stable failure-name block, so the
              gate deliberately cannot attribute it to a baseline entry.
============  ==================================================================

Collapsing 1 and 2 is how a broken harness reads as a finding and how a real
finding gets waved away as "probably just the harness".

# Control first, then observation

``--self-test`` drives the classifier over synthetic cargo output and asserts it
reaches all three verdicts and all three finding kinds. It invokes no cargo, so a
broken toolchain cannot make the observation below it vacuously green.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
from dataclasses import dataclass, field
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_MANIFEST = Path(__file__).with_name("package_test_baselines.json")

# `test result: FAILED. 2 passed; 4 failed; 0 ignored; 0 measured; 0 filtered out`
SUMMARY_RE = re.compile(
    r"^test result: (?P<verdict>ok|FAILED)\. (?P<passed>\d+) passed; "
    r"(?P<failed>\d+) failed; (?P<ignored>\d+) ignored; "
    r"(?P<measured>\d+) measured; (?P<filtered>\d+) filtered out",
    re.MULTILINE,
)

# `     Running tests/chain_identity_split_2713.rs (target/debug/deps/chain_…)`
# `     Running unittests src/lib.rs (target/debug/deps/wrfcoin_native_chain-…)`
RUNNING_RE = re.compile(r"^\s*Running (?P<unit>unittests )?(?P<path>\S+)", re.MULTILINE)
DOCTESTS_RE = re.compile(r"^\s*Doc-tests (?P<crate>\S+)", re.MULTILINE)

# libtest's trailing name list:
#
#     failures:
#         unset_chain_id_reports_and_signs_the_same_identity
#         empty_string_export_diverges_by_a_different_mechanism
#
# The same word heads the per-test stdout dumps above it, but those are followed
# by a blank line and a `---- name stdout ----` banner, so only the block whose
# entries are bare indented identifiers matches.
FAILURE_BLOCK_RE = re.compile(r"^failures:\n((?:[ \t]+[\w:]+\n)+)", re.MULTILINE)

# Per-test result lines. Presence here is how a DELETED pinned test is told apart
# from one that started passing: absent from both this and the failure block
# means the assertion is gone, not that it succeeded.
TEST_LINE_RE = re.compile(
    r"^test (?P<name>[\w:]+) \.\.\. (?P<outcome>ok|FAILED|ignored)\b", re.MULTILINE
)

BASELINE, FINDING, VOID = "baseline", "finding", "void"
EXIT = {BASELINE: 0, FINDING: 1, VOID: 2}

# An invocation without this stops at the first failing binary, so the standing
# red would prevent most of the suite from ever being built. Enforced, not
# documented: this flag is the difference between "the rest passed" and "the
# rest never ran", and those are not the same claim.
REQUIRED_CARGO_FLAG = "--no-fail-fast"
NON_EXECUTING_FLAGS = ("--no-run", "--list")


@dataclass
class Section:
    """One test binary's slice of the output, with its identity attached."""

    kind: str  # "itest" | "unit" | "doc" | "unknown"
    label: str  # source path, or crate name for doc-tests
    target: str  # canonical name matched against the manifest
    passed: int = 0
    failed: int = 0
    ignored: int = 0
    measured: int = 0
    filtered: int = 0
    verdict: str = ""
    has_summary: bool = False
    failure_names: set[str] = field(default_factory=set)
    # name -> "ok" | "FAILED" | "ignored". Presence alone cannot tell a pinned
    # observation that started PASSING from one that was #[ignore]d, and those
    # are different findings with different remedies.
    outcomes: dict[str, str] = field(default_factory=dict)


@dataclass
class Report:
    verdict: str
    new_failures: list[str] = field(default_factory=list)
    healed: list[str] = field(default_factory=list)
    silenced: list[str] = field(default_factory=list)
    missing: list[str] = field(default_factory=list)
    confirmed: list[str] = field(default_factory=list)
    voids: list[str] = field(default_factory=list)
    executed: int = 0
    summaries: int = 0


def section_target(kind: str, label: str) -> str:
    """Canonical target name for a section.

    Integration targets are named by file stem, which is exactly what
    ``cargo test --test <name>`` takes, so a manifest entry can be re-run by
    hand from the name the gate prints (the #2895 rule: a target's NAME is its
    filename).
    """
    if kind == "itest":
        return Path(label).stem
    if kind == "unit":
        return f"unittests:{label}"
    if kind == "doc":
        return f"doctests:{label}"
    return f"unknown:{label}"


def parse_sections(output: str) -> list[Section]:
    """Split merged cargo output into per-binary sections, in emission order.

    Requires stdout and stderr to have been captured into ONE stream: cargo
    writes `Running …` to stderr and libtest writes results to stdout, so
    concatenating two separately-captured buffers puts every header after every
    result and destroys attribution entirely. `run_package` uses
    ``stderr=subprocess.STDOUT`` for this reason.
    """
    events: list[tuple[int, str, object]] = []
    for match in RUNNING_RE.finditer(output):
        kind = "unit" if match.group("unit") else "itest"
        events.append((match.start(), "header", (kind, match.group("path"))))
    for match in DOCTESTS_RE.finditer(output):
        events.append((match.start(), "header", ("doc", match.group("crate"))))
    for match in SUMMARY_RE.finditer(output):
        events.append((match.start(), "summary", match))
    events.sort(key=lambda event: event[0])

    sections: list[Section] = []
    current = Section("unknown", "?", "unknown:?")
    boundaries: list[tuple[int, Section]] = [(0, current)]
    for position, event_kind, value in events:
        if event_kind == "header":
            kind, label = value  # type: ignore[misc]
            current = Section(kind, label, section_target(kind, label))
            boundaries.append((position, current))
        else:
            match = value  # type: ignore[assignment]
            current.has_summary = True
            current.verdict = match.group("verdict")
            current.passed += int(match.group("passed"))
            current.failed += int(match.group("failed"))
            current.ignored += int(match.group("ignored"))
            current.measured += int(match.group("measured"))
            current.filtered += int(match.group("filtered"))

    # Attach the text between each header and the next to its section, so
    # failure names and per-test lines are attributed rather than pooled.
    for index, (start, section) in enumerate(boundaries):
        end = boundaries[index + 1][0] if index + 1 < len(boundaries) else len(output)
        chunk = output[start:end]
        for block in FAILURE_BLOCK_RE.findall(chunk):
            for line in block.splitlines():
                name = line.strip()
                if name:
                    section.failure_names.add(name)
        for match in TEST_LINE_RE.finditer(chunk):
            section.outcomes[match.group("name")] = match.group("outcome")
        if section.has_summary or section.failure_names or section.outcomes:
            # Identity, not equality: two distinct binaries can produce
            # field-identical Sections, and a dataclass `in` test would silently
            # drop the second one.
            if not any(existing is section for existing in sections):
                sections.append(section)
    return sections


def declared_key(entry: dict) -> str:
    return f"{entry['target']}::{entry['test']}"


def classify_run(output: str, returncode: int, spec: dict) -> Report:
    """Classify one package run against its declared baseline. Pure.

    Purity is what lets ``--self-test`` and the unit tests drive the SHIPPED
    verdict rather than a re-typed copy of it (the #2602 rule).
    """
    report = Report(BASELINE)
    sections = parse_sections(output)
    executable = [s for s in sections if s.has_summary]
    report.summaries = len(executable)
    report.executed = sum(s.passed + s.failed for s in executable)

    if not executable:
        report.voids.append(
            f"no libtest summary anywhere in the output (cargo exited {returncode}). "
            "Nothing was executed, so this run is not evidence about the code in "
            "either direction — most likely a compile error, which under "
            "--no-fail-fast still aborts before any test binary starts."
        )
        report.verdict = VOID
        return report

    # An integration binary that ran nothing and declared nothing is a green
    # indistinguishable from a passing one, and it hides inside its siblings'
    # totals (the shared-protocol `yubikey_signer` shape, #2861). Unit and
    # doc-test sections are exempt: a bin with no unit tests and a crate with no
    # doc-tests legitimately print all-zero summaries; a `tests/*.rs` file exists
    # only to hold tests.
    for section in executable:
        if section.kind in ("doc", "unit"):
            continue
        if section.passed == section.failed == section.ignored == section.filtered == 0:
            report.voids.append(
                f"EMPTY_TEST_BINARY {section.label}: executed=0 ignored=0 filtered=0 — "
                "this target ran nothing and declared nothing, so neither its green "
                "nor the package verdict covers it. Give it a "
                '#[cfg(not(feature = "…"))] #[test] #[ignore] sentinel so the '
                "absence is countable (#2748), or drop it from the invocation."
            )

    min_executed = int(spec.get("min_executed", 0))
    # Native-chain's exact floor is 49, not the original 50 (#3073): commit
    # 182171faa1de6caf13ba302e315bcfca579777cd (#2888, merged by #3026)
    # deliberately deleted tests/rate_limit_envelope_1276.rs. Its three tests
    # only asserted local JSON/struct values and assert!(true), never project
    # code. The current command has 44 integration + 4 unit + 1 doctest
    # sections. Keep this floor exact: losing any further binary is still VOID.
    # The manifest retains the 1700 executed floor and all four #2713 failures.
    min_summaries = int(spec.get("min_summaries", 0))
    if min_executed and report.executed < min_executed:
        report.voids.append(
            f"FLOOR_UNMET executed={report.executed} < min_executed={min_executed}. "
            "The suite ran materially less than declared, so 'only the known "
            "failures failed' is a statement about a suite that did not happen."
        )
    if min_summaries and report.summaries < min_summaries:
        report.voids.append(
            f"FLOOR_UNMET summaries={report.summaries} < min_summaries={min_summaries}. "
            "A whole test binary that stops being built contributes no summary at "
            "all, and the executed count can still clear its floor (#2889). This is "
            "the dimension that sees it."
        )

    by_target = {s.target: s for s in executable}
    declared = spec.get("expected_failures", [])
    declared_by_target: dict[str, set[str]] = {}
    for entry in declared:
        declared_by_target.setdefault(entry["target"], set()).add(entry["test"])

    # --- the declared failures: still failing, healed, or deleted? ---
    for entry in declared:
        key = declared_key(entry)
        section = by_target.get(entry["target"])
        if section is None:
            report.voids.append(
                f"DECLARED_TARGET_ABSENT {key}: target '{entry['target']}' produced no "
                "libtest summary in this run, so whether its pinned failure still "
                "fails is UNKNOWN. Absence is not a pass and not a fail — the gate is "
                "broken, not the code."
            )
            continue
        outcome = section.outcomes.get(entry["test"])
        if entry["test"] in section.failure_names:
            report.confirmed.append(f"{key} (#{entry.get('issue', '?')})")
        elif outcome == "ok":
            report.healed.append(
                f"HEALED {key}: this test is declared to FAIL for #{entry.get('issue', '?')} "
                f"and it PASSED. Either the defect was fixed — remove it from the "
                "baseline and let the package assert green on it — or the assertion "
                "was weakened. Both need a human; neither may be absorbed."
            )
        elif outcome == "ignored":
            report.silenced.append(
                f"SILENCED {key}: this test is declared to FAIL for "
                f"#{entry.get('issue', '?')} and it is now #[ignore]d, so it neither "
                "passes nor fails. That is not a fix and not a baseline — it is the "
                "standing red being switched off. Restore it, or close the issue and "
                "remove the declaration."
            )
        else:
            report.missing.append(
                f"MISSING {key}: this test is declared to FAIL for #{entry.get('issue', '?')} "
                "and libtest never reported it at all — deleted or renamed. (An "
                "#[ignore]d test IS reported and comes back as SILENCED instead.) "
                "Deleting the assertion is the other way to make a standing red "
                "disappear, and it must not read as baseline."
            )

    # --- everything else that failed is NEW, including inside a declared target ---
    for section in executable:
        expected_here = declared_by_target.get(section.target, set())
        for name in sorted(section.failure_names - expected_here):
            line = f"NEW_FAILURE {section.target}::{name} — not declared in the baseline."
            if section.kind == "itest":
                # The name a reviewer needs is the one they can re-run, so print
                # the exact command rather than making them reconstruct it.
                line += (
                    f" Reproduce with: cargo test --locked -p {spec['package']} "
                    f"--test {section.target} -- --exact {name}"
                )
            report.new_failures.append(line)
        # Cargo's doctest harness reports a FAILED summary without the stable
        # libtest failure-name block used for integration/unit attribution. Do
        # not let that category fall through to generic UNNAMED_FAILURES: this
        # is an intentional VOID, with a distinct observable and a self-test
        # control. A doctest failure is not evidence that a baseline entry
        # passed, failed, or disappeared.
        if section.kind == "doc" and section.failed:
            report.voids.append(
                f"DOCTEST_FAILURES {section.label}: {section.failed} doctest(s) failed, "
                "but cargo's doctest harness emits no stable failure-name block. "
                "This is an intentional VOID: the gate cannot attribute a doctest "
                "failure to the baseline, so it must be investigated explicitly."
            )
        # A failing non-doctest summary whose names libtest never printed cannot
        # be attributed at all, which is the exact ambiguity this gate exists to
        # remove.
        elif section.failed and not section.failure_names:
            report.voids.append(
                f"UNNAMED_FAILURES {section.label}: {section.failed} test(s) failed but "
                "no failure-name block was printed, so they cannot be checked against "
                "the baseline. Not evidence in either direction."
            )

    if report.voids:
        report.verdict = VOID
    elif report.new_failures or report.healed or report.silenced or report.missing:
        report.verdict = FINDING
    else:
        report.verdict = BASELINE
    return report


def render(report: Report, spec: dict) -> None:
    package = spec["package"]
    print(
        f"\n=== {package}: executed={report.executed} across "
        f"{report.summaries} test binar{'y' if report.summaries == 1 else 'ies'} ==="
    )
    for line in report.confirmed:
        print(f"  [BASELINE-RED, as declared] {line}")
    for line in report.new_failures:
        print(f"  [FINDING] {line}")
    for line in report.healed + report.silenced + report.missing:
        print(f"  [FINDING] {line}")
    for line in report.voids:
        print(f"  [VOID] {line}")

    if report.new_failures or report.healed or report.silenced or report.missing:
        # Printed even when the exit code is 2: a named failing test is
        # attributable evidence on its own, and must not be lost inside a
        # "gate broken" verdict.
        print(
            f"\nREAL-RED, ATTRIBUTABLE: {len(report.new_failures)} new failure(s), "
            f"{len(report.healed)} healed, {len(report.silenced)} silenced, "
            f"{len(report.missing)} missing — "
            f"against {len(report.confirmed)} declared failure(s) that are still red."
        )
    if report.verdict == VOID:
        print(
            f"\nGATE BROKEN / VOID ({len(report.voids)} problem(s)). This run is NOT "
            "evidence that the package is right and NOT evidence that it is wrong. "
            "Fix the gate, then re-read. Exit 2."
        )
    elif report.verdict == FINDING:
        print(
            "\nEvery declared target executed and the floors held, so the above is a "
            "finding about the code, not about the harness. Exit 1."
        )
    else:
        print(
            f"\nBASELINE: exactly the {len(report.confirmed)} declared failure(s) "
            f"failed, by name, and nothing else did. {package} is as red as it is "
            "declared to be and no redder. Exit 0."
        )


def load_manifest(path: Path) -> dict:
    manifest = json.loads(path.read_text())
    if manifest.get("schema") != 1:
        raise SystemExit(f"FAIL-CLOSED: manifest schema must be 1, got {manifest.get('schema')!r}")
    if not manifest.get("packages"):
        raise SystemExit("FAIL-CLOSED: manifest declares no packages; a gate over nothing is VOID")
    return manifest


def preflight(spec: dict) -> list[str]:
    """Structural checks that must hold before the observation is worth reading."""
    errors: list[str] = []
    package = spec.get("package", "<unnamed>")
    for field_name in ("package", "cargo_args", "why", "expected_failures"):
        if field_name not in spec:
            errors.append(f"{package}: manifest entry is missing '{field_name}'")
    args = spec.get("cargo_args", [])
    if REQUIRED_CARGO_FLAG not in args:
        errors.append(
            f"{package}: cargo_args must contain {REQUIRED_CARGO_FLAG}. Without it cargo "
            "STOPS at the first failing test binary, so a declared standing failure "
            "prevents the rest of the suite from being built at all — the masked tests "
            "would not have passed, they would never have run, and this gate would "
            "certify a suite that did not happen."
        )
    for flag in NON_EXECUTING_FLAGS:
        if flag in args:
            errors.append(
                f"{package}: cargo_args contains {flag}, which legitimately executes "
                "nothing. A baseline over a run that executes nothing is vacuous."
            )
    seen: set[str] = set()
    for entry in spec.get("expected_failures", []):
        for field_name in ("target", "test", "issue", "why"):
            if not entry.get(field_name):
                errors.append(
                    f"{package}: expected_failures entry {entry!r} is missing '{field_name}'. "
                    "An undocumented standing red is a stale exemption in waiting."
                )
        if not all(entry.get(f) for f in ("target", "test")):
            continue
        key = declared_key(entry)
        if key in seen:
            errors.append(f"{package}: {key} is declared twice")
        seen.add(key)
        path = ROOT / entry.get("path", "")
        if not entry.get("path") or not path.is_file():
            errors.append(
                f"{package}: {key} names path {entry.get('path')!r}, which is not a file. "
                "Either the target moved or this baseline is stale — both make the "
                "gate's claim about what it covers false."
            )
    return errors


def run_package(spec: dict) -> tuple[int, str]:
    command = ["cargo", "test", *spec["cargo_args"]]
    environment = os.environ.copy()
    environment["CARGO_TERM_COLOR"] = "never"
    print(f"BASELINE_GATE_CMD {' '.join(command)}", flush=True)
    completed = subprocess.run(
        command,
        cwd=ROOT,
        env=environment,
        text=True,
        stdout=subprocess.PIPE,
        # ONE stream: `Running …` is stderr, results are stdout. Capturing them
        # separately and concatenating would put every header after every result
        # and make per-target attribution impossible.
        stderr=subprocess.STDOUT,
    )
    # Echo the raw cargo output. The classification below is a claim ABOUT this
    # text; a reader who cannot see the text has to take the claim on trust, and
    # a gate whose evidence is not in its own log is one prose edit away from
    # saying whatever it likes.
    print(completed.stdout, end="", flush=True)
    return completed.returncode, completed.stdout


# ─────────────────────────── the CONTROL ───────────────────────────
#
# Synthetic cargo output, close enough to the real thing that the shipped
# classifier — not a re-typed copy of it — is what the cases drive.

OBSERVATIONS = [
    "unset_chain_id_reports_and_signs_the_same_identity",
    "unset_network_id_reports_and_signs_the_same_identity",
    "declared_signing_defaults_disagree_with_the_reporting_fallbacks",
    "empty_string_export_diverges_by_a_different_mechanism",
]
CONTROLS = [
    "control_a_reporting_fallbacks_are_still_written_where_this_test_claims",
    "control_b_explicit_configuration_makes_both_sides_agree",
]


def fake_section(
    path: str,
    ok: list[str],
    failed: list[str],
    unit: bool = False,
    ignored: list[str] | None = None,
) -> str:
    """One `Running …` section as cargo actually prints it."""
    ignored = ignored or []
    header = f"     Running {'unittests ' if unit else ''}{path} (target/debug/deps/x-0)\n"
    body = f"\nrunning {len(ok) + len(failed)} tests\n"
    for name in ok:
        body += f"test {name} ... ok\n"
    for name in ignored:
        body += f"test {name} ... ignored\n"
    for name in failed:
        body += f"test {name} ... FAILED\n"
    if failed:
        body += "\nfailures:\n\n"
        for name in failed:
            body += f"---- {name} stdout ----\nthread '{name}' panicked at src/x.rs:1:1:\n\n"
        body += "\nfailures:\n" + "".join(f"    {name}\n" for name in failed) + "\n"
    verdict = "FAILED" if failed else "ok"
    body += (
        f"test result: {verdict}. {len(ok)} passed; {len(failed)} failed; "
        f"{len(ignored)} ignored; 0 measured; 0 filtered out; finished in 0.01s\n"
    )
    return header + body


def fake_spec(**overrides) -> dict:
    spec = {
        "package": "wrfcoin-native-chain",
        "cargo_args": ["--locked", "--no-fail-fast", "-p", "wrfcoin-native-chain"],
        "why": "control",
        "expected_failures": [
            {
                "target": "chain_identity_split_2713",
                "test": name,
                "issue": 2713,
                "why": "control",
                "path": "native-chain/tests/chain_identity_split_2713.rs",
            }
            for name in OBSERVATIONS
        ],
    }
    spec.update(overrides)
    return spec


def self_test() -> int:
    split = "tests/chain_identity_split_2713.rs"
    other = "tests/network_info.rs"

    baseline_out = (
        fake_section(other, ["network_info_reports_the_configured_chain"], [])
        + fake_section(split, CONTROLS, OBSERVATIONS)
    )
    # The 27-day hole, exactly: a fresh failure in an unrelated target, with the
    # four standing failures still present. Same exit 101, same four names.
    new_elsewhere = (
        fake_section(other, [], ["network_info_reports_the_configured_chain"])
        + fake_section(split, CONTROLS, OBSERVATIONS)
    )
    # The subtler mask: the fresh failure is INSIDE the standing-red target, so
    # the target's polarity, its exit code and its failure COUNT all still look
    # like the accepted red.
    new_inside = fake_section(other, ["network_info_reports_the_configured_chain"], []) + (
        fake_section(split, [CONTROLS[1]], OBSERVATIONS + [CONTROLS[0]])
    )
    healed_one = fake_section(other, ["network_info_reports_the_configured_chain"], []) + (
        fake_section(split, CONTROLS + OBSERVATIONS[:1], OBSERVATIONS[1:])
    )
    deleted_one = fake_section(other, ["network_info_reports_the_configured_chain"], []) + (
        fake_section(split, CONTROLS, OBSERVATIONS[1:])
    )
    # Switching the standing red off rather than fixing it. libtest reports the
    # test, so it is neither missing nor passing — and a gate that folded this
    # into either would let the #2713 proof be disabled without a word.
    silenced_one = fake_section(other, ["network_info_reports_the_configured_chain"], []) + (
        fake_section(split, CONTROLS, OBSERVATIONS[1:], ignored=[OBSERVATIONS[0]])
    )
    target_absent = fake_section(other, ["network_info_reports_the_configured_chain"], [])
    empty_binary = baseline_out + fake_section("tests/feature_gated.rs", [], [])
    unnamed = fake_section(other, [], []).replace(
        "test result: ok. 0 passed; 0 failed",
        "test result: FAILED. 0 passed; 3 failed",
    )
    nothing = "error[E0432]: unresolved import `wrfcoin_native_chain::nope`\nerror: could not compile\n"

    cases: list[tuple[str, tuple, str, str | None]] = [
        ("declared four fail, nothing else", (baseline_out, 101, fake_spec()), BASELINE, None),
        (
            "fresh failure in an UNRELATED target (the 27-day hole)",
            (new_elsewhere, 101, fake_spec()),
            FINDING,
            "NEW_FAILURE network_info::network_info_reports_the_configured_chain",
        ),
        (
            "fresh failure INSIDE the standing-red target",
            (new_inside, 101, fake_spec()),
            FINDING,
            f"NEW_FAILURE chain_identity_split_2713::{CONTROLS[0]}",
        ),
        (
            "a declared failure started PASSING",
            (healed_one, 101, fake_spec()),
            FINDING,
            f"HEALED chain_identity_split_2713::{OBSERVATIONS[0]}",
        ),
        (
            "a declared failure was DELETED",
            (deleted_one, 101, fake_spec()),
            FINDING,
            f"MISSING chain_identity_split_2713::{OBSERVATIONS[0]}",
        ),
        (
            "a declared failure was #[ignore]d",
            (silenced_one, 101, fake_spec()),
            FINDING,
            f"SILENCED chain_identity_split_2713::{OBSERVATIONS[0]}",
        ),
        (
            "the declared target never ran",
            (target_absent, 0, fake_spec()),
            VOID,
            "DECLARED_TARGET_ABSENT",
        ),
        ("nothing compiled", (nothing, 101, fake_spec()), VOID, "no libtest summary"),
        ("an integration binary ran nothing", (empty_binary, 101, fake_spec()), VOID, "EMPTY_TEST_BINARY"),
        ("failures present but unnamed", (unnamed, 101, fake_spec()), VOID, "UNNAMED_FAILURES"),
        (
            "executed floor unmet",
            (baseline_out, 101, fake_spec(min_executed=10_000)),
            VOID,
            "FLOOR_UNMET executed=",
        ),
        (
            "binary-count floor unmet",
            (baseline_out, 101, fake_spec(min_summaries=47)),
            VOID,
            "FLOOR_UNMET summaries=",
        ),
    ]

    failures = 0
    for name, args, expected, needle in cases:
        report = classify_run(*args)
        lines = (
            report.new_failures + report.healed + report.silenced
            + report.missing + report.voids
        )
        ok = report.verdict == expected and (needle is None or any(needle in l for l in lines))
        print(f"  [{'ok' if ok else 'SELF-TEST FAILED'}] {name}: {report.verdict} (expected {expected})")
        if not ok:
            failures += 1
            for line in lines:
                print(f"      {line}")

    produced = {classify_run(*a).verdict for _, a, _, _ in cases}
    if produced != {BASELINE, FINDING, VOID}:
        print(
            f"FAIL-CLOSED: the classifier reached only {sorted(produced)} across the "
            "corpus. One that cannot reach all three verdicts cannot tell a finding "
            "from a broken gate."
        )
        failures += 1

    # Reachability of the VERDICTS is not reachability of the KINDS: every case
    # above could exit 1 through NEW_FAILURE alone and the check above would
    # still pass, leaving HEALED, SILENCED and MISSING as prose. Pin the kinds
    # by the prefix each one prints.
    kinds = {
        line.split()[0]
        for _, a, _, _ in cases
        for report in [classify_run(*a)]
        for line in report.new_failures + report.healed + report.silenced + report.missing
    }
    for kind in ("NEW_FAILURE", "HEALED", "SILENCED", "MISSING"):
        if kind not in kinds:
            print(f"FAIL-CLOSED: the corpus never produced a {kind} finding")
            failures += 1

    # The preflight must be able to refuse the invocation that would silently
    # halve the suite. A gate whose own load-bearing flag is unenforced is
    # exactly the shape #2911 records.
    without_flag = fake_spec(cargo_args=["--locked", "-p", "wrfcoin-native-chain"])
    if not any(REQUIRED_CARGO_FLAG in error for error in preflight(without_flag)):
        print(f"FAIL-CLOSED: preflight accepted cargo_args missing {REQUIRED_CARGO_FLAG}")
        failures += 1

    if failures:
        print(f"SELF-TEST FAILED ({failures} case(s)) — any observation below would be VOID")
        return 2
    print(
        f"SELF-TEST OK: {len(cases)} cases; all three verdicts and all four finding "
        "kinds (NEW_FAILURE / HEALED / SILENCED / MISSING) reachable"
    )
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, default=DEFAULT_MANIFEST)
    parser.add_argument(
        "--package",
        help="Only gate this package (default: every package in the manifest).",
    )
    parser.add_argument("--self-test", action="store_true", help="run the control and exit")
    parser.add_argument(
        "--preflight-only",
        action="store_true",
        help="structural checks only, no cargo",
    )
    parser.add_argument(
        "--from-file",
        type=Path,
        help="Classify a saved cargo log instead of running cargo. For "
        "re-reading captured evidence; --returncode supplies the exit status.",
    )
    parser.add_argument("--returncode", type=int, default=101, help="with --from-file")
    args = parser.parse_args()

    if args.self_test:
        return self_test()

    manifest = load_manifest(args.manifest)
    specs = manifest["packages"]
    if args.package:
        specs = [s for s in specs if s.get("package") == args.package]
        if not specs:
            raise SystemExit(f"FAIL-CLOSED: no manifest entry for package {args.package!r}")

    errors: list[str] = []
    for spec in specs:
        errors.extend(preflight(spec))
    if errors:
        for error in errors:
            print(f"FAIL-CLOSED: {error}")
        print(
            "\nGATE BROKEN — the manifest does not describe this tree, so nothing below "
            "would be evidence about the code. Exit 2."
        )
        return 2
    print(f"Preflight OK: {len(specs)} package baseline(s) well-formed and present.")
    if args.preflight_only:
        return 0

    worst = 0
    for spec in specs:
        if args.from_file:
            returncode, output = args.returncode, args.from_file.read_text(errors="replace")
        else:
            returncode, output = run_package(spec)
        report = classify_run(output, returncode, spec)
        render(report, spec)
        if report.verdict != BASELINE and os.environ.get("GITHUB_ACTIONS") == "true":
            for line in report.new_failures + report.healed + report.silenced + report.missing:
                print(f"::error::{line}")
            for line in report.voids:
                print(f"::error::{line}")
        worst = max(worst, EXIT[report.verdict])
    return worst


if __name__ == "__main__":
    raise SystemExit(main())
