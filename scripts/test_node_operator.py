"""Actual CLI, loopback-only operator controls. Synthetic keys never leave temp roots.
Usage: python3 scripts/test_node_operator.py CLI CONSENSUS_HELPER EVIDENCE_DIR
"""
import base64,hashlib,json,os,socket,struct,subprocess,sys,tempfile,threading
from datetime import datetime,timedelta,timezone
from pathlib import Path
from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey,Ed25519PublicKey
from cryptography.hazmat.primitives.serialization import Encoding,PublicFormat
PROBE=len(sys.argv)>4 and sys.argv[4]=="--defect-probes"
B=Path(sys.argv[1]).resolve();C=Path(sys.argv[2]).resolve();E=Path(sys.argv[3]);E.mkdir(parents=True,exist_ok=True)
lp=lambda b:struct.pack('<Q',len(b))+b
keys={n:Ed25519PrivateKey.from_private_bytes(bytes([n])*32) for n in [11,12,22,30,99]}
pub=lambda n:keys[n].public_key().public_bytes(Encoding.Raw,PublicFormat.Raw)
address=lambda p:'0x'+hashlib.sha3_256(p).digest()[:20].hex()
frame=lambda tag,body:lp(tag)+lp(b'777')+lp(b'testnet')+struct.pack('<I',2)+lp(body)
def auth_bytes(a):
 return frame(b'wrfcoin.node_bond_authorization.v1',b''.join(lp(bytes(a[f])) for f in ['genesis_hash','deployment_id','owner_account','escrow_account','payout_account','server_key','consumer_key'])+lp(a['service_alias'].encode())+struct.pack('<IQQQ',0,a['amount_units'],a['expected_nonce'],a['effective_epoch']))
def txid(tx):
 v=tx['id'];v=v['bytes'] if isinstance(v,dict) else v
 return bytes(v).hex() if isinstance(v,list) else v
