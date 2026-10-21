"""Source-check the pinned B3 operation surface; stdlib only, no node requests."""
import argparse
import json
from pathlib import Path
import re
import subprocess
import tomllib

PIN = '65134a644921'
RELEASE_FEATURES = ['full-mining', 'hsm', 'p2p', 'upgrade-notice-fetch']
HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
FILES = {
    'routes': 'native-chain/src/bin/launch_api/routes/mod.rs',
    'cargo': 'native-chain/Cargo.toml',
    'mount': 'native-chain/src/bin/launch_api/main.rs',
}
METHODS = {'get', 'post', 'put', 'delete', 'patch', 'head', 'options', 'trace'}
TOKEN = re.compile(r'//[^\n]*|/\*[\s\S]*?\*/|"(?:\\.|[^"\\])*"|[A-Za-z_][A-Za-z_0-9]*|::|[^\s]')


def tokens(source):
    return [m.group() for m in TOKEN.finditer(source) if not m.group().startswith(('//', '/*'))]


def closing(t, start):
    pairs = {'(': ')', '[': ']', '{': '}'}
    stack = []
    for i in range(start, len(t)):
        if t[i] in pairs:
            stack.append(pairs[t[i]])
        elif t[i] in pairs.values():
            if not stack or stack.pop() != t[i]:
                raise ValueError('unbalanced registration syntax')
            if not stack:
                return i
    raise ValueError('unterminated registration syntax')


def enabled(t, features):
    def expression(i):
        if t[i:i+2] == ['feature', '=']:
            return json.loads(t[i+2]) in features, i+3
        if t[i] == 'test':
            return False, i+1
        name = t[i]
        if name not in {'any', 'all', 'not'} or t[i+1] != '(':
            raise ValueError('unsupported cfg expression')
        end = closing(t, i+1)
        values, j = [], i+2
        while j < end:
            value, j = expression(j)
            values.append(value)
            if j < end:
                if t[j] != ',':
                    raise ValueError('unsupported cfg separator')
                j += 1
        if name == 'not' and len(values) != 1:
            raise ValueError('not requires one cfg argument')
        return (any(values) if name == 'any' else all(values) if name == 'all' else not values[0]), end+1
    value, end = expression(0)
    if end != len(t):
        raise ValueError('unparsed cfg tokens')
    return value


def collect(t, prefix=''):
    result, i = [], 0
    while i < len(t):
        if t[i:i+4] == ['web', '::', 'scope', '(']:
            end = closing(t, i+3)
            if end != i+5 or not t[i+4].startswith('"'):
                raise ValueError('scope must have one literal prefix')
            prefix += json.loads(t[i+4])
            i = end+1
        elif t[i:i+3] == ['.', 'service', '(']:
            end = closing(t, i+2)
            child = t[i+3:end]
            if child[:4] != ['web', '::', 'scope', '(']:
                raise ValueError('unsupported child service registration')
            result += collect(child, prefix)
            i = end+1
        elif t[i:i+3] == ['.', 'route', '(']:
            end = closing(t, i+2)
            arg = t[i+3:end]
            if len(arg) < 12 or not arg[0].startswith('"') or arg[1:4] != [',', 'web', '::'] or arg[4] not in METHODS or arg[5:10] != ['(', ')', '.', 'to', '(']:
                raise ValueError('unsupported route registration')
            handler_end = closing(arg, 9)
            handler = ''.join(arg[10:handler_end])
            if not re.fullmatch(r'[A-Za-z_]\w*(?:::[A-Za-z_]\w*)+', handler) or arg[handler_end+1:] not in ([], [',']):
                raise ValueError('route handler must be an explicit module function')
            result.append({'method': arg[4], 'path': prefix+json.loads(arg[0]), 'handler': handler})
            i = end+1
        elif t[i:i+2] == ['.', 'configure'] or t[i:i+4] == ['web', '::', 'resource', '(']:
            raise ValueError('unsupported registration form; update the checker explicitly')
        else:
            i += 1
    return result


