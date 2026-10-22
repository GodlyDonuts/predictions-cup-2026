"""Qualify per-call source verification sharing; never replace the live worker."""
from pathlib import Path
import hashlib
import json
import os
import shutil
import subprocess
import time

ROOT = Path('/tmp/moloch-generation-pins-r103-20261022')
PRIOR = Path('/tmp/moloch-preintent-orders-r101-20261022')
BASE = Path('/tmp/moloch-entry-diagnostic-r102-20261022')
RUST = '/home/archon/.rustup/toolchains/stable-aarch64-unknown-linux-gnu/bin/rustc'


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    os.nice(15)
    Path('/proc/self/oom_score_adj').write_text('1000')
    patch = json.loads((ROOT / 'source-patch.json').read_bytes())
    shutil.copytree(BASE / 'source', ROOT / 'source')
    for name, entry in patch.items():
        target = ROOT / 'source' / name
        assert sha(target) == entry['oldSha256']
        lines = target.read_text(encoding='utf-8').splitlines(keepends=True)
        for change in reversed(entry['changes']):
            assert lines[change['from']:change['to']] == change['old']
            lines[change['from']:change['to']] = change['new']
        target.write_text(''.join(lines), encoding='utf-8')
        assert sha(target) == entry['newSha256']
    changed = {str(p.relative_to(ROOT / 'source')): sha(p)
        for p in (ROOT / 'source').rglob('*') if p.is_file()
        and sha(p) != sha(BASE / 'source' / p.relative_to(ROOT / 'source'))}
    assert changed == {name: entry['newSha256'] for name, entry in patch.items()}
    tmp = Path('/dev/shm/r103-generation'); tmp.mkdir(mode=0o700, exist_ok=True)
    checks = {}
    stages = [('runtime-fixture', None), ('order-tests', ['generation_content_tests', '--test-threads=1']),
        ('fundamental-tests', ['--test-threads=1']), ('rank-cohort-tests', ['--test-threads=1']),
        ('runtime-release', None), ('worker-tests', ['--test-threads=1']), ('worker-release', None)]
    for name, run_args in stages:
        args = json.loads((PRIOR / (name + '-command.json')).read_bytes())
        args = [arg.replace(str(PRIOR / 'source'), str(ROOT / 'source'))
            .replace('dependency=' + str(PRIOR), 'dependency=' + str(ROOT))
            .replace('moloch_live_runtime=' + str(PRIOR), 'moloch_live_runtime=' + str(ROOT)) for arg in args]
        args[args.index('-o') + 1] = str(ROOT / Path(args[args.index('-o') + 1]).name)
        args = ['-L', 'dependency=' + str(ROOT), *args]
        (ROOT / (name + '-command.json')).write_text(json.dumps(args))
        env = dict(os.environ, CARGO_MANIFEST_DIR=str(ROOT / 'source/moloch/live-runtime'), TMPDIR=str(tmp))
        started = time.monotonic()
        with (ROOT / (name + '-build.log')).open('x') as log:
            subprocess.run([RUST, *args], check=True, stdout=log, stderr=subprocess.STDOUT, env=env)
        if run_args:
            with (ROOT / (name + '-results.log')).open('x') as log:
                subprocess.run([args[args.index('-o') + 1], *run_args], check=True,
                    stdout=log, stderr=subprocess.STDOUT, env=env)
            checks[name] = next(line for line in (ROOT / (name + '-results.log')).read_text().splitlines()
                if line.startswith('test result:'))
            assert '0 failed;' in checks[name]
            print(checks[name], flush=True)
        print(name, 'completed', round(time.monotonic() - started, 3), flush=True)
        if name == 'order-tests':
            state = json.loads(Path('/home/archon/apps/moloch/shared/fundamental-live-reference-20261021/state.json').read_bytes())
            control = Path(state['controlPath'])
            benchmark_env = dict(env, MOLOCH_GENERATION_BENCH_CONTROL=str(control))
            with (ROOT / 'retained-generation-benchmark.log').open('x') as log:
                subprocess.run([args[args.index('-o') + 1], 'retained_production_sources_benchmark',
                    '--ignored', '--nocapture', '--test-threads=1'], check=True,
                    stdout=log, stderr=subprocess.STDOUT, env=benchmark_env)
            line = next(x for x in (ROOT / 'retained-generation-benchmark.log').read_text().splitlines()
                if 'GENERATION_BENCH ' in x)
            benchmark = json.loads(line.split('GENERATION_BENCH ', 1)[1])
            (ROOT / 'benchmark.public.json').write_text(json.dumps(dict(benchmark=benchmark,
                sourceControlSha256=sha(control), originalSourceClocksChanged=False), indent=2))
            print('BENCHMARK', json.dumps(benchmark), flush=True)
    binary = ROOT / 'moloch-live-runtime'
    cli = subprocess.run([str(binary)], capture_output=True, text=True)
    assert 'usage:' in (cli.stdout + cli.stderr).lower()
    native = Path('/tmp/moloch-upgrade-bound-r100-20261022/libmoloch_replay_release.rlib')
    prior = json.loads((PRIOR / 'qualified.public.json').read_bytes())
    assert sha(native) == prior['nativeLibrarySha256']
    result = dict(changedSources=changed, inheritedWorkerSourceSha256=sha(ROOT / 'source/moloch/live-runtime/src/worker.rs'),
        checks=checks, benchmark=json.loads((ROOT / 'benchmark.public.json').read_bytes()),
        binarySha256=sha(binary), runtimeLibrarySha256=sha(ROOT / 'libmoloch_live_runtime_release.rlib'),
        nativeLibrarySha256=sha(native), deployed=False, workerReplaced=False,
        newAccountClients=0, originalFactClocksChanged=False, originalDurabilityChanged=False,
        riskCapsChanged=False, crossCallContentCache=False, cliLoads=True, profitabilityEstablished=False)
    (ROOT / 'qualified.public.json').write_text(json.dumps(result, indent=2) + '\n')
    print('QUALIFIED_UNDEPLOYED', flush=True)


if __name__ == '__main__':
    main()
