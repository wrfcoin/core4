#!/usr/bin/env python3
"""Summarize detect-secrets metadata without reading source files or printing hashes."""
import argparse
from collections import Counter
import json
from pathlib import Path, PurePosixPath
import re
import sys


def summarize(data):
    if not isinstance(data, dict) or not isinstance(data.get('results'), dict):
        raise ValueError('expected a baseline results object')
    directories, detectors, matrix = Counter(), Counter(), Counter()
    files = 0
    for filename, entries in data['results'].items():
        if not isinstance(filename, str) or not filename or not isinstance(entries, list):
            raise ValueError('invalid baseline file record')
        path = PurePosixPath(filename)
        if path.is_absolute() or '..' in path.parts or re.search(r'[\x00-\x1f|`]', filename):
            raise ValueError('invalid baseline path')
        directory = path.parts[0] if len(path.parts) > 1 else '(root)'
        files += 1
        for entry in entries:
            if not isinstance(entry, dict):
                raise ValueError('invalid baseline entry')
            detector = entry.get('type')
            if (not isinstance(detector, str) or not detector
                    or re.search(r'[\x00-\x1f|`]', detector)
                    or entry.get('filename') != filename
                    or type(entry.get('line_number')) is not int
                    or entry['line_number'] < 1
                    or not isinstance(entry.get('hashed_secret'), str)
                    or re.fullmatch(r'[0-9a-f]{40}', entry['hashed_secret']) is None):
                raise ValueError('invalid baseline finding metadata')
            directories[directory] += 1
            detectors[detector] += 1
            matrix[(directory, detector)] += 1
    return {'files': files, 'entries': sum(directories.values()),
            'by_directory': dict(sorted(directories.items())),
            'by_type': dict(sorted(detectors.items())),
            'by_directory_and_type': [{'directory': d, 'type': t, 'entries': n}
                                      for (d, t), n in sorted(matrix.items())]}


def markdown(summary):
    lines = [f"Baseline metadata: {summary['entries']} entries in {summary['files']} files.",
             '', 'Counts are detector findings, not a count of active secrets or proof of safety.']
    for label, key in [('Directory', 'by_directory'), ('Detector type', 'by_type')]:
        lines += ['', f'| {label} | Entries |', '|---|---:|']
        lines += [f'| {name} | {count} |' for name, count in summary[key].items()]
    return '\n'.join(lines) + '\n'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--baseline', type=Path,
                        default=Path(__file__).resolve().parents[1] / '.secrets.baseline')
    parser.add_argument('--format', choices=('markdown', 'json'), default='markdown')
    args = parser.parse_args()
    try:
        report = summarize(json.loads(args.baseline.read_text(encoding='utf-8')))
    except (OSError, ValueError, TypeError):
        # Neither parse diagnostics nor exception repr may echo source input.
        print('ERROR: baseline could not be read or its metadata is invalid', file=sys.stderr)
        return 2
    print(json.dumps(report, indent=2) if args.format == 'json' else markdown(report), end='\n')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
