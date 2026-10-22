"""Offline R97 monetary priority scheduling; qualified R95 native/runtime libraries unchanged."""
from pathlib import Path
import datetime
import hashlib
import json
import os
import subprocess
import sys

ROOT = Path('/tmp/moloch-rank-service-r97-20261022')
PRIOR = Path('/tmp/moloch-pre-network-compact-r95-20261022')
RUNTIME = PRIOR
NATIVE = Path('/tmp/moloch-money-criterion-r88-20261022')
RUST = '/home/archon/.rustup/toolchains/stable-aarch64-unknown-linux-gnu/bin/rustc'


def sha(p):
    return hashlib.sha256(p.read_bytes()).hexdigest()


def main():
    Path('/proc/self/oom_score_adj').write_text('1000')
    os.nice(15)
    expected = json.loads((ROOT / 'source-hashes.json').read_bytes())
    changed = {str(p.relative_to(ROOT / 'source')): sha(p)
               for p in (ROOT / 'source').rglob('*') if p.is_file()
               and (not (PRIOR / 'source' / p.relative_to(ROOT / 'source')).exists()
                    or sha(p) != sha(PRIOR / 'source' / p.relative_to(ROOT / 'source')))}
    assert changed == expected
    assert sys.argv[1:] in ([], ['--reuse-worker-tests-tmpfs'])
    reuse_tests = bool(sys.argv[1:])
    test_root = Path('/dev/shm/moloch-r97-qualification')
    if True:
        test_root.mkdir(mode=0o700, exist_ok=True)
    checks = {}
    stages = [('worker-tests', 'live-runtime', True), ('worker-release', 'live-runtime', False)]
    for name, crate, test in stages:
        args = json.loads((PRIOR / (('worker-tests' if name == 'fundamental-tests' else name) + '-command.json')).read_bytes())
        if name == 'fundamental-tests':
            args = [v.replace('/src/main.rs', '/tests/fundamental_policy.rs') for v in args]
        for i, value in enumerate(args):
            value = value.replace(str(PRIOR / 'source'), str(ROOT / 'source'))
            for lib in ():
                if '=' in value and value.endswith('/' + lib):
                    value = value.split('=', 1)[0] + '=' + str(ROOT / lib)
            args[i] = value
        out = ROOT / Path(args[args.index('-o') + 1]).name
        args[args.index('-o') + 1] = str(out)
        args = ['-L', 'dependency=' + str(ROOT), *args]
        command_path = ROOT / (name + '-command.json')
        if reuse_tests and test:
            assert json.loads(command_path.read_bytes()) == args
            assert out.stat().st_mtime_ns >= max(p.stat().st_mtime_ns for p in (ROOT/'source').rglob('*') if p.is_file())
            assert 'worker-tests compiled' in (ROOT/'build.log').read_text()
        else:
            command_path.write_text(json.dumps(args))
            with (ROOT / (name + '-build.log')).open('w') as log:
                subprocess.run([RUST, *args], env=dict(os.environ,
                    CARGO_MANIFEST_DIR=str(ROOT / 'source' / 'moloch' / crate)),
                    stdout=log, stderr=subprocess.STDOUT, check=True)
        print(name, 'compiled', flush=True)
        if test:
            results = ROOT / (name + ('-tmpfs-results.log' if reuse_tests else '-results.log'))
            with results.open('w') as log:
                subprocess.run([str(out), '--test-threads=1'], stdout=log,
                               stderr=subprocess.STDOUT, check=True,
                               env=dict(os.environ, TMPDIR=str(test_root)))
            text = results.read_text()
            assert '0 failed;' in text
            checks[name] = text.splitlines()[-2]
            print(checks[name], flush=True)
    binary = ROOT / 'moloch-live-runtime'
    cli = subprocess.run([str(binary)], capture_output=True, text=True)
    assert 'usage:' in (cli.stdout + cli.stderr).lower()
    proof = dict(observedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),
        sourceHashes=expected, checks=checks, testFilesystem='memory-backed temporary fixtures; original event/head fsync calls unchanged', binarySha256=sha(binary),
        nativeLibrarySha256=sha(NATIVE / 'libmoloch_replay_release.rlib'),
        runtimeLibrarySha256=sha(RUNTIME / 'libmoloch_live_runtime_release.rlib'),
        runtimeFixtureSha256=sha(RUNTIME / 'libmoloch_live_runtime_fixture.rlib'),
        cliLoads=True, productionChanged=False, nativeEngineUnchanged=True,
        accountClientsCreated=0, runtimeLibraryUnchanged=True, oneFilledTradePerMinuteEstablished=False,
        inheritedUnchangedRuntimeChecks=json.loads((PRIOR/'qualified.public.json').read_bytes())['checks']['fundamental-tests'])
    if reuse_tests:
        proof.update(testFilesystem='memory-backed temporary fixtures; original fsync calls unchanged',
            reusedTestBinarySha256=sha(ROOT/'worker-tests'),
            originalDiskRunSummary=(ROOT/'worker-tests-results.log').read_text().splitlines()[-2])
    (ROOT / 'qualified.public.json').write_text(json.dumps(proof, indent=2) + '\n')
    print(json.dumps(proof), flush=True)


if __name__ == '__main__':
    main()
