#!/usr/bin/env python3
"""Public launch runner boundary: untrusted events never reach persistent runners."""
from pathlib import Path
import sys
import yaml

GUARD = "github.event_name == 'push' || github.event_name == 'workflow_dispatch'"
HOSTED = {'ubuntu-latest', 'ubuntu-22.04', 'ubuntu-24.04', 'windows-latest', 'windows-2022', 'macos-latest', 'macos-14', 'macos-15'}


def findings(document):
    errors = []
    events = document.get('on', document.get(True, {})) or {}
    if not isinstance(events, dict):
        events = dict.fromkeys(events if isinstance(events, list) else [events])
    if document.get('permissions', {}).get('contents') != 'read':
        errors.append('workflow must default contents to read')
    if any(v == 'write' for v in document.get('permissions', {}).values()):
        errors.append('write permissions must be job scoped')
    for name, job in document.get('jobs', {}).items():
        runner = job.get('runs-on')
        if runner is None and 'uses' in job:
            if not job['uses'].startswith('./.github/workflows/'):
                errors.append(f'{name}: external reusable runner boundary needs explicit review')
            continue
        if runner == '${{ matrix.os }}':
            matrix = job.get('strategy', {}).get('matrix', {})
            runners = matrix.get('os', []) + [row['os'] for row in matrix.get('include', []) if 'os' in row]
            hosted = bool(runners) and all(x in HOSTED for x in runners)
        else:
            hosted = isinstance(runner, str) and runner in HOSTED
        if hosted:
            continue
        if set(events) - {'push', 'workflow_dispatch'}:
            errors.append(f'{name}: persistent/dynamic runner has untrusted or scheduled trigger')
        if 'push' in events and (events['push'] or {}).get('branches') != ['main']:
            errors.append(f'{name}: persistent runner push is not main-only')
        if job.get('if') != GUARD:
            errors.append(f'{name}: persistent/dynamic runner lacks explicit event guard')
    return errors


def main():
    root = Path(__file__).resolve().parents[1]
    paths = sorted((root / '.github/workflows').glob('*.yml'))
    errors = [(p.name, e) for p in paths for e in findings(yaml.safe_load(p.read_text()))]
    for name, error in errors:
        print(f'{name}: {error}')
    print(f'Public workflow policy: {len(paths)} workflows, {len(errors)} violations')
    return bool(errors)


if __name__ == '__main__':
    sys.exit(main())
