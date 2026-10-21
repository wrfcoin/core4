"""Public fixture comparison must not require secrets on a fork."""
from pathlib import Path
import copy
import unittest
import yaml

WORKFLOW = Path(__file__).resolve().parents[1] / '.github/workflows/infra-signing-fixture-freshness.yml'

def validate(document):
    steps = document['jobs']['assert-infra-vendored-fixture-is-current']['steps']
    checkout = next(s for s in steps if s.get('with', {}).get('repository') == 'wrfcoin/infra')
    assert checkout['with']['token'] == '${{ secrets.WRFCOIN_INFRA_READ_TOKEN || github.token }}'
    assert checkout['with']['persist-credentials'] is False
    assert not any('INFRA_READ_TOKEN' in s.get('run', '') for s in steps)
    compare = next(s['run'] for s in steps if s.get('name') == 'Reject stale infra fixture or provenance')
    assert 'cmp -- "$source" "$vendored"' in compare
    assert 'jq -er' in compare

class PublicFixtureCheckout(unittest.TestCase):
    def setUp(self):
        self.document = yaml.safe_load(WORKFLOW.read_text())
    def test_public_checkout(self):
        validate(self.document)
    def test_fork_summary_avoids_comment_write(self):
        path = WORKFLOW.with_name('tdd-phase3c.yml')
        document = yaml.safe_load(path.read_text())
        steps = document['jobs']['phase3c-summary']['steps']
        comment = next(s for s in steps if 'const rust' in s.get('with', {}).get('script', ''))
        self.assertEqual(comment['if'], "github.event_name == 'pull_request' && github.event.pull_request.head.repo.full_name == github.repository")
    def test_secret_only_mutation_is_rejected(self):
        document = copy.deepcopy(self.document)
        steps = document['jobs']['assert-infra-vendored-fixture-is-current']['steps']
        next(s for s in steps if s.get('with', {}).get('repository') == 'wrfcoin/infra')['with']['token'] = '${{ secrets.WRFCOIN_INFRA_READ_TOKEN }}'
        with self.assertRaises(AssertionError):
            validate(document)
    def test_secret_preflight_mutation_is_rejected(self):
        document = copy.deepcopy(self.document)
        document['jobs']['assert-infra-vendored-fixture-is-current']['steps'].insert(1, {'run': 'test -n "$INFRA_READ_TOKEN"'})
        with self.assertRaises(AssertionError):
            validate(document)

if __name__ == '__main__':
    unittest.main()
