#!/usr/bin/env python3
"""Execute admission outcome boundaries without building Rust."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

import yaml

ROOT = Path(__file__).resolve().parents[1]


def workflow():
    return yaml.safe_load((ROOT / '.github/workflows/public-pr-admission.yml').read_text())


class PublicAdmissionOutcomes(unittest.TestCase):
    def test_only_completed_clippy_is_advisory_unit_tests_stay_blocking(self):
        job = workflow()['jobs']['rust']
        steps = job['steps']
        clippy = next(s for s in steps if s.get('id') == 'clippy')
        report = next(s for s in steps if s.get('name') == 'Report Clippy outcome')
        units = next(s for s in steps if s.get('name') == 'Workspace library unit tests')
        self.assertIs(clippy.get('continue-on-error'), True)
        self.assertEqual(clippy['run'], 'cargo clippy --locked --workspace --lib --all-features -- -D warnings')
        self.assertEqual(report['if'], 'always()')
        self.assertEqual(report['env']['CLIPPY_OUTCOME'], '${{ steps.clippy.outcome }}')
        self.assertFalse(report.get('continue-on-error', False))
        self.assertFalse(units.get('continue-on-error', False))
        self.assertFalse(job.get('continue-on-error', False))
        self.assertNotIn('if', units)
        self.assertEqual(units['run'], 'cargo test --locked --workspace --lib --all-features')
        self.assertLess(steps.index(clippy), steps.index(report))
        self.assertLess(steps.index(report), steps.index(units))

    def test_actual_report_accepts_completed_results_and_rejects_incomplete(self):
        report = next(s for s in workflow()['jobs']['rust']['steps'] if s.get('name') == 'Report Clippy outcome')
        for outcome in ['success', 'failure', 'skipped', 'cancelled', 'timed_out', '', 'unknown']:
            with self.subTest(outcome=outcome), tempfile.TemporaryDirectory() as directory:
                summary = Path(directory) / 'summary'
                env = dict(os.environ, CLIPPY_OUTCOME=outcome, GITHUB_STEP_SUMMARY=str(summary))
                result = subprocess.run(['bash', '-c', report['run']], env=env, text=True, capture_output=True)
                self.assertEqual(result.returncode == 0, outcome in ['success', 'failure'])
                if outcome == 'success':
                    self.assertIn('Workspace Clippy: PASS', summary.read_text())
                elif outcome == 'failure':
                    self.assertIn('::warning::', result.stdout)
                    self.assertIn('FAIL (advisory; #3618)', summary.read_text())
                else:
                    self.assertIn('did not execute to completion', result.stdout)

    def test_actual_terminal_rejects_non_success_unit_or_policy_workers(self):
        gate = workflow()['jobs']['admission']
        self.assertEqual(gate['if'], 'always()')
        self.assertEqual(set(gate['needs']), {'rust', 'policy'})
        step = next(s for s in gate['steps'] if s.get('name') == 'Require executed successful workers')
        self.assertEqual(step['env']['GATE_REQUIRED'], 'rust policy')
        self.assertEqual(step['env']['GATE_CONDITIONAL'], '')
        for rust, policy in [('success', 'success'), ('failure', 'success'), ('cancelled', 'success'),
                             ('skipped', 'success'), ('success', 'failure'), ('success', 'skipped')]:
            with self.subTest(rust=rust, policy=policy):
                needs = {'rust': {'result': rust}, 'policy': {'result': policy}}
                env = dict(os.environ, GATE_NEEDS=json.dumps(needs), GATE_REQUIRED='rust policy', GATE_CONDITIONAL='')
                result = subprocess.run(['bash', '-c', step['run']], cwd=ROOT, env=env, text=True, capture_output=True)
                self.assertEqual(result.returncode == 0, rust == policy == 'success', result.stdout + result.stderr)


if __name__ == '__main__':
    unittest.main()
