#!/usr/bin/env python3
"""Regression tests for the #2953 executed-crate floor."""

from __future__ import annotations

import importlib.util
import subprocess
import sys
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("check_crate_execution_floor.py")
SPEC = importlib.util.spec_from_file_location("check_crate_execution_floor", SCRIPT)
assert SPEC and SPEC.loader
FLOOR = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = FLOOR
SPEC.loader.exec_module(FLOOR)


LANES_ALL_GREEN = [
    "consensus-engines=success",
    "shared-protocol=success",
    "privacy-zk-execution=success",
    "leaf-crates=success",
]


class CrateExecutionFloorTests(unittest.TestCase):
    def test_all_success_is_green_with_full_attribution(self):
        report, errors = FLOOR.evaluate(LANES_ALL_GREEN)
        self.assertEqual(errors, [])
        self.assertIn("CRATE_EXECUTION_ATTEMPTED count=4 of=4", report)
        for lane in ("consensus-engines", "shared-protocol",
                     "privacy-zk-execution", "leaf-crates"):
            self.assertIn(f"CRATE_EXECUTION_RAN {lane} outcome=success", report)
        self.assertFalse(any("SKIPPED" in line for line in report))

    def test_zero_lanes_executed_is_red_naming_the_shape(self):
        # The job-99552091866 shape: every crate lane skipped.
        report, errors = FLOOR.evaluate(
            [pair.replace("success", "skipped") for pair in LANES_ALL_GREEN]
        )
        self.assertIn("CRATE_EXECUTION_ATTEMPTED count=0 of=4", report)
        self.assertTrue(
            any("ZERO_CRATE_LANES_EXECUTED" in error for error in errors)
        )
        self.assertEqual(
            sum(1 for error in errors if error.startswith("LANE_SKIPPED ")), 4
        )

    def test_early_failure_counts_as_executed_not_skipped(self):
        # Bob's inherited finding (#2953): a lane that ran and FAILED must be
        # attributed EXECUTED. Success-only counting would record it as not
        # executed, letting a genuinely red crate lower the executed count.
        report, errors = FLOOR.evaluate(
            [
                "consensus-engines=failure",
                "shared-protocol=skipped",
                "privacy-zk-execution=skipped",
                "leaf-crates=skipped",
            ]
        )
        # The failing lane is in the RUN list, never the skip list...
        self.assertIn(
            "CRATE_EXECUTION_RAN consensus-engines outcome=failure", report
        )
        self.assertNotIn("CRATE_EXECUTION_SKIPPED consensus-engines", report)
        # ...and it raises the attempted count.
        self.assertIn("CRATE_EXECUTION_ATTEMPTED count=1 of=4", report)
        # The gate is still RED, under the lane's own name plus the skips.
        self.assertTrue(
            any("LANE_EXECUTED_RED consensus-engines outcome=failure" in e
                for e in errors)
        )
        self.assertEqual(
            sum(1 for error in errors if error.startswith("LANE_SKIPPED ")), 3
        )
        # But NOT the zero-executed error: one lane genuinely ran.
        self.assertFalse(
            any("ZERO_CRATE_LANES_EXECUTED" in error for error in errors)
        )

    def test_cancelled_lane_is_executed_and_red(self):
        _report, errors = FLOOR.evaluate(
            ["consensus-engines=cancelled"] + LANES_ALL_GREEN[1:]
        )
        self.assertTrue(
            any("LANE_EXECUTED_RED consensus-engines outcome=cancelled" in e
                for e in errors)
        )

    def test_partial_skip_is_red_even_with_other_lanes_green(self):
        _report, errors = FLOOR.evaluate(
            LANES_ALL_GREEN[:3] + ["leaf-crates=skipped"]
        )
        self.assertTrue(
            any("LANE_SKIPPED leaf-crates" in error for error in errors)
        )

    def test_malformed_invocations_fail_closed(self):
        for pairs, needle in [
            ([], "NO_LANES_DECLARED"),
            (["consensus-engines"], "MALFORMED_LANE"),
            (["consensus-engines=mystery"], "UNKNOWN_OUTCOME"),
            (["a=success", "a=success"], "DUPLICATE_LANE"),
        ]:
            with self.subTest(pairs=pairs):
                _report, errors = FLOOR.evaluate(pairs)
                self.assertTrue(any(needle in error for error in errors), errors)

    # -- process level: the exit code is the caller-visible contract ---------

    def _run_cli(self, args):
        return subprocess.run(
            [sys.executable, str(SCRIPT), *args],
            capture_output=True,
            text=True,
            timeout=60,
        )

    def test_cli_green_exits_zero(self):
        completed = self._run_cli(LANES_ALL_GREEN)
        self.assertEqual(completed.returncode, 0)
        self.assertIn("CRATE_EXECUTION_FLOOR_OK executed=4 of=4", completed.stdout)

    def test_cli_zero_executed_exits_nonzero(self):
        completed = self._run_cli(
            [pair.replace("success", "skipped") for pair in LANES_ALL_GREEN]
        )
        self.assertEqual(completed.returncode, 1)
        self.assertIn("ZERO_CRATE_LANES_EXECUTED", completed.stderr)
        self.assertNotIn("CRATE_EXECUTION_FLOOR_OK", completed.stdout)

    def test_cli_early_failure_exits_nonzero_with_executed_attribution(self):
        completed = self._run_cli(
            [
                "consensus-engines=failure",
                "shared-protocol=skipped",
                "privacy-zk-execution=skipped",
                "leaf-crates=skipped",
            ]
        )
        self.assertEqual(completed.returncode, 1)
        self.assertIn(
            "CRATE_EXECUTION_RAN consensus-engines outcome=failure",
            completed.stdout,
        )
        self.assertIn("LANE_EXECUTED_RED consensus-engines", completed.stderr)


if __name__ == "__main__":
    unittest.main()
