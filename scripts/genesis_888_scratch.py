#!/usr/bin/env python3
"""Disposable loopback-only 888 proof; never point at live data or keys."""
import argparse,os,json,subprocess,time,hashlib,socket,sys
from pathlib import Path
from urllib.request import Request,urlopen
from urllib.error import HTTPError
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives import serialization
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--flat-reward-base-units',type=int,choices=[2222222222,3333333333],required=True)
parser.add_argument('--height',type=int,default=1040)
args=parser.parse_args()
assert args.height >= 1040, 'cover the epoch close and challenge window'
os.umask(0o077)
R=Path(__file__).resolve().parents[1]
(R/'round-r-evidence').mkdir(exist_ok=True)
D=R/'round-r-evidence'/('scratch-'+str(int(time.time())));D.mkdir(exist_ok=True)
BIN=R/'target/debug/launch_api'
SCHEDULE='0:v1,2:schedule_extension,3:reward_gross_outlier_guard,4:weather_skew_block_time_allowance,5:station_active_reward_gate,6:station_owner_actions,7:station_owner_consent,8:station_registration_nonce,9:node_share_settlement'
ports=[18560,18561,18562,19010,19011,19012]
for port in ports:
 with socket.socket() as sock:
  sock.setsockopt(socket.SOL_SOCKET,socket.SO_REUSEADDR,1);sock.bind(('127.0.0.1',port))
key=Ed25519PrivateKey.generate();raw=key.private_bytes(serialization.Encoding.Raw,serialization.PrivateFormat.Raw,serialization.NoEncryption());pub=key.public_key().public_bytes(serialization.Encoding.Raw,serialization.PublicFormat.Raw)
keyfile=D/'synthetic-producer.key'
with keyfile.open('x') as f:
 os.chmod(keyfile,0o600);f.write(raw.hex())
payout='0x'+hashlib.sha3_256(pub).digest()[:20].hex()
roster=D/'producers.json';roster.write_text(json.dumps({'version':1,'members':[{'producer_id':'scratch-producer','payout_address':payout,'public_key':pub.hex()}]}))
# Only synthetic credentials are generated; no host env files are read.
base={'PATH':os.defpath,'HOME':str(D),'RUST_LOG':'info','ALLOW_UNENCRYPTED_STORAGE':'true','WRFCOIN_ALLOW_NO_AUTH':'1','WRFCOIN_CHAIN_ID':'888','WRFCOIN_NETWORK_ID':'testnet','WRFCOIN_SIGNING_PROTOCOL_VERSION':'v1','WRFCOIN_GENESIS_ACTIVATION_SCHEDULE':SCHEDULE,'WRFCOIN_GENESIS_PRODUCER_SET':'@'+str(roster),'WRFCOIN_REQUIRE_PRODUCER_AUTH':'true','WRFCOIN_PRODUCER_SIGNER_KEY_FILE':str(keyfile),'WRFCOIN_PRODUCER_PAYOUT_ADDRESS':payout,'WRFCOIN_GENESIS_ORACLE_SET':json.dumps({'oracle_set':[Ed25519PrivateKey.generate().public_key().public_bytes(serialization.Encoding.Raw,serialization.PublicFormat.Raw).hex()],'oracle_quorum':1}),'WRFCOIN_GENESIS_ORACLE_QUORUM':'1','WRFCOIN_GENESIS_FAUCET_ADDRESS':'0x'+'23'*20,'WRFCOIN_GENESIS_FLAT_REWARD_SECONDS':'600','WRFCOIN_GENESIS_FAUCET_ALLOCATION_BASE_UNITS':'10000000000000','WRFCOIN_GENESIS_SCHEDULE_GOVERNANCE_KEYS':'e7bec166ad446314d26fb7526bfa8cec1b363b438440262fb001f7b04b20bf00','WRFCOIN_GENESIS_NODE_SHARE_EPOCH_BLOCKS':'512','WRFCOIN_GENESIS_NODE_SHARE_CHALLENGE_BLOCKS':'512','WRFCOIN_API_HOST':'127.0.0.1','WRFCOIN_P2P_HOST':'127.0.0.1','WRFCOIN_MAX_PEERS_PER_SUBNET':'0','WRFCOIN_P2P_BOOTSTRAP_REDIAL_INTERVAL_SECS':'1','WRFCOIN_P2P_BOOTSTRAP_REDIAL_MAX_BACKOFF_SECS':'2','WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS':pub.hex()}
base['WRFCOIN_GENESIS_FLAT_REWARD_BASE_UNITS']=str(args.flat_reward_base_units)
base['WRFCOIN_OPERATOR_TOKEN']=os.urandom(32).hex()
children=[]
def api(i,path,data=None):
 req=Request(f'http://127.0.0.1:{ports[i]}'+path,data=None if data is None else json.dumps(data).encode(),headers={'Content-Type':'application/json','Authorization':'Bearer '+base['WRFCOIN_OPERATOR_TOKEN']})
 for attempt in range(10):
  try:
   with urlopen(req,timeout=20) as f:return json.load(f)
  except HTTPError as e:
   if e.code!=429:raise RuntimeError(f'HTTP {e.code}: '+e.read().decode()[:1800])
   delay=max(1,int(e.headers.get('Retry-After','60')))
   if delay>300:raise RuntimeError('scratch Retry-After exceeds bounded wait')
   print('RATE_LIMIT_WAIT',delay,flush=True);time.sleep(delay)
 raise RuntimeError('scratch API remains rate limited after bounded retries')
