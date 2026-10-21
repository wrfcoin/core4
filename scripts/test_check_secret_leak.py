#!/usr/bin/env python3
"""Planted-leak regression suite for check_secret_leak.py (#2129).

Review of PR #2239 made the point that matters: without fixtures that plant a
leak on **every** claimed surface, the scanner proves nothing. It found that a
one-line planted leak was caught while the semantically identical **multiline**
form exited 0, and a planted `bail!("...{signing_key}")` exited 0 because error
macros were not sinks. Both were true, and both were invisible because nothing
tested the scanner itself.

So: every fixture under `secret_leak_fixtures/` that plants a leak must be
rejected, the clean one must be accepted, and the two production exposures filed
as #2259 must be detected in the real tree.

Run: python3 scripts/test_check_secret_leak.py
"""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
FIXTURES = HERE / "tests" / "secret_leak_fixtures"
SCANNER = HERE / "check_secret_leak.py"
REPO = HERE.parent

# Every fixture that plants a leak, with the surface it exercises. Deliberately
# multiline where the surface allows it — the multiline form is the one that
# regressed.
MUST_REJECT = {
    "log_multiline.rs.txt": "log macro, multiline",
    "format_multiline.rs.txt": "format! macro, multiline",
    "panic_multiline.rs.txt": "panic payload, multiline",
    "metric_multiline.rs.txt": "metric label, multiline",
    "error_bail.rs.txt": "anyhow::bail! error constructor",
    "error_context.rs.txt": ".context() error context",
    "derived_debug.rs.txt": "derived Debug/Serialize over secret fields",
    "derived_clone.rs.txt": "derived Clone over secret fields (#2266 review)",
    "tuple_secret.rs.txt": "derives on a secret-named TUPLE struct (#2266 review)",
    "token_format.rs.txt": "bearer token formatted into an identifier",
    "pin_named_secret.rs.txt": "exact `pin` identifier name now covered",
    "display_write_str.rs.txt":
        "custom Display leaking via f.write_str — no macro involved (#2239 blocker 3)",
    # The two #2259 production exposures, pinned verbatim from pre-#2266 main.
    # These were previously asserted against the live tree, which goes green —
    # correctly — the moment #2266 merges; pinning keeps detection tested.
    "regression_2259_stealth_keys.rs.txt": "#2259 StealthKeys derived exposure (pinned)",
    "regression_2259_token_node_id.rs.txt": "#2259 token-as-node-id (pinned)",
    # #3284: one sink-shaped literal must not blind the rest of a file. Each
    # plants the leak BELOW the span-breaking construct, with an allowlisted
    # line further below — the shape that hid everything past ~line 8082 of
    # p2p_events.rs.
    "blind_span_escaped_string.rs.txt":
        "sink text inside an escaped string literal must not open a span (#3284)",
    "blind_span_char_and_raw.rs.txt":
        "char literal '(' and raw string ending in a backslash (#3284)",
    "blind_span_unclosed.rs.txt":
        "an unbalanced invocation is bounded, not extended to EOF (#3284)",
    "derived_single_line_body.rs.txt":
        "derive over a single-line struct body `{ seed: [u8; 32] }` (#3284)",
    "derived_pub_restricted_field.rs.txt":
        "derive over a `pub(crate)` secret field (#3284)",
    # #3287: #3286 review follow-ups.
    "raw_c_string.rs.txt": "C-string raw literal cr\"C:\\\" lexed without escapes",
    "marker_in_string.rs.txt":
        "a secret-leak-ok marker inside a string literal does not suppress",
    "derived_serde_rename_brace.rs.txt":
        "a `}` inside an attribute string does not end the struct body",
    "error_attribute.rs.txt": "thiserror #[error(\"{signing_key}\")] is a sink",
}
# A leak "caught" at the wrong line is the #3284 failure in disguise: an
# EOF-length span reports the line of the broken literal, not the leak. These
# fixtures must report the leak at exactly this line.
EXPECT_LINE = {
    "blind_span_escaped_string.rs.txt": 10,
    "blind_span_char_and_raw.rs.txt": 13,
    "blind_span_unclosed.rs.txt": 8,
    "derived_single_line_body.rs.txt": 4,
    "derived_pub_restricted_field.rs.txt": 4,
    "raw_c_string.rs.txt": 9,
    "marker_in_string.rs.txt": 4,
    "derived_serde_rename_brace.rs.txt": 4,
    "error_attribute.rs.txt": 5,
}
MUST_ACCEPT = {
    "marker_in_comment_ok.rs.txt":
        "a real trailing-comment secret-leak-ok marker still suppresses",
    "error_attribute_prose_ok.rs.txt":
        "#[error] with secret words in prose and a positional capture only",
    "sink_text_in_string_ok.rs.txt":
        "sink-shaped text inside a string literal is data, not a call (#3284)",
    "clean.rs.txt": "secret words in prose only, no value interpolated",
    "derived_allowlisted_ok.rs.txt":
        "secret-leak-ok annotation on a derive must suppress the structural "
        "finding, as the failure message promises",
}


def run_scanner(target: Path) -> tuple[int, str]:
    proc = subprocess.run(
        [sys.executable, str(SCANNER), str(target), "--no-baseline"],
        capture_output=True,
        text=True,
        cwd=REPO,
    )
    return proc.returncode, proc.stdout + proc.stderr


