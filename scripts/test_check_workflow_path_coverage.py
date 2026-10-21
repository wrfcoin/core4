#!/usr/bin/env python3
"""Fail-closed tests for check_workflow_path_coverage.py (#2916).

Every case here asserts the guard produces an ERROR rather than a silent pass. A
path-coverage guard that quietly skips what it does not understand is itself a gate
covering nothing, which is the failure this whole change exists to remove — so the
skip-shaped outcomes are the ones worth pinning.
"""

from __future__ import annotations

import json
import sys
import subprocess
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import check_workflow_path_coverage as guard  # noqa: E402

FILTERED_WORKFLOW = """\
name: Example
on:
  pull_request:
    paths:
      - 'alpha/**'
jobs:
  go:
    runs-on: ubuntu-latest
    steps:
      - run: 'true'
"""


def build(tmp: Path, workflows: dict[str, str], manifest: dict | None) -> Path:
    (tmp / ".github" / "workflows").mkdir(parents=True, exist_ok=True)
    for name, body in workflows.items():
        (tmp / ".github" / "workflows" / name).write_text(body, encoding="utf-8")
    if manifest is not None:
        (tmp / "scripts").mkdir(parents=True, exist_ok=True)
        (tmp / "scripts" / "workflow_path_coverage.json").write_text(
            json.dumps(manifest), encoding="utf-8"
        )
    return tmp


class FailClosed(unittest.TestCase):
    def run_guard(self, workflows, manifest):
        with tempfile.TemporaryDirectory() as raw:
            return guard.check(build(Path(raw), workflows, manifest))

    def test_baseline_case_passes_so_the_failures_below_mean_something(self):
        problems = self.run_guard(
            {"ex.yml": FILTERED_WORKFLOW},
            {"max_unreviewed": 0, "workflows": {
                ".github/workflows/ex.yml": {"why": "x", "must_trigger": ["alpha/thing.rs"]}}},
        )
        self.assertEqual(problems, [], f"control case must pass: {problems}")

    def test_filtered_workflow_with_no_manifest_entry_is_an_error(self):
        problems = self.run_guard({"ex.yml": FILTERED_WORKFLOW}, {"max_unreviewed": 0, "workflows": {}})
        self.assertTrue(any("no scripts/workflow_path_coverage.json entry" in p for p in problems), problems)

    def test_absent_manifest_is_an_error_not_a_pass(self):
        problems = self.run_guard({"ex.yml": FILTERED_WORKFLOW}, None)
        self.assertTrue(any("not found" in p for p in problems), problems)
        self.assertTrue(any("not a pass" in p for p in problems), problems)

    def test_entry_for_a_workflow_that_does_not_exist_is_an_error(self):
        problems = self.run_guard(
            {"ex.yml": FILTERED_WORKFLOW},
            {"max_unreviewed": 0, "workflows": {
                ".github/workflows/ex.yml": {"why": "x", "must_trigger": ["alpha/thing.rs"]},
                ".github/workflows/gone.yml": {"why": "x", "must_trigger": ["alpha/x"]}}},
        )
        self.assertTrue(any("does not exist" in p for p in problems), problems)

    def test_empty_must_trigger_is_an_error_because_it_passes_unconditionally(self):
        problems = self.run_guard(
            {"ex.yml": FILTERED_WORKFLOW},
            {"max_unreviewed": 0, "workflows": {
                ".github/workflows/ex.yml": {"why": "x", "must_trigger": []}}},
        )
        self.assertTrue(any("guards nothing" in p for p in problems), problems)

    def test_unreviewed_requires_a_reason(self):
        problems = self.run_guard(
            {"ex.yml": FILTERED_WORKFLOW},
            {"max_unreviewed": 1, "workflows": {
                ".github/workflows/ex.yml": {"unreviewed": True}}},
        )
        self.assertTrue(any("without a `why`" in p for p in problems), problems)

    def test_unreviewed_ceiling_only_goes_down(self):
        problems = self.run_guard(
            {"ex.yml": FILTERED_WORKFLOW},
            {"max_unreviewed": 0, "workflows": {
                ".github/workflows/ex.yml": {"unreviewed": True, "why": "pending"}}},
        )
        self.assertTrue(any("ceiling" in p for p in problems), problems)

    def test_missing_ceiling_is_an_error(self):
        problems = self.run_guard({"ex.yml": FILTERED_WORKFLOW}, {"workflows": {}})
        self.assertTrue(any("max_unreviewed" in p for p in problems), problems)

    def test_a_parser_blind_spot_is_an_error_not_no_filter_to_check(self):
        """A `pull_request:` key the trigger parser cannot read must not read as 'nothing here'.

        This is the shape that would let the guard pass a workflow it never understood.
        """
        opaque = "name: X\n" + "on:\n  pull_request:\n" + "jobs: {}\n"
        with tempfile.TemporaryDirectory() as raw:
            root = build(Path(raw), {"ex.yml": opaque}, {"max_unreviewed": 0, "workflows": {}})
            original = guard._trigger_paths
            try:
                guard._trigger_paths = lambda text, trigger: None  # simulate a blind spot
                problems = guard.check(root)
            finally:
                guard._trigger_paths = original
        self.assertTrue(any("blind spot" in p for p in problems), problems)

    def test_matcher_self_test_has_no_failures(self):
        self.assertEqual(guard.self_test(), [])


