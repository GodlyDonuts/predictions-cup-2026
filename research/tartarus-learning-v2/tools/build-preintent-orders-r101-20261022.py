"""Qualify selected-row sharing without touching the active official worker."""
from pathlib import Path
import hashlib
import json
import os
import subprocess
import sys
import time

ROOT = Path('/tmp/moloch-preintent-orders-r101-20261022')
PRIOR = Path('/tmp/moloch-upgrade-bound-r100-20261022')
RUST = '/home/archon/.rustup/toolchains/stable-aarch64-unknown-linux-gnu/bin/rustc'


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    assert sys.argv[1:] in ([], ['--resume-after-fixture'])
    os.nice(10)
    Path('/proc/self/oom_score_adj').write_text('1000')
    expected = json.loads((ROOT/'changed-source-hashes.json').read_bytes())
    actual = {str(p.relative_to(ROOT/'source')): sha(p)
              for p in (ROOT/'source').rglob('*') if p.is_file()
              and (not (PRIOR/'source'/p.relative_to(ROOT/'source')).exists()
                   or sha(p) != sha(PRIOR/'source'/p.relative_to(ROOT/'source')))}
    assert actual == expected
    temporary = Path('/dev/shm/r101')
    temporary.mkdir(mode=0o700, exist_ok=True)
    checks = {}
    stages = [('runtime-fixture', None), ('order-tests', ['order_snapshot::tests', '--test-threads=1']),
              ('fundamental-tests', ['--test-threads=1']),
              ('rank-cohort-tests', ['--test-threads=1']),
              ('runtime-release', None), ('worker-release', None)]
    for name, run_args in stages:
        origin = 'runtime-fixture' if name == 'order-tests' else name
        args = json.loads((PRIOR/(origin+'-command.json')).read_bytes())
        for i, value in enumerate(args):
            value = value.replace(str(PRIOR/'source'), str(ROOT/'source'))
            if value.startswith('moloch_live_runtime='):
                value = value.replace(str(PRIOR), str(ROOT))
            if i and args[i-1] == '-o':
                value = str(ROOT/('order-tests' if name == 'order-tests' else Path(value).name))
            args[i] = value
        args = ['-L', 'dependency='+str(ROOT), *args]
        if name == 'order-tests':
            kind = args.index('--crate-type')
            del args[kind:kind+2]
            args.append('--test')
            args.extend(['-C', 'lto=thin', '-C', 'embed-bitcode=yes', '-C', 'codegen-units=1'])
        command = ROOT/(name+'-command.json')
        if name == 'runtime-fixture' and sys.argv[1:]:
            assert json.loads(command.read_bytes()) == args
            assert (ROOT/'libmoloch_live_runtime_fixture.rlib').is_file()
            assert 'runtime-fixture compiled' in (ROOT/'build-retry-001.log').read_text()
            assert 'error:' not in (ROOT/'runtime-fixture-build.log').read_text()
            print('runtime-fixture retained, exact source and command unchanged', flush=True)
            continue
        if command.exists():
            command.rename(ROOT/(name+'-command.original.json'))
        command.write_text(json.dumps(args))
        started = time.monotonic()
        build_log = ROOT/(name+'-build.log')
        if build_log.exists():
            build_log = ROOT/(name+'-build-retry-001.log')
        with build_log.open('x') as log:
            subprocess.run([RUST, *args], check=True, stdout=log, stderr=subprocess.STDOUT,
                           env=dict(os.environ, CARGO_MANIFEST_DIR=str(ROOT/'source/moloch/live-runtime')))
        print(name+' compiled', flush=True)
        if run_args:
            with (ROOT/(name+'-results.log')).open('x') as log:
                subprocess.run([args[args.index('-o')+1], *run_args], check=True,
                               stdout=log, stderr=subprocess.STDOUT,
                               env=dict(os.environ, TMPDIR=str(temporary)))
            summary = next(line for line in (ROOT/(name+'-results.log')).read_text().splitlines()
                           if line.startswith('test result:'))
            assert '0 failed;' in summary and int(summary.split('passed;')[0].split()[-1]) > 0
            checks[name] = summary
            print(summary, flush=True)
        print(name+' elapsedSeconds='+str(round(time.monotonic()-started, 3)), flush=True)
    binary = ROOT/'moloch-live-runtime'
    cli = subprocess.run([str(binary)], capture_output=True, text=True)
    assert 'usage:' in (cli.stdout+cli.stderr).lower()
    proof = dict(sourceHashes=expected, checks=checks, binarySha256=sha(binary),
                 runtimeLibrarySha256=sha(ROOT/'libmoloch_live_runtime_release.rlib'),
                 nativeLibrarySha256=sha(PRIOR/'libmoloch_replay_release.rlib'),
                 nativeLibraryChanged=False, productionChanged=False,
                 accountClientsCreated=0, monetaryCapsChanged=False, cliLoads=True)
    with (ROOT/'qualified.public.json').open('x') as f:
        json.dump(proof, f, indent=2)
        f.write('\n')
    print('QUALIFIED_OFFLINE', flush=True)


if __name__ == '__main__':
    main()