def parse_routes(source, features):
    t = tokens(source)
    marker = ['fn', 'configure_launch_routes']
    starts = [i for i in range(len(t)-1) if t[i:i+2] == marker]
    if len(starts) != 1:
        raise ValueError('expected one configure_launch_routes function')
    start = t.index('{', starts[0])
    body = t[start+1:closing(t, start)]
    registered, absent, cursor = [], [], 0
    while cursor < len(body):
        end = cursor
        while end < len(body) and body[end] != ';':
            if body[end] in {'(', '[', '{'}:
                end = closing(body, end)
            end += 1
        statement = body[cursor:end]
        cursor = end+1
        condition, selected = [], True
        if statement[:4] == ['#', '[', 'cfg', '(']:
            cfg_end = closing(statement, 3)
            condition = statement[4:cfg_end]
            selected = enabled(condition, features)
            statement = statement[cfg_end+2:]
        if statement[:3] == ['let', 'scope', '=']:
            rows = collect(statement[3:])
            for row in rows:
                row['cfg'] = ''.join(condition) or 'always'
            (registered if selected else absent).extend(rows)
        elif statement != ['cfg', '.', 'service', '(', 'scope', ')']:
            raise ValueError('unsupported router statement; update the checker explicitly')
    keys = [(row['path'], row['method']) for row in registered]
    if not keys or len(keys) != len(set(keys)):
        raise ValueError('empty or duplicate selected route ledger')
    return sorted(registered, key=lambda row: (row['path'], row['method'])), absent


def build_features(build, cargo):
    commands = build.replace('\\\n', ' ').split('&&')
    node = [c for c in commands if re.search(r'cargo\s+build\b', c) and '--bin launch_api' in c]
    if len(node) != 1:
        raise ValueError('expected one launch_api build command')
    matches = re.findall(r'--features\s+"([^"]+)"', node[0])
    if len(matches) != 1:
        raise ValueError('expected explicit literal node build features')
    definitions = tomllib.loads(cargo)['features']
    features = set(matches[0].split(','))
    if '--no-default-features' not in node[0]:
        features.update(definitions.get('default', []))
    pending = list(features)
    while pending:
        feature = pending.pop()
        if feature not in definitions:
            raise ValueError('unknown node feature: '+feature)
        for implied in definitions[feature]:
            if ':' not in implied and '/' not in implied and implied not in features:
                features.add(implied)
                pending.append(implied)
    return sorted(features)


def sources(revision=PIN, working_tree=False):
    if working_tree:
        source = {key: (ROOT/path).read_text() for key, path in FILES.items()}
    else:
        oid = subprocess.check_output(['git', 'rev-parse', '--verify', revision+'^{commit}'], cwd=ROOT, text=True).strip()
        source = {key: subprocess.check_output(['git', 'show', oid+':'+path], cwd=ROOT, text=True) for key, path in FILES.items()}
    source['build'] = (HERE/'deployed-node-build.Dockerfile').read_text()
    return source


