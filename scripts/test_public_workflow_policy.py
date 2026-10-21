#!/usr/bin/env python3
"""Negative controls for the public CI trust boundary."""
import copy
import unittest
from check_public_workflow_policy import findings, GUARD

class PublicPolicy(unittest.TestCase):
    def setUp(self):
        self.doc = {'on': {'push': {'branches': ['main']}, 'workflow_dispatch': {}},
                    'permissions': {'contents': 'read'},
                    'jobs': {'heavy': {'runs-on': ['self-hosted', 'n5pro'], 'if': GUARD}}}

    def test_main_manual_only(self):
        self.assertEqual(findings(self.doc), [])

    def test_untrusted_and_scheduled_events_rejected(self):
        for event in ['pull_request', 'pull_request_target', 'workflow_run', 'issue_comment', 'schedule']:
            d = copy.deepcopy(self.doc)
            d['on'][event] = {}
            self.assertTrue(findings(d), event)

    def test_branch_and_guard_mutations_rejected(self):
        d = copy.deepcopy(self.doc)
        d['on']['push']['branches'] = ['feature/**']
        self.assertTrue(findings(d))
        del self.doc['jobs']['heavy']['if']
        self.assertTrue(findings(self.doc))

    def test_hosted_fork_allowed(self):
        self.doc['on'] = {'pull_request': {}}
        self.doc['jobs']['heavy'] = {'runs-on': 'ubuntu-latest'}
        self.assertEqual(findings(self.doc), [])

    def test_global_write_rejected(self):
        self.doc['permissions']['contents'] = 'write'
        self.assertTrue(findings(self.doc))

    def test_matrix_cannot_hide_persistent_runner(self):
        self.doc['on'] = {'pull_request': {}}
        self.doc['jobs']['heavy'] = {'runs-on': '${{ matrix.os }}',
                                    'strategy': {'matrix': {'os': ['ubuntu-latest', 'n5pro']}}}
        self.assertTrue(findings(self.doc))

if __name__ == '__main__':
    unittest.main()
