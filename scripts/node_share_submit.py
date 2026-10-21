#!/usr/bin/env python3
"""Explicitly provisioned owner submission worker for S6b public archives.

Run on the server owner's host, after transferring the consumer's public archive.
No receiver, producer, payout or governance key is inherited by the CLI child.
Existing durable packets always block re-signing; uncertain delivery needs inspection.
"""
import argparse
import hashlib
import json
import os
import stat
import subprocess
from pathlib import Path
from node_share_rehearsal import durable, endpoint, identifier, read_json, require, validate_testnet_domain


def protected_file(path):
    path = Path(path)
    require(path.is_absolute() and not path.is_symlink() and path.is_file(), 'regular protected file required')
    require(stat.S_IMODE(path.stat().st_mode) & 0o177 == 0, 'owner-only file required')
    return path


def address(value):
    # Canonical registration bindings serialize Address as twenty numeric bytes.
    if isinstance(value, list):
        require(len(value) == 20 and all(type(v) is int and 0 <= v <= 255 for v in value), 'invalid account bytes')
        return '0x' + bytes(value).hex()
    require(isinstance(value, str) and value.startswith('0x') and len(value) == 42 and all(c in '0123456789abcdef' for c in value[2:]), 'invalid account address')
    return value


def packet_status(packet, journal, base, token, epoch):
    if not journal.exists():
        return {'status': 'UNCERTAIN_PACKET_REQUIRES_INSPECTION', 'included': False, 'epoch': epoch}
    require(journal.stat().st_size <= 256 * 1024, 'journal exceeds bound')
    result = json.loads(journal.read_text())
    ids = result.get('receipt_ids', [])
    require(0 < len(ids) <= 8, 'journal receipt identifiers missing')
    views = [read_json(base + '/node/receipts/' + identifier(i), token) for i in ids]
    included = all(v.get('included') is True and v.get('status') == 'INCLUDED' and v.get('receipt_id') == i for i, v in zip(ids, views))
    return {'status': 'INCLUDED' if included else 'WAITING_INCLUSION', 'included': included, 'epoch': epoch, 'receipt_ids': ids}


def run_once(plan):
    validate_testnet_domain(plan)
    deployment = identifier(plan['deployment_id'])
    genesis = identifier(plan['genesis_hash'])
    base = endpoint(plan['api_url'])
    cli = Path(plan['cli_binary'])
    require(cli.is_absolute() and cli.is_file() and hashlib.sha256(cli.read_bytes()).hexdigest() == plan['cli_binary_sha256'], 'CLI pin mismatch')
    owner_key = protected_file(plan['owner_key_file'])
    token_file = protected_file(plan['operator_token_file'])
    require(token_file.stat().st_size <= 16 * 1024, 'credential exceeds bound')
    token = token_file.read_text().strip()
    require(token, 'operator credential unavailable')
    view = read_json(base + '/node/bonds/' + deployment, token)
    require(view.get('status') == 'REGISTERED' and view.get('jailed') is False and view.get('principal_base_units') == '25000000000', 'funded registered server unavailable')
    sender = address(plan['sender'])
    require(address(view['registration']['binding']['owner_account']) == sender, 'owner mismatch')
    require(address(view['registration']['binding']['payout_account']) != sender, 'automation refuses owner/payout alias; use off-host signing')
    epoch = view['epoch']
    require(isinstance(epoch, str) and epoch.isdecimal(), 'epoch unavailable')
    archive_root = Path(plan['archive_root'])
    packet_root = Path(plan['packet_root'])
    home = Path(plan['isolated_home'])
    require(all(p.is_absolute() and p.is_dir() and not p.is_symlink() for p in [archive_root, packet_root, home]), 'owned roots required')
    archive = archive_root / deployment / epoch
    if not archive.is_dir():
        return {'status': 'NO_ARCHIVED_CLAIMS', 'included': False, 'epoch': epoch}
    packet = packet_root / (deployment + '-' + epoch + '.json')
    journal = packet_root / (deployment + '-' + epoch + '-status.json')
    if packet.exists():
        return packet_status(packet, journal, base, token, epoch)
    # Do not reuse the sender's nonce while a prior-epoch packet is outstanding.
    packets = [p for p in packet_root.glob(deployment + '-*.json') if not p.name.endswith('-status.json')]
    require(len(packets) <= 128, 'packet scan exceeds bound; archive completed epochs')
    for previous in packets:
        old_epoch = previous.stem.removeprefix(deployment + '-')
        require(old_epoch.isdecimal(), 'malformed packet epoch')
        status = packet_status(previous, previous.with_name(previous.stem + '-status.json'), base, token, old_epoch)
        if not status['included']:
            return status
    command = [str(cli), '--json', 'node', 'receipt', 'submit', '--archive', str(archive), '--epoch', epoch, '--deployment-id', deployment, '--sender', plan['sender'], '--genesis-hash', genesis, '--chain-id', plan['chain_id'], '--network-id', plan['network_id'], '--api-url', base, '--key-file', str(owner_key), '--operator-token-file', str(token_file), '--fee', str(plan['fee_base_units']), '--out', str(packet)]
    require(isinstance(plan['timeout_seconds'], int) and 1 <= plan['timeout_seconds'] <= 3600, 'timeout exceeds bound')
    completed = subprocess.run(command, env={'PATH': os.defpath, 'HOME': str(home), 'RUST_LOG': 'error'}, cwd=packet_root, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, timeout=plan['timeout_seconds'])
    require(completed.returncode == 0, 'submission unavailable; inspect retained public packet')
    response = json.loads(completed.stdout)
    require(response.get('ok') is True and response['data'].get('admitted') is True, 'submission not admitted')
    durable(journal, response['data'])
    return response['data']


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--plan', type=Path, required=True)
    args = parser.parse_args()
    try:
        require(args.plan.stat().st_size <= 16 * 1024, 'plan exceeds bound')
        result = run_once(json.loads(args.plan.read_text()))
        print(json.dumps(result))
    except Exception as error:
        print(json.dumps({'status': 'UNAVAILABLE', 'included': False, 'failure_class': type(error).__name__}))
        raise SystemExit(1)


if __name__ == '__main__':
    main()
