"""Compile the worker-only diagnostic move; never stop or replace production."""
from pathlib import Path
import hashlib
import json
import os
import shutil
import subprocess
import time

ROOT = Path('/tmp/moloch-entry-diagnostic-r102-20261022')
PRIOR = Path('/tmp/moloch-preintent-orders-r101-20261022')
NATIVE = Path('/tmp/moloch-upgrade-bound-r100-20261022')
RUST = '/home/archon/.rustup/toolchains/stable-aarch64-unknown-linux-gnu/bin/rustc'


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    os.nice(15)
    Path('/proc/self/oom_score_adj').write_text('1000')
    patch = json.loads((ROOT/'worker-patch.json').read_bytes())
    prior = json.loads((PRIOR/'qualified.public.json').read_bytes())
    shutil.copytree(PRIOR/'source', ROOT/'source')
    path = ROOT/'source'/patch['path']
    assert sha(path) == patch['oldSha256']
    lines = path.read_text(encoding='utf-8').splitlines(keepends=True)
    for change in reversed(patch['changes']):
        assert lines[change['from']:change['to']] == change['old']
        lines[change['from']:change['to']] = change['new']
    path.write_text(''.join(lines), encoding='utf-8')
    assert sha(path) == patch['newSha256']
    changed = [str(p.relative_to(ROOT/'source')) for p in (ROOT/'source').rglob('*')
               if p.is_file() and sha(p) != sha(PRIOR/'source'/p.relative_to(ROOT/'source'))]
    assert changed == [patch['path']]
    libraries = {
        str(PRIOR/'libmoloch_live_runtime_release.rlib'): prior['runtimeLibrarySha256'],
        str(PRIOR/'libmoloch_live_runtime_fixture.rlib'): prior['runtimeFixtureSha256'],
        str(NATIVE/'libmoloch_replay_release.rlib'): prior['nativeLibrarySha256'],
    }
    for name, digest in libraries.items():
        assert sha(Path(name)) == digest
    tmp = Path('/dev/shm/r102-entry')
    tmp.mkdir(mode=0o700, exist_ok=True)
    checks = {}
    for name in ('worker-tests', 'worker-release'):
        args = json.loads((PRIOR/(name+'-command.json')).read_bytes())
        args = [x.replace(str(PRIOR/'source'), str(ROOT/'source')) for x in args]
        args[args.index('-o')+1] = str(ROOT/('worker-tests' if name == 'worker-tests' else 'moloch-live-runtime'))
        (ROOT/(name+'-command.json')).write_text(json.dumps(args))
        started = time.monotonic()
        with (ROOT/(name+'-build.log')).open('x') as log:
            subprocess.run([RUST, *args], check=True, stdout=log, stderr=subprocess.STDOUT,
                env=dict(os.environ, CARGO_MANIFEST_DIR=str(ROOT/'source/moloch/live-runtime')))
        if name == 'worker-tests':
            with (ROOT/'worker-tests-results.log').open('x') as log:
                subprocess.run([str(ROOT/'worker-tests'), '--test-threads=1'], check=True,
                    stdout=log, stderr=subprocess.STDOUT, env=dict(os.environ, TMPDIR=str(tmp)))
            summary = next(line for line in (ROOT/'worker-tests-results.log').read_text().splitlines()
                           if line.startswith('test result:'))
            assert '271 passed; 0 failed;' in summary
            checks[name] = summary
        print(name, 'completed', round(time.monotonic()-started, 3), flush=True)
    cli = subprocess.run([str(ROOT/'moloch-live-runtime')], capture_output=True, text=True)
    assert 'usage:' in (cli.stdout+cli.stderr).lower()
    for name, digest in libraries.items():
        assert sha(Path(name)) == digest
    proof = dict(sourceCommit=patch['sourceCommit'], changedSources={patch['path']:sha(path)},
        checks=checks, reusedUnchangedLibraries=libraries, binarySha256=sha(ROOT/'moloch-live-runtime'),
        cliLoads=True, deployed=False, workerReplaced=False, newAccountClients=0,
        originalFactClocksChanged=False, originalDurabilityChanged=False,
        entryApprovalChecksChanged=False, profitabilityEstablished=False)
    with (ROOT/'qualified.public.json').open('x') as file:
        json.dump(proof, file, indent=2)
    print('QUALIFIED_UNDEPLOYED', flush=True)


if __name__ == '__main__':
    main()
