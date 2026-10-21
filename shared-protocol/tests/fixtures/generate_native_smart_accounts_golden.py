"""Independent stdlib framing + cryptography ed25519 reference, no Rust signing.

All keys are synthetic fixed test keys. Only public preimages/digests/signatures
are written to the fixture; each public vector uses the documented scanner pragma.
"""
from hashlib import sha3_256
from pathlib import Path
from struct import pack
import subprocess
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat

OUT = Path(__file__).with_name('native_smart_accounts_golden.rs')
u32 = lambda n: pack('<I', n)
u64 = lambda n: pack('<Q', n)
field = lambda b: u64(len(b)) + b
seq = lambda xs: u64(len(xs)) + b''.join(field(x) for x in xs)
keys = [Ed25519PrivateKey.from_private_bytes(bytes([n])*32) for n in [7,8,9]]
pubs = [k.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw) for k in keys]
chain, network, genesis = b'aa-test', b'local', bytes([3])*32
target = bytes([4])*20
base = field(chain)+field(network)+field(genesis)+u32(1)+field(target)+field(pubs[0])+u32(0)+u64(5)+u64(6)+u64(8)+u64(9)+u64(10)
frame = lambda tag, body: field(tag)+field(chain)+field(network)+u32(2)+field(body)
session = field(pubs[1])+u64(2)+seq([bytes([11])*20])+u64(20)+u64(40)+u64(8)+u64(9)+u64(3)
guardians = sorted(pubs[:2])
policy = seq(guardians)+u32(2)+u64(100)+u64(50)+u64(2)
recovery = u64(3)+u64(2)+field(pubs[1])
actions = [field(pubs[0]),seq([field(bytes([11])*20)+u64(12)]),session,field(pubs[1])+u64(2),policy,recovery,recovery,recovery,u64(20),u64(20)+u64(2),u64(20)+u64(2)]
rows=[]
for tag, action in enumerate(actions):
    body=base+field(u32(tag)+action)+bytes([0])
    msg=frame(b'wrfcoin.native_smart_account.tx.v1',body)
    rows.append((f'ACTION_{tag}',body,msg,keys[0]))
terms = field(pubs[2])+u64(13)+u64(14)+u64(8)+u64(9)
sponsored = base+field(u32(1)+actions[1])+bytes([1])+field(terms)
sponsored_msg=frame(b'wrfcoin.native_smart_account.tx.v1',sponsored)
rows.append(('SPONSORED',sponsored,sponsored_msg,keys[0]))
sponsor_body=field(sha3_256(sponsored_msg).digest())+field(terms)
rows.append(('SPONSOR',sponsor_body,frame(b'wrfcoin.native_smart_account.sponsor.v1',sponsor_body),keys[2]))
registration_msg=rows[2][2]
possession_body=field(sha3_256(registration_msg).digest())
rows.append(('POSSESSION',possession_body,frame(b'wrfcoin.native_smart_account.session_possession.v1',possession_body),keys[1]))
address_body=field(genesis)+field(pubs[0])+u64(5)
address_msg=frame(b'wrfcoin.native_smart_account.address.v1',address_body)
lines=['// Independent Python reference vectors; all material here is public.']
for name,body,msg,signer in rows:
    for suffix,value in [('BODY',body),('PREIMAGE',msg),('DIGEST',sha3_256(msg).digest()),('SIGNATURE',signer.sign(msg))]:
        lines.append(f'pub const {name}_{suffix}: &str = "{value.hex()}"; // pragma: allowlist secret (public synthetic golden vector)')
lines.append(f'pub const ADDRESS: &str = "{sha3_256(address_msg).digest()[:20].hex()}"; // pragma: allowlist secret (public synthetic address)')
OUT.parent.mkdir(parents=True,exist_ok=True)
OUT.write_text('\n'.join(lines)+'\n')
subprocess.run(['rustfmt', '--edition', '2021', str(OUT)], check=True,
               stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
print(f'Wrote {len(rows)} public signing vectors plus one address vector')
