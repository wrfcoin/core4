"""Run with python3 -m unittest discover -s docs/api/testnet-b3 -v."""
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch
from check_contract import check, sources, render, parse_routes

HERE = Path(__file__).resolve().parent
SPEC = HERE / 'launch-api.openapi.json'


class DeployedContract(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.source = sources()

    def test_deployed_spec_matches_router(self):
        self.assertTrue(SPEC.is_file(), 'deployed API has no checked OpenAPI contract')
        self.assertEqual(check(json.loads(SPEC.read_text()), self.source), [])

    def test_complete_release_surface_and_absent_families(self):
        document = json.loads(SPEC.read_text())
        operations = {(p, m) for p, item in document['paths'].items() for m in item}
        self.assertEqual(len(operations), 123)
        self.assertIn(('/nft/list', 'get'), operations)
        self.assertIn(('/transactions/oracle/pending', 'get'), operations)
        self.assertIn(('/transactions/submit', 'post'), operations)
        self.assertIn(('/governance/proposals/{id}/activate', 'put'), operations)
        self.assertIn(('/network/peers/reputation', 'get'), operations)
        for family in ['/privacy/', '/zk/', '/execution/', '/debug/']:
            self.assertFalse(any(p.startswith(family) for p, _ in operations))
        self.assertEqual(len(document['x-excluded-operations']), 27)
        ids = [item['operationId'] for path in document['paths'].values() for item in path.values()]
        self.assertEqual(len(ids), len(set(ids)))

    def test_actual_deployed_infra_build_profile(self):
        document = json.loads(SPEC.read_text())
        self.assertEqual(document['x-build-features'], ['full-mining', 'hsm', 'p2p', 'upgrade-notice-fetch'])
        self.assertEqual(document['x-build-profile']['infra-revision'], 'wrfcoin/infra@750298cdf20c')

    def test_spec_addition_and_removal_are_both_rejected(self):
        for mutate in ['add', 'remove', 'method']:
            with self.subTest(mutate=mutate):
                document = json.loads(SPEC.read_text())
                if mutate == 'add':
                    document['paths']['/invented'] = document['paths']['/health']
                elif mutate == 'remove':
                    del document['paths']['/health']
                else:
                    document['paths']['/health']['post'] = document['paths']['/health'].pop('get')
                self.assertTrue(any('operation:' in e for e in check(document, self.source)))

    def test_source_path_method_and_scope_drift(self):
        for old, new in [('"/health"', '"/health-new"'), ('"/health", web::get()', '"/health", web::post()'), ('web::scope("/governance")', 'web::scope("/changed")')]:
            with self.subTest(new=new):
                source = self.source.copy()
                self.assertIn(old, source['routes'])
                source['routes'] = source['routes'].replace(old, new, 1)
                errors = check(json.loads(SPEC.read_text()), source)
                self.assertTrue(any('missing operation:' in e for e in errors))
                self.assertTrue(any('phantom operation:' in e for e in errors))

    def test_registration_addition_is_missing_from_spec(self):
        source = self.source.copy()
        source['routes'] = source['routes'].replace('cfg.service(scope);', 'let scope = scope.route("/new", web::get().to(chain::health)); cfg.service(scope);')
        self.assertIn('missing operation: GET /new', check(json.loads(SPEC.read_text()), source))

    def test_build_feature_change_is_not_accepted(self):
        source = self.source.copy()
        source['build'] = source['build'].replace('"full-mining,p2p,hsm,upgrade-notice-fetch"', '"full-mining,p2p,hsm,upgrade-notice-fetch,privacy"')
        errors = check(json.loads(SPEC.read_text()), source)
        self.assertIn('missing operation: POST /privacy/keys/register', errors)
        self.assertIn('metadata drift: x-build-features', errors)

    def test_feature_gate_mutation_changes_selected_surface(self):
        source = self.source.copy()
        source['routes'] = source['routes'].replace('#[cfg(feature = "privacy")]', '#[cfg(feature = "p2p")]')
        self.assertIn('missing operation: POST /privacy/keys/register', check(json.loads(SPEC.read_text()), source))

    def test_production_mount_removed_despite_test_mounts(self):
        source = self.source.copy()
        source['mount'] = source['mount'].replace('app.configure(configure_launch_routes)', 'app', 1)
        self.assertIn('.configure(configure_launch_routes)', source['mount'])
        with self.assertRaisesRegex(ValueError, 'production App'):
            render(source)
        source['mount'] = source['mount'].replace('let server = HttpServer::new(move || {', 'let server = HttpServer::new(move || { // app.configure(configure_launch_routes)')
        with self.assertRaisesRegex(ValueError, 'production App'):
            render(source)

    def test_actual_cfg_and_child_scope_grammar(self):
        fixture = '''fn configure_launch_routes(cfg: &mut web::ServiceConfig) {
          let scope = web::scope("").service(web::scope("/other").route("/{id}", web::put().to(a::b)));
          // .route("/comment-phantom", web::get().to(a::b))
          #[cfg(all(feature = "chosen", not(feature = "off")))]
          let scope = scope.route("/unusual-prefix", web::post().to(a::b));
          #[cfg(any(test, feature = "absent"))]
          let scope = scope.route("/compiled-out", web::get().to(a::b));
          cfg.service(scope);
        }'''
        rows, absent = parse_routes(fixture, {'chosen'})
        self.assertEqual([(r['path'], r['method']) for r in rows], [('/other/{id}', 'put'), ('/unusual-prefix', 'post')])
        self.assertEqual([r['path'] for r in absent], ['/compiled-out'])

    def test_unknown_registration_forms_are_refused(self):
        variants = ['scope.route(dynamic_path, web::get().to(a::b))', 'scope.service(other_routes())', 'scope.configure(other_routes)', 'scope.service(web::resource("/x"))']
        for variant in variants:
            with self.subTest(variant=variant), self.assertRaises(ValueError):
                parse_routes('fn configure_launch_routes(cfg: &mut web::ServiceConfig) {let scope = '+variant+';cfg.service(scope);}', set())

    def test_snapshot_metadata_and_parameter_drift(self):
        for field in ['x-core4-revision', 'x-build-features', 'x-build-profile', 'x-excluded-operations']:
            document = json.loads(SPEC.read_text())
            document[field] = 'incorrect'
            self.assertIn('metadata drift: '+field, check(document, self.source))
        document = json.loads(SPEC.read_text())
        document['paths']['/addresses/{address}']['get']['parameters'][0]['required'] = False
        self.assertIn('operation drift: GET /addresses/{address}', check(document, self.source))

    def test_full_feature_inventory_accounts_for_all_150_routes(self):
        rows, absent = parse_routes(self.source['routes'], {'p2p', 'privacy', 'execution', 'zk', 'dev-endpoints'})
        self.assertEqual(len(rows), 150)
        self.assertEqual(absent, [])

    def test_writer_cannot_label_other_source_as_the_deployed_release(self):
        with tempfile.TemporaryDirectory(prefix='sol-i-api-writer-') as directory:
            for args in [['--working-tree'], ['--revision', 'HEAD']]:
                with self.subTest(args=args):
                    output = Path(directory)/'unapproved.json'
                    result = subprocess.run(['python3', str(HERE/'check_contract.py'), '--emit', str(output), *args], capture_output=True, text=True)
                    self.assertNotEqual(result.returncode, 0, result.stdout)
                    self.assertFalse(output.exists())

    def test_writer_cannot_label_a_changed_build_profile_as_the_release(self):
        from check_contract import main
        source = self.source.copy()
        source['build'] = source['build'].replace('p2p,hsm', 'p2p,privacy,hsm')
        with tempfile.TemporaryDirectory(prefix='sol-i-api-profile-writer-') as directory:
            output = Path(directory)/'unapproved.json'
            with patch('check_contract.sources', return_value=source), patch('sys.argv', ['check_contract', '--emit', str(output)]):
                self.assertEqual(main(), 1)
            self.assertFalse(output.exists())


if __name__ == '__main__':
    unittest.main()
