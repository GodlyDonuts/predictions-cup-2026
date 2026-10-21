"""Use the deployed V3 review rules in the data updater; worker stays live."""
from pathlib import Path
from datetime import datetime, timezone
import fcntl
import hashlib
import json
import os
import shutil
import signal
import subprocess
import time

K = Path('/tmp/moloch-target-cache-r73-20261021')
J = Path('/tmp/moloch-stream-join-r72-20261021')
R = Path('/home/archon/apps/moloch/shared/live-production-v1')
F = R.parent / 'fundamental-live-reference-20261021'
UPDATER = Path('/home/archon/apps/moloch/releases/live-reference-dcaaec81/run-live-reference-reload.py')
CONFIG = F / 'config-r72.json'


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def stop_exact(pid, required):
    proc = Path('/proc') / str(pid)
    args = (proc / 'cmdline').read_bytes().split(b'\0')
    assert all(str(item).encode() in args for item in required)
    children = subprocess.run(['pgrep', '-P', str(pid)], capture_output=True, text=True)
    assert children.returncode == 1, 'Wait for the existing child to finish'
    os.kill(pid, signal.SIGTERM)
    for _ in range(100):
        if not proc.exists():
            return
        time.sleep(.05)
    raise RuntimeError('Process did not stop')


def main():
    original = CONFIG.read_bytes()
    config = json.loads(original)
    process = Path('/proc') / str(config['workerPid'])
    assert sha(process / 'exe') == config['binarySha256']
    assert not (R / 'EXECUTION_STOP').exists()
    state = json.loads((F / 'state.json').read_bytes())
    control = Path(state['controlPath'])
    assert sha(control) == state['controlSha256']
    new = K / 'moloch-fundamental-review'
    old_validation = subprocess.run([config['reviewHelper'], 'validate-control', str(control), str(time.time_ns() // 1_000_000)], capture_output=True, text=True)
    assert old_validation.returncode != 0 and 'FUNDAMENTAL_TOP_UP_POLICY' in old_validation.stderr
    validation = json.loads(subprocess.check_output([str(new), 'validate-control', str(control), str(time.time_ns() // 1_000_000)], text=True))
    assert len(validation['reviews']) == 14
    assert validation['accountRequests'] == 0
    # Rebuild uses the exact already deployed native/runtime libraries.
    H = Path('/tmp/moloch-recurring-cohort-20261021')
    assert sha(H / 'libmoloch_replay_release.rlib') == 'fb56b89e5a6af2788baf4d858fcca02e675f0752a3cabf355aac791b767ba287'
    assert sha(H / 'libmoloch_live_runtime_release.rlib') == '85aa59961b09e53ae4ce000581a1d9f5f1d85aaa387e7000435410e6295be4f7'
    reviews = json.loads(control.read_bytes())['fundamentalPolicy']['reviews']
    for review in reviews:
        args = ['hash-review', review['path']]
        assert subprocess.check_output([config['reviewHelper']] + args) == subprocess.check_output([str(new)] + args)
    stop_exact(1035150, [J / 'watch-recurring-opt-in-20261021.py', CONFIG])
    assert json.loads((F / 'pending.json').read_bytes()).get('resolved') is True
    updater_pid = json.loads((J / 'reference-updater-resumed.json').read_bytes())['pid']
    stop_exact(updater_pid, [UPDATER, CONFIG])
    with (F / 'updater.lock').open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        assert CONFIG.read_bytes() == original
        assert json.loads((F / 'pending.json').read_bytes()).get('resolved') is True
        release = R.parent.parent / 'releases' / ('recurring-review-' + sha(new)[:12])
        release.mkdir(mode=0o700)
        helper = release / 'review'
        shutil.copy2(new, helper)
        helper.chmod(0o700)
        assert sha(helper) == sha(new)
        backup = F / 'config-r72-before-recurring-helper.json'
        with backup.open('xb') as out:
            out.write(original)
        config.update(reviewHelper=str(helper), reviewHelperSha256=sha(helper))
        changed = {key for key in config if config[key] != json.loads(original)[key]}
        assert changed == {'reviewHelper', 'reviewHelperSha256'}
        temp = CONFIG.with_name('config-r72.helper-next')
        with temp.open('xb') as out:
            out.write((json.dumps(config, indent=2) + '\n').encode())
            out.flush()
            os.fsync(out.fileno())
        os.replace(temp, CONFIG)
    log_path = F / ('recurring-helper-updater-' + str(time.time_ns()) + '.log')
    with log_path.open('x') as log:
        updater = subprocess.Popen(['python3', '-u', str(UPDATER), '--config', str(CONFIG)], stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
    (J / 'reference-updater-resumed.json').write_text(json.dumps(dict(pid=updater.pid, log=str(log_path), at=datetime.now(timezone.utc).isoformat())))
    with (J / 'quiet-opt-in-watch-v3-helper.log').open('x') as log:
        watcher = subprocess.Popen(['python3', '-u', str(J / 'watch-recurring-opt-in-20261021.py'), '--release-dir', str(J), '--reference-config', str(CONFIG)], stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
    assert process.exists() and sha(process / 'exe') == config['binarySha256']
    result = dict(observedUtc=datetime.now(timezone.utc).isoformat(), workerPid=config['workerPid'],
        workerRestarted=False, helperSha256=sha(helper), helperSourceSha256=sha(K / 'moloch-fundamental-review.rs'),
        oldHelperSha256=json.loads(original)['reviewHelperSha256'], previousError='FUNDAMENTAL_TOP_UP_POLICY',
        reviewsValidated=14, allTypedReviewHashesUnchanged=True, configFieldsChanged=sorted(changed),
        updaterPid=updater.pid, watcherPid=watcher.pid, accountRequests=0,
        freshReferenceCommitVerified=False, financialStateModified=False)
    (J / 'recurring-helper-upgrade.public.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result))


if __name__ == '__main__':
    main()
