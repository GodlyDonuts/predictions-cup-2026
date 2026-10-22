"""Qualify the R80 worker against the exact unchanged R79 runtime/native libs.

The prepared source tree is copied from R79 with only worker.rs changed.
No production path, account client, control or trading process is changed.
"""
from pathlib import Path
import datetime
import hashlib
import json
import os
import subprocess

ROOT = Path('/tmp/moloch-managed-age-r80-20261022')
PREVIOUS = Path('/tmp/moloch-candidate-sweep-r79-20261022')
LIBRARY = Path('/tmp/moloch-exit-priority-r78-20261021')
RUST = '/home/archon/.rustup/toolchains/stable-aarch64-unknown-linux-gnu/bin/rustc'
WORKER_SHA = 'da5bcd0cb7e600e1bb33adf381f1cdcd4d1f65f74de5fcfd93c868c989559714'


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    prior = json.loads((PREVIOUS / 'qualified.public.json').read_bytes())
    worker = ROOT / 'source/moloch/live-runtime/src/worker.rs'
    assert sha(worker) == WORKER_SHA
    assert sha(LIBRARY / 'libmoloch_live_runtime_release.rlib') == prior['runtimeLibrarySha256']
    assert sha(Path('/tmp/moloch-recurring-cohort-20261021/libmoloch_replay_release.rlib')) == prior['nativeLibrarySha256']
    for source in (PREVIOUS / 'source').rglob('*'):
        if source.is_file():
            relative = source.relative_to(PREVIOUS / 'source')
            if relative.as_posix() != 'moloch/live-runtime/src/worker.rs':
                assert sha(ROOT / 'source' / relative) == sha(source), str(relative)
    env = dict(os.environ, CARGO_MANIFEST_DIR=str(ROOT / 'source/moloch/live-runtime'))
    results = []
    for name, target in [('worker-test', 'worker-tests'), ('worker-release', 'moloch-live-runtime')]:
        args = []
        for arg in json.loads((PREVIOUS / (name + '-command.json')).read_bytes()):
            if arg == str(PREVIOUS / target):
                arg = str(ROOT / target)
            else:
                arg = arg.replace(str(PREVIOUS / 'source'), str(ROOT / 'source'))
            args.append(arg)
        (ROOT / (name + '-command.json')).write_text(json.dumps(args))
        with (ROOT / (name + '-build.log')).open('x') as log:
            subprocess.run(['nice', '-n', '10', RUST, *args], env=env, stdout=log,
                           stderr=subprocess.STDOUT, check=True)
        print(name, 'compiled', flush=True)
        if name == 'worker-test':
            with (ROOT / 'worker-results.log').open('x') as log:
                subprocess.run([str(ROOT / target), '--test-threads=2'], stdout=log,
                               stderr=subprocess.STDOUT, check=True)
            result = (ROOT / 'worker-results.log').read_text().splitlines()[-2]
            assert 'test result: ok.' in result and '0 failed' in result
            results.append(result)
            print(result, flush=True)
    binary = ROOT / 'moloch-live-runtime'
    cli = subprocess.run([str(binary)], text=True, capture_output=True)
    assert 'usage:' in (cli.stdout + cli.stderr).lower()
    proof = dict(observedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),
                 binarySha256=sha(binary), workerSha256=sha(worker), cliLoads=True,
                 runtimeLibrarySha256=prior['runtimeLibrarySha256'],
                 nativeLibrarySha256=prior['nativeLibrarySha256'],
                 workerTests=results, onlyWorkerSourceChanged=True,
                 nativeEngineChanged=False, productionChanged=False,
                 accountClientsCreated=0, ordersSubmitted=0)
    (ROOT / 'qualified.public.json').write_text(json.dumps(proof, indent=2) + '\n')
    print(json.dumps(proof), flush=True)


if __name__ == '__main__':
    main()
