#!/usr/bin/env python3
"""Hermetic regression controls for the #2911 package-baseline gate.

These drive the SHIPPED classifier (`classify_run`) and the SHIPPED preflight
over synthetic cargo output, so a change that weakens either is caught here
rather than by a reviewer reading prose (the #2602 rule).
"""
import importlib.util
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("package_test_baseline_gate.py")
SPEC = importlib.util.spec_from_file_location("package_test_baseline_gate", SCRIPT)
assert SPEC and SPEC.loader
GATE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = GATE
SPEC.loader.exec_module(GATE)

MANIFEST = SCRIPT.with_name("package_test_baselines.json")

OBS = GATE.OBSERVATIONS
ROSTER_GUARD = "no_placeholder_key_may_reach_a_real_cut"  # #3078; declared 2026-09-09..10 while mango-mini carried placeholders, undeclared once its real key and payout were seated
CTRL = GATE.CONTROLS
SPLIT = "tests/chain_identity_split_2713.rs"
OTHER = "tests/network_info.rs"


def lines(report):
    return (
        report.new_failures + report.healed + report.silenced
        + report.missing + report.voids
    )


class ParsingTests(unittest.TestCase):
    def test_sections_are_attributed_to_their_own_binary(self):
        output = GATE.fake_section(OTHER, ["a"], ["b"]) + GATE.fake_section(SPLIT, CTRL, OBS)
        sections = {s.target: s for s in GATE.parse_sections(output)}
        self.assertEqual(sections["network_info"].failure_names, {"b"})
        self.assertEqual(sections["chain_identity_split_2713"].failure_names, set(OBS))

    def test_per_test_stdout_dump_is_not_read_as_a_name_list(self):
        # libtest prints `failures:` twice — once heading the panic dumps, once
        # heading the bare name list. Only the second is a name list.
        output = GATE.fake_section(SPLIT, CTRL, OBS)
        self.assertIn("---- ", output)
        section = GATE.parse_sections(output)[0]
        self.assertEqual(section.failure_names, set(OBS))

    def test_target_name_is_the_filename_stem(self):
        # The name the gate prints must be the one `cargo test --test <name>`
        # takes, or the reproduce command it emits does not work.
        self.assertEqual(GATE.section_target("itest", SPLIT), "chain_identity_split_2713")
        self.assertEqual(GATE.section_target("unit", "src/lib.rs"), "unittests:src/lib.rs")


