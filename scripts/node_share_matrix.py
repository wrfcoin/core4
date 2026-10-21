"""Registered ON corpus runner. Every selected target must execute nonzero tests."""
from __future__ import annotations
import argparse, json, re, subprocess, sys
from pathlib import Path
def require(condition, message):
    if not condition:
        raise AssertionError(message)

ROOT = Path(__file__).resolve().parents[1]
ON_TEST = 'tests::node_share_slice6b::tests::node_share_on_mining_peer_checkpoint_wal_snapshot_and_outage_parity'
REQUIRED_TESTS = [ON_TEST, 'tests::node_receipt_slice3::tests::actual_bonded_receiver_install_endorse_archive_submit_and_extractor_parity', 'p2p_events::tests::producer_applier_and_wal_agree_on_node_bond_lifecycle_1682', 'tests::node_settlement_slice4::tests::close_and_maturity_producer_peer_extractor_wal_checkpoint_and_reorg_parity']
REQUIRED_SHARED = ['node_share_contracts', 'node_settlement_slice4', 'node_fraud_slice5']
EXPECTED = 'tests/integration/fixtures/determinism/expected_node_share_roots.json'

def validate_registration(manifest: dict, root: Path=ROOT) -> list[str]:
    entry = manifest.get('node_share_matrix')
    if not isinstance(entry, dict):
        return ['node-share ON matrix registration missing (Refs #2849 #3527)']
    errors = []
    required = {'package': 'wrfcoin-native-chain', 'binary': 'launch_api', 'features': 'p2p', 'tests': REQUIRED_TESTS, 'shared_targets': REQUIRED_SHARED, 'expected_roots': EXPECTED}
    for key, value in required.items():
        if entry.get(key) != value:
            errors.append(f'node-share ON matrix registration mismatch: {key}')
    source = (root / 'native-chain/src/bin/launch_api/tests/node_share_slice6b.rs').read_text()
    if 'async fn ' + ON_TEST.rsplit('::', 1)[-1] + '(' not in source:
        errors.append('node-share ON fixture absent')
    if 'mod node_share_slice6b;' not in (root / 'native-chain/src/bin/launch_api/main.rs').read_text():
        errors.append('node-share ON fixture not compiled by launch_api')
    workflow = (root / '.github/workflows/determinism-matrix.yml').read_text()
    for marker in ['scripts/emit_determinism_matrix.py', 'node-share-roots-*.json', EXPECTED]:
        if marker not in workflow:
            errors.append(f'node-share actual matrix emitter missing {marker}')
    emitter = (root / 'scripts/emit_determinism_matrix.py').read_text()
    if "run(args.out.with_name('node-share-'" not in emitter:
        errors.append('actual matrix emitter omits the registered ON runner')
    if 'run_node_share_matrix(Path(directory)' not in (root / 'scripts/integration_test_ratchet.py').read_text():
        errors.append('ratchet omits the registered ON runner')
    return errors

HEAVY_BUILD_MIN_FREE_GIB = 40

def heavy_build_min_free_gib(environ=None) -> int:
    """40 GiB unless WRFCOIN_HEAVY_BUILD_MIN_FREE_GIB names a whole number of GiB.
    GitHub-hosted heavy-build preparation sets 10 after reclaiming disk;
    self-hosted runners and local runs leave it unset and keep 40."""
    raw = (__import__('os').environ if environ is None else environ).get('WRFCOIN_HEAVY_BUILD_MIN_FREE_GIB', '')
    if raw == '':
        return HEAVY_BUILD_MIN_FREE_GIB
    require(re.fullmatch('[1-9][0-9]{0,3}', raw) is not None, 'WRFCOIN_HEAVY_BUILD_MIN_FREE_GIB must be a whole number of GiB')
    return int(raw)

def execute(command: list[str]) -> str:
    if command[0] in ['cargo', 'cross']:
        minimum = heavy_build_min_free_gib()
        free = __import__('shutil').disk_usage(ROOT).free
        print(f'NODE_SHARE_DISK_GUARD free_gib={free / 1024**3:.1f} min_gib={minimum}', flush=True)
        require(free >= minimum * 1024**3, f'heavy build disk guard: less than {minimum} GiB free')
    print('NODE_SHARE_REGISTERED_COMMAND ' + json.dumps(command), flush=True)
    completed = subprocess.run(command, cwd=ROOT, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    output = completed.stdout
    print(output, end='', flush=True)
    counts = re.findall('test result: (?:ok|FAILED)\\. (\\d+) passed; (\\d+) failed;', output)
    passed = sum((int(row[0]) for row in counts))
    failed = sum((int(row[1]) for row in counts))
    require(completed.returncode == 0 and passed > 0 and (failed == 0), f'VOID/RED registered target: returncode={completed.returncode} passed={passed} failed={failed}')
    print(f'NODE_SHARE_RUNTIME_NONZERO passed={passed} failed={failed}', flush=True)
    return output

def run(out: Path, profile: str='dev', target: str | None=None, cross: bool=False, write_golden: bool=False) -> dict[str, str]:
    manifest = json.loads((ROOT / 'scripts/integration_test_ratchet.json').read_text())
    errors = validate_registration(manifest)
    require(not errors, errors)
    common = ['cross' if cross else 'cargo', 'test', '--locked']
    if profile == 'release':
        common += ['--release']
    if target:
        common += ['--target', target]
    shared = common + ['-p', 'wrfcoin-shared-protocol']
    for name in manifest['node_share_matrix']['shared_targets']:
        execute(shared + ['--test', name, '--', '--test-threads=1'])
    roots = None
    for name in manifest['node_share_matrix']['tests']:
        output = execute(common + ['-p', 'wrfcoin-native-chain', '--bin', 'launch_api', '--features', 'p2p', name, '--', '--exact', '--nocapture'])
        values = [json.loads(line.removeprefix('NODE_SHARE_MATRIX_ROOTS ')) for line in output.splitlines() if line.startswith('NODE_SHARE_MATRIX_ROOTS ')]
        if name == ON_TEST:
            require(len(values) == 1, 'ON fixture must emit exactly one root document')
            roots = values[0]
    require(isinstance(roots, dict) and len(roots) == 3, 'ON fixture missing or zero steps')
    require(all((str(h).isdigit() and isinstance(v, str) and re.fullmatch('[0-9a-f]{64}', v) for h, v in roots.items())), 'required check failed')
    expected = ROOT / EXPECTED
    if write_golden:
        require(not expected.exists(), 'preserve existing ON golden; do not regenerate to hide a divergence')
        expected.write_text(json.dumps({h:list(bytes.fromhex(v)) for h,v in roots.items()}, indent=2, sort_keys=True) + '\n')
    else:
        require(roots == {h:bytes(v).hex() for h,v in json.loads(expected.read_text()).items()}, 'ON roots differ from the committed expected corpus')
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps({h:list(bytes.fromhex(v)) for h,v in roots.items()}, indent=2, sort_keys=True) + '\n')
    print(f'NODE_SHARE_MATRIX_REAL_GREEN targets={len(REQUIRED_TESTS) + len(REQUIRED_SHARED)} roots={len(roots)} out={out}', flush=True)
    return roots

def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--profile', choices=['dev', 'release'], default='dev')
    parser.add_argument('--target')
    parser.add_argument('--cross', action='store_true')
    parser.add_argument('--write-golden', action='store_true')
    args = parser.parse_args()
    run(args.out, args.profile, args.target, args.cross, args.write_golden)
    return 0
if __name__ == '__main__':
    raise SystemExit(main())
