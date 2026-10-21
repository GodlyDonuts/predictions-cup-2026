"""Offline R78 exit scheduling qualification against the unchanged deployed native engine.

Prepare from R77 with only the FIFO hint classification, scheduler and tests. No production files or API are touched.
"""
from pathlib import Path
import argparse
import datetime
import hashlib
import json
import os
import subprocess

L = Path('/tmp/moloch-exit-priority-r78-20261021')
OLD = Path('/tmp/moloch-closed-index-r77-20261021')
H = Path('/tmp/moloch-recurring-cohort-20261021')
RUST = '/home/archon/.rustup/toolchains/stable-aarch64-unknown-linux-gnu/bin/rustc'


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main(resume=False):
    native = H / 'libmoloch_replay_release.rlib'
    assert sha(native) == 'fb56b89e5a6af2788baf4d858fcca02e675f0752a3cabf355aac791b767ba287'
    env = dict(os.environ, CARGO_MANIFEST_DIR=str(L / 'source/moloch/live-runtime'))
    results = []
    for name, binary in [('runtime-fixture', None), ('fifo', 'fundamental_fifo_profit_exits-tests'), ('fundamental', 'fundamental_policy-tests'),
                         ('runtime-release', None), ('worker-test', 'worker-tests'),
                         ('worker-release', None)]:
        if resume and name == 'runtime-fixture':
            assert (L / 'libmoloch_live_runtime_fixture.rlib').exists()
            continue
        template = 'fundamental' if name == 'fifo' else name
        args = [s.replace(str(OLD), str(L)) for s in json.loads((OLD / (template + '-command.json')).read_bytes())]
        if name == 'fifo':
            args = [s.replace('fundamental_policy.rs', 'fundamental_fifo_profit_exits.rs').replace('fundamental_policy-tests', 'fundamental_fifo_profit_exits-tests') for s in args]
        (L / (name + '-command.json')).write_text(json.dumps(args))
        with (L / (name + '-build.log')).open('w') as log:
            subprocess.run(['nice', '-n', '10', RUST, *args], env=env, stdout=log,
                           stderr=subprocess.STDOUT, check=True)
        print(name, 'compiled', flush=True)
        if binary:
            with (L / (name + '-results.log')).open('w') as log:
                subprocess.run([str(L / binary), '--test-threads=2'], stdout=log,
                               stderr=subprocess.STDOUT, check=True)
            summary = (L / (name + '-results.log')).read_text().splitlines()[-2]
            results.append(summary)
            print(summary, flush=True)
    binary = L / 'moloch-live-runtime'
    cli = subprocess.run([str(binary)], text=True, capture_output=True)
    assert 'usage:' in (cli.stdout + cli.stderr).lower()
    proof = dict(observedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),
                 binarySha256=sha(binary), runtimeLibrarySha256=sha(L / 'libmoloch_live_runtime_release.rlib'),
                 nativeLibrarySha256=sha(native), cliLoads=True,
                 sourceSha256={str(p.relative_to(L / 'source')): sha(p) for p in [
                     L / 'source/moloch/live-runtime/src/fundamental_policy.rs',
                     L / 'source/moloch/live-runtime/tests/fundamental_policy.rs',
                     L / 'source/moloch/live-runtime/tests/fundamental_fifo_profit_exits.rs',
                     L / 'source/moloch/live-runtime/src/worker.rs', L / 'source/research/moloch-fundamental-closure-cache-20261017/tests.rs']},
                 tests=results, nativeEngineChanged=False, accountClientsCreated=0, productionChanged=False)
    (L / 'qualified.public.json').write_text(json.dumps(proof, indent=2) + '\n')
    print(json.dumps(proof), flush=True)


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--resume', action='store_true', help='Use only after a test-only correction')
    main(parser.parse_args().resume)
