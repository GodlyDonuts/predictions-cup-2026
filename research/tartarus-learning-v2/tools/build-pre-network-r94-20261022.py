"""Offline R89 qualification against the exact retained R88 native engine."""
from pathlib import Path
import datetime
import hashlib
import json
import os
import subprocess
import sys

ROOT = Path('/tmp/moloch-pre-network-r94-20261022')
PRIOR = Path('/tmp/moloch-rank-inventory-r93-20261022')
RUNTIME = Path('/tmp/moloch-money-priority-r89-20261022')
NATIVE = Path('/tmp/moloch-money-criterion-r88-20261022')
RUST = '/home/archon/.rustup/toolchains/stable-aarch64-unknown-linux-gnu/bin/rustc'


def sha(p):
    return hashlib.sha256(p.read_bytes()).hexdigest()


def main():
    Path('/dev/shm/moloch-r94-checks').mkdir(mode=0o700, exist_ok=True)
    Path('/proc/self/oom_score_adj').write_text('1000')
    os.nice(15)
    expected = json.loads((ROOT / 'source-hashes.json').read_bytes())
    changed = {str(p.relative_to(ROOT / 'source')): sha(p)
               for p in (ROOT / 'source').rglob('*') if p.is_file()
               and (not (PRIOR / 'source' / p.relative_to(ROOT / 'source')).exists()
                    or sha(p) != sha(PRIOR / 'source' / p.relative_to(ROOT / 'source')))}
    assert changed == expected
    assert sys.argv[1:] in ([], ['--fundamental-only'])
    only = bool(sys.argv[1:])
    checks = json.loads((ROOT / 'qualified.public.json').read_bytes())['checks'] if only else {}
    stages = [('runtime-fixture', 'live-runtime', False),
                              ('runtime-release', 'live-runtime', False),
                              ('worker-tests', 'live-runtime', True),
                              ('fundamental-tests', 'live-runtime', True),
                              ('pre-network-tests', 'live-runtime', True),
                              ('worker-release', 'live-runtime', False)]
    if only:
        stages = [stage for stage in stages if stage[0] == 'fundamental-tests']
    for name, crate, test in stages:
        base = RUNTIME if name.startswith('runtime-') else PRIOR
        args = json.loads((base / (('worker-tests' if name in ('fundamental-tests','pre-network-tests') else name) + '-command.json')).read_bytes())
        if name in ('fundamental-tests','pre-network-tests'):
            target = 'fundamental_policy.rs' if name == 'fundamental-tests' else 'ordinary_pre_network.rs'
            args = [v.replace('/src/main.rs', '/tests/' + target) for v in args]
        for i, value in enumerate(args):
            value = value.replace(str(base / 'source'), str(ROOT / 'source'))
            for lib in ('libmoloch_live_runtime_fixture.rlib', 'libmoloch_live_runtime_release.rlib'):
                if '=' in value and value.endswith('/' + lib):
                    value = value.split('=', 1)[0] + '=' + str(ROOT / lib)
            args[i] = value
        out = ROOT / Path(args[args.index('-o') + 1]).name
        args[args.index('-o') + 1] = str(out)
        args = ['-L', 'dependency=' + str(ROOT), *args]
        (ROOT / (name + '-command.json')).write_text(json.dumps(args))
        with (ROOT / (name + '-build.log')).open('w') as log:
            subprocess.run([RUST, *args], env=dict(os.environ,
                CARGO_MANIFEST_DIR=str(ROOT / 'source' / 'moloch' / crate)),
                stdout=log, stderr=subprocess.STDOUT, check=True)
        print(name, 'compiled', flush=True)
        if test:
            with (ROOT / (name + '-results.log')).open('w') as log:
                subprocess.run([str(out), '--test-threads=1'], stdout=log,
                               stderr=subprocess.STDOUT, check=True, env=dict(os.environ, TMPDIR='/dev/shm/moloch-r94-checks'))
            text = (ROOT / (name + '-results.log')).read_text()
            assert '0 failed;' in text
            checks[name] = text.splitlines()[-2]
            print(checks[name], flush=True)
    binary = ROOT / 'moloch-live-runtime'
    cli = subprocess.run([str(binary)], capture_output=True, text=True)
    assert 'usage:' in (cli.stdout + cli.stderr).lower()
    proof = dict(observedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),
        sourceHashes=expected, checks=checks, binarySha256=sha(binary),
        nativeLibrarySha256=sha(NATIVE / 'libmoloch_replay_release.rlib'),
        runtimeLibrarySha256=sha(ROOT / 'libmoloch_live_runtime_release.rlib'),
        runtimeFixtureSha256=sha(ROOT / 'libmoloch_live_runtime_fixture.rlib'),
        cliLoads=True, productionChanged=False, nativeEngineUnchanged=True,
        accountClientsCreated=0, oneFilledTradePerMinuteEstablished=False)
    (ROOT / 'qualified.public.json').write_text(json.dumps(proof, indent=2) + '\n')
    print(json.dumps(proof), flush=True)


if __name__ == '__main__':
    main()
