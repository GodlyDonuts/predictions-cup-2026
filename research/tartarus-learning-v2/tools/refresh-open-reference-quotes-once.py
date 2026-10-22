"""Roll only the public-data updater for one fresh open-quote review pass.

The account worker stays running. No old source clock, unknown request or
cancel acknowledgment is cleared. Resulting cancels use the worker's normal
review-change/reconciliation path; this process makes no SIG API request.
"""
import argparse
from datetime import datetime, timezone
import fcntl
from hashlib import sha256
import json
import os
from pathlib import Path
import signal
import subprocess
import time


def run(a):
    os.umask(0o077)
    original = a.config.read_bytes()
    config = json.loads(original)
    process = Path('/proc', str(a.previous_updater_pid))
    command = (process / 'cmdline').read_bytes().split(b'\0')
    assert str(a.config).encode() in command and str(config['updaterScript']).encode() in command
    digest = lambda path: sha256(path.read_bytes()).hexdigest()
    assert digest(Path(config['updaterScript'])) == config['updaterSha256']
    assert digest(Path('/proc', str(config['workerPid']), 'exe')) == config['binarySha256']
    root, out = Path(config['workerRoot']), Path(config['outputRoot'])
    assert not (root / 'EXECUTION_STOP').exists()
    deadline = time.monotonic() + 180
    while time.monotonic() < deadline:
        if json.loads((out / 'pending.json').read_bytes()).get('resolved') is True:
            os.kill(a.previous_updater_pid, signal.SIGSTOP)
            try:
                safe = json.loads((out / 'pending.json').read_bytes()).get('resolved') is True
                children = subprocess.run(['pgrep', '-P', str(a.previous_updater_pid)], capture_output=True, text=True)
                sockets = [p for p in (process / 'fd').iterdir() if str(p.readlink()).startswith('socket:')]
            except BaseException:
                os.kill(a.previous_updater_pid, signal.SIGCONT)
                raise
            if safe and children.returncode == 1 and not sockets:
                break
            os.kill(a.previous_updater_pid, signal.SIGCONT)
        time.sleep(.1)
    else:
        raise RuntimeError('NO_QUIET_DATA_UPDATER_BOUNDARY_WORKER_UNCHANGED')
    os.kill(a.previous_updater_pid, signal.SIGTERM)
    os.kill(a.previous_updater_pid, signal.SIGCONT)
    deadline = time.monotonic() + 10
    while process.exists():
        assert time.monotonic() < deadline, 'DATA_UPDATER_DID_NOT_EXIT'
        time.sleep(.05)
    with (out / 'updater.lock').open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        assert json.loads((out / 'pending.json').read_bytes()).get('resolved') is True
        assert a.config.read_bytes() == original
        next_config = dict(config, updaterScript=str(a.script), updaterSha256=digest(a.script), sourceCommit=a.source_commit)
        with a.output_config.open('xb') as stream:
            stream.write((json.dumps(next_config, indent=2) + '\n').encode())
            stream.flush()
            os.fsync(stream.fileno())
    with a.log.open('x') as log:
        command = ['python3', '-u', str(a.script), '--config', str(a.output_config)]
        if not a.normal:
            command.append('--refresh-open-targets-once')
        child = subprocess.Popen(command, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
    assert Path('/proc', str(config['workerPid'])).exists() and not (root / 'EXECUTION_STOP').exists()
    result = dict(observedUtc=datetime.now(timezone.utc).isoformat(), updaterPid=child.pid,
        previousUpdaterPid=a.previous_updater_pid, workerPid=config['workerPid'],
        workerRestarted=False, financialLimitsChanged=False, sourceClocksRenewed=False,
        accountClientsCreated=0, ordersSubmittedByUpdater=0, refreshedKnownOpenTargetsOnce=not a.normal,
        sourceCommit=a.source_commit, updaterScriptSha256=digest(a.script), freshGenerationCommitted=False)
    with a.proof.open('x') as stream:
        stream.write(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result), flush=True)


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    for name in ('config', 'script', 'output-config', 'log', 'proof'):
        p.add_argument('--' + name, type=Path, required=True)
    p.add_argument('--previous-updater-pid', type=int, required=True)
    p.add_argument('--source-commit', required=True)
    p.add_argument('--normal', action='store_true', help='Resume ordinary data refresh without the one-shot open-target pass.')
    run(p.parse_args())
