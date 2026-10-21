#!/usr/bin/env python3
"""Mutation proofs for the stable determinism-matrix aggregate context."""

from __future__ import annotations

import json
import unittest
from pathlib import Path

import yaml

from assert_gate_results import check, names
from check_terminal_skip_guards import WorkflowLoader


ROOT = Path(__file__).resolve().parents[1]
WORKFLOW = ROOT / ".github/workflows/determinism-matrix.yml"


class DeterminismMatrixGateTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        document = yaml.load(WORKFLOW.read_text(), Loader=WorkflowLoader)
        cls.jobs = document["jobs"]
        cls.aggregate = cls.jobs["terminal-skip-guard"]
        cls.needs = cls.aggregate["needs"]
        gate_steps = [
            step
            for step in cls.aggregate["steps"]
            if "python3 scripts/assert_gate_results.py" in str(step.get("run", ""))
        ]
        if len(gate_steps) != 1:
            raise AssertionError("determinism aggregate must have exactly one result-check step")
        environment = gate_steps[0]["env"]
        cls.required = names(str(environment.get("GATE_REQUIRED", "")))
        cls.conditional = names(str(environment.get("GATE_CONDITIONAL", "")))

    def results(self, mutation: tuple[str, str] | None = None) -> str:
        results = {dependency: {"result": "success"} for dependency in self.needs}
        if mutation is not None:
            dependency, result = mutation
            results[dependency]["result"] = result
        return json.dumps(results)

    def test_context_is_stable_and_version_free(self) -> None:
        self.assertEqual("Determinism matrix gate", self.aggregate["name"])
        self.assertNotIn("${{", self.aggregate["name"])
        self.assertNotRegex(self.aggregate["name"], r"\d+\.\d+")

    def test_aggregate_has_complete_fan_in(self) -> None:
        compare_needs = self.jobs["compare"]["needs"]
        if isinstance(compare_needs, str):
            compare_needs = [compare_needs]
        self.assertEqual({"emit-roots", "compare"}, set(self.needs))
        self.assertEqual({"emit-roots"}, set(compare_needs))
        self.assertIn("always()", str(self.aggregate["if"]))
        self.assertEqual(set(self.needs), self.required)
        self.assertEqual(set(), self.conditional)

    def test_all_success_is_green(self) -> None:
        self.assertEqual([], check(self.results(), self.required, self.conditional))

    def test_failed_matrix_leg_mutation_is_red(self) -> None:
        errors = check(
            self.results(("emit-roots", "failure")), self.required, self.conditional
        )
        self.assertTrue(any("emit-roots" in error and "failure" in error for error in errors))

    def test_skipped_matrix_leg_mutation_is_red(self) -> None:
        errors = check(
            self.results(("emit-roots", "skipped")), self.required, self.conditional
        )
        self.assertTrue(any("emit-roots" in error and "skipped" in error for error in errors))


if __name__ == "__main__":
    unittest.main()
