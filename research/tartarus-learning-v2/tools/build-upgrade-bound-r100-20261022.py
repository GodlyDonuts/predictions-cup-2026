"""Finite full R100 qualification of the bounded approval-envelope correction."""
from pathlib import Path
import datetime
import hashlib
import json
import os
import subprocess
import sys

ROOT = Path('/tmp/moloch-upgrade-bound-r100-20261022')
PRIOR = Path('/tmp/moloch-rank-service-r97-20261022')
NATIVE = Path('/tmp/moloch-money-criterion-r88-20261022')
RUNTIME = Path('/tmp/moloch-pre-network-compact-r95-20261022')
RUST = '/home/archon/.rustup/toolchains/stable-aarch64-unknown-linux-gnu/bin/rustc'
LIBRARIES = ('libmoloch_replay_release.rlib', 'libmoloch_live_runtime_release.rlib',
             'libmoloch_live_runtime_fixture.rlib')


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    Path('/proc/self/oom_score_adj').write_text('1000')
    os.nice(15)
    source = ROOT/'source'
    expected = json.loads((ROOT/'source-hashes.json').read_bytes())
    changed = {str(p.relative_to(source)): sha(p) for p in source.rglob('*')
               if p.is_file() and (not (PRIOR/'source'/p.relative_to(source)).exists()
               or sha(p) != sha(PRIOR/'source'/p.relative_to(source)))}
    assert changed == expected, 'SOURCE_BOUNDARY_CHANGED'
    temporary = Path('/dev/shm/r100')
    temporary.mkdir(mode=0o700, exist_ok=True)
    assert sys.argv[1:] in ([], ['--resume-qualified-stages'])
    retained = None
    if sys.argv[1:]:
        retained = json.loads((ROOT/'retained-compiled-stages.json').read_bytes())
        mutable = {'moloch/live-runtime/src/worker.rs',
                   'research/tartarus-learning-v2/tools/build-same-side-handoff-r99-20261022.py'}
        current = {str(p.relative_to(source)):sha(p) for p in source.rglob('*')
                   if p.is_file() and str(p.relative_to(source)) not in mutable}
        assert current == retained['unchangedLibraryAndFixtureSources'], 'RETAINED_LIBRARY_SOURCE_CHANGED'
        if 'worker-tests' in retained['compiled']:
            assert sha(source/'moloch/live-runtime/src/worker.rs') == retained['compiled']['worker-tests']['workerSourceSha256'], 'RETAINED_WORKER_SOURCE_CHANGED'
    checks = {}
    stages = [
        (NATIVE, 'native-tests-build', 'native', 'native-tests', ['--test-threads=1']),
        (NATIVE, 'native-release', 'native', LIBRARIES[0], None),
        (RUNTIME, 'runtime-fixture', 'live-runtime', LIBRARIES[2], None),
        (RUNTIME, 'runtime-release', 'live-runtime', LIBRARIES[1], None),
        (RUNTIME, 'rank-tests', 'live-runtime', 'rank-tests', ['rank_campaign::tests', '--test-threads=1']),
        (RUNTIME, 'fundamental-tests', 'live-runtime', 'fundamental-tests', ['--test-threads=1']),
        (RUNTIME, 'rank-cohort-tests', 'live-runtime', 'rank-cohort-tests', ['--test-threads=1']),
        (PRIOR, 'worker-tests', 'live-runtime', 'worker-tests', ['--test-threads=1']),
        (PRIOR, 'worker-release', 'live-runtime', 'moloch-live-runtime', None),
    ]
    for origin, name, crate, output_name, run_args in stages:
        command_name = ('runtime-fixture' if name == 'rank-tests' else
                        'fundamental-tests' if name == 'rank-cohort-tests' else name)
        args = json.loads((origin/(command_name+'-command.json')).read_bytes())
        for i, value in enumerate(args):
            for old in (PRIOR, NATIVE, RUNTIME):
                value = value.replace(str(old/'source'), str(source))
            for lib in LIBRARIES:
                if '=' in value and value.endswith('/'+lib):
                    value = value.split('=', 1)[0]+'='+str(ROOT/lib)
            args[i] = value
        if name == 'rank-cohort-tests':
            args = [v.replace('/tests/fundamental_policy.rs', '/tests/rank_cohort_ownership.rs') for v in args]
        if name == 'rank-tests':
            at = args.index('--crate-type')
            assert args[at+1] == 'rlib'
            del args[at:at+2]
            args += ['--test', '-C', 'lto=thin']
        output = ROOT/output_name
        args[args.index('-o')+1] = str(output)
        args = ['-L', 'dependency='+str(ROOT), *args]
        reuse = retained and name in retained['compiled']
        if reuse:
            old = retained['compiled'][name]
            assert sha(output) == old['binarySha256']
            assert sha(ROOT/(name+'-command.json')) == old['commandSha256']
            assert json.loads((ROOT/(name+'-command.json')).read_bytes()) == args
        else:
            (ROOT/(name+'-command.json')).write_text(json.dumps(args))
            with (ROOT/(name+'-build.log')).open('x') as log:
                subprocess.run([RUST, *args], env=dict(os.environ,
                    CARGO_MANIFEST_DIR=str(source/'moloch'/crate)),
                    stdout=log, stderr=subprocess.STDOUT, check=True)
        print(name+' compiled', flush=True)
        if run_args:
            if reuse and name in retained['passed']:
                results = ROOT/retained['passed'][name].get('resultsFile', name+'-results.log')
                assert sha(results) == retained['passed'][name]['resultsSha256']
            else:
                results = ROOT/(name+('-retry-short-path-results.log' if reuse else '-results.log'))
                with results.open('x') as log:
                    subprocess.run([str(output), *run_args], env=dict(os.environ, TMPDIR=str(temporary)),
                        stdout=log, stderr=subprocess.STDOUT, check=True)
            result = results.read_text()
            summary = next(line for line in result.splitlines() if line.startswith('test result:'))
            assert '0 failed;' in summary and int(summary.split('passed;', 1)[0].split()[-1]) > 0
            checks[name] = summary
            print(summary, flush=True)
    binary = ROOT/'moloch-live-runtime'
    cli = subprocess.run([str(binary)], capture_output=True, text=True)
    assert 'usage:' in (cli.stdout+cli.stderr).lower()
    proof = dict(observedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),
        sourceHashes=expected, checks=checks, binarySha256=sha(binary),
        nativeLibrarySha256=sha(ROOT/LIBRARIES[0]), runtimeLibrarySha256=sha(ROOT/LIBRARIES[1]),
        runtimeFixtureSha256=sha(ROOT/LIBRARIES[2]), cliLoads=True,
        testFilesystem='memory-backed temporary fixtures; original fsync calls unchanged',
        productionChanged=False, accountClientsCreated=0, monetaryCapsChanged=False,
        conditionalFutureProceedsCredited=False, futureLiquidityCertified=False,
        conditionalMonetaryRotationObjectiveQualified=True, sameSideBuyHandoffQualified=True,
        oneFilledTradePerMinuteEstablished=False,
        retainedCompiledStages=list(retained['compiled']) if retained else [],
        originalHarnessFailureRetained='fundamental-tests-results.log' if retained else None)
    with (ROOT/'qualified.public.json').open('x') as f:
        json.dump(proof, f, indent=2)
        f.write('\n')
    print('QUALIFIED_OFFLINE', flush=True)


if __name__ == '__main__':
    main()