def start(i,genesis='0'*64,p2p=False,bootstrap=None):
 nd=D/f'node{i}';nd.mkdir(exist_ok=True)
 env=base|{'WRFCOIN_DATA_DIR':str(nd),'WRFCOIN_API_PORT':str(ports[i]),'WRFCOIN_P2P_PORT':str(ports[i+3]),'WRFCOIN_P2P_ENABLED':str(p2p).lower(),'WRFCOIN_GENESIS_HASH':genesis}
 if i:env['WRFCOIN_SYNC_ONLY']='true'
 if bootstrap:env['WRFCOIN_P2P_BOOTSTRAP']=bootstrap
 out=open(D/f'node{i}-{int(time.time())}.log','wb');p=subprocess.Popen([str(BIN)],env=env,cwd=D,stdout=out,stderr=subprocess.STDOUT);children.append(p)
 for _ in range(100):
  if p.poll() is not None:raise RuntimeError(f'node {i} exited {p.returncode}; inspect scratch log')
  try:api(i,'/network/info');return p
  except Exception:time.sleep(.2)
 raise RuntimeError(f'node {i} did not become ready')
def stop(p):
 p.terminate()
 try:p.wait(timeout=10)
 except subprocess.TimeoutExpired:p.kill();p.wait()
try:
 producer=start(0)
 oracle=json.loads(base['WRFCOIN_GENESIS_ORACLE_SET'])['oracle_set'][0]
 registration=json.loads(subprocess.check_output([str(R/'target/debug/examples/genesis_888_scratch_registration'),str(keyfile),oracle],env=base,cwd=D))
 admitted=api(0,'/transactions/oracle/registrations',registration);print('ORACLE_ADMITTED',json.dumps(admitted),flush=True)
 first=api(0,'/blocks/mine',{'miner_address':payout});print('CUT',json.dumps(first),flush=True)
 block=api(0,'/chain/block/1');(D/'block1.json').write_text(json.dumps(block,indent=2))
 print('BLOCK1_SHAPE',list(block),flush=True)
 (D/'cut-response.json').write_text(json.dumps(first))
 stop(producer)
 producer=start(0,first['hash'],True,'/ip4/127.0.0.1/tcp/19011')
 start(1,first['hash'],True,'/ip4/127.0.0.1/tcp/19010')
 start(2,first['hash'],True,'/ip4/127.0.0.1/tcp/19010')
 time.sleep(3)
 for height in range(2,args.height+1):
  mined=api(0,'/blocks/mine',{'miner_address':payout})
  assert mined['height']==height, mined
  if height%50==0:print('MINED',height,flush=True)
  if height in (519,520,1032,1033,1040):
   boundary=api(0,'/node/settlement/0')
   assert boundary['maturity_height']=='1033',boundary
   assert boundary['status']==('OPEN' if height==519 else 'UNISSUED'),boundary
   if height>=520:assert boundary['unissued_base_units']==boundary['scheduled_base_units'],boundary
   assert boundary['payouts_base_units']=={},boundary
   (D/f'epoch0-at-{height}.json').write_text(json.dumps(boundary,indent=2))
   print('EPOCH_BOUNDARY',height,boundary['status'],boundary['maturity_height'],flush=True)
  time.sleep(.12)
 for attempt in range(150):
  try:
   blocks=[api(i,f'/chain/block/{args.height}')['data'] for i in range(3)]
   hashes=[b['hash'] for b in blocks]
   if len(set(hashes))==1:break
  except Exception:pass
  time.sleep(1)
 else:raise RuntimeError(f'followers did not converge at {args.height}')
 info=[api(i,'/network/info') for i in range(3)]
 snapshots=[api(i,'/state/snapshot/export?limit=1') for i in range(3)]
 roots=[v['state_root_hex'] for v in snapshots]
 assert len(set(roots))==1, roots
 bonds=[api(i,'/node/bonds/'+'01'*32) for i in range(3)]
 settlements=[api(i,'/node/settlement/0') for i in range(3)]
 upgrades=[api(i,'/network/upgrade') for i in range(3)]
 assert all(v['activation_height']=='9' and v['height']==str(args.height) for v in bonds),bonds
 assert all(v['status']=='UNISSUED' and int(v['scheduled_base_units'])>0 for v in settlements),settlements
 (D/f'height{args.height}-state-roots.json').write_text(json.dumps(snapshots,indent=2))
 (D/f'height{args.height}-node-pay.json').write_text(json.dumps({'bonds':bonds,'settlements':settlements,'upgrades':upgrades},indent=2))
 print('THREE_NODE_STATE_ROOT',roots[0],flush=True)
 print('NODE_PAY_ACTIVE',settlements[0]['scheduled_base_units'],flush=True)

 (D/f'height{args.height}-blocks.json').write_text(json.dumps(blocks,indent=2))
 (D/f'height{args.height}-network-info.json').write_text(json.dumps(info,indent=2))
 assert block['data']['metadata']['protocol_activation_schedule']==SCHEDULE
 assert block['data']['metadata']['protocol_version']=='v1'
 assert block['data']['metadata']['testnet_flat_reward_base_units']==str(args.flat_reward_base_units)
 print('THREE_NODE_888_CONVERGED',args.height,hashes[0],flush=True)
 print('EVIDENCE_DIR',D,flush=True)

finally:
 for p in children:
  if p.poll() is None:stop(p)
 print('SCRATCH_PROCESSES_STOPPED',len(children),flush=True)