class ClassifierTests(unittest.TestCase):
    def setUp(self):
        self.spec = GATE.fake_spec()
        self.baseline = GATE.fake_section(OTHER, ["ok_test"], []) + GATE.fake_section(
            SPLIT, CTRL, OBS
        )

    def test_declared_red_and_nothing_else_is_baseline(self):
        report = GATE.classify_run(self.baseline, 101, self.spec)
        self.assertEqual(report.verdict, GATE.BASELINE)
        self.assertEqual(len(report.confirmed), 4)
        self.assertEqual(lines(report), [])

    def test_a_fresh_failure_elsewhere_is_named_not_masked(self):
        # THE #2911 DEFECT. Same exit 101, same four standing names, one more
        # broken test. The old package command could not tell these apart.
        output = GATE.fake_section(OTHER, [], ["ok_test"]) + GATE.fake_section(SPLIT, CTRL, OBS)
        report = GATE.classify_run(output, 101, self.spec)
        self.assertEqual(report.verdict, GATE.FINDING)
        self.assertEqual(len(report.new_failures), 1)
        self.assertIn("network_info::ok_test", report.new_failures[0])
        # and the four are still reported as the declared baseline, by name
        self.assertEqual(len(report.confirmed), 4)

    def test_a_fresh_failure_inside_the_standing_red_target_is_named(self):
        # The subtler mask: the new failure shares the binary with the accepted
        # red, so the target's polarity, exit code and failure COUNT all still
        # look ordinary. Identity is the only dimension that sees it.
        output = GATE.fake_section(OTHER, ["ok_test"], []) + GATE.fake_section(
            SPLIT, [CTRL[1]], OBS + [CTRL[0]]
        )
        report = GATE.classify_run(output, 101, self.spec)
        self.assertEqual(report.verdict, GATE.FINDING)
        self.assertTrue(any(CTRL[0] in line for line in report.new_failures))

    def test_a_declared_failure_that_passes_is_a_finding(self):
        output = GATE.fake_section(OTHER, ["ok_test"], []) + GATE.fake_section(
            SPLIT, CTRL + OBS[:1], OBS[1:]
        )
        report = GATE.classify_run(output, 101, self.spec)
        self.assertEqual(report.verdict, GATE.FINDING)
        self.assertTrue(any(line.startswith("HEALED") for line in report.healed))

    def test_all_declared_failures_passing_is_a_finding_not_a_green(self):
        # If #2713 is fixed the package goes green — and that must be LOUD, or
        # the baseline silently becomes a permanent exemption nobody revisits.
        output = GATE.fake_section(OTHER, ["ok_test"], []) + GATE.fake_section(
            SPLIT, CTRL + OBS, []
        )
        report = GATE.classify_run(output, 0, self.spec)
        self.assertEqual(report.verdict, GATE.FINDING)
        self.assertEqual(len(report.healed), 4)

    def test_deleting_a_declared_failure_is_a_finding_not_baseline(self):
        # The other way to make a standing red go away.
        output = GATE.fake_section(OTHER, ["ok_test"], []) + GATE.fake_section(
            SPLIT, CTRL, OBS[1:]
        )
        report = GATE.classify_run(output, 101, self.spec)
        self.assertEqual(report.verdict, GATE.FINDING)
        self.assertTrue(any(line.startswith("MISSING") for line in report.missing))

    def test_ignoring_a_declared_failure_is_a_finding_distinct_from_healed(self):
        # Switching the standing red off is neither a fix nor a baseline, and
        # folding it into HEALED would report "#2713 may be fixed" about a test
        # that no longer runs.
        output = GATE.fake_section(OTHER, ["ok_test"], []) + GATE.fake_section(
            SPLIT, CTRL, OBS[1:], ignored=[OBS[0]]
        )
        report = GATE.classify_run(output, 101, self.spec)
        self.assertEqual(report.verdict, GATE.FINDING)
        self.assertEqual(report.healed, [])
        self.assertEqual(report.missing, [])
        self.assertTrue(any(line.startswith("SILENCED") for line in report.silenced))

    def test_a_declared_target_that_never_ran_is_void_not_healed(self):
        # Absence is not a pass. A target that failed to build must never be
        # read as "#2713 is fixed".
        output = GATE.fake_section(OTHER, ["ok_test"], [])
        report = GATE.classify_run(output, 0, self.spec)
        self.assertEqual(report.verdict, GATE.VOID)
        self.assertEqual(report.healed, [])
        self.assertTrue(any("DECLARED_TARGET_ABSENT" in line for line in report.voids))

    def test_compile_failure_is_void_not_a_finding(self):
        report = GATE.classify_run("error: could not compile `x`\n", 101, self.spec)
        self.assertEqual(report.verdict, GATE.VOID)
        self.assertEqual(report.new_failures, [])

    def test_empty_integration_binary_is_void(self):
        output = self.baseline + GATE.fake_section("tests/feature_gated.rs", [], [])
        report = GATE.classify_run(output, 101, self.spec)
        self.assertEqual(report.verdict, GATE.VOID)
        self.assertTrue(any("EMPTY_TEST_BINARY" in line for line in report.voids))

    def test_empty_unit_and_doc_sections_are_not_void(self):
        # A bin with no unit tests and a crate with no doc-tests print all-zero
        # summaries legitimately; only a tests/*.rs file exists solely to hold
        # tests.
        output = self.baseline + GATE.fake_section("src/main.rs", [], [], unit=True)
        report = GATE.classify_run(output, 101, self.spec)
        self.assertEqual(report.verdict, GATE.BASELINE)

    def test_unnamed_failures_are_void_not_new_findings(self):
        output = self.baseline.replace(
            "test result: ok. 1 passed; 0 failed", "test result: FAILED. 1 passed; 3 failed"
        )
        report = GATE.classify_run(output, 101, self.spec)
        self.assertEqual(report.verdict, GATE.VOID)
        self.assertTrue(any("UNNAMED_FAILURES" in line for line in report.voids))

    def test_doctest_failures_are_an_explicit_void_not_generic_unnamed_failures(self):
        output = (
            "   Doc-tests wrfcoin_native_chain\n\n"
            "running 2 tests\n\n"
            "test result: FAILED. 0 passed; 2 failed; 0 ignored; 0 measured; "
            "0 filtered out; finished in 0.01s\n"
        )
        report = GATE.classify_run(output, 101, self.spec)
        self.assertEqual(report.verdict, GATE.VOID)
        self.assertTrue(any("DOCTEST_FAILURES" in line for line in report.voids))
        self.assertFalse(any("UNNAMED_FAILURES" in line for line in report.voids))

    def test_floors_make_a_shrunken_suite_void(self):
        for key, needle in (("min_executed", "executed="), ("min_summaries", "summaries=")):
            report = GATE.classify_run(self.baseline, 101, GATE.fake_spec(**{key: 9999}))
            self.assertEqual(report.verdict, GATE.VOID)
            self.assertTrue(any(f"FLOOR_UNMET {needle}" in line for line in report.voids))

    def test_void_never_swallows_the_attributed_finding(self):
        # Exit 2 wins the status code, but a named failing test is attributable
        # on its own and must still appear in the output.
        output = (
            GATE.fake_section(OTHER, [], ["ok_test"])
            + GATE.fake_section(SPLIT, CTRL, OBS)
            + GATE.fake_section("tests/feature_gated.rs", [], [])
        )
        report = GATE.classify_run(output, 101, self.spec)
        self.assertEqual(report.verdict, GATE.VOID)
        self.assertEqual(len(report.new_failures), 1)


