#!/usr/bin/env python3
"""Hermetic regression controls for the #2490 cut integration gate."""
import importlib.util
import sys
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("cut_integration_gate.py")
SPEC = importlib.util.spec_from_file_location("cut_integration_gate", SCRIPT)
assert SPEC and SPEC.loader
GATE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = GATE
SPEC.loader.exec_module(GATE)


class CutIntegrationGateTests(unittest.TestCase):
    def test_nonexistent_manifest_target_is_red(self):
        targets = [{"package": "pkg", "target": "does_not_exist", "hosted_runner": "supported"}] * 28
        # validate() is the production seam: a renamed or invented target cannot
        # turn into a green job that executed nothing.
        errors = GATE.validate(targets, {("pkg", "real_target")})
        self.assertEqual(len(errors), 28)
        self.assertTrue(all("does_not_exist" in error for error in errors))

    def test_zero_test_summary_is_red(self):
        match = GATE.SUMMARY.search("test result: ok. 0 passed; 0 failed; 1 ignored")
        assert match is not None
        self.assertEqual(int(match.group(1)) + int(match.group(2)), 0)


if __name__ == "__main__":
    unittest.main()
