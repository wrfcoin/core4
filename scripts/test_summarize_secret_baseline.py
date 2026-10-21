import copy
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).with_name('summarize_secret_baseline.py')
spec = importlib.util.spec_from_file_location('audit', SCRIPT)
audit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audit)


def fixture():
    return {'results': {p: [dict(filename=p, type=t, line_number=1, hashed_secret='a' * 40)
                            for t in types]
                        for p, types in [('tests/mock.rs', ['Hex', 'Hex']),
                                         ('src/main.rs', ['Entropy']), ('README.md', ['Hex'])]}}


class BaselineAuditTests(unittest.TestCase):
    def test_counts_and_no_finding_material(self):
        report = audit.summarize(fixture())
        self.assertEqual(report['entries'], 4)
        self.assertEqual(report['files'], 3)
        self.assertEqual(report['by_directory'], {'(root)': 1, 'src': 1, 'tests': 2})
        self.assertEqual(report['by_type'], {'Entropy': 1, 'Hex': 3})
        self.assertEqual(sum(x['entries'] for x in report['by_directory_and_type']), 4)
        self.assertNotIn('a' * 40, json.dumps(report) + audit.markdown(report))
        self.assertNotIn('mock.rs', json.dumps(report))

    def test_bad_metadata_is_refused(self):
        for key, value in [('filename', 'other'), ('line_number', True),
                           ('hashed_secret', 'raw-value'), ('type', 'bad\nvalue')]:
            data = fixture()
            data['results']['tests/mock.rs'][0][key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                audit.summarize(data)

    def test_cli_does_not_echo_invalid_input(self):
        with tempfile.TemporaryDirectory() as temp:
            p = Path(temp) / 'baseline'
            p.write_text('SYNTHETIC_DO_NOT_ECHO_INVALID_INPUT')
            run = subprocess.run([sys.executable, str(SCRIPT), '--baseline', str(p)],
                                 capture_output=True, text=True)
            self.assertEqual(run.returncode, 2)
            self.assertNotIn('SYNTHETIC_DO_NOT_ECHO', run.stdout + run.stderr)

    def test_does_not_need_source_files(self):
        with tempfile.TemporaryDirectory() as temp:
            p = Path(temp) / 'baseline'
            p.write_text(json.dumps(fixture()))
            run = subprocess.run([sys.executable, str(SCRIPT), '--baseline', str(p), '--format', 'json'],
                                 capture_output=True, text=True)
            self.assertEqual(run.returncode, 0)
            self.assertEqual(json.loads(run.stdout)['entries'], 4)


if __name__ == '__main__':
    unittest.main()
