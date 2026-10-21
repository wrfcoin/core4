"""Serial scheduled real restores into new owned scratch databases.

Public plan in a file; credentials and dedicated receiver signer configuration
on stdin. No spend key, payout endpoint, on-chain submission or live DB target.
"""
from __future__ import annotations
import argparse, hashlib, json, os, secrets, shutil, socket, subprocess, tempfile, time
from pathlib import Path
from urllib.request import Request, HTTPRedirectHandler, build_opener
from urllib.parse import urlsplit
def require(condition, message):
    if not condition:
        raise AssertionError(message)

MARKER = '.node-share-rehearsal-root'
PUBLIC_SIGNER_ENV = {'WRFCOIN_HSM_MODULE_PATH', 'WRFCOIN_HSM_KEY_LABEL', 'WRFCOIN_HSM_PUBKEY_LABEL', 'WRFCOIN_HSM_PIN_ENV'}

def identifier(text):
    require(isinstance(text, str) and len(text) == 64 and all((c in '0123456789abcdef' for c in text)), 'invalid public identity')
    return text

def endpoint(text):
    parsed = urlsplit(text)
    require(parsed.scheme in ['http', 'https'] and parsed.hostname and (not parsed.username) and (not parsed.password) and (not parsed.query) and (not parsed.fragment), 'invalid public endpoint')
    return text.rstrip('/')

def validate_testnet_domain(plan):
    chain = plan.get('chain_id')
    require(isinstance(chain, str) and 0 < len(chain) <= 64
            and chain.isascii() and all(c.isalnum() or c in '-_' for c in chain)
            and plan.get('network_id') == 'testnet', 'explicit testnet domain required')

def validate(plan):
    identifier(plan['consumer_deployment_id'])
    identifier(plan['genesis_hash'])
    validate_testnet_domain(plan)
    require(1 <= plan['poll_seconds'] <= 3600 and 1 <= plan['timeout_seconds'] <= 3600, 'required check failed')
    require(isinstance(plan['min_free_bytes'], int) and plan['min_free_bytes'] > 0, 'required check failed')
    for name in ['scratch_root', 'archive_root']:
        path = Path(plan[name])
        require(path.is_absolute() and path.is_dir() and (not path.is_symlink()), 'required check failed')
    require((Path(plan['scratch_root']) / MARKER).is_file(), 'scratch root requires explicit isolated-rehearsal marker')
    for name in ['cli_binary', 'receiver_binary']:
        path = Path(plan[name])
        require(path.is_absolute() and path.is_file(), 'required check failed')
        actual = hashlib.sha256(path.read_bytes()).hexdigest()
        require(actual == plan[name + '_sha256'], 'binary pin mismatch')
    require(Path(plan['receiver_binary']).name == 'launch_api' and Path(plan['cli_binary']).name == 'wrfcoin', 'required check failed')
    require(1 <= len(plan['providers']) <= 3, 'required check failed')
    seen = set()
    for provider in plan['providers']:
        identity = identifier(provider['deployment_id'])
        require(identity != plan['consumer_deployment_id'] and identity not in seen, 'self/duplicate deployment')
        seen.add(identity)
        endpoint(provider['base_url'])
        require(0 < len(provider['service_alias'].encode()) <= 128, 'required check failed')
        require(provider['trusted_signers'], 'required check failed')
        [identifier(s) for s in provider['trusted_signers']]
    return plan

