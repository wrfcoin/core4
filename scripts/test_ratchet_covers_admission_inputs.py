#!/usr/bin/env python3
"""Drift guard: the integration ratchet's path filters must select every admission input (Refs #3446).

`.github/repository-input-admission.json` lists the repository inputs an integration assertion reads
(`relevant_globs`). If the ratchet's own `pull_request`/`push` path filters do not select one of them, a change
to it never starts the ratchet, and the admission check can only refuse it for lack of a run (or, before the
admission context, nothing ran at all: #3436/#3446). This test fails when the two lists drift.

Run: python3 scripts/test_ratchet_covers_admission_inputs.py
"""

from __future__ import annotations

import json
import sys
import unittest
from pathlib import Path

import yaml

sys.path.insert(0, str(Path(__file__).resolve().parent))
from check_workflow_path_coverage import matches_any  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
MANIFEST = json.loads((ROOT / ".github/repository-input-admission.json").read_text())
WF = yaml.safe_load((ROOT / ".github/workflows" / MANIFEST["evidence_workflow"]).read_text())
TRIGGERS = WF.get("on") or WF.get(True)  # PyYAML parses a bare `on:` key as boolean True


class RatchetCoversAdmissionInputs(unittest.TestCase):
    def test_public_launch_event_boundary(self):
        self.assertNotIn("pull_request", TRIGGERS)
        self.assertNotIn("pull_request_target", TRIGGERS)
        self.assertEqual(TRIGGERS["push"]["branches"], ["main"])
        self.assertIn("workflow_dispatch", TRIGGERS)

    def filters(self, event):
        return TRIGGERS[event]["paths"]

    def test_every_manifest_glob_is_a_post_merge_ratchet_filter(self):
        for event in ("push",):
            missing = [g for g in MANIFEST["relevant_globs"] if g not in self.filters(event)]
            self.assertEqual(missing, [], f"{event} paths filter lacks admission globs {missing}")

    def test_every_relevant_corpus_path_selects_the_ratchet(self):
        for event in ("push",):
            for p in MANIFEST["corpus"]["must_be_relevant"]:
                self.assertTrue(matches_any(self.filters(event), p), f"{event}: {p} would not start the ratchet")

    def test_the_motivating_input_selects_the_ratchet(self):
        for event in ("push",):
            self.assertTrue(matches_any(self.filters(event), "docs/deployment/live-feature-schedule/777-testnet.json"))

    def test_the_check_can_say_no(self):
        # Control: an unrelated path must not select the ratchet, so the assertions above are not vacuous.
        for p in MANIFEST["corpus"]["must_not_be_relevant"]:
            self.assertFalse(matches_any(self.filters("push"), p), p)


if __name__ == "__main__":
    unittest.main(verbosity=1)
