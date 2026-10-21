#!/usr/bin/env python3
"""Controls for receiver isolation, outage selection and nonvacuous registration."""
import copy,json,os,subprocess,tempfile,unittest,threading,shutil
from types import SimpleNamespace
from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
from urllib.error import HTTPError
from urllib.request import Request
from pathlib import Path
from unittest.mock import patch
import node_share_rehearsal as receiver
import node_share_matrix as matrix

class OperatorControls(unittest.TestCase):
    def test_new_testnet_domains_and_wrong_network_controls(self):
        for chain in ['777', '888', 'scratch-testnet']:
            receiver.validate_testnet_domain({'chain_id':chain,'network_id':'testnet'})
        for chain, network in [('', 'testnet'), ('888', 'mainnet'), ('888\n', 'testnet'), ('a'*65, 'testnet')]:
            with self.assertRaises(AssertionError):
                receiver.validate_testnet_domain({'chain_id':chain,'network_id':network})

    def test_outage_chooses_distinct_survivor_and_does_not_repeat_archived_claim(self):
        consumer='01'*32; mango='02'*32; workstation='03'*32
        plan={'consumer_deployment_id':consumer,'providers':[{'deployment_id':mango,'base_url':'http://mango.invalid'},{'deployment_id':workstation,'base_url':'http://ws.invalid'}]}
        credentials={'servers':{mango:'synthetic-reader',workstation:'synthetic-reader'}}
        def read(url,_credential):
            if 'mango.invalid' in url:raise OSError('unavailable')
            return {'status':'REGISTERED','principal_base_units':'25000000000','jailed':False,'epoch':'4'}
        with patch.object(receiver,'read_json',read):
            chosen=receiver.select_provider(plan,credentials,{})
            self.assertEqual(chosen[0]['deployment_id'],workstation)
            self.assertIsNone(receiver.select_provider(plan,credentials,{chosen[2]:{'status':'INSTALLED_SIGNED_NOT_INCLUDED'}}))
    def test_signer_configuration_cannot_override_owned_database_or_api_lease(self):
        with tempfile.TemporaryDirectory() as directory:
            plan={'chain_id':'777','network_id':'testnet','genesis_hash':'07'*32}
            config={'receiver_signer_env':{'WRFCOIN_HSM_MODULE_PATH':'synthetic-module','WRFCOIN_HSM_KEY_LABEL':'consumer','WRFCOIN_HSM_PIN_ENV':'WRFCOIN_DATA_DIR','WRFCOIN_DATA_DIR':'/live/database'}}
            with self.assertRaises(AssertionError):receiver.receiver_environment(plan,config,Path(directory),1234,'synthetic-lease')
    def test_fresh_receiver_environment_does_not_inherit_producer_or_spend_keys(self):
        with tempfile.TemporaryDirectory() as directory:
            plan={'chain_id':'777','network_id':'testnet','genesis_hash':'07'*32}
            signer={'WRFCOIN_HSM_MODULE_PATH':'synthetic-module','WRFCOIN_HSM_KEY_LABEL':'registered-consumer','WRFCOIN_HSM_PIN_ENV':'NODE_SHARE_RECEIVER_TEST_PIN','NODE_SHARE_RECEIVER_TEST_PIN':'synthetic-credential'}
            result=receiver.receiver_environment(plan,{'receiver_signer_env':signer,'receiver_storage_key':'66'*32},Path(directory),1234,'synthetic-lease')
            self.assertEqual(result['WRFCOIN_DATA_DIR'],str(Path(directory)/'database'))
            self.assertEqual(result['WRFCOIN_API_HOST'],'127.0.0.1')
            self.assertEqual(result['WRFCOIN_SYNC_ONLY'],'false')
            self.assertEqual(result['WRFCOIN_NODE_SHARE_REHEARSAL'],'true')
            receiver.validate_receiver_environment(result)
            wrong=dict(result);wrong['WRFCOIN_SYNC_ONLY']='true'
            with self.assertRaisesRegex(AssertionError,'refuses sync-only'):receiver.validate_receiver_environment(wrong)
            self.assertNotIn('WRFCOIN_PRODUCER_SIGNER_PRIVATE_KEY',result)
            self.assertNotIn('WRFCOIN_GAMIFICATION_PAYOUT_KEY',result)
    def test_matrix_rejects_missing_registration_and_zero_step_runtime(self):
        manifest=json.loads((matrix.ROOT/'scripts/integration_test_ratchet.json').read_text())
        self.assertEqual(matrix.validate_registration(manifest),[])
        omitted=copy.deepcopy(manifest);del omitted['node_share_matrix']
        self.assertTrue(matrix.validate_registration(omitted))
        omitted=copy.deepcopy(manifest);omitted['node_share_matrix']['tests'].remove(matrix.ON_TEST)
        self.assertTrue(matrix.validate_registration(omitted))
        with patch.object(matrix.subprocess,'run',return_value=subprocess.CompletedProcess([],0,'test result: ok. 0 passed; 0 failed;')):
            with self.assertRaises(AssertionError):matrix.execute(['synthetic-zero-step-command'])
    def test_required_safety_checks_remain_active_under_python_optimization(self):
        result=subprocess.run(['python3','-O','-c','import node_share_rehearsal as r; r.require(False,"executed guard")'],cwd=Path(__file__).parent,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        self.assertNotEqual(result.returncode,0)
        self.assertIn(b'AssertionError: executed guard',result.stderr)
    def test_public_endpoint_rejects_credentials_in_url(self):
        for url in ['http://' + ':'.join(['user', 'credential']) + '@localhost','file:///live/database','http://localhost/?credential=value']:
            with self.assertRaises(AssertionError):receiver.endpoint(url)

    def test_each_shared_target_refuses_zero_steps(self):
        expected={h:bytes(v).hex() for h,v in json.loads((matrix.ROOT/matrix.EXPECTED).read_text()).items()}
        for zero in matrix.REQUIRED_SHARED:
            with self.subTest(zero=zero),tempfile.TemporaryDirectory() as directory:
                def transport(args,**kwargs):
                    names=[args[i+1] for i,x in enumerate(args) if x=='--test']
                    output=''.join('test result: ok. %s passed; 0 failed;\n' % ('0' if name==zero else '1') for name in names)
                    if not names:output='test result: ok. 1 passed; 0 failed;\n'
                    if matrix.ON_TEST in args:output+='NODE_SHARE_MATRIX_ROOTS '+json.dumps(expected)+'\n'
                    return subprocess.CompletedProcess(args,0,output)
                with patch.object(matrix.subprocess,'run',side_effect=transport),patch.object(shutil,'disk_usage',return_value=SimpleNamespace(free=80*1024**3)):
                    with self.assertRaisesRegex(AssertionError,'VOID/RED registered target'):matrix.run(Path(directory)/'roots.json')
    def test_heavy_build_guard_defaults_to_40_gib_and_takes_only_a_whole_number(self):
        self.assertEqual(matrix.heavy_build_min_free_gib({}),40)
        self.assertEqual(matrix.heavy_build_min_free_gib({'WRFCOIN_HEAVY_BUILD_MIN_FREE_GIB':''}),40)
        self.assertEqual(matrix.heavy_build_min_free_gib({'WRFCOIN_HEAVY_BUILD_MIN_FREE_GIB':'12'}),12)
        for bad in ['0','-1','1.5',' 12','12GiB','99999']:
            with self.subTest(bad=bad),self.assertRaisesRegex(AssertionError,'whole number of GiB'):
                matrix.heavy_build_min_free_gib({'WRFCOIN_HEAVY_BUILD_MIN_FREE_GIB':bad})
    def test_heavy_build_guard_refuses_below_the_threshold_before_running_cargo(self):
        ran=[]
        with patch.object(matrix.subprocess,'run',side_effect=lambda *a,**k:ran.append(a)),patch.dict(os.environ,{},clear=False):
            os.environ.pop('WRFCOIN_HEAVY_BUILD_MIN_FREE_GIB',None)
            with patch.object(shutil,'disk_usage',return_value=SimpleNamespace(free=39*1024**3)):
                with self.assertRaisesRegex(AssertionError,'less than 40 GiB free'):matrix.execute(['cargo','test'])
            with patch.dict(os.environ,{'WRFCOIN_HEAVY_BUILD_MIN_FREE_GIB':'12'}),patch.object(shutil,'disk_usage',return_value=SimpleNamespace(free=11*1024**3)):
                with self.assertRaisesRegex(AssertionError,'less than 12 GiB free'):matrix.execute(['cargo','test'])
        self.assertEqual(ran,[],'cargo must not run when the guard refuses')
    def test_authenticated_read_refuses_cross_origin_redirect(self):
        received=[]
        class Target(BaseHTTPRequestHandler):
            def log_message(self,*args):pass
            def do_GET(self):
                received.append(self.headers.get('Authorization'));self.send_response(200);self.end_headers();self.wfile.write(b'{}')
        target=ThreadingHTTPServer(('127.0.0.1',0),Target)
        class Redirect(BaseHTTPRequestHandler):
            def log_message(self,*args):pass
            def do_GET(self):
                self.send_response(302);self.send_header('Location','http://127.0.0.1:'+str(target.server_port)+'/target');self.end_headers()
        source=ThreadingHTTPServer(('127.0.0.1',0),Redirect)
        threads=[threading.Thread(target=server.serve_forever,daemon=True) for server in [target,source]]
        for thread in threads:thread.start()
        try:
            with self.assertRaises(HTTPError):receiver.read_json('http://127.0.0.1:'+str(source.server_port)+'/redirect','synthetic-reader')
            self.assertEqual(received,[], 'redirect target must receive no authenticated request')
            self.assertIsNone(receiver.RefuseRedirects().redirect_request(Request('https://source.invalid'),None,302,'redirect',{},'http://other.invalid'))
        finally:
            for server in [source,target]:server.shutdown();server.server_close()
            for thread in threads:thread.join()

if __name__=='__main__':unittest.main()