def as_rust(fixture: Path, tmp: Path) -> Path:
    """Copy a `.rs.txt` fixture to a real `.rs` path the scanner will read."""
    dest = tmp / (fixture.name.removesuffix(".txt"))
    dest.write_text(fixture.read_text())
    return dest


def main() -> int:
    import tempfile

    failures: list[str] = []
    checked = 0

    with tempfile.TemporaryDirectory() as td:
        tmp = Path(td)

        for name, surface in MUST_REJECT.items():
            fixture = FIXTURES / name
            if not fixture.exists():
                failures.append(f"MISSING fixture {name} ({surface})")
                continue
            target = as_rust(fixture, tmp)
            code, out = run_scanner(target)
            checked += 1
            if code == 0:
                failures.append(
                    f"NOT DETECTED: {name} [{surface}] exited 0 — a planted leak on a "
                    f"claimed surface was not rejected\n{out}"
                )
            elif name in EXPECT_LINE and f"{target}:{EXPECT_LINE[name]}:" not in out:
                failures.append(
                    f"WRONG LINE: {name} [{surface}] was rejected, but not at line "
                    f"{EXPECT_LINE[name]} where the leak is planted\n{out}"
                )

        for name, why in MUST_ACCEPT.items():
            fixture = FIXTURES / name
            if not fixture.exists():
                failures.append(f"MISSING fixture {name}")
                continue
            code, out = run_scanner(as_rust(fixture, tmp))
            checked += 1
            if code != 0:
                failures.append(
                    f"FALSE POSITIVE: {name} [{why}] exited {code} — a clean file was "
                    f"rejected; the scanner is too aggressive to gate on\n{out}"
                )

    # The #2259 production exposures are covered by the two pinned
    # `regression_2259_*` fixtures above. They are deliberately NOT asserted
    # against the live tree: once #2266 lands, the tree is clean and a
    # live-tree assertion would fail on the fix itself.

    # Blocker 4 (#2239): a missing root must FAIL, not warn-and-exit-0. A
    # scanner that scans nothing and reports clean is a gate whose own tests
    # pass while it verifies nothing.
    code, out = run_scanner(Path("this-root-does-not-exist-xyzzy"))
    checked += 1
    if code == 0:
        failures.append(
            "FAIL-OPEN: scanning a nonexistent root exited 0 — a missing "
            f"production root must fail the run\n{out}"
        )

    # Blocker 1 (#2239): the shipped CLI binary must be part of the default
    # scan scope — it generates and loads mnemonics, private keys and
    # operator tokens. Asserting on the config catches a silent re-exclusion.
    import check_secret_leak as scanner_mod
    checked += 1
    if "tools/cli/WrfcoinCLI/src" not in scanner_mod.PRODUCTION_ROOTS:
        failures.append(
            "SCOPE REGRESSION: tools/cli/WrfcoinCLI/src is no longer in "
            "PRODUCTION_ROOTS — the shipped CLI handles key material and must "
            "stay in the default scan"
        )

    # Scope completeness (#3124 infra review): every cargo workspace member's
    # src/ must be scanned or deliberately omitted. A new crate in neither list
    # is silently unscanned — the scanner reports clean over code it never
    # read. Checked against the LIVE workspace, so adding a crate without
    # classifying it fails here.
    checked += 1
    unclassified = scanner_mod.unclassified_member_roots(REPO)
    if unclassified:
        failures.append(
            "UNCLASSIFIED WORKSPACE MEMBER(S): "
            + ", ".join(unclassified)
            + " — add each to PRODUCTION_ROOTS, or to OMITTED_ROOTS with a reason"
        )
    if not scanner_mod.workspace_member_src_roots(REPO):
        failures.append(
            "SCOPE CHECK VOID: no workspace member src/ roots were found — the "
            "completeness check would pass vacuously"
        )

    # The completeness check itself must fail on an unclassified member: a
    # synthetic workspace with one scanned crate, one omitted crate, and one in
    # neither list. The unclassified name is synthetic on purpose: `da` (the
    # crate that prompted this check) is now classified, and a real crate name
    # would silently stop exercising the failure path once it is.
    import tempfile as _tempfile
    checked += 1
    with _tempfile.TemporaryDirectory() as td:
        ws = Path(td)
        (ws / "Cargo.toml").write_text(
            '[workspace]\nmembers = ["shared-protocol", "tests/integration", "zz-unclassified-crate"]\n'
        )
        for member in ("shared-protocol", "tests/integration", "zz-unclassified-crate"):
            (ws / member / "src").mkdir(parents=True)
            (ws / member / "Cargo.toml").write_text("[package]\n")
        got = scanner_mod.unclassified_member_roots(ws)
        if got != ["zz-unclassified-crate/src"]:
            failures.append(
                f"SCOPE CHECK BLIND: synthetic workspace expected "
                f"['zz-unclassified-crate/src'] "
                f"unclassified, got {got}"
            )

    if failures:
        print(f"check_secret_leak self-test: {len(failures)} failure(s) of {checked} checks\n")
        for f in failures:
            print(f"  {f}\n")
        return 1

    print(f"check_secret_leak self-test: all {checked} checks passed "
          f"({len(MUST_REJECT)} planted surfaces rejected — including the two "
          f"pinned #2259 production exposures — {len(MUST_ACCEPT)} clean accepted)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
