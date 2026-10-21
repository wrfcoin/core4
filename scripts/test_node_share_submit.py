import hashlib
import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch
from types import SimpleNamespace
import node_share_submit as submit


class SubmissionControls(unittest.TestCase):
    def test_owner_payout_alias_refused_before_subprocess(self):
        self.exercise(alias=True)

    def test_once_then_readback_without_resigning(self):
        self.exercise(alias=False)

    def test_uncertain_packet_blocks_resigning(self):
        self.exercise(alias=False, uncertain=True)

    def test_prior_epoch_uncertain_packet_blocks_new_nonce(self):
        self.exercise(alias=False, uncertain=True, prior_epoch=True)

    def test_888_submission_uses_explicit_testnet_domain(self):
        self.exercise(alias=False, chain_id='888')

    def exercise(self, alias, uncertain=False, prior_epoch=False, chain_id='777'):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            deployment = '01' * 32
            receipt = '02' * 32
            cli = root / 'wrfcoin'
            cli.write_text('synthetic pinned CLI')
            key = root / 'owner-key'
            token = root / 'operator-token'
            for path in [key, token]:
                path.write_text('synthetic-test-only')
                path.chmod(0o600)
            for name in ['archives', 'packets', 'home']:
                (root / name).mkdir()
            (root / 'archives' / deployment / '1').mkdir(parents=True)
            plan = {'chain_id': chain_id, 'network_id': 'testnet', 'deployment_id': deployment, 'genesis_hash': '07' * 32, 'api_url': 'http://127.0.0.1:8556', 'sender': '0x' + '10' * 20, 'cli_binary': str(cli), 'cli_binary_sha256': hashlib.sha256(cli.read_bytes()).hexdigest(), 'owner_key_file': str(key), 'operator_token_file': str(token), 'archive_root': str(root / 'archives'), 'packet_root': str(root / 'packets'), 'isolated_home': str(root / 'home'), 'fee_base_units': 1000000, 'timeout_seconds': 30}
            view = {'status': 'REGISTERED', 'jailed': False, 'principal_base_units': '25000000000', 'epoch': '1', 'registration': {'binding': {'owner_account': [16] * 20, 'payout_account': [16] * 20 if alias else [48] * 20}}}
            packet = root / 'packets' / (deployment + '-1.json')
            if uncertain:
                (root / 'packets' / (deployment + '-0.json') if prior_epoch else packet).write_text('{}')
            def read(url, credential):
                return view if '/node/bonds/' in url else {'receipt_id': receipt, 'included': True, 'status': 'INCLUDED'}
            def run(command, **kwargs):
                self.assertNotIn('synthetic-test-only', command)
                self.assertEqual(set(kwargs['env']), {'PATH', 'HOME', 'RUST_LOG'})
                self.assertIn('node', command)
                packet.write_text('{}')
                return SimpleNamespace(returncode=0, stdout=json.dumps({'ok': True, 'data': {'admitted': True, 'included': False, 'receipt_ids': [receipt]}}))
            with patch.object(submit, 'read_json', read), patch.object(submit.subprocess, 'run', side_effect=run) as child:
                if alias:
                    with self.assertRaisesRegex(AssertionError, 'owner/payout alias'):
                        submit.run_once(plan)
                    child.assert_not_called()
                elif uncertain:
                    self.assertEqual(submit.run_once(plan)['status'], 'UNCERTAIN_PACKET_REQUIRES_INSPECTION')
                    child.assert_not_called()
                else:
                    self.assertFalse(submit.run_once(plan)['included'])
                    self.assertTrue(submit.run_once(plan)['included'])
                    self.assertEqual(child.call_count, 1)


if __name__ == '__main__':
    unittest.main()
