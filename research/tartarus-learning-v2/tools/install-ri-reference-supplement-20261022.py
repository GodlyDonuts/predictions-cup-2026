"""Replace only the source updater at a resolved boundary; never the worker."""
import hashlib
import argparse
import json
import os
from pathlib import Path
import signal
import subprocess
import time

ROOT = Path('/home/archon/apps/moloch/shared/fundamental-live-reference-20261021')
RELEASE = Path('/home/archon/apps/moloch/releases/ri-reference-supplement-005')
OLD_PID = 1132574
OLD_CONFIG = ROOT/'config-agent-maker-r101.json'
NEW_CONFIG = ROOT/'config-agent-maker-r101-ri-005.json'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--old-pid', type=int, required=True)
    parser.add_argument('--old-config', type=Path, required=True)
    parser.add_argument('--new-config', type=Path, required=True)
    parser.add_argument('--release', type=Path, required=True)
    args = parser.parse_args()
    global OLD_PID, OLD_CONFIG, NEW_CONFIG, RELEASE
    OLD_PID, OLD_CONFIG, NEW_CONFIG, RELEASE = args.old_pid, args.old_config, args.new_config, args.release
    assert OLD_PID > 1 and all(x.is_absolute() for x in (OLD_CONFIG, NEW_CONFIG, RELEASE))
    os.umask(0o077)
    proof_path = RELEASE/'deployment.public.json'
    assert not proof_path.exists()
    config = json.loads(NEW_CONFIG.read_bytes())
    runtime = Path(config['workerRoot'])
    worker = Path('/proc')/str(config['workerPid'])
    assert worker.exists() and not (runtime/'EXECUTION_STOP').exists()
    assert int((worker/'stat').read_text().rsplit(') ', 1)[1].split()[19]) == config['workerStartTicks']
    old = Path('/proc')/str(OLD_PID)
    command = (old/'cmdline').read_bytes().split(b'\0')
    assert str(OLD_CONFIG).encode() in command
    for key in ('updater', 'reviewHelper', 'stageScript', 'binary'):
        path = config['updaterScript'] if key == 'updater' else config[key]
        assert hashlib.sha256(Path(path).read_bytes()).hexdigest() == config[key+'Sha256']
    deadline = time.monotonic()+180
    while True:
        assert time.monotonic() < deadline, 'NO_RESOLVED_SOURCE_BOUNDARY'
        os.kill(OLD_PID, signal.SIGSTOP)
        try:
            quiet = json.loads((ROOT/'pending.json').read_bytes()).get('resolved') is True
            quiet = quiet and not subprocess.run(['ps','-o','pid=','--ppid',str(OLD_PID)], capture_output=True, text=True).stdout.strip()
        except BaseException:
            os.kill(OLD_PID, signal.SIGCONT)
            raise
        if quiet:
            break
        os.kill(OLD_PID, signal.SIGCONT)
        time.sleep(1)
    os.kill(OLD_PID, signal.SIGTERM)
    os.kill(OLD_PID, signal.SIGCONT)
    deadline = time.monotonic()+15
    while old.exists():
        assert time.monotonic() < deadline, 'OLD_SOURCE_UPDATER_STILL_PRESENT'
        time.sleep(.1)
    with (RELEASE/'updater.log').open('x') as log:
        process = subprocess.Popen(['python3','-u',config['updaterScript'],'--config',str(NEW_CONFIG)],
            stdin=subprocess.DEVNULL,stdout=log,stderr=log,start_new_session=True)
    proof = dict(observedAtMs=time.time_ns()//1000000, oldSourceUpdaterPid=OLD_PID,
        sourceUpdaterPid=process.pid, sourceUpdaterConfig=str(NEW_CONFIG),
        configSha256=hashlib.sha256(NEW_CONFIG.read_bytes()).hexdigest(),
        updaterSha256=config['updaterSha256'], supplementalReference=config['supplementalReference'],
        workerPid=config['workerPid'], workerRestarted=False, newAccountClients=0,
        relayReplaced=False, monetaryCapsChanged=False, sourceClocksRenewed=False,
        actualFreshCaptureProbe=str(RELEASE/'capture-probe.original.json'))
    with proof_path.open('x') as file:
        json.dump(proof, file, indent=2)
        file.flush()
        os.fsync(file.fileno())
    print(json.dumps(proof), flush=True)


if __name__ == '__main__':
    main()
