"""Finish the R78 worker-only fairness revision after its base build completes.

Reuses exactly the already checked runtime/native libraries. No production
files, account client or execution service is changed by this offline build.
"""
from pathlib import Path
import datetime
import hashlib
import json
import os
import subprocess
import time

ROOT = Path('/tmp/moloch-exit-priority-r78-20261021')
RUST = '/home/archon/.rustup/toolchains/stable-aarch64-unknown-linux-gnu/bin/rustc'


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    until = time.monotonic() + 900
    proof = ROOT / 'qualified.public.json'
    while not proof.exists():
        assert time.monotonic() < until, 'Base build did not qualify; no source changed'
        time.sleep(10)
    q = json.loads(proof.read_bytes())
    assert len(q['tests']) == 3 and all('0 failed' in t for t in q['tests'])
    for name, expected in q['sourceSha256'].items():
        assert sha(ROOT / 'source' / name) == expected
    assert sha(ROOT / 'libmoloch_live_runtime_release.rlib') == q['runtimeLibrarySha256']
    assert sha(ROOT / 'moloch-live-runtime') == q['binarySha256']
    target = ROOT / 'source/moloch/live-runtime/src/worker.rs'
    pending = ROOT / 'worker.fair-queue.next.rs'
    expected = (ROOT / 'worker.fair-queue.next.sha256').read_text().strip()
    assert sha(pending) == expected
    proof.rename(ROOT / 'pre-fair-queue-qualified.public.json')
    target.write_bytes(pending.read_bytes())
    env = dict(os.environ, CARGO_MANIFEST_DIR=str(ROOT / 'source/moloch/live-runtime'))
    for name in ['worker-test', 'worker-release']:
        args = json.loads((ROOT / (name + '-command.json')).read_bytes())
        with (ROOT / (name + '-fair-queue-build.log')).open('x') as log:
            subprocess.run(['nice', '-n', '10', RUST, *args], env=env, stdout=log,
                           stderr=subprocess.STDOUT, check=True)
        print(name, 'fair queue compiled', flush=True)
        if name == 'worker-test':
            with (ROOT / 'worker-fair-queue-results.log').open('x') as log:
                subprocess.run([str(ROOT / 'worker-tests'), '--test-threads=2'],
                               stdout=log, stderr=subprocess.STDOUT, check=True)
            result = (ROOT / 'worker-fair-queue-results.log').read_text().splitlines()[-2]
            assert 'test result: ok.' in result and '0 failed' in result
            q['tests'][-1] = result
            print(result, flush=True)
    binary = ROOT / 'moloch-live-runtime'
    cli = subprocess.run([str(binary)], text=True, capture_output=True)
    assert 'usage:' in (cli.stdout + cli.stderr).lower()
    assert sha(ROOT / 'libmoloch_live_runtime_release.rlib') == q['runtimeLibrarySha256']
    q.update(observedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),
             binarySha256=sha(binary), finalSharedQueueRevision=True)
    q['sourceSha256']['moloch/live-runtime/src/worker.rs'] = expected
    for name, expected_hash in q['sourceSha256'].items():
        assert sha(ROOT / 'source' / name) == expected_hash
    proof.write_text(json.dumps(q, indent=2) + '\n')
    print(json.dumps(q), flush=True)


if __name__ == '__main__':
    main()
