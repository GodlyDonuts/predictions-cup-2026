"""Qualify only the reviewed SELL scheduling change; no account client or deployment."""
from pathlib import Path
import datetime
import hashlib
import json
import os
import subprocess

OLD = Path('/tmp/moloch-exit-audit-r81-20261022')
ROOT = Path('/tmp/moloch-exit-final-facts-r82-20261022')
RUST = '/home/archon/.rustup/toolchains/stable-aarch64-unknown-linux-gnu/bin/rustc'


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    native = Path('/tmp/moloch-recurring-cohort-20261021/libmoloch_replay_release.rlib')
    runtime = OLD/'libmoloch_live_runtime_release.rlib'
    assert sha(native) == 'fb56b89e5a6af2788baf4d858fcca02e675f0752a3cabf355aac791b767ba287'
    assert sha(runtime) == '95211bc67bf59a27dfa5f0394d4c186be52b2da0369234dc139009724798b5e1'
    changed = []
    for original in (OLD/'source').rglob('*'):
        if original.is_file():
            relative = original.relative_to(OLD/'source')
            if sha(original) != sha(ROOT/'source'/relative):
                changed.append(str(relative))
    assert changed == ['moloch/live-runtime/src/worker.rs'], changed
    env = dict(os.environ, CARGO_MANIFEST_DIR=str(ROOT/'source/moloch/live-runtime'))
    for name in ['worker-test', 'worker-release']:
        args = json.loads((OLD/(name+'-command.json')).read_bytes())
        args = [value.replace(str(OLD/'source'), str(ROOT/'source')) for value in args]
        output_index = args.index('-o')+1
        args[output_index] = str(ROOT/Path(args[output_index]).name)
        (ROOT/(name+'-command.json')).write_text(json.dumps(args))
        with (ROOT/(name+'-build.log')).open('w') as log:
            subprocess.run(['nice', '-n', '10', RUST, *args], env=env, stdout=log,
                           stderr=subprocess.STDOUT, check=True)
        print(name, 'compiled', flush=True)
        if name == 'worker-test':
            with (ROOT/'worker-results.log').open('w') as log:
                subprocess.run(['nice','-n','10',str(ROOT/'worker-tests'),'--test-threads=1'],
                               stdout=log,stderr=subprocess.STDOUT,check=True)
            summary = (ROOT/'worker-results.log').read_text().splitlines()[-2]
            print(summary,flush=True)
    binary = ROOT/'moloch-live-runtime'
    result = subprocess.run([str(binary)],text=True,capture_output=True)
    assert 'usage:' in (result.stdout+result.stderr).lower()
    proof = dict(observedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),
        binarySha256=sha(binary),workerSourceSha256=sha(ROOT/'source/moloch/live-runtime/src/worker.rs'),
        nativeLibrarySha256=sha(native),runtimeLibrarySha256=sha(runtime),
        changedSourcePaths=changed,tests=summary,cliLoads=True,
        nativeEngineChanged=False,runtimeLibraryChanged=False,
        accountClientsCreated=0,productionChanged=False)
    (ROOT/'qualified.public.json').write_text(json.dumps(proof,indent=2)+'\n')
    print(json.dumps(proof),flush=True)


if __name__ == '__main__':
    main()
