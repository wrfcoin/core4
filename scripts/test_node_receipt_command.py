import hashlib,json,struct,tempfile,subprocess,threading,socket
from pathlib import Path
from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
import os,sys
E=Path(sys.argv[2])
E.mkdir(parents=True,exist_ok=True)
F=json.loads((E/'fixture.json').read_text())
BIN=Path(sys.argv[1]).resolve()
a=F['authorization']
lp=lambda b:struct.pack('<Q',len(b))+b
body=lp(bytes(a['genesis_hash']))+lp(bytes(a['sender']))+struct.pack('<Q',a['nonce'])+lp(bytes(a['batch_hash']))
manual=lp(b'wrfcoin.node_receipt_submission.v1')+lp(b'777')+lp(b'testnet')+struct.pack('<I',2)+lp(body)
assert manual.hex()==F['preimage_hex']
print('Independent byte framing matches consensus preimage; SHA3-256:',hashlib.sha3_256(manual).hexdigest())
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey,Ed25519PrivateKey
from cryptography.hazmat.primitives.serialization import Encoding,PublicFormat
pub=lambda n:list(Ed25519PrivateKey.from_private_bytes(bytes([n])*32).public_key().public_bytes(Encoding.Raw,PublicFormat.Raw))
Ed25519PublicKey.from_public_bytes(bytes(F['owner_key'])).verify(bytes(F['owner_signature']),manual)
print('Independent Ed25519 verifier accepts consensus fixture signature.')
sender='0x'+bytes(a['sender']).hex()
with tempfile.TemporaryDirectory(prefix='sol3-r149-controls-') as d:
 root=Path(d)
 key=root/'key';key.write_text('0a'*32);key.chmod(0o600)
 marker='synthetic-sol3-operator-only-control'
 token=root/'token';token.write_text(marker);token.chmod(0o600)
 def invoke(archive,out,api,extra=(),keypath=None):
  cmd=[str(BIN),'--config',str(root/'cli-config.json'),'--json','node','receipt','submit','--archive',str(archive),'--epoch','1','--deployment-id','01'*32,'--sender',sender,'--genesis-hash','07'*32,'--chain-id','777','--network-id','testnet','--api-url',api,'--key-file',str(keypath or key),'--operator-token-file',str(token),'--out',str(out),*extra]
  if '--why' in cmd:
   cmd.remove('--why');cmd.insert(1,'--why')
  return subprocess.run(cmd,capture_output=True,text=True,timeout=75,env={'PATH':os.environ.get('PATH',''),'HOME':str(root),'RUST_LOG':'warn'})
 for case in ['dry-run','valid','corrupt-consumer-signature','v1-consumer','wrong-domain-consumer','lost-response','echo-token-error','echo-token-error-why','echo-token-503','echo-token-503-why','echo-success','explicit-retry-loss']:
  archive=root/case;archive.mkdir()
  item=F[{'corrupt-consumer-signature':'bad_archive','v1-consumer':'v1_archive','wrong-domain-consumer':'domain_archive'}.get(case,'archive')]
  (archive/'bonded-one.json').write_text(json.dumps(item))
  received=[]
  reads=[]
  class Handler(BaseHTTPRequestHandler):
   def log_message(self,*args):pass
   def reply(self,status,body):
    data=json.dumps(body).encode();self.send_response(status);self.send_header('Content-Length',str(len(data)));self.end_headers();self.wfile.write(data)
   def do_GET(self):
    reads.append(self.path)
    if self.path=='/network/info':self.reply(200,{'chain_id':'777','network_id':'testnet','genesis_hash':'07'*32})
    elif self.path.startswith('/node/bonds/'):
     self.reply(200,{'registration':{'binding':{'owner_account':a['sender'],'server_key':pub(11),'consumer_key':pub(22)}}})
    elif self.path.startswith('/addresses/'):
     self.reply(200,{'address':sender,'balance':'100000000000','spendable_balance':'100000000000','stake_locked':'0','earned_balance_total':'0','nonce':'1'})
    else:self.reply(200,{'receipt_id':item['receipt_id'],'status':'UNKNOWN','included':None,'untrusted':marker})
   def do_POST(self):
    raw=self.rfile.read(int(self.headers['Content-Length']));tx=json.loads(raw);received.append(tx)
    if (case=='lost-response' and len(received)==1) or (case=='explicit-retry-loss' and len(received)==2):
     self.connection.shutdown(socket.SHUT_RDWR);self.connection.close();return
    if case.startswith('echo-token-'):self.reply(503 if '503' in case else 403,{'error':marker});return
    txid=tx['id'];txid=txid['bytes'] if isinstance(txid,dict) else txid
    if isinstance(txid,list):txid=bytes(txid).hex()
    self.reply(202,{'admitted':True,'included':False,'tx_id':txid})
  server=ThreadingHTTPServer(('127.0.0.1',0),Handler);thread=threading.Thread(target=server.serve_forever,daemon=True);thread.start()
  try:
   if case=='dry-run':r=invoke(archive,root/(case+'.json'),'http://127.0.0.1:1',['--dry-run'],root/'absent-key')
   else:r=invoke(archive,root/(case+'.json'),f'http://127.0.0.1:{server.server_port}', ['--why'] if case.endswith('-why') else [])
   if case=='explicit-retry-loss':
    assert r.returncode==0 and len(received)==1
    packet=root/(case+'.json');before=packet.read_bytes()
    r=subprocess.run([str(BIN),'--config',str(root/'cli-config.json'),'--json','snapshot','submit-receipt','--transaction-file',str(packet)],capture_output=True,text=True,timeout=20,env={'PATH':os.environ.get('PATH',''),'HOME':str(root),'RUST_LOG':'warn','WRFCOIN_API_URL':f'http://127.0.0.1:{server.server_port}','WRFCOIN_OPERATOR_TOKEN':marker})
    assert r.returncode!=0 and len(received)==2 and received[0]==received[1]
    assert 'UNCERTAIN' in r.stdout+r.stderr and packet.read_bytes()==before
  finally:server.shutdown();server.server_close();thread.join()
  (E/(case+'.stdout')).write_text(r.stdout);(E/(case+'.stderr')).write_text(r.stderr)
  if (root/(case+'.json')).exists():
   saved=(root/(case+'.json')).read_text();assert marker not in saved and '0a'*32 not in saved
  print(case,{'exit':r.returncode,'POSTs':len(received),'token_in_output':marker in r.stdout+r.stderr,'durable_packet_exists':(root/(case+'.json')).exists()})
  if case=='dry-run':assert r.returncode==0 and not received
  if case=='valid':assert r.returncode==0 and len(received)==1
  if case in ['corrupt-consumer-signature','v1-consumer','wrong-domain-consumer']:assert r.returncode!=0 and len(received)==0 and not (root/(case+'.json')).exists()
  if case=='lost-response':
   packet=root/(case+'.json');before=packet.read_bytes();again=invoke(archive,packet,f'http://127.0.0.1:{server.server_port}',keypath=root/'nonexistent-key')
   assert any(path.startswith('/node/receipts/') for path in reads)
   assert len(received)==1 and r.returncode!=0 and 'UNCERTAIN' in r.stdout+r.stderr
   assert again.returncode!=0 and packet.read_bytes()==before
  assert marker not in r.stdout+r.stderr
  if case.startswith('echo-token-'):assert r.returncode!=0 and len(received)==1