class RefuseRedirects(HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        # Never forward an authenticated request, including to the same origin.
        return None

def read_json(url, credential):
    request = Request(url, headers={'Authorization': 'Bearer ' + credential})
    with build_opener(RefuseRedirects()).open(request, timeout=15) as response:
        body = response.read(256 * 1024 + 1)
        require(len(body) <= 256 * 1024, 'read model exceeds bound')
        return json.loads(body)

def durable(path, body):
    raw = json.dumps(body, sort_keys=True).encode()
    temporary = path.with_name(path.name + '.tmp')
    with temporary.open('xb') as file:
        file.write(raw)
        file.flush()
        os.fsync(file.fileno())
    os.replace(temporary, path)
    descriptor = os.open(path.parent, os.O_RDONLY)
    try:
        os.fsync(descriptor)
    finally:
        os.close(descriptor)

def select_provider(plan, credentials, journal):
    for provider in plan['providers']:
        identity = provider['deployment_id']
        try:
            view = read_json(endpoint(provider['base_url']) + '/node/bonds/' + identity, credentials['servers'][identity])
        except (OSError, ValueError, KeyError):
            continue
        if view.get('status') != 'REGISTERED' or view.get('principal_base_units') != '25000000000' or view.get('jailed') is not False:
            continue
        require(isinstance(view.get('epoch'), str) and view['epoch'].isdigit(), 'epoch unavailable')
        epoch = view['epoch']
        key = identity + ':' + plan['consumer_deployment_id'] + ':' + epoch
        if key in journal:
            return None
        return (provider, epoch, key)
    return None

def receiver_environment(plan, secret_config, directory, port, operator):
    signer = secret_config['receiver_signer_env']
    pin_name = signer.get('WRFCOIN_HSM_PIN_ENV')
    require(isinstance(pin_name, str) and pin_name.startswith('NODE_SHARE_RECEIVER_') and pin_name.endswith('_PIN'), 'dedicated receiver PIN variable required')
    allowed = PUBLIC_SIGNER_ENV | ({pin_name} if isinstance(pin_name, str) and pin_name else set())
    require(set(signer) <= allowed and signer.get('WRFCOIN_HSM_MODULE_PATH') and signer.get('WRFCOIN_HSM_KEY_LABEL'), 'dedicated hardware receiver signer configuration required')
    require(pin_name and pin_name in signer, 'hardware signer credential unavailable')
    storage_key = secret_config.get('receiver_storage_key')
    require(isinstance(storage_key, str) and len(storage_key) == 64 and all(c in '0123456789abcdef' for c in storage_key), 'receiver storage encryption configuration required')
    public = plan.get('receiver_public_env', {})
    require(set(public) <= {'WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS', 'WRFCOIN_ORACLE_GOVERNANCE_PUBKEY_COUNT'}, 'unexpected receiver public configuration')
    require(directory.is_absolute() and not directory.is_symlink() and not (directory / 'database').exists(), 'receiver requires new owned database')
    (directory / '.node-share-rehearsal-receiver').write_text(plan['genesis_hash'])
    home = directory / 'home'
    home.mkdir()
    environment = {'PATH': os.defpath, 'HOME': str(home), 'WRFCOIN_DATA_DIR': str(directory / 'database'), 'WRFCOIN_CHAIN_ID': plan['chain_id'], 'WRFCOIN_NETWORK_ID': plan['network_id'], 'WRFCOIN_GENESIS_HASH': plan['genesis_hash'], 'WRFCOIN_API_HOST': '127.0.0.1', 'WRFCOIN_API_PORT': str(port), 'WRFCOIN_P2P_ENABLED': 'false', 'WRFCOIN_SYNC_ONLY': 'false', 'WRFCOIN_NODE_SHARE_REHEARSAL': 'true', 'WRFCOIN_OPERATOR_TOKEN': operator, 'WRFCOIN_STORAGE_ENCRYPTION_KEY': storage_key, 'WRFCOIN_SNAPSHOT_TRUSTED_SIGNERS': ','.join(s for p in plan.get('providers', []) for s in p['trusted_signers']), 'RUST_LOG': 'error', **public, **signer}
    validate_receiver_environment(environment)
    return environment

def validate_receiver_environment(environment):
    require(environment.get('WRFCOIN_NODE_SHARE_REHEARSAL') == 'true'
        and environment.get('WRFCOIN_SYNC_ONLY') == 'false', 'signing rehearsal refuses sync-only posture')
    require(environment.get('WRFCOIN_P2P_ENABLED') == 'false'
        and environment.get('WRFCOIN_API_HOST') == '127.0.0.1', 'receiver must be isolated from live chain')

def run_once(plan, credentials, journal):
    selected = select_provider(plan, credentials, journal)
    if selected is None:
        return {'status': 'UNAVAILABLE_OR_ALREADY_ARCHIVED', 'included': False}
    provider, epoch, key = selected
    require(shutil.disk_usage(plan['scratch_root']).free >= plan['min_free_bytes'], 'receiver capacity unavailable')
    directory = Path(tempfile.mkdtemp(prefix='receiver-', dir=plan['scratch_root']))
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0))
        port = listener.getsockname()[1]
    operator = secrets.token_urlsafe(32)
    environment = receiver_environment(plan, credentials, directory, port, operator)
    validate_receiver_environment(environment)
    child = subprocess.Popen([plan['receiver_binary']], cwd=directory, env=environment, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    local = 'http://127.0.0.1:' + str(port)
    try:
        deadline = time.monotonic() + plan['timeout_seconds']
        while True:
            require(child.poll() is None, 'owned scratch receiver stopped before installation')
            try:
                read_json(local + '/network/snapshot-sync/receipts', operator)
                status = read_json(local + '/node/sync/status', operator)
                require(status.get('chain_height') == 0 and status.get('blocks_in_memory') == 0, 'receiver must be a fresh empty node')
                break
            except OSError:
                require(time.monotonic() < deadline, 'owned receiver readiness timed out')
                time.sleep(0.25)
        archive = Path(plan['archive_root']) / provider['deployment_id'] / epoch
        archive.mkdir(parents=True, exist_ok=True)
        command = [plan['cli_binary'], '--json', 'snapshot', 'fetch', '--from', endpoint(provider['base_url']), '--to', local, '--trusted-signers', ','.join(provider['trusted_signers']), '--server-id', provider['service_alias'], '--bonded', '--receipt-archive', str(archive), '--auth-stdin']
        auth = {'server_reader': credentials['servers'][provider['deployment_id']], 'receiver_operator': operator}
        completed = subprocess.run(command, cwd=directory, env={'PATH': os.defpath, 'HOME': str(directory / 'home'), 'RUST_LOG': 'error', 'WRFCOIN_CHAIN_ID': plan['chain_id'], 'WRFCOIN_NETWORK_ID': plan['network_id']}, input=json.dumps(auth), text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=plan['timeout_seconds'])
        require(completed.returncode == 0, 'verified receiver installation/signing failed; inspect retained public archives')
        result = json.loads(completed.stdout)
        require(result.get('ok') is True, 'required check failed')
        data = result['data']
        require(data['ingest'].get('applied') is True, 'required check failed')
        claim = data['bonded_claim']
        identifier(claim['receipt_id'])
        require(claim.get('included') is False and claim.get('claim_hex'), 'required check failed')
        require(list(archive.glob('installed-*.json')) and list(archive.glob('bonded-*.json')), 'durable installation/endorsement archive missing')
        record = {'status': 'INSTALLED_SIGNED_NOT_INCLUDED', 'epoch': epoch, 'receipt_id': claim['receipt_id'], 'included': False, 'scratch_database': str(directory / 'database'), 'archive': str(archive)}
        journal[key] = record
        return record
    finally:
        child.terminate()
        try:
            child.wait(timeout=10)
        except subprocess.TimeoutExpired:
            child.kill()
            child.wait(timeout=10)

def main():
    p = argparse.ArgumentParser()
    p.add_argument('--plan', type=Path, required=True)
    p.add_argument('--once', action='store_true')
    args = p.parse_args()
    plan = validate(json.loads(args.plan.read_text()))
    raw = sys_stdin_bounded()
    credentials = json.loads(raw)
    require(isinstance(credentials.get('servers'), dict) and isinstance(credentials.get('receiver_signer_env'), dict), 'receiver credentials unavailable')
    path = Path(plan['archive_root']) / 'rehearsal-journal.json'
    journal = json.loads(path.read_text()) if path.exists() else {}
    while True:
        try:
            record = run_once(plan, credentials, journal)
            durable(path, journal)
            print(json.dumps(record), flush=True)
        except Exception as error:
            print(json.dumps({'status': 'UNAVAILABLE', 'included': False, 'failure_class': type(error).__name__}), flush=True)
            raise SystemExit(1)
        if args.once:
            break
        time.sleep(plan['poll_seconds'])

def sys_stdin_bounded():
    import sys
    body = sys.stdin.buffer.read(16 * 1024 + 1)
    require(len(body) <= 16 * 1024, 'credential input exceeds bound')
    return body
if __name__ == '__main__':
    main()
