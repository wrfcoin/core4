#!/usr/bin/env python3
"""Disk preparation and refusal controls; never cleans disk or runs Cargo locally."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

import yaml

ROOT = Path(__file__).resolve().parents[1]
CLEANUP = 'Free unused preinstalled toolchains (GitHub-hosted only)'
# Jobs in the public-runner conversion that compile Rust, including wrapper calls.
JOBS = {
    'adversarial-tests': ['adversarial-suite'],
    'build-provenance-gate': ['provenance-tests'],
    'ci-deploy-features': ['deploy-feature-gate', 'oracle-attester-custody-inventory', 'rich-ambient-reward-activation'],
    'ci-readiness': ['preflight-rust', 'rust-lint'],
    'cli-test-gate': ['cli-tests'],
    'crate-execution-gates': ['execute'],
    'declared-ceiling-enforcement': ['ceiling-enforcement-advisory'],
    'deploy-gate': ['release-endpoints'],
    'hsm-signer-ci': ['hsm-signer'],
    'launch-api-bin-tests': ['launch-api-bin-tests'],
    'named-test-targets': ['named-test-targets'],
    'native-chain-package-gate': ['native-chain-baseline'],
    'public-pr-admission': ['rust'],
    'sbom-generate': ['generate-sbom'],
    'secret-scanning': ['secret-leak-scan'],
    'supply-chain-security': ['cargo-audit', 'cargo-vet'],
    'tdd-phase3c': ['tdd-phase3c-rust', 'tdd-phase3c-api'],
    'wrfcoin-cli-integration': ['mocked-integration', 'live-launch-api-contract'],
}


def jobs(workflow):
    return yaml.safe_load((ROOT / '.github/workflows' / (workflow + '.yml')).read_text())['jobs']


class HostedHeavyBuildDisk(unittest.TestCase):
    def test_converted_builds_prepare_disk_before_cache_toolchain_or_guard(self):
        matrix_steps = jobs('determinism-matrix')['emit-roots']['steps']
        template = next(s for s in matrix_steps if s.get('name') == CLEANUP)
        self.assertEqual(template['if'], "runner.environment == 'github-hosted'")
        for workflow, names in JOBS.items():
            for name in names:
                with self.subTest(workflow=workflow, job=name):
                    job = jobs(workflow)[name]
                    self.assertEqual(job['runs-on'], 'ubuntu-latest')
                    steps = job['steps']
                    at = next(i for i, s in enumerate(steps) if s.get('name') == CLEANUP)
                    self.assertEqual(steps[at], template)
                    self.assertTrue(any(s.get('uses', '').startswith('actions/checkout@') for s in steps[:at]))
                    # Every executable build/preflight step and cache/toolchain action is later.
                    self.assertTrue(all('run' not in s and s.get('uses', '').startswith('actions/checkout@') for s in steps[:at]))

    def test_cleanup_command_is_the_approved_allowlist_and_exports_hosted_floor(self):
        step = next(s for s in jobs('named-test-targets')['named-test-targets']['steps'] if s.get('name') == CLEANUP)
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)
            sudo = path / 'sudo'
            sudo.write_text('#!/bin/bash\nprintf "%s\\n" "$*" >> "$DISK_TEST_CALLS"\n')
            sudo.chmod(0o755)
            calls = path / 'calls'
            envfile = path / 'github-env'
            env = dict(os.environ, PATH=str(path) + os.pathsep + os.environ['PATH'],
                       GITHUB_ENV=str(envfile), DISK_TEST_CALLS=str(calls))
            # All sudo operations are captured by the stub; no cleanup executes.
            result = subprocess.run(['bash', '-c', step['run']], env=env, text=True, capture_output=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(envfile.read_text(), 'WRFCOIN_HEAVY_BUILD_MIN_FREE_GIB=10\n')
            self.assertEqual(calls.read_text().splitlines(), [
                'rm -rf /usr/share/dotnet /usr/local/lib/android /opt/ghc /usr/local/.ghcup '
                '/opt/hostedtoolcache/CodeQL /usr/share/swift /usr/local/share/boost '
                '/usr/local/share/powershell /usr/lib/jvm',
                'docker image prune --all --force',
            ])

    def preflight(self, workflow, minimum, available, target=None):
        job = next(iter(jobs(workflow).values()))
        step = next(s for s in job['steps'] if s.get('name') == 'Preflight — enough disk for the build')
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)
            df = path / 'df'
            df.write_text('#!/bin/bash\nprintf "Avail\\n%sG\\n" "$DISK_TEST_AVAILABLE"\nprintf "%s\\n" "$*" > "$DISK_TEST_PROBE"\n')
            df.chmod(0o755)
            env = dict(os.environ, PATH=str(path) + os.pathsep + os.environ['PATH'],
                       DISK_TEST_AVAILABLE=str(available), DISK_TEST_PROBE=str(path / 'probe'))
            env.pop('WRFCOIN_HEAVY_BUILD_MIN_FREE_GIB', None)
            env.pop('CARGO_TARGET_DIR', None)
            if minimum is not None:
                env['WRFCOIN_HEAVY_BUILD_MIN_FREE_GIB'] = minimum
            if target is not None:
                env['CARGO_TARGET_DIR'] = str(path / 'not-created' / 'target')
            result = subprocess.run(['bash', '-c', step['run']], env=env, cwd=path, text=True, capture_output=True)
            probe = (path / 'probe').read_text() if (path / 'probe').exists() else ''
            return result, probe, str(path)

    def test_hosted_and_self_hosted_floors_and_explicit_override(self):
        for workflow in ['named-test-targets', 'native-chain-package-gate']:
            for minimum, available, code, floor in [(None, 13, 2, 40), ('', 13, 2, 40),
                    (None, 41, 0, 40), ('10', 13, 0, 10), ('10', 9, 2, 10),
                    ('12', 13, 0, 12), ('12', 11, 2, 12)]:
                with self.subTest(workflow=workflow, minimum=minimum, available=available):
                    result, _, _ = self.preflight(workflow, minimum, available)
                    self.assertEqual(result.returncode, code, result.stderr)
                    self.assertIn(f'floor={floor}G', result.stdout)
                    if code:
                        self.assertIn('GATE VOID', result.stdout)

    def test_invalid_floors_refuse_before_probing_or_building(self):
        for workflow in ['named-test-targets', 'native-chain-package-gate']:
            for minimum in ['0', '-1', '1.5', ' 12', '12GiB', '99999', '10\n40']:
                with self.subTest(workflow=workflow, minimum=minimum):
                    result, probe, _ = self.preflight(workflow, minimum, 100)
                    self.assertEqual(result.returncode, 2)
                    self.assertIn('whole number of GiB', result.stdout)
                    self.assertEqual(probe, '')

    def test_probe_uses_existing_ancestor_of_build_target(self):
        for workflow in ['named-test-targets', 'native-chain-package-gate']:
            result, probe, parent = self.preflight(workflow, '10', 13, target=True)
            self.assertEqual(result.returncode, 0)
            self.assertEqual(probe.strip(), '-BG --output=avail ' + parent)


if __name__ == '__main__':
    unittest.main()
