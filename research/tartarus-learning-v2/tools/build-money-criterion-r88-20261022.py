"""Finite offline combined Rust qualification; never changes production state."""
from pathlib import Path
import datetime
import hashlib
import json
import os
import subprocess
import sys

ROOT = Path('/tmp/moloch-money-criterion-r88-20261022')
NATIVE = Path('/tmp/moloch-source-projection-r84-20261022')
RUNTIME = Path('/tmp/moloch-local-guard-cohort-r85-20261022')
WORKER = Path('/tmp/moloch-cohort-index-r87-20261022')
RUST = '/home/archon/.rustup/toolchains/stable-aarch64-unknown-linux-gnu/bin/rustc'


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    Path('/proc/self/oom_score_adj').write_text('1000')
    os.nice(15)
    source = ROOT / 'source'
    expected = json.loads((ROOT / 'source-hashes.json').read_bytes())
    for name, value in expected.items():
        assert sha(source / name) == value, name
    changed = []
    for path in source.rglob('*'):
        if path.is_file():
            name = path.relative_to(source)
            prior = WORKER / 'source' / name
            if not prior.exists() or sha(prior) != sha(path):
                changed.append(str(name))
    assert set(changed) == set(expected), changed
    checks = {}
    stages = [(NATIVE, 'native-tests-build', 'native', True),
              (NATIVE, 'native-release', 'native', False),
              (RUNTIME, 'runtime-fixture', 'live-runtime', False),
              (RUNTIME, 'runtime-release', 'live-runtime', False),
              (WORKER, 'worker-tests', 'live-runtime', True),
              (WORKER, 'worker-release', 'live-runtime', False)]
    assert sys.argv[1:] in ([], ['--resume-runtime'])
    if sys.argv[1:]:
        retained = json.loads((ROOT / 'native-completed.json').read_bytes())
        for name, value in retained['fileHashes'].items():
            assert sha(ROOT / name) == value, name
        for name, value in retained['nativeSourceHashes'].items():
            assert sha(source / name) == value, name
        checks['native-tests-build'] = retained['nativeChecks']
        assert '140 passed; 0 failed' in checks['native-tests-build']
        stages = stages[2:]
    for origin, name, crate, test in stages:
        args = json.loads((origin / (name + '-command.json')).read_bytes())
        for i, value in enumerate(args):
            for old in (NATIVE, RUNTIME, WORKER):
                value = value.replace(str(old / 'source'), str(source))
            for library in ('libmoloch_replay_release.rlib',
                            'libmoloch_live_runtime_release.rlib',
                            'libmoloch_live_runtime_fixture.rlib'):
                if '=' in value and value.endswith('/' + library):
                    value = value.split('=', 1)[0] + '=' + str(ROOT / library)
            args[i] = value
        output = ROOT / Path(args[args.index('-o') + 1]).name
        args[args.index('-o') + 1] = str(output)
        args = ['-L', 'dependency=' + str(ROOT), *args]
        (ROOT / (name + '-command.json')).write_text(json.dumps(args))
        with (ROOT / (name + '-build.log')).open('w') as log:
            subprocess.run([RUST, *args], env=dict(os.environ,
                CARGO_MANIFEST_DIR=str(source / 'moloch' / crate)),
                stdout=log, stderr=subprocess.STDOUT, check=True)
        print(name, 'compiled', flush=True)
        if test:
            with (ROOT / (name + '-results.log')).open('w') as log:
                subprocess.run([str(output), '--test-threads=1'],
                    stdout=log, stderr=subprocess.STDOUT, check=True)
            result = (ROOT / (name + '-results.log')).read_text()
            assert '0 failed;' in result
            checks[name] = result.splitlines()[-2]
            print(checks[name], flush=True)
        if name == 'native-release':
            retained = dict(nativeChecks=checks['native-tests-build'],
                nativeSourceHashes={str(p.relative_to(source)): sha(p)
                    for p in (source / 'moloch/native').rglob('*') if p.is_file()},
                fileHashes={name: sha(ROOT / name) for name in
                    ('libmoloch_replay_release.rlib', 'native-tests')})
            (ROOT / 'native-completed.json').write_text(json.dumps(retained, indent=2))
    binary = ROOT / 'moloch-live-runtime'
    cli = subprocess.run([str(binary)], capture_output=True, text=True)
    assert 'usage:' in (cli.stdout + cli.stderr).lower()
    proof = dict(observedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),
        sourceHashes=expected, changedSourcePaths=changed, checks=checks,
        binarySha256=sha(binary), nativeLibrarySha256=sha(ROOT / 'libmoloch_replay_release.rlib'),
        runtimeLibrarySha256=sha(ROOT / 'libmoloch_live_runtime_release.rlib'),
        runtimeFixtureSha256=sha(ROOT / 'libmoloch_live_runtime_fixture.rlib'),
        productionChanged=False, accountClientsCreated=0, cliLoads=True,
        endToEndSpeedupEstablished=False, oneFilledTradePerMinuteEstablished=False)
    (ROOT / 'qualified.public.json').write_text(json.dumps(proof, indent=2) + '\n')
    print(json.dumps(proof), flush=True)


if __name__ == '__main__':
    main()
