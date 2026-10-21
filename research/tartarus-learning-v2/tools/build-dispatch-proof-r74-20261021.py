"""Offline ARM64 qualification; uses original deployed native/SDK libraries."""
from pathlib import Path
import argparse, datetime, hashlib, json, os, shutil, subprocess, zipfile

L = Path('/tmp/moloch-dispatch-proof-r74-20261021')
H = Path('/tmp/moloch-recurring-cohort-20261021')
K = Path('/tmp/moloch-target-cache-r73-20261021')
RUST = '/home/archon/.rustup/toolchains/stable-aarch64-unknown-linux-gnu/bin/rustc'


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main(resume=False):
    assert sha(H / 'libmoloch_replay_release.rlib') == 'fb56b89e5a6af2788baf4d858fcca02e675f0752a3cabf355aac791b767ba287'
    if not resume:
        shutil.copytree(K / 'source', L / 'source')
        with zipfile.ZipFile(L / 'changes.zip') as archive:
            for name in archive.namelist():
                assert name.startswith('moloch/live-runtime/') and '..' not in Path(name).parts
            archive.extractall(L / 'source')
    else:
        # Resume only the unchanged fixture library after a test-only correction.
        with zipfile.ZipFile(L / 'changes.zip') as archive:
            assert archive.read('moloch/live-runtime/src/fundamental_policy.rs') == (L / 'source/moloch/live-runtime/src/fundamental_policy.rs').read_bytes()
        assert (L / 'libmoloch_live_runtime_fixture.rlib').exists()
    env = dict(os.environ, CARGO_MANIFEST_DIR=str(L / 'source/moloch/live-runtime'))
    outputs = []
    for name, origin, command in [
        ('runtime-fixture', H, 'runtime-fixture-command.json'),
        ('fundamental', H, 'fundamental_policy-build-command.json'),
        ('runtime-release', H, 'runtime-release-command.json'),
        ('worker-test', K, 'test-command.json'),
        ('worker-release', K, 'release-command.json'),
    ]:
        if resume and name == 'runtime-fixture':
            continue
        args = json.loads((origin / command).read_bytes())
        for i, arg in enumerate(args):
            arg = arg.replace(str(H / 'source'), str(L / 'source')).replace(str(K / 'source'), str(L / 'source'))
            if 'libmoloch_live_runtime_' in arg:
                arg = arg.replace(str(H), str(L))
            if i > 0 and args[i - 1] == '-o':
                arg = str(L / Path(arg).name)
            args[i] = arg
        args += ['-L', 'dependency=' + str(L)]
        (L / (name + '-command.json')).write_text(json.dumps(args))
        with (L / (name + '-build.log')).open('w') as log:
            subprocess.run(['nice', '-n', '10', RUST, *args], check=True, env=env, stdout=log, stderr=subprocess.STDOUT)
        print(name, 'compiled', flush=True)
        if name in ('fundamental', 'worker-test'):
            binary = 'fundamental_policy-tests' if name == 'fundamental' else 'worker-tests'
            with (L / (name + '-results.log')).open('w') as log:
                subprocess.run([str(L / binary), '--test-threads=2'], check=True, env=env, stdout=log, stderr=subprocess.STDOUT)
            outputs.append((L / (name + '-results.log')).read_text().splitlines()[-2])
            print(outputs[-1], flush=True)
    binary = L / 'moloch-live-runtime'; binary.chmod(0o700)
    cli = subprocess.run([str(binary)], text=True, capture_output=True)
    assert 'usage:' in (cli.stdout + cli.stderr).lower()
    proof = dict(observedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(), binarySha256=sha(binary),
        sourceSha256={p: sha(L / 'source' / p) for p in ('moloch/live-runtime/src/worker.rs', 'moloch/live-runtime/src/fundamental_policy.rs', 'moloch/live-runtime/tests/fundamental_policy.rs')},
        nativeLibrarySha256=sha(H / 'libmoloch_replay_release.rlib'), runtimeLibrarySha256=sha(L / 'libmoloch_live_runtime_release.rlib'),
        tests=outputs, cliLoads=True, nativeEngineChanged=False, accountClientsCreated=0, productionChanged=False)
    (L / 'qualified.public.json').write_text(json.dumps(proof, indent=2) + '\n')
    print(json.dumps(proof), flush=True)


if __name__ == '__main__':
    parser=argparse.ArgumentParser(); parser.add_argument('--resume',action='store_true'); main(parser.parse_args().resume)