class Semantics(unittest.TestCase):
    def test_star_does_not_cross_a_separator(self):
        self.assertTrue(guard.matches_any(["scripts/*.py"], "scripts/a.py"))
        self.assertFalse(guard.matches_any(["scripts/*.py"], "scripts/deep/a.py"))

    def test_doublestar_crosses_separators(self):
        self.assertTrue(guard.matches_any(["a/**"], "a/b/c/d.rs"))

    def test_subdirectory_glob_does_not_cover_its_parent(self):
        """#2245 blocker 2, in one assertion."""
        self.assertFalse(
            guard.matches_any(["shared-protocol/src/blockchain/**"], "shared-protocol/src/lib.rs")
        )

    def test_double_quoted_globs_are_unquoted(self):
        """#2919: the local `_unquote` shim is gone; this file now depends on
        `_trigger_paths` doing this itself, so exercise it through that
        import rather than a shim this module no longer defines."""
        double = 'pull_request:\n  paths:\n    - "fuzz/**"\n'
        single = "pull_request:\n  paths:\n    - 'fuzz/**'\n"
        bare = "pull_request:\n  paths:\n    - fuzz/**\n"
        self.assertEqual(guard._trigger_paths(double, "pull_request"), ["fuzz/**"])
        self.assertEqual(guard._trigger_paths(single, "pull_request"), ["fuzz/**"])
        self.assertEqual(guard._trigger_paths(bare, "pull_request"), ["fuzz/**"])


# A `pull_request:` trigger declaring a `paths:` key that yields no usable globs.
# These are the REAL shapes. The prior suite only mocked `_trigger_paths` to
# return None, which exercises the branch that already worked — the branch that
# did NOT work is the one where parsing succeeds and produces an empty list.
EMPTY_BLOCK = """\
name: Example
on:
  pull_request:
    paths:
jobs:
  go:
    runs-on: ubuntu-latest
    steps:
      - run: 'true'
"""

INLINE_EMPTY = """\
name: Example
on:
  pull_request:
    paths: []
jobs:
  go:
    runs-on: ubuntu-latest
    steps:
      - run: 'true'
"""

MALFORMED_LIST = """\
name: Example
on:
  pull_request:
    paths: [
jobs:
  go:
    runs-on: ubuntu-latest
    steps:
      - run: 'true'
"""

PATHS_IGNORE = """\
name: Example
on:
  pull_request:
    paths-ignore:
      - 'docs/**'
jobs:
  go:
    runs-on: ubuntu-latest
    steps:
      - run: 'true'
"""

NO_FILTER = """\
name: Example
on:
  pull_request:
  push:
    branches: [main]
jobs:
  go:
    runs-on: ubuntu-latest
    steps:
      - run: 'true'
"""