class PreflightTests(unittest.TestCase):
    def test_missing_no_fail_fast_is_refused(self):
        spec = GATE.fake_spec(cargo_args=["--locked", "-p", "wrfcoin-native-chain"])
        self.assertTrue(any(GATE.REQUIRED_CARGO_FLAG in e for e in GATE.preflight(spec)))

    def test_no_run_is_refused(self):
        spec = GATE.fake_spec(
            cargo_args=["--locked", "--no-fail-fast", "--no-run", "-p", "wrfcoin-native-chain"]
        )
        self.assertTrue(any("--no-run" in e for e in GATE.preflight(spec)))

    def test_an_expected_failure_must_name_an_existing_file(self):
        spec = GATE.fake_spec()
        spec["expected_failures"][0]["path"] = "native-chain/tests/does_not_exist.rs"
        self.assertTrue(any("not a file" in e for e in GATE.preflight(spec)))

    def test_an_expected_failure_must_cite_an_issue(self):
        spec = GATE.fake_spec()
        del spec["expected_failures"][0]["issue"]
        self.assertTrue(any("'issue'" in e for e in GATE.preflight(spec)))


class ShippedManifestTests(unittest.TestCase):
    """The manifest in this repo, not a synthetic one."""

    def setUp(self):
        self.manifest = json.loads(MANIFEST.read_text())

    def test_the_shipped_manifest_passes_its_own_preflight(self):
        for spec in self.manifest["packages"]:
            self.assertEqual(GATE.preflight(spec), [], spec.get("package"))

    def test_native_chain_declares_exactly_the_known_reds(self):
        spec = next(
            s for s in self.manifest["packages"] if s["package"] == "wrfcoin-native-chain"
        )
        declared = {e["test"] for e in spec["expected_failures"]}
        # Exactly the four #2713 observations. The #3078 roster guard was
        # declared while mango-mini's seat carried placeholders and came OUT
        # when the real key and payout were seated (2026-09-10) — it must not
        # linger: the gate's HEALED finding fires on a declared test that
        # passes. Pinned here so a declaration cannot be added (or linger)
        # without touching this test.
        self.assertEqual(declared, set(OBS))
        self.assertNotIn(ROSTER_GUARD, declared)
        # The two controls in the same file are deliberately NOT declared: they
        # must PASS, so a control that breaks is a NEW_FAILURE rather than being
        # absorbed by the failure count (#2854).
        self.assertTrue(declared.isdisjoint(set(CTRL)))
        issues = {e["test"]: e["issue"] for e in spec["expected_failures"]}
        self.assertTrue(all(issues[t] == 2713 for t in OBS))

    def test_floors_are_declared_and_positive(self):
        # A floor of 0 is no floor: a suite that shrinks to the four declared
        # failures alone would otherwise read as BASELINE.
        for spec in self.manifest["packages"]:
            self.assertGreater(spec["min_executed"], 0, spec["package"])
            self.assertGreater(spec["min_summaries"], 0, spec["package"])
            self.assertNotEqual(spec.get("floors_measured"), "PENDING")


class CliTests(unittest.TestCase):
    def test_self_test_exits_zero(self):
        completed = subprocess.run(
            [sys.executable, str(SCRIPT), "--self-test"], capture_output=True, text=True
        )
        self.assertEqual(completed.returncode, 0, completed.stdout + completed.stderr)
        self.assertIn("all three verdicts", completed.stdout)
        self.assertIn("all four finding kinds", completed.stdout)

    def test_preflight_only_runs_no_cargo(self):
        completed = subprocess.run(
            [sys.executable, str(SCRIPT), "--preflight-only"], capture_output=True, text=True
        )
        self.assertEqual(completed.returncode, 0, completed.stdout + completed.stderr)
        self.assertNotIn("BASELINE_GATE_CMD", completed.stdout)

    def test_from_file_classifies_a_saved_log_with_the_shipped_exit_codes(self):
        spec = GATE.fake_spec()
        baseline = GATE.fake_section(OTHER, ["ok_test"], []) + GATE.fake_section(SPLIT, CTRL, OBS)
        cases = ((baseline, 0), (GATE.fake_section(OTHER, [], ["ok_test"]) + GATE.fake_section(SPLIT, CTRL, OBS), 1))
        with tempfile.TemporaryDirectory() as tmp:
            manifest = Path(tmp) / "m.json"
            spec = dict(spec, min_executed=1, min_summaries=2)
            for entry in spec["expected_failures"]:
                entry["path"] = "native-chain/tests/chain_identity_split_2713.rs"
            manifest.write_text(json.dumps({"schema": 1, "why": "test", "packages": [spec]}))
            for output, expected_exit in cases:
                log = Path(tmp) / "log.txt"
                log.write_text(output)
                completed = subprocess.run(
                    [sys.executable, str(SCRIPT), "--manifest", str(manifest),
                     "--from-file", str(log), "--returncode", "101"],
                    capture_output=True, text=True,
                )
                self.assertEqual(completed.returncode, expected_exit, completed.stdout)


if __name__ == "__main__":
    unittest.main()
