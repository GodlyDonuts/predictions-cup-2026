"""Replace only the scoped source helper; never stop/restart the account worker."""
from datetime import datetime, timezone
import fcntl
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import time

STAGE = Path('/tmp/moloch-review-window-009-20261022')
RUNTIME = Path('/home/archon/apps/moloch/shared/live-production-v1')
REFERENCE = RUNTIME.parent / 'fundamental-live-reference-20261021'
OLD_CONFIG = REFERENCE / 'config-agent-maker-r103.json'
OLD_HELPER_PID = 1136153


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def write_new(path, raw):
    with path.open('xb') as stream:
        stream.write(raw); stream.flush(); os.fsync(stream.fileno())


def main():
    os.umask(0o077)
    source = (STAGE/'approved-source-commit.txt').read_text().strip()
    assert len(source) == 40 and all(x in '0123456789abcdef' for x in source)
    q = json.loads((STAGE/'qualified.public.json').read_bytes())
    assert all(row['exitCode'] == 0 for row in q['checks'].values())
    assert 'Ran 8 tests' in (STAGE/'window-results.log').read_text() and 'skipped' not in (STAGE/'window-results.log').read_text()
    assert 'Ran 18 tests' in (STAGE/'source-results.log').read_text()
    deployment = json.loads(Path('/tmp/moloch-generation-pins-r103-20261022/deployment.public.json').read_bytes())
    worker = Path('/proc')/str(deployment['pid'])
    assert sha(worker/'exe') == deployment['binarySha256']
    assert int((worker/'stat').read_text().rsplit(') ',1)[1].split()[19]) == deployment['startTicks']
    assert not (RUNTIME/'EXECUTION_STOP').exists() and not (RUNTIME.parent/'EXECUTION_STOP').exists()
    previous_raw = OLD_CONFIG.read_bytes(); previous = json.loads(previous_raw)
    assert previous['workerPid'] == deployment['pid'] and previous['binarySha256'] == deployment['binarySha256']
    assert set(previous['exchanges']) == {'1051','1076','1077','983'}
    helper = Path('/proc')/str(OLD_HELPER_PID)
    assert (helper/'cmdline').read_bytes().split(b'\0')[:-1] == [b'python3',b'-u',previous['updaterScript'].encode(),b'--config',str(OLD_CONFIG).encode()]
    assert sha(previous['updaterScript']) == previous['updaterSha256']
    release = RUNTIME.parent.parent/'releases/review-generation-window-009'
    release.mkdir(mode=0o700)
    for name in ['run-live-reference-reload.py','review_generation_window.py','publish-current-money-review.py','money-reservation-projection.py']:
        matches = [value for path,value in q['sourceHashes'].items() if Path(path).name == name]
        assert len(matches) == 1 and sha(STAGE/name) == matches[0]
        shutil.copyfile(STAGE/name,release/name); (release/name).chmod(0o600)
        assert sha(release/name) == matches[0]
    config = dict(previous, sourceCommit=source,
        updaterScript=str(release/'run-live-reference-reload.py'), updaterSha256=sha(release/'run-live-reference-reload.py'),
        reviewGenerationWindow=dict(modulePath=str(release/'review_generation_window.py'), moduleSha256=sha(release/'review_generation_window.py')))
    changed = {key for key in config if config[key] != previous.get(key)}
    assert changed == {'sourceCommit','updaterScript','updaterSha256','reviewGenerationWindow'}
    config_path = REFERENCE/'config-agent-maker-r103-window-009.json'
    write_new(config_path,(json.dumps(config,indent=2)+'\n').encode())
    write_new(STAGE/'previous-config.private.json',previous_raw)
    deadline = time.monotonic()+240
    while True:
        assert time.monotonic() < deadline, 'NO_QUIET_SOURCE_BOUNDARY_WORKER_UNCHANGED'
        if json.loads((REFERENCE/'pending.json').read_bytes()).get('resolved') is True:
            os.kill(OLD_HELPER_PID,signal.SIGSTOP)
            try:
                quiet = json.loads((REFERENCE/'pending.json').read_bytes()).get('resolved') is True
                quiet = quiet and not subprocess.run(['ps','-o','pid=','--ppid',str(OLD_HELPER_PID)],capture_output=True,text=True).stdout.strip()
            except BaseException:
                os.kill(OLD_HELPER_PID,signal.SIGCONT); raise
            if quiet:
                break
            os.kill(OLD_HELPER_PID,signal.SIGCONT)
        time.sleep(.2)
    os.kill(OLD_HELPER_PID,signal.SIGTERM); os.kill(OLD_HELPER_PID,signal.SIGCONT)
    deadline = time.monotonic()+10
    while helper.exists():
        assert time.monotonic() < deadline, 'SOURCE_HELPER_DID_NOT_EXIT_NO_FORCED_KILL'
        time.sleep(.1)
    with (REFERENCE/'updater.lock').open('a') as lock:
        fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
        assert json.loads((REFERENCE/'pending.json').read_bytes()).get('resolved') is True
    log = (REFERENCE/'window-009-updater.log').open('xb')
    child = subprocess.Popen(['python3','-u',config['updaterScript'],'--config',str(config_path)],stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
    assert sha(worker/'exe') == deployment['binarySha256']
    result = dict(observedUtc=datetime.now(timezone.utc).isoformat(), sourceCommit=source,
        workerPid=deployment['pid'],workerStartTicks=deployment['startTicks'],workerBinarySha256=deployment['binarySha256'],
        previousHelperPid=OLD_HELPER_PID,helperPid=child.pid,configPath=str(config_path),configSha256=sha(config_path),
        modifiedConfigFields=sorted(changed),exchanges=sorted(config['exchanges']),
        publisherPath=str(release/'publish-current-money-review.py'),sourceReferenceRoot=str(REFERENCE),
        workerReplaced=False,stopChanged=False,financialControlsChanged=False,sourceClocksRenewed=False,newAccountClients=0)
    write_new(STAGE/'deployment.public.json',(json.dumps(result,indent=2)+'\n').encode())
    print(json.dumps(result),flush=True)


if __name__ == '__main__':
    main()
