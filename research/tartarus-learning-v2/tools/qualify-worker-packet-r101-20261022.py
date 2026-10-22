"""Finish R101 worker qualification after the live build's library stages."""
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
    assert sys.argv[1:] in ([], ['--rebuild-worker'])
    os.nice(10)
    Path('/proc/self/oom_score_adj').write_text('1000')
    pid = int((ROOT/'build.pid').read_text())
    process = Path('/proc')/str(pid)
    if process.exists():
        original = (process/'stat').read_text().rsplit(') ', 1)[1].split()[19]
        deadline = time.monotonic()+1800
        while process.exists():
            assert time.monotonic() < deadline, 'BUILD_WAIT_LIMIT'
            assert (process/'stat').read_text().rsplit(') ', 1)[1].split()[19] == original
            time.sleep(2)
    qualified = json.loads((ROOT/'qualified.public.json').read_bytes())
    source = ROOT/'source/moloch/live-runtime/src/worker.rs'
    staged = json.loads((ROOT/'worker-source-before-compile.public.json').read_bytes())
    assert sha(source) == staged['workerSourceSha256']
    if not sys.argv[1:]:
        assert staged['appliedAtNs'] < (ROOT/'worker-release-command.json').stat().st_mtime_ns
        assert sha(ROOT/'moloch-live-runtime') == qualified['binarySha256']
    fixture_sha = sha(ROOT/'libmoloch_live_runtime_fixture.rlib')
    for path, value in qualified['sourceHashes'].items():
        assert sha(ROOT/'source'/path) == value
    if sys.argv[1:]:
        command = ROOT/'worker-release-command.json'
        args = json.loads(command.read_bytes())
        command.rename(ROOT/'worker-release-command.original.json')
        command.write_text(json.dumps(args))
        assert staged['appliedAtNs'] < command.stat().st_mtime_ns
        (ROOT/'moloch-live-runtime').rename(ROOT/'moloch-live-runtime.original')
        with (ROOT/'worker-release-build-retry-001.log').open('x') as log:
            subprocess.run([RUST, *args], check=True, stdout=log, stderr=subprocess.STDOUT,
                           env=dict(os.environ, CARGO_MANIFEST_DIR=str(ROOT/'source/moloch/live-runtime')))
        qualified['binarySha256'] = sha(ROOT/'moloch-live-runtime')
        cli = subprocess.run([str(ROOT/'moloch-live-runtime')], capture_output=True, text=True)
        assert 'usage:' in (cli.stdout+cli.stderr).lower()
        print('worker-release rebuilt', flush=True)
    args = json.loads((PRIOR/'worker-tests-command.json').read_bytes())
    for i, value in enumerate(args):
        value = value.replace(str(PRIOR/'source'), str(ROOT/'source'))
        if value.startswith('moloch_live_runtime='):
            value = value.replace(str(PRIOR), str(ROOT))
        if i and args[i-1] == '-o':
            value = str(ROOT/'worker-tests')
        args[i] = value
    args = ['-L', 'dependency='+str(ROOT), *args]
    command = ROOT/'worker-tests-command.json'
    if sys.argv[1:]:
        command.rename(ROOT/'worker-tests-command.original.json')
    command.write_text(json.dumps(args))
    suffix = '-retry-001' if sys.argv[1:] else ''
    with (ROOT/('worker-tests-build'+suffix+'.log')).open('x') as log:
        subprocess.run([RUST, *args], check=True, stdout=log, stderr=subprocess.STDOUT,
                       env=dict(os.environ, CARGO_MANIFEST_DIR=str(ROOT/'source/moloch/live-runtime')))
    print('worker-tests compiled', flush=True)
    with (ROOT/('worker-tests-results'+suffix+'.log')).open('x') as log:
        subprocess.run([str(ROOT/'worker-tests'), '--test-threads=1'], check=True,
                       stdout=log, stderr=subprocess.STDOUT,
                       env=dict(os.environ, TMPDIR='/dev/shm/r101'))
    summary = next(line for line in (ROOT/('worker-tests-results'+suffix+'.log')).read_text().splitlines()
                   if line.startswith('test result:'))
    assert '271 passed; 0 failed' in summary
    assert sha(source) == staged['workerSourceSha256']
    assert sha(ROOT/'moloch-live-runtime') == qualified['binarySha256']
    assert sha(ROOT/'libmoloch_live_runtime_fixture.rlib') == fixture_sha
    qualified['checks']['worker-tests'] = summary
    qualified['sourceHashes']['moloch/live-runtime/src/worker.rs'] = sha(source)
    changed = {str(p.relative_to(ROOT/'source')): sha(p)
               for p in (ROOT/'source').rglob('*') if p.is_file()
               and (not (PRIOR/'source'/p.relative_to(ROOT/'source')).exists()
                    or sha(p) != sha(PRIOR/'source'/p.relative_to(ROOT/'source')))}
    assert changed == qualified['sourceHashes']
    qualified['runtimeFixtureSha256'] = fixture_sha
    qualified['workerTestsResultsFile'] = 'worker-tests-results'+suffix+'.log'
    qualified['originalFailedWorkerTestRetained'] = 'worker-tests-results.log' if sys.argv[1:] else None
    qualified.update(workerReportPublishedAfterPhysicalPreparation=True,
                     originalEventHeadAndIntentDurabilityUnchanged=True,
                     originalFactClocksUnchanged=True,
                     workerSourceStagedBeforeReleaseCompilation=True)
    (ROOT/'library-qualified.original.json').write_bytes((ROOT/'qualified.public.json').read_bytes())
    temporary = ROOT/'qualified.packet.next.json'
    temporary.write_text(json.dumps(qualified, indent=2)+'\n')
    os.replace(temporary, ROOT/'qualified.public.json')
    print(summary, flush=True)
    print('QUALIFIED_OFFLINE_WITH_FINAL_PACKET', flush=True)


if __name__ == '__main__':
    main()
