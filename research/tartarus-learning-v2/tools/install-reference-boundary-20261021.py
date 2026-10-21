"""Replace only the reference updater at a serialized local-channel boundary."""
from pathlib import Path
import datetime
import fcntl
import hashlib
import json
import os
import signal
import subprocess
import time

F=Path('/home/archon/apps/moloch/shared/fundamental-live-reference-20261021')
J=Path('/tmp/moloch-stream-join-r72-20261021')
D=Path('/tmp/moloch-reference-boundary-20261021')
R=F.parent/'live-production-v1'
expected='85e17cf68c22664e55d14b0cb8a4f4961b533d33dc800045a4631594769d93fb'
config_path=F/'config-r72.json'
config=json.loads(config_path.read_bytes())
assert Path('/proc/1031505').exists() and config['workerPid']==1031505
assert not Path('/proc/1035884').exists(), 'Retire recurring watcher first'
old_pid=json.loads((J/'reference-updater-resumed.json').read_bytes())['pid']
proc=Path('/proc')/str(old_pid)
old_cmd=(proc/'cmdline').read_bytes()
assert b'run-live-reference-reload.py' in old_cmd and str(config_path).encode() in old_cmd
source=D/'run-live-reference-reload.py'
assert hashlib.sha256(source.read_bytes()).hexdigest()==expected
release=Path('/home/archon/apps/moloch/releases/reference-boundary-'+expected[:12])
release.mkdir(exist_ok=False)
target=release/source.name
target.write_bytes(source.read_bytes())
os.kill(old_pid,signal.SIGSTOP)
try:
    deadline=time.monotonic()+100
    while subprocess.run(['ps','-o','pid=','--ppid',str(old_pid)],capture_output=True,text=True).stdout.strip():
        assert time.monotonic()<deadline, 'Existing reload child did not complete'
        time.sleep(.25)
    os.kill(old_pid,signal.SIGTERM)
    os.kill(old_pid,signal.SIGCONT)
    deadline=time.monotonic()+5
    while proc.exists():
        assert time.monotonic()<deadline
        time.sleep(.05)
except BaseException:
    if proc.exists():os.kill(old_pid,signal.SIGCONT)
    raise
with (F/'updater.lock').open('a') as lock:
    fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
    original=config_path.read_bytes()
    assert json.loads(original)==config
    (D/'config-before.json').write_bytes(original)
    pending_before=(F/'pending.json').read_bytes()
    config.update(updaterScript=str(target),updaterSha256=expected)
    temp=config_path.with_suffix('.boundary-next');temp.write_text(json.dumps(config,indent=2)+'\n');temp.replace(config_path)
    coordinator=(D/'enable-recurring-reviews-20261021.py').read_text().replace("H = Path('/tmp/moloch-recurring-cohort-20261021')", "H = Path('/tmp/moloch-stream-join-r72-20261021')")
    (J/'enable-recurring-reviews-20261021.py').write_text(coordinator)
    assert (F/'pending.json').read_bytes()==pending_before
    log_path=F/('reference-boundary-'+str(time.time_ns())+'.log')
    with log_path.open('xb') as log:
        child=subprocess.Popen(['python3','-u',str(target),'--config',str(config_path)],stdout=log,stderr=log,start_new_session=True)
    resumed=dict(pid=child.pid,log=str(log_path),at=datetime.datetime.now(datetime.timezone.utc).isoformat())
    (J/'reference-updater-resumed.json').write_text(json.dumps(resumed))
with (J/'quiet-opt-in-watch-reference-boundary.log').open('xb') as log:
    watcher=subprocess.Popen(['python3','-u',str(J/'watch-recurring-opt-in-20261021.py'),'--release-dir',str(J),'--reference-config',str(config_path)],stdout=log,stderr=log,start_new_session=True)
proof=dict(observedUtc=resumed['at'],workerPid=1031505,workerRestarted=False,
    updaterPid=child.pid,watcherPid=watcher.pid,updaterSha256=expected,
    pendingRequestPreserved=True,sourceClocksChanged=False,requestLifetimeMs=60000,
    accountClientsCreated=0,orderRequests=0,changedConfigKeys=['updaterScript','updaterSha256'])
(D/'deployment.public.json').write_text(json.dumps(proof,indent=2)+'\n')
print(json.dumps(proof))