def render(source):
    mount = tokens(source['mount'])
    marker = ['let', 'server', '=', 'HttpServer', '::', 'new', '(', 'move', '|', '|', '{']
    starts = [i for i in range(len(mount)) if mount[i:i+len(marker)] == marker]
    if len(starts) != 1:
        raise ValueError('expected one production HttpServer')
    start = starts[0]+len(marker)-1
    end = closing(mount, start)
    call = ['app', '.', 'configure', '(', 'configure_launch_routes', ')']
    if mount[end+1:end+4] != [')', '.', 'bind'] or not any(mount[i:i+len(call)] == call for i in range(start+1, end)):
        raise ValueError('production App no longer mounts configure_launch_routes')
    features = build_features(source['build'], source['cargo'])
    rows, absent = parse_routes(source['routes'], features)
    paths = {}
    for row in rows:
        operation = {
            'operationId': row['method']+'_'+re.sub(r'\W+', '_', row['path']).strip('_'),
            'description': 'Source-registered operation. Handler response schemas and authentication are not checked by this route inventory.',
            'x-handler': row['handler'], 'x-rust-cfg': row['cfg'],
            'parameters': [{'name': name, 'in': 'path', 'required': True, 'schema': {'type': 'string'}, 'description': 'Route parameter; see the handler for parsing and bounds.'} for name in re.findall(r'\{([^}]+)\}', row['path'])],
            'responses': {'default': {'description': 'Handler-defined response; this document checks paths and methods only.'}},
        }
        paths.setdefault(row['path'], {})[row['method']] = operation
    return {
        'openapi': '3.0.3',
        'info': {'title': 'WRFCoin B3 launch_api registered routes', 'version': PIN[:12], 'description': 'Complete source-checked method/path contract for the pinned B3 Dockerfile feature set. Registered does not imply capability availability, successful writes, authorization or public edge access. No response schema or runtime conformance claim.'},
        'servers': [{'url': 'http://localhost:8556', 'description': 'Local node API; no public hostname is selected.'}],
        'x-core4-revision': PIN, 'x-build-features': features,
        'x-build-profile': {'infra-revision': 'wrfcoin/infra@750298cdf20c', 'source': 'infra/docker/Dockerfile.core:43-45', 'fixture': 'deployed-node-build.Dockerfile'},
        'x-excluded-operations': sorted(absent, key=lambda row: (row['path'], row['method'])),
        'paths': paths,
    }


def check(document, source):
    expected = render(source)
    errors = []
    actual_paths = document.get('paths', {})
    actual = {(path, method) for path, item in actual_paths.items() for method in item if method in METHODS}
    registered = {(path, method) for path, item in expected['paths'].items() for method in item}
    errors += ['phantom operation: '+method.upper()+' '+path for path, method in sorted(actual-registered)]
    errors += ['missing operation: '+method.upper()+' '+path for path, method in sorted(registered-actual)]
    for key in ['openapi', 'info', 'servers', 'x-core4-revision', 'x-build-features', 'x-build-profile', 'x-excluded-operations']:
        if document.get(key) != expected[key]:
            errors.append('metadata drift: '+key)
    for path, method in sorted(actual & registered):
        if actual_paths[path][method] != expected['paths'][path][method]:
            errors.append('operation drift: '+method.upper()+' '+path)
    if set(document) != set(expected) or any(set(item)-METHODS for item in actual_paths.values()):
        errors.append('unsupported document fields')
    return errors


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--spec', type=Path, default=HERE/'launch-api.openapi.json')
    parser.add_argument('--revision', default=PIN, help='Read this git tree instead of the release pin; comparison must still match the pinned document.')
    parser.add_argument('--working-tree', action='store_true', help='Check local source edits against the pinned document.')
    parser.add_argument('--emit', type=Path, help='Explicitly write a fresh inventory; never deploys or changes Rust sources.')
    args = parser.parse_args()
    try:
        if args.emit:
            oid = lambda ref: subprocess.check_output(['git', 'rev-parse', '--verify', ref+'^{commit}'], cwd=ROOT, text=True).strip()
            if args.working_tree or oid(args.revision) != oid(PIN):
                raise ValueError('inventory writer only accepts the pinned release tree')
        source = sources(args.revision, args.working_tree)
        if args.emit:
            if build_features(source['build'], source['cargo']) != RELEASE_FEATURES:
                raise ValueError('inventory writer refuses a changed deployment build profile')
            args.emit.write_text(json.dumps(render(source), indent=2)+'\n')
        errors = check(json.loads(args.spec.read_text()), source)
        for error in errors:
            print('ERROR:', error)
        if not errors:
            rows, absent = parse_routes(source['routes'], build_features(source['build'], source['cargo']))
            print(f'PASS: {len(rows)} registered operations; {len(absent)} excluded by release cfg; revision {PIN}; paths/methods only')
        return int(bool(errors))
    except (ValueError, KeyError, OSError, subprocess.CalledProcessError) as error:
        print('ERROR:', error)
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
