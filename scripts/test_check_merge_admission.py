#!/usr/bin/env python3
"""Tests for check_merge_admission.py — the comparator must reach both answers, and VOID."""
from __future__ import annotations

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import check_merge_admission as cma  # noqa: E402

SCRIPT = HERE / "check_merge_admission.py"


def run(*args: str) -> subprocess.CompletedProcess:
    return subprocess.run([sys.executable, str(SCRIPT), *args], capture_output=True, text=True)


class SelfTest(unittest.TestCase):
    def test_self_test_passes(self):
        proc = run("--self-test")
        self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
        self.assertIn("All three answers reachable", proc.stdout)
        self.assertIn("blind paths => VOID", proc.stdout)


class LiveShapedFixtures(unittest.TestCase):
    """Drive the CLI with a manifest, a document and a fixture shaped like the real API."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = Path(self.tmp.name)
        self.expected = {
            "ruleset_id": 19875840, "ruleset_name": "main-protection",
            "rules": ["deletion", "non_fast_forward", "required_linear_history", "pull_request",
                      "required_status_checks"],
            "required_status_checks": ["schedule-lint", "detect-secrets baseline audit (merge tree)",
                                       "verify-terminal-skip-guards", "repository-input integration admission"],
            "strict_required_status_checks_policy": True,
            "pull_request": {"required_approving_review_count": 0, "allowed_merge_methods": ["squash", "rebase"]},
            "bypass_actors": [{"actor_type": "OrganizationAdmin", "bypass_mode": "always"}],
        }
        self.infra = {
            "ruleset_id": 19875881, "ruleset_name": "main-protection",
            "rules": ["deletion", "non_fast_forward", "required_linear_history", "pull_request", "required_status_checks"],
            "required_status_checks": ["scripts/tests suites", "dependabot-coverage"], "strict_required_status_checks_policy": False,
            "pull_request": {"required_approving_review_count": 0, "allowed_merge_methods": ["squash", "rebase"]},
            "bypass_actors": [{"actor_type": "OrganizationAdmin", "bypass_mode": "always"}],
        }
        self.manifest = self.dir / "m.json"
        self.manifest.write_text(json.dumps({"repos": {"wrfcoin/core4": self.expected, "wrfcoin/infra": self.infra}}))
        self.doc = self.dir / "d.md"
        self.doc.write_text("ruleset 19875840 requires `schedule-lint`, `detect-secrets baseline audit (merge tree)` "
                            "and `verify-terminal-skip-guards`, `repository-input integration admission`. "
                            "infra ruleset 19875881 requires `scripts/tests suites` and `dependabot-coverage`.")
        pr = {"type": "pull_request", "ruleset_id": 19875840,
              "parameters": {"allowed_merge_methods": ["squash", "rebase"], "required_approving_review_count": 0,
                             "require_last_push_approval": False}}
        self.live = {
            "wrfcoin/core4": {
                "rules": [
                    {"type": "deletion", "ruleset_id": 19875840},
                    {"type": "non_fast_forward", "ruleset_id": 19875840},
                    {"type": "required_linear_history", "ruleset_id": 19875840},
                    pr,
                    {"type": "required_status_checks", "ruleset_id": 19875840,
                     "parameters": {"do_not_enforce_on_create": False, "strict_required_status_checks_policy": True,
                                    "required_status_checks": [{"context": "schedule-lint"},
                                                               {"context": "detect-secrets baseline audit (merge tree)"},
                                                               {"context": "verify-terminal-skip-guards"},
                                                               {"context": "repository-input integration admission"}]}},
                ],
                "ruleset": {"name": "main-protection", "enforcement": "active",
                            "bypass_actors": [{"actor_type": "OrganizationAdmin", "bypass_mode": "always", "actor_id": None}]},
                "ruleset_http": 200,
            },
            "wrfcoin/infra": {
                "rules": [
                    {"type": "deletion", "ruleset_id": 19875881},
                    {"type": "non_fast_forward", "ruleset_id": 19875881},
                    {"type": "required_linear_history", "ruleset_id": 19875881},
                    dict(pr, ruleset_id=19875881),
                    {"type": "required_status_checks", "ruleset_id": 19875881,
                     "parameters": {"strict_required_status_checks_policy": False,
                                    "required_status_checks": [{"context": "scripts/tests suites"},
                                                               {"context": "dependabot-coverage"}]}},
                ],
                "ruleset": {"name": "main-protection", "enforcement": "active",
                            "bypass_actors": [{"actor_type": "OrganizationAdmin", "bypass_mode": "always"}]},
                "ruleset_http": 200,
            },
        }

    def tearDown(self):
        self.tmp.cleanup()

    def _run(self, live=None, doc=None, *extra):
        fx = self.dir / "fx.json"
        fx.write_text(json.dumps(live if live is not None else self.live))
        if doc is not None:
            self.doc.write_text(doc)
        return run("--manifest", str(self.manifest), "--doc", str(self.doc), "--fixture", str(fx), *extra)

    def test_match_is_zero_for_both_repos(self):
        proc = self._run(None, None, "--all")
        self.assertEqual(proc.returncode, 0, proc.stdout)
        self.assertEqual(proc.stdout.count("OK: "), 2)

    def test_owner_adds_a_context_without_the_document(self):
        live = json.loads(json.dumps(self.live))
        live["wrfcoin/core4"]["rules"][4]["parameters"]["required_status_checks"].append({"context": "input-panic-lint"})
        proc = self._run(live)
        self.assertEqual(proc.returncode, 1, proc.stdout)
        self.assertIn("live adds ['input-panic-lint']", proc.stdout)

    def test_document_ahead_of_ruleset(self):
        """The manifest+doc name a context the owner has not enabled yet (MERGE_ADMISSION.md §5 step 1)."""
        self.expected["required_status_checks"].append("input-panic-lint")
        self.manifest.write_text(json.dumps({"repos": {"wrfcoin/core4": self.expected, "wrfcoin/infra": self.infra}}))
        proc = self._run(None, self.doc.read_text() + " and `input-panic-lint`.")
        self.assertEqual(proc.returncode, 1, proc.stdout)
        self.assertIn("live lacks ['input-panic-lint']", proc.stdout)

    def test_strict_flipped_off(self):
        live = json.loads(json.dumps(self.live))
        live["wrfcoin/core4"]["rules"][4]["parameters"]["strict_required_status_checks_policy"] = False
        proc = self._run(live)
        self.assertEqual(proc.returncode, 1)
        self.assertIn("strict_required_status_checks_policy live=False expected=True", proc.stdout)

    def test_infra_loses_a_status_rule_silently(self):
        live = json.loads(json.dumps(self.live))
        live["wrfcoin/infra"]["rules"] = [r for r in live["wrfcoin/infra"]["rules"]
                                           if r["type"] != "required_status_checks"]
        proc = self._run(live, None, "--repo", "wrfcoin/infra")
        self.assertEqual(proc.returncode, 1, proc.stdout)
        self.assertIn("required_status_checks rule present=False expected=True", proc.stdout)

    def test_repository_input_context_cannot_disappear(self):
        live = json.loads(json.dumps(self.live))
        checks = live["wrfcoin/core4"]["rules"][4]["parameters"]["required_status_checks"]
        checks[:] = [c for c in checks if c["context"] != "repository-input integration admission"]
        proc = self._run(live)
        self.assertEqual(proc.returncode, 1, proc.stdout)
        self.assertIn("live lacks ['repository-input integration admission']", proc.stdout)

    def test_infra_dependabot_context_cannot_disappear(self):
        live = json.loads(json.dumps(self.live))
        checks = live["wrfcoin/infra"]["rules"][4]["parameters"]["required_status_checks"]
        checks[:] = [c for c in checks if c["context"] != "dependabot-coverage"]
        proc = self._run(live, None, "--repo", "wrfcoin/infra")
        self.assertEqual(proc.returncode, 1, proc.stdout)
        self.assertIn("live lacks ['dependabot-coverage']", proc.stdout)

    def test_document_drops_a_context_name(self):
        proc = self._run(None, "ruleset 19875840 requires `schedule-lint` and `verify-terminal-skip-guards`.")
        self.assertEqual(proc.returncode, 1)
        self.assertIn("does not name required context `detect-secrets baseline audit (merge tree)`", proc.stdout)

    def test_ruleset_metadata_forbidden_is_void_not_ok(self):
        """#3018 review (Alice): a 403 on rulesets/<id> used to print UNVERIFIED and exit 0 —
        a checker passing while blind. It must be VOID (exit 2) and must not print OK."""
        live = json.loads(json.dumps(self.live))
        live["wrfcoin/core4"]["ruleset"] = None
        live["wrfcoin/core4"]["ruleset_http"] = 403
        proc = self._run(live)
        self.assertEqual(proc.returncode, 2, proc.stdout)
        self.assertIn("VOID", proc.stdout)
        self.assertNotIn("OK:", proc.stdout)
        self.assertNotIn("UNVERIFIED", proc.stdout)

    def test_ruleset_metadata_malformed_is_void(self):
        live = json.loads(json.dumps(self.live))
        live["wrfcoin/core4"]["ruleset"] = "not an object"
        proc = self._run(live)
        self.assertEqual(proc.returncode, 2, proc.stdout)
        self.assertIn("VOID", proc.stdout)

    def test_void_on_one_repo_is_void_overall_even_if_the_other_matches(self):
        live = json.loads(json.dumps(self.live))
        live["wrfcoin/infra"]["ruleset"] = None
        live["wrfcoin/infra"]["ruleset_http"] = 403
        proc = self._run(live, None, "--all")
        self.assertEqual(proc.returncode, 2, proc.stdout)

    def test_rules_endpoint_unusable_is_void(self):
        proc = self._run({"wrfcoin/core4": {"nothing": True}})
        self.assertEqual(proc.returncode, 2, proc.stdout)
        self.assertIn("VOID", proc.stdout)

    def test_missing_document_is_void(self):
        fx = self.dir / "fx.json"; fx.write_text(json.dumps(self.live))
        proc = run("--manifest", str(self.manifest), "--doc", str(self.dir / "absent.md"), "--fixture", str(fx))
        self.assertEqual(proc.returncode, 2)

    def test_repo_absent_from_manifest_is_void(self):
        proc = self._run(None, None, "--repo", "wrfcoin/backend")
        self.assertEqual(proc.returncode, 2)


class RealManifestAndDocument(unittest.TestCase):
    """The committed manifest and document must agree with each other, offline."""

    def test_owner_2026_10_07_live_admission_policy(self):
        repos = json.loads(cma.DEFAULT_MANIFEST.read_text())["repos"]
        self.assertEqual(set(repos["wrfcoin/core4"]["required_status_checks"]), {
            "schedule-lint", "detect-secrets baseline audit (merge tree)",
            "verify-terminal-skip-guards", "repository-input integration admission",
        })
        self.assertIs(repos["wrfcoin/core4"]["strict_required_status_checks_policy"], True)
        self.assertEqual(set(repos["wrfcoin/infra"]["required_status_checks"]), {
            "scripts/tests suites", "dependabot-coverage",
        })
        self.assertIs(repos["wrfcoin/infra"]["strict_required_status_checks_policy"], False)

    def test_live_drift_step_remains_advisory(self):
        import yaml
        from check_workflow_inventory import WorkflowLoader
        path = HERE.parent / '.github/workflows/merge-admission-contract.yml'
        workflow = yaml.load(path.read_text(), Loader=WorkflowLoader)
        steps = workflow['jobs']['contract']['steps']
        advisory = [step for step in steps if step.get('name', '').startswith('ADVISORY - Live')]
        self.assertEqual(len(advisory), 1)
        self.assertEqual(str(advisory[0]['continue-on-error']).lower(), 'true')
        self.assertIn('#3198', path.read_text())
        for step in steps:
            if '--self-test' in step.get('run', '') or 'test_check_merge_admission.py' in step.get('run', ''):
                self.assertNotIn('continue-on-error', step)

    def test_document_names_every_manifest_context(self):
        manifest = json.loads(cma.DEFAULT_MANIFEST.read_text())
        doc = cma.DEFAULT_DOC.read_text()
        for repo, expected in manifest["repos"].items():
            self.assertEqual(cma.check_document(doc, repo, expected), [])


if __name__ == "__main__":
    unittest.main()