class DeclaredButEmpty(unittest.TestCase):
    """#2917 review blocker 1.

    A workflow whose `paths:` parsed to zero globs was dropped from the scan
    BEFORE either manifest assertion ran. So the positive `must_trigger` claims
    that are supposed to make the negative `must_not_trigger` claims admissible
    were never evaluated either: both directions vanished together and the
    checker reported success. The polarity argument was sound; the wiring did not
    implement it.
    """

    def run_guard(self, workflows, manifest):
        with tempfile.TemporaryDirectory() as raw:
            return guard.check(build(Path(raw), workflows, manifest))

    def empty_manifest(self):
        return {"max_unreviewed": 0, "workflows": {}}

    def test_real_empty_paths_block_is_a_defect(self):
        problems = self.run_guard({"ex.yml": EMPTY_BLOCK}, self.empty_manifest())
        self.assertTrue(any("ZERO usable globs" in p for p in problems), problems)

    def test_inline_empty_list_is_a_defect(self):
        problems = self.run_guard({"ex.yml": INLINE_EMPTY}, self.empty_manifest())
        self.assertTrue(any("ZERO usable globs" in p for p in problems), problems)

    def test_malformed_list_is_a_defect(self):
        problems = self.run_guard({"ex.yml": MALFORMED_LIST}, self.empty_manifest())
        self.assertTrue(any("ZERO usable globs" in p for p in problems), problems)

    def test_paths_ignore_is_not_silently_treated_as_unfiltered(self):
        """It IS filtered. An unmodelled filter must fail, not be exempted."""
        problems = self.run_guard({"ex.yml": PATHS_IGNORE}, self.empty_manifest())
        self.assertTrue(any("paths-ignore" in p for p in problems), problems)

    def test_no_filter_at_all_is_legitimate_and_needs_no_entry(self):
        """The distinction that matters: declaring nothing is fine, declaring
        something that yields nothing is not. The repo's always-on gate
        workflows deliberately carry no filter."""
        problems = self.run_guard({"ex.yml": NO_FILTER}, self.empty_manifest())
        self.assertEqual(problems, [], f"an unfiltered workflow must not be flagged: {problems}")

    def test_declared_filter_keys_distinguishes_the_three_states(self):
        self.assertEqual(guard.declared_filter_keys(EMPTY_BLOCK, "pull_request"), {"paths"})
        self.assertEqual(guard.declared_filter_keys(INLINE_EMPTY, "pull_request"), {"paths"})
        self.assertEqual(guard.declared_filter_keys(PATHS_IGNORE, "pull_request"), {"paths-ignore"})
        self.assertEqual(guard.declared_filter_keys(NO_FILTER, "pull_request"), set())
        self.assertIsNone(guard.declared_filter_keys("name: x\non:\n  push:\n", "pull_request"))

    def test_push_declaring_paths_is_not_read_as_pull_request_declaring_it(self):
        text = ("name: x\non:\n  pull_request:\n  push:\n    paths:\n      - 'a/**'\n"
                "jobs:\n  go:\n    runs-on: ubuntu-latest\n    steps:\n      - run: 'true'\n")
        self.assertEqual(guard.declared_filter_keys(text, "pull_request"), set())


class CeilingRatchet(unittest.TestCase):
    """#2917 review blocker 2: a ceiling a change can raise is not a ceiling."""

    def make_repo(self, ceiling: int):
        tmp = Path(tempfile.mkdtemp())
        (tmp / "scripts").mkdir(parents=True)
        (tmp / ".github" / "workflows").mkdir(parents=True)
        (tmp / "scripts" / "workflow_path_coverage.json").write_text(
            json.dumps({"max_unreviewed": ceiling, "workflows": {}}), encoding="utf-8"
        )
        for cmd in (["init", "-q"], ["config", "user.email", "t@e"], ["config", "user.name", "t"]):
            subprocess.run(["git", "-C", str(tmp)] + cmd, check=True)
        subprocess.run(["git", "-C", str(tmp), "add", "-A"], check=True)
        subprocess.run(["git", "-C", str(tmp), "commit", "-qm", "base"], check=True)
        return tmp

    def set_ceiling(self, repo: Path, ceiling: int):
        path = repo / "scripts" / "workflow_path_coverage.json"
        data = json.loads(path.read_text())
        data["max_unreviewed"] = ceiling
        path.write_text(json.dumps(data), encoding="utf-8")

    def test_raising_the_ceiling_against_the_anchor_is_red(self):
        repo = self.make_repo(2)
        self.set_ceiling(repo, 3)
        problems = guard.check(repo, anchor_ref="HEAD")
        self.assertTrue(any("raised from 2 to 3" in p for p in problems), problems)

    def test_holding_or_lowering_the_ceiling_is_green(self):
        repo = self.make_repo(2)
        self.assertEqual(guard.check(repo, anchor_ref="HEAD"), [])
        self.set_ceiling(repo, 1)
        self.assertEqual(guard.check(repo, anchor_ref="HEAD"), [])

    def test_unanchored_does_not_silently_look_anchored(self):
        """No anchor is a weaker check, so it must not be indistinguishable from
        a passing anchored one. The run prints UNANCHORED; here we only pin that
        it does not fail, so the push-to-main path stays usable."""
        repo = self.make_repo(2)
        self.set_ceiling(repo, 3)
        self.assertEqual(guard.check(repo, anchor_ref=None), [])


if __name__ == "__main__":
    unittest.main(verbosity=2)