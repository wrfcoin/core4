"""Owner 2026-10-07 relevance/nightly policy; schedule policy Refs #2498."""
import unittest
import tomllib
from check_workflow_path_coverage import glob_to_regex
from pathlib import Path
import yaml
from check_workflow_inventory import WorkflowLoader

ROOT = Path(__file__).resolve().parents[1]
CANDIDATES = 'tdd-phase3c build-provenance-gate ci-deploy-features unsafe-policy native-chain-package-gate determinism-lint bincode-boundary-guard cli-test-gate wrfcoin-cli-integration launch-api-bin-tests crate-execution-gates named-test-targets input-panic-lint workflow-path-coverage retired-component-sdks adversarial-tests'.split()

PACKAGE_ROOTS = {'tdd-phase3c': ['shared-protocol', 'native-chain'], 'build-provenance-gate': ['build-provenance'], 'ci-deploy-features': ['*'], 'determinism-lint': ['*'], 'native-chain-package-gate': ['native-chain'], 'cli-test-gate': ['tools/cli/WrfcoinCLI'], 'wrfcoin-cli-integration': ['tools/cli/WrfcoinCLI', 'native-chain'], 'launch-api-bin-tests': ['native-chain'], 'crate-execution-gates': ['consensus-engines', 'shared-protocol', 'privacy', 'zk-circuits', 'execution-engine', 'evm', 'meteo-types', 'geo-grid', 'forecast-score', 'dataset-manifest', 'keeper-proof', 'da', 'oracle-packet', 'sensor-attestation', 'reputation-engine', 'primitives'], 'named-test-targets': ['native-chain', 'shared-protocol'], 'adversarial-tests': ['native-chain', 'consensus-engines', 'tests/integration']}

class RelevanceNightly(unittest.TestCase):
    def test_daily_staggered_schedules_and_dispatch(self):
        crons = []
        for name in CANDIDATES:
            with self.subTest(workflow=name):
                doc = yaml.load((ROOT / '.github/workflows' / (name + '.yml')).read_text(), Loader=WorkflowLoader)
                events = doc['on']
                self.assertIn('workflow_dispatch', events)
                cron = events['schedule'][0]['cron']
                minute, hour, day, month, weekday = cron.split()
                self.assertTrue(2 <= int(hour) < 5)
                self.assertTrue(0 <= int(minute) < 60)
                self.assertEqual((day, month, weekday), ('*', '*', '*'))
                crons.append(cron)
        self.assertEqual(len(crons), len(CANDIDATES))
        self.assertEqual(len(set(crons)), len(crons))

    def test_local_dependency_inputs_are_covered(self):
        workspace = tomllib.loads((ROOT / 'Cargo.toml').read_text())['workspace']
        for workflow, roots in PACKAGE_ROOTS.items():
            doc = yaml.load((ROOT / '.github/workflows' / (workflow + '.yml')).read_text(), Loader=WorkflowLoader)
            patterns = [glob_to_regex(p) for p in doc['on']['pull_request']['paths']]
            pending = [ROOT / p for p in (workspace['members'] if roots == ['*'] else roots)]
            seen = set()
            while pending:
                package = pending.pop().resolve()
                if package in seen:
                    continue
                seen.add(package)
                relative = package.relative_to(ROOT).as_posix()
                # Also cover non-Rust fixtures/build inputs, not only .rs changes.
                for suffix in ['src/lib.rs', 'tests/fixtures/input.bin', 'build.rs', 'Cargo.toml']:
                    path = relative + '/' + suffix
                    with self.subTest(workflow=workflow, input=path):
                        self.assertTrue(any(p.fullmatch(path) for p in patterns), path)
                manifest = tomllib.loads((package / 'Cargo.toml').read_text())
                for table in [manifest, *manifest.get('target', {}).values()]:
                    for kind in ['dependencies', 'dev-dependencies', 'build-dependencies']:
                        for name, spec in table.get(kind, {}).items():
                            if not isinstance(spec, dict):
                                continue
                            base = package
                            if spec.get('workspace'):
                                base = ROOT
                                spec = workspace['dependencies'].get(name, {})
                            if isinstance(spec, dict) and 'path' in spec:
                                pending.append(base / spec['path'])

    def test_workflow_keys_and_retained_nightly_runs(self):
        allowed = {'name', 'on', 'run-name', 'permissions', 'env', 'defaults', 'concurrency', 'jobs'}
        for name in CANDIDATES:
            doc = yaml.load((ROOT / '.github/workflows' / (name + '.yml')).read_text(), Loader=WorkflowLoader)
            with self.subTest(workflow=name):
                self.assertFalse(set(doc) - allowed)
                self.assertIn("github.run_id", doc['concurrency']['group'])
                self.assertEqual(doc['concurrency']['cancel-in-progress'], "${{ github.event_name == 'pull_request' }}")

    def test_required_workflows_remain_unfiltered(self):
        # Public launch replaces pre-merge integration admission with hosted
        # lint/unit admission. The legacy verifier remains post-merge only.
        for name in ['schedule-lint', 'secret-scanning', 'terminal-skip-guard-integrity', 'public-pr-admission']:
            with self.subTest(workflow=name):
                doc = yaml.load((ROOT / '.github/workflows' / (name + '.yml')).read_text(), Loader=WorkflowLoader)
                self.assertIn('pull_request', doc['on'])
                self.assertFalse((doc['on']['pull_request'] or {}).get('paths'))
                self.assertFalse((doc['on']['pull_request'] or {}).get('paths-ignore'))

    def test_legacy_integration_admission_is_main_push_only(self):
        doc = yaml.load((ROOT / '.github/workflows/repository-input-admission.yml').read_text(), Loader=WorkflowLoader)
        self.assertEqual(set(doc['on']), {'push'})
        self.assertEqual(doc['on']['push'], {'branches': ['main']})

if __name__ == '__main__':
    unittest.main()
