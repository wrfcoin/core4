#!/usr/bin/env python3
"""Hermetic regression controls for the #2861 zero-match vacuity wrapper."""
import importlib.util
import subprocess
import sys
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("cargo_test_nonvacuous.py")
SPEC = importlib.util.spec_from_file_location("cargo_test_nonvacuous", SCRIPT)
assert SPEC and SPEC.loader
WRAPPER = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = WRAPPER
SPEC.loader.exec_module(WRAPPER)

ZERO_MATCH = (
    "running 0 tests\n\n"
    "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; "
    "1747 filtered out; finished in 0.00s\n"
)
ONE_PASS = (
    "running 1 test\ntest a ... ok\n\n"
    "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; "
    "1746 filtered out; finished in 0.05s\n"
)
# A filter run over a package emits one summary per test binary plus the
# doc-test harness: several zero-executed summaries must not add up to green.
MULTI_BINARY_ZERO = ZERO_MATCH + (
    "   Doc-tests wrfcoin_shared_protocol\n\nrunning 0 tests\n\n"
    "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; "
    "0 filtered out; finished in 0.00s\n"
)


class NonvacuousWrapperTests(unittest.TestCase):
    def test_zero_match_filter_is_red(self):
        errors = WRAPPER.evaluate(ZERO_MATCH, 0, 1)
        self.assertTrue(errors and "VACUOUS_TEST_RUN" in errors[0])
        self.assertIn("filtered_out=1747", errors[0])

    def test_multi_binary_zero_executed_is_red(self):
        errors = WRAPPER.evaluate(MULTI_BINARY_ZERO, 0, 1)
        self.assertTrue(errors and "VACUOUS_TEST_RUN" in errors[0])

    def test_genuine_match_is_green(self):
        self.assertEqual(WRAPPER.evaluate(ONE_PASS, 0, 1), [])

    def test_one_executed_among_zero_summaries_is_green(self):
        self.assertEqual(WRAPPER.evaluate(ZERO_MATCH + ONE_PASS, 0, 1), [])

    def test_cargo_failure_propagates_untouched(self):
        errors = WRAPPER.evaluate(ONE_PASS, 101, 1)
        self.assertTrue(errors and "cargo exited 101" in errors[0])

    def test_summary_free_output_is_red(self):
        errors = WRAPPER.evaluate("   Compiling foo v0.1.0\n", 0, 1)
        self.assertTrue(errors and "no libtest summary" in errors[0])

    def test_min_executed_floor(self):
        self.assertTrue(WRAPPER.evaluate(ONE_PASS, 0, 2))
        self.assertEqual(WRAPPER.evaluate(ONE_PASS, 0, 1), [])

    # ---- #2889: the binary-count dimension ----

    def test_min_summaries_rejects_a_vanished_test_binary(self):
        """A whole binary dropping out clears a count floor but not this one.

        This is the p2p shape: 486 tests across ten binaries, nine of which
        contribute 16 between them. Losing one of the nine moves the count by
        a rounding error.
        """
        ten_binaries = ONE_PASS * 10
        nine_binaries = ONE_PASS * 9
        # Count floor alone: both pass.
        self.assertEqual(WRAPPER.evaluate(nine_binaries, 0, 9), [])
        # Binary floor: the nine-binary run is REJECTED.
        self.assertEqual(WRAPPER.evaluate(ten_binaries, 0, 9, 10), [])
        errors = WRAPPER.evaluate(nine_binaries, 0, 9, 10)
        self.assertTrue(errors)
        self.assertIn("summaries=9", errors[0])

    def test_min_summaries_defaults_to_disabled(self):
        """Every existing caller passes no value and must be unaffected."""
        self.assertEqual(WRAPPER.evaluate(ONE_PASS, 0, 1), [])
        self.assertEqual(WRAPPER.evaluate(ONE_PASS, 0, 1, 0), [])

    def test_cargo_failure_still_wins_over_the_summaries_floor(self):
        """The wrapper may only ADD failure modes, never mask a real red."""
        errors = WRAPPER.evaluate(ONE_PASS, 101, 1, 99)
        self.assertEqual(len(errors), 1)
        self.assertIn("cargo exited 101", errors[0])

    # ---- #2889: the staleness detector ----

    # ---- #2899 review: --min-summaries must not fail open ----

    def test_negative_min_summaries_is_rejected_at_the_boundary(self):
        for bad in ("-1", "-10"):
            with self.assertRaises(Exception):
                WRAPPER.nonnegative_int(bad)

    def test_zero_and_positive_min_summaries_are_accepted(self):
        self.assertEqual(WRAPPER.nonnegative_int("0"), 0)
        self.assertEqual(WRAPPER.nonnegative_int("6"), 6)

    def test_cli_rejects_negative_min_summaries_before_running_cargo(self):
        """argparse must refuse it, so no cargo is spawned at all."""
        result = subprocess.run(
            [sys.executable, str(SCRIPT), "--min-summaries", "-1",
             "--", "test", "-p", "does-not-exist"],
            capture_output=True, text=True,
        )
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertIn("--min-summaries must be >= 0", result.stderr)
        self.assertNotIn("NONVACUOUS_TEST_CMD", result.stdout)

    def test_negative_floor_would_have_been_silently_unenforceable(self):
        """The defect this guards: a negative floor never triggers the check."""
        nine = ONE_PASS * 9
        self.assertTrue(WRAPPER.evaluate(nine, 0, 1, 10))   # honest floor: RED
        self.assertEqual(WRAPPER.evaluate(nine, 0, 1, -1), [])  # negative: no-op

    # ---- #2899 review: staleness must cover the summaries dimension ----

    def test_summaries_staleness_is_reported(self):
        note = WRAPPER.stale_floor_note(20, 10, "summaries")
        self.assertIsNotNone(note)
        self.assertIn("summaries=20", note)
        self.assertIn("test binaries", note)

    def test_exact_summaries_floor_is_not_stale(self):
        self.assertIsNone(WRAPPER.stale_floor_note(10, 10, "summaries"))
        self.assertIsNone(WRAPPER.stale_floor_note(6, 6, "summaries"))

    def test_executed_dimension_still_says_tests(self):
        note = WRAPPER.stale_floor_note(486, 1)
        self.assertIn("executed=486", note)
        self.assertIn("tests", note)

    def test_stale_floor_is_reported(self):
        note = WRAPPER.stale_floor_note(486, 1)
        self.assertIsNotNone(note)
        self.assertIn("FLOOR_STALE", note)
        self.assertIn("486x", note)

    def test_healthy_floor_is_not_reported_stale(self):
        self.assertIsNone(WRAPPER.stale_floor_note(8, 8))
        self.assertIsNone(WRAPPER.stale_floor_note(486, 430))

    def test_staleness_never_turns_a_green_run_red(self):
        """Growth is good news; the detector must not fail the build for it."""
        self.assertEqual(WRAPPER.evaluate(ONE_PASS, 0, 1), [])
        self.assertIsNotNone(WRAPPER.stale_floor_note(486, 1))

    def test_self_test_reaches_both_verdicts(self):
        self.assertEqual(WRAPPER.self_test(), 0)

    def test_nonpositive_floor_is_rejected_at_the_boundary(self):
        # #2867 review: an accepted --min-executed 0 printed
        # NONVACUOUS_TEST_OK executed=0 on a genuine zero-match run — the
        # guard failing open at its own boundary. Rejection, not comparison.
        import argparse
        for bad in ("0", "-1", "-8"):
            with self.assertRaises(argparse.ArgumentTypeError):
                WRAPPER.positive_int(bad)
        self.assertEqual(WRAPPER.positive_int("1"), 1)
        self.assertEqual(WRAPPER.positive_int("8"), 8)

    # EMPTY_TEST_BINARY: a target whose summary is all-zero ran nothing and
    # declared nothing — fake green behind its siblings (the shared-protocol
    # yubikey_signer shape: 2,120 sibling tests green, one target empty,
    # package gate success). Doc-tests sections are exempt; an all-ignored
    # sentinel binary is a countable absence (#2748) and stays green.
    def test_empty_test_binary_behind_green_siblings_is_red(self):
        real = (
            "     Running tests/real.rs (target/debug/deps/r)\n"
            "running 1 test\ntest a ... ok\n\n"
            "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; "
            "0 filtered out; finished in 0.01s\n"
        )
        empty = (
            "     Running tests/yubikey_signer.rs (target/debug/deps/y)\n"
            "running 0 tests\n\n"
            "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; "
            "0 filtered out; finished in 0.00s\n"
        )
        errors = WRAPPER.evaluate(real + empty, 0, 1)
        self.assertTrue(any("EMPTY_TEST_BINARY" in e for e in errors), errors)
        self.assertTrue(any("yubikey_signer" in e for e in errors), errors)

    def test_all_zero_doctests_summary_is_green(self):
        real = (
            "     Running tests/real.rs (target/debug/deps/r)\n"
            "running 1 test\ntest a ... ok\n\n"
            "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; "
            "0 filtered out; finished in 0.01s\n"
        )
        doc = (
            "   Doc-tests wrfcoin_shared_protocol\n"
            "running 0 tests\n\n"
            "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; "
            "0 filtered out; finished in 0.00s\n"
        )
        self.assertEqual(WRAPPER.evaluate(real + doc, 0, 1), [])

    def test_all_ignored_sentinel_binary_is_green(self):
        real = (
            "     Running tests/real.rs (target/debug/deps/r)\n"
            "running 1 test\ntest a ... ok\n\n"
            "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; "
            "0 filtered out; finished in 0.01s\n"
        )
        sentinel = (
            "     Running tests/yubikey_signer.rs (target/debug/deps/y)\n"
            "running 1 test\n"
            "test yubikey_tests_require_the_hsm_feature ... ignored\n\n"
            "test result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; "
            "0 filtered out; finished in 0.00s\n"
        )
        self.assertEqual(WRAPPER.evaluate(real + sentinel, 0, 1), [])

    def test_testless_bin_unittests_summary_is_green(self):
        # A bin/lib target with no unit tests prints an all-zero "unittests"
        # summary in every broad `cargo test -p` run; that is normal, not
        # vacuous — only tests/*.rs integration targets exist solely to hold
        # tests. Without this exemption the rule would red every wrapped
        # broad suite that includes a test-less binary.
        real = (
            "     Running tests/real.rs (target/debug/deps/r)\n"
            "running 1 test\ntest a ... ok\n\n"
            "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; "
            "0 filtered out; finished in 0.01s\n"
        )
        binsum = (
            "     Running unittests src/bin/tool.rs (target/debug/deps/t)\n"
            "running 0 tests\n\n"
            "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; "
            "0 filtered out; finished in 0.00s\n"
        )
        self.assertEqual(WRAPPER.evaluate(real + binsum, 0, 1), [])

    def test_filter_miss_is_not_reported_as_empty_binary(self):
        # A zero-match FILTER run (filtered > 0) is the pre-existing #2861
        # min-executed class, not an empty binary; the messages must not blur.
        errors = WRAPPER.evaluate(ZERO_MATCH, 0, 1)
        self.assertTrue(errors)
        self.assertFalse(any("EMPTY_TEST_BINARY" in e for e in errors), errors)

    def test_cli_rejects_zero_floor_before_running_cargo(self):
        # Real CLI-level regression: the process must refuse --min-executed 0
        # (and -1) BEFORE any cargo command is spawned.
        import subprocess
        for bad in ("0", "-1"):
            completed = subprocess.run(
                [sys.executable, str(SCRIPT), "--min-executed", bad,
                 "--", "test", "definitely_absent_filter"],
                capture_output=True, text=True,
            )
            self.assertNotEqual(completed.returncode, 0)
            self.assertIn("must be >= 1", completed.stderr)
            # Argparse rejects before main() runs cargo: no command banner.
            self.assertNotIn("NONVACUOUS_TEST_CMD", completed.stdout)
            self.assertNotIn("NONVACUOUS_TEST_OK", completed.stdout)


if __name__ == "__main__":
    unittest.main()
