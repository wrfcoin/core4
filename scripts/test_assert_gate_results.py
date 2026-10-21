#!/usr/bin/env python3
"""Regression tests for the terminal skip-guard result semantics."""

import unittest

from assert_gate_results import check


class GateResultTests(unittest.TestCase):
    def test_required_success_passes(self):
        self.assertEqual([], check('{"rust": {"result": "success"}}', {"rust"}, set()))

    def test_required_skip_is_a_failure(self):
        errors = check('{"rust": {"result": "skipped"}}', {"rust"}, set())
        self.assertTrue(any("required gate 'rust'" in error for error in errors))

    def test_conditional_skip_is_explicitly_allowed(self):
        self.assertEqual([], check('{"api": {"result": "skipped"}}', set(), {"api"}))

    def test_unclassified_dependency_fails_closed(self):
        errors = check('{"rust": {"result": "success"}, "new": {"result": "success"}}', {"rust"}, set())
        self.assertTrue(any("unclassified" in error for error in errors))


if __name__ == "__main__":
    unittest.main()