with tempfile.TemporaryDirectory(prefix='node-operator-controls-') as d:
 root=Path(d);child_env={'PATH':os.environ.get('PATH',''),'HOME':str(root),'RUST_LOG':'warn'}
 def run(args):return subprocess.run([str(B),'--config',str(root/'config.json'),'--json',*args],capture_output=True,text=True,env=child_env,timeout=30)
 # Init and new-address both persist the exact supported wallet JSON format.
 init=run(['wallet','list-addresses']);assert init.returncode==0
 generated=run(['wallet','new-address']);assert generated.returncode==0
 owner=json.loads(generated.stdout)['data']['address'];key=root/'.wrfcoin'/'keys'/(owner+'.json')
 wallet=json.loads(key.read_text());owner_pub=base64.b64decode(wallet['public_key']);assert owner==address(owner_pub)
 assert key.stat().st_mode&0o777==0o600
 info=run(['node','bond','key-info','--key-file',str(key)]);assert info.returncode==0
 assert json.loads(info.stdout)['data']=={'address':owner,'public_key':owner_pub.hex()}
 marker='synthetic-r169-only-bearer';token=root/'token';token.write_text(marker);token.chmod(0o600)
 state={'tip':3,'nonce':0,'binding':None,'posts':[],'drop':False,'echo':False,'drift':False,'reads':[]}
 class Handler(BaseHTTPRequestHandler):
  def log_message(self,*args):pass
  def reply(self,status,body):
   data=json.dumps(body).encode();self.send_response(status);self.send_header('Content-Length',str(len(data)));self.end_headers();self.wfile.write(data)
  def do_GET(self):
   state['reads'].append(self.path)
   if self.path=='/network/info':body={'chain_id':'777','network_id':'testnet','genesis_hash':'07'*32}
   elif self.path=='/blockchain':body={'height':state['tip']}
   elif self.path=='/node/bonds/'+'01'*32:
    body={'height':str(state['tip']),'activation_height':'3','status':'REGISTERED' if state['binding'] else 'UNKNOWN','registration':{'binding':state['binding']} if state['binding'] else None}
   elif self.path=='/node/bonds/'+'02'*32:body={'registration':{'binding':{'consumer_key':list(pub(22))}}}
   elif self.path.startswith('/addresses/'):
    body={'address':owner,'nonce':str(state['nonce']),'balance':'100000000000','spendable_balance':'100000000000','stake_locked':'0','earned_balance_total':'0'}
    if state['drift']:state['tip']=1026
   elif self.path.startswith('/node/receipts/'):
    body={'receipt_id':self.path.rsplit('/',1)[1],'status':'UNKNOWN','included':None,'untrusted':marker}
   else:raise AssertionError(self.path)
   self.reply(200,body)
  def do_POST(self):
   tx=json.loads(self.rfile.read(int(self.headers['Content-Length'])));state['posts'].append((self.path,tx))
   if self.path=='/node/bonds/submit':
    p=json.loads(tx['data']);a=p['signed']['authorization'];pre=auth_bytes(a)
    for n,sig in [(99,p['governance_signature']),(30,p['payout_signature'])]:keys[n].public_key().verify(bytes(sig),pre)
    Ed25519PublicKey.from_public_bytes(owner_pub).verify(bytes(p['signed']['owner_signature']),pre)
    for n,role,name in [(11,0,'server_signature'),(12,1,'consumer_signature')]:
     subject=lp(hashlib.sha3_256(pre).digest())+struct.pack('<I',role)
     keys[n].public_key().verify(bytes(p['signed'][name]),frame(b'wrfcoin.node_bond_possession.v1',subject))
    assert a['owner_account']==list(bytes.fromhex(owner[2:])) and a['payout_account']==list(bytes.fromhex(address(pub(30))[2:]))
    assert a['amount_units']==25000000000 and a['expected_nonce']==0 and a['effective_epoch']==1
    expected=hashlib.sha3_256(lp(b'wrfcoin.node_bond_escrow.v2')+lp(bytes([1])*32)).digest()[:20]
    assert bytes(a['escrow_account'])==expected
    state['binding']=a;state['nonce']=1
   else:assert self.path=='/node/receipts/submit'
   if state['drop']:
    self.connection.shutdown(socket.SHUT_RDWR);self.connection.close();return
   if state['echo']:self.reply(503,{'error':marker});return
   self.reply(202,{'tx_id':txid(tx),'admitted':True,'included':False,'untrusted':marker})
 server=ThreadingHTTPServer(('127.0.0.1',0),Handler);thread=threading.Thread(target=server.serve_forever,daemon=True);thread.start()
 api=f'http://127.0.0.1:{server.server_port}'
 common=['node','bond','deposit','--deployment-id','01'*32,'--owner-key',owner_pub.hex(),'--payout-address',address(pub(30)),'--payout-key',pub(30).hex(),'--server-key',pub(11).hex(),'--consumer-key',pub(12).hex(),'--governance-key',pub(99).hex(),'--service-alias','node-1','--genesis-hash','07'*32,'--activation-height','3','--api-url',api,'--operator-token-file',str(token)]
 results=[];defects=[]
 window=int(subprocess.check_output([str(C),'--submission-window'],env=child_env,text=True))
 def record(name,p,count,no_packet=None):
  assert marker not in p.stdout+p.stderr
  (E/(name+'.stdout')).write_text(p.stdout);(E/(name+'.stderr')).write_text(p.stderr)
  if no_packet:assert not no_packet.exists()
  results.append({'case':name,'exit':p.returncode,'POSTs':count});print(results[-1])
 def reset():state.update(tip=3,nonce=0,binding=None,posts=[],drop=False,echo=False,drift=False,reads=[])
 try:
  bad=common.copy();bad[bad.index('--payout-address')+1]=owner;bad[bad.index('--payout-key')+1]=owner_pub.hex()
  p=run(bad+['--dry-run']);assert p.returncode!=0 and not state['reads'];record('payout-alias',p,0)
  bad=common.copy();bad[bad.index('--governance-key')+1]=owner_pub.hex()
  p=run(bad+['--dry-run']);assert p.returncode!=0 and not state['reads'];record('governance-owner-alias',p,0)
  for role,n in [('server',11),('consumer',12)]:
   reset();bad=common.copy();bad[bad.index('--payout-address')+1]=address(pub(n));bad[bad.index('--payout-key')+1]=pub(n).hex()
   out=root/('payout-'+role+'-alias.json')
   p=run(bad+['--dry-run','--key-file',str(root/'nonexistent'),'--out',str(out)])
   passed=p.returncode!=0 and not state['reads'] and not state['posts'] and not out.exists()
   record('payout-'+role+'-alias',p,len(state['posts']),out if passed else None)
   results[-1]['policy_pass']=passed;defects.append(results[-1])
   if not PROBE:assert passed,'payout/service alias must refuse before reads/key/output'
  reset()
  state['tip']=2;p=run(common+['--dry-run']);assert p.returncode!=0 and 'before activation' in p.stdout+p.stderr and not state['posts'];record('pre-H',p,0);reset()
  unsigned=root/'unsigned.json';p=run(common+['--dry-run','--key-file',str(root/'nonexistent'),'--out',str(unsigned)]);assert p.returncode==0 and not state['posts']
  tx=json.loads(p.stdout)['data']['transaction'];assert tx==json.loads(unsigned.read_text());assert json.loads(tx['data'])['signed']['authorization']['effective_epoch']==1
  record('keyless-dry-run',p,0)
  approvals=root/'approvals.json';packet=json.loads(unsigned.read_text());payload=json.loads(packet['data']);a=payload['signed']['authorization'];pre=auth_bytes(a)
  payload['governance_signature']=list(keys[99].sign(pre));payload['payout_signature']=list(keys[30].sign(pre))
  for n,role,name in [(11,0,'server_signature'),(12,1,'consumer_signature')]:
   subject=lp(hashlib.sha3_256(pre).digest())+struct.pack('<I',role)
   payload['signed'][name]=list(keys[n].sign(frame(b'wrfcoin.node_bond_possession.v1',subject)))
  packet['data']=json.dumps(payload,separators=(',',':'));approvals.write_text(json.dumps(packet))
  owner_signed=root/'owner-signed.json'
  p=run(['node','bond','sign','--transaction-file',str(approvals),'--role','owner','--chain-id','777','--network-id','testnet','--key-file',str(key),'--out',str(owner_signed)])
  assert p.returncode==0 and not state['posts']
  signed=json.loads(json.loads(owner_signed.read_text())['data']);Ed25519PublicKey.from_public_bytes(owner_pub).verify(bytes(signed['signed']['owner_signature']),pre)
  record('owner-lifecycle-sign',p,0)
  reset();delayed=json.loads(approvals.read_text())
  delayed['timestamp']=(datetime.fromisoformat(delayed['timestamp'].replace('Z','+00:00'))-timedelta(seconds=2*window)).isoformat().replace('+00:00','Z')
  delayed_file=root/'delayed-approvals.json';delayed_file.write_text(json.dumps(delayed));out=root/'delayed-deposit.json'
  p=run(common+['--approvals-file',str(delayed_file),'--key-file',str(key),'--out',str(out)])
  assert p.returncode==0 and len(state['posts'])==1 and out.is_file()
  signed_packet=json.loads(out.read_text());stamp=datetime.fromisoformat(signed_packet['timestamp'].replace('Z','+00:00'))
  c=subprocess.run([str(C),str(out),str(root/'delayed-claim.json')],capture_output=True,text=True,env=child_env,timeout=20)
  passed=signed_packet['timestamp']!=delayed['timestamp'] and abs((datetime.now(timezone.utc)-stamp).total_seconds())<=window and c.returncode==0
  record('delayed-approval',p,1);results[-1]['policy_pass']=passed;defects.append(results[-1])
  assert json.loads(signed_packet['data'])['signed']['authorization']==json.loads(delayed['data'])['signed']['authorization']
  (E/'delayed-approval-public.json').write_text(json.dumps({'approval_timestamp':delayed['timestamp'],'submitted_timestamp':signed_packet['timestamp'],'approval_id':txid(delayed),'submitted_id':txid(signed_packet),'producer_window_secs':window,'producer_window_pass':c.returncode==0},indent=2)+'\n')
  if not PROBE:assert passed,'delayed approval must refresh outer timestamp and fit producer window'
  reset()
  for role in ['producer','governance','payout']:
   out=root/(role+'-signature.json');p=run(['node','bond','sign','--transaction-file',str(approvals),'--role',role,'--chain-id','777','--network-id','testnet','--key-file',str(key),'--out',str(out)])
   assert p.returncode!=0;record(role+'-software-role',p,0,out)
  key.chmod(0o644);out=root/'bad-mode.json';p=run(common+['--approvals-file',str(approvals),'--key-file',str(key),'--out',str(out)]);assert p.returncode!=0 and not state['posts'];record('key-mode',p,0,out);key.chmod(0o600)
  reset();state['drift']=True;out=root/'epoch-drift.json';p=run(common+['--approvals-file',str(approvals),'--key-file',str(key),'--out',str(out)]);assert p.returncode!=0 and not state['posts'];record('epoch-drift',p,0,out)
  for name,mode in [('lost-response','drop'),('echoed-error','echo')]:
   reset();state[mode]=True;out=root/(name+'.json');p=run(common+['--approvals-file',str(approvals),'--key-file',str(key),'--out',str(out)])
   assert p.returncode!=0 and 'UNCERTAIN' in p.stdout+p.stderr and len(state['posts'])==1 and out.is_file()
   before=out.read_bytes();again=run(common+['--approvals-file',str(approvals),'--key-file',str(root/'nonexistent'),'--out',str(out)])
   assert again.returncode!=0 and out.read_bytes()==before and len(state['posts'])==1
   assert any(x.startswith('/node/bonds/') for x in state['reads']);record(name,p,1)
  reset();deposit=root/'deposit.json';p=run(common+['--approvals-file',str(approvals),'--key-file',str(key),'--out',str(deposit)])
  assert p.returncode==0 and len(state['posts'])==1 and deposit.is_file();record('deposit-submit',p,1)
  archive=root/'archive';archive.mkdir();public_claim=archive/'bonded-one.json'
  c=subprocess.run([str(C),str(deposit),str(public_claim)],capture_output=True,text=True,env=child_env,timeout=20);assert c.returncode==0,c.stderr
  state['tip']=1027;receipt=root/'receipt.json'
  p=run(['node','receipt','submit','--archive',str(archive),'--epoch','1','--deployment-id','01'*32,'--sender',owner,'--genesis-hash','07'*32,'--api-url',api,'--key-file',str(key),'--operator-token-file',str(token),'--out',str(receipt)])
  assert p.returncode==0 and len(state['posts'])==2 and receipt.is_file();record('same-wallet-receipt',p,1)
  c=subprocess.run([str(C),str(deposit),str(public_claim),str(receipt)],capture_output=True,text=True,env=child_env,timeout=20)
  assert c.returncode==0,c.stderr
  print(c.stdout.strip());(E/'consensus.log').write_text(c.stdout)
  for packet in [deposit,receipt]:assert marker not in packet.read_text() and wallet['private_key'] not in packet.read_text()
 finally:server.shutdown();server.server_close();thread.join()
 (E/'results.json').write_text(json.dumps(results,indent=2)+'\n')
 if PROBE:
  (E/'defect-probes.json').write_text(json.dumps(defects,indent=2)+'\n')
  print('NODE_OPERATOR_DEFECT_POLICIES',json.dumps(defects))
  raise SystemExit(0 if all(row['policy_pass'] for row in defects) else 1)
 print('NODE_OPERATOR_ACTUAL_COMMAND_CONTROLS_OK',len(results))
