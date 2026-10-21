#!/usr/bin/env python3
"""Interleave exact test binaries under identical bounded CPU contention."""
import argparse, concurrent.futures, hashlib, json, os, pathlib, subprocess, sys, time

p = argparse.ArgumentParser()
p.add_argument('--before', required=True)
p.add_argument('--after')
p.add_argument('--iterations', type=int, default=50)
p.add_argument('--parallel', type=int, default=8)
p.add_argument('--hogs', type=int, default=8)
p.add_argument('--cpus', type=int, default=4)
p.add_argument('--output', required=True)
p.add_argument('--log-level', default='wrfcoin_p2p=debug')
a = p.parse_args()
if any(value <= 0 for value in (a.iterations, a.parallel, a.hogs, a.cpus)):
    p.error('iterations, parallel, hogs and cpus must be positive')
out = pathlib.Path(a.output)
out.mkdir(parents=True, exist_ok=True)
if any(out.iterdir()):
    p.error('output directory must be empty to avoid mixing evidence from different runs')
cpus = sorted(os.sched_getaffinity(0))[:a.cpus]
test = 'tests::two_node::two_nodes_exchange_messages_with_tcp_bootstrap_and_quic_enabled'
metadata = {'cpus': cpus, 'parallel': a.parallel, 'hogs': a.hogs, 'iterations_each': a.iterations, 'test': test}
for label, binary in [('before', a.before), ('after', a.after)]:
    if binary:
        metadata[label] = {'path': binary, 'sha256': hashlib.sha256(pathlib.Path(binary).read_bytes()).hexdigest()}
(out / 'metadata.json').write_text(json.dumps(metadata, indent=2) + '\n')

def run(label, index, binary):
    started = time.monotonic()
    with (out / f'{label}-{index:03d}.log').open('w') as log:
        try:
            result = subprocess.run(['taskset', '-c', ','.join(map(str, cpus)), binary, test, '--exact', '--nocapture', '--test-threads=1'],
                                    stdout=log, stderr=subprocess.STDOUT, timeout=120,
                                    env={**os.environ, 'RUST_LOG': a.log_level})
            code = result.returncode
        except subprocess.TimeoutExpired:
            code = 124
    text = (out / f'{label}-{index:03d}.log').read_text()
    return {'label': label, 'iteration': index, 'exit_code': code,
            'passed': code == 0 and f'test {test} ...' in text and 'test result: ok. 1 passed; 0 failed;' in text,
            'seconds': round(time.monotonic() - started, 3)}

hogs = [subprocess.Popen(['taskset', '-c', ','.join(map(str, cpus)), sys.executable, '-c', 'import hashlib\nwhile True: hashlib.sha256(b"3525 synthetic CPU contention" * 8192).digest()'],
                        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL) for _ in range(a.hogs)]
rows = []
try:
    with concurrent.futures.ThreadPoolExecutor(max_workers=a.parallel) as pool:
        for batch, start in enumerate(range(0, a.iterations, a.parallel)):
            labels = [('before', a.before)] + ([('after', a.after)] if a.after else [])
            if batch % 2:
                labels.reverse()
            for label, binary in labels:
                futures = [pool.submit(run, label, i, binary) for i in range(start, min(start+a.parallel, a.iterations))]
                for future in futures:
                    row = future.result()
                    rows.append(row)
                    with (out / 'results.jsonl').open('a') as result_file:
                        result_file.write(json.dumps(row) + '\n')
                    print(json.dumps(row), flush=True)
finally:
    for hog in hogs:
        hog.terminate()
    for hog in hogs:
        hog.wait()
summary = {label: {'iterations': sum(r['label']==label for r in rows),
                   'failures': sum(r['label']==label and not r['passed'] for r in rows)}
           for label in ['before', 'after'] if any(r['label']==label for r in rows)}
(out / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n')
print(json.dumps(summary), flush=True)
