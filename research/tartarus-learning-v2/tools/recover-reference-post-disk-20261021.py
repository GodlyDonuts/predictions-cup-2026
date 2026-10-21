"""Exact file-only recovery of the October21 R66 disk-outage reload.

Dry run by default. --apply records a proven expired/noncommitted reload and
creates a new process binding for the same updater. Never sends that request.
"""
from pathlib import Path
from datetime import datetime, timezone
import argparse
import fcntl
import hashlib
import importlib.util
import json
import os
import subprocess
import time

R=Path('/home/archon/apps/moloch/shared/live-production-v1')
F=R.parent/'fundamental-live-reference-20261021'
PID=1010157
TICKS=56518875
BINARY='7b6ba68644b3e3749dbd78d73d1fefebb3b2c9cf1649fd8deeb660413c0b9ace'
WANTED='d66a14f9158c7adb08acd4c7494a2afb48284767e15bea23d4e92dadab1bc442'
PREVIOUS='affe4cf1e3e13962a109bcee2bd499dacbec858a26931a5e88843c0ac1237f55'
BASE='5782b409256e6bf7cf40035a9f8aaa43dbea7d42af23a6bff17f828d8089d272'
UPDATER=Path('/home/archon/apps/moloch/releases/live-reference-dcaaec81/run-live-reference-reload.py')


def sha(path):
    with Path(path).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()


def ticks(pid):
    return int((Path('/proc')/str(pid)/'stat').read_text().rsplit(')',1)[1].split()[19])


def main(apply):
    os.umask(0o077)
    assert R.resolve()==R and F.resolve()==F
    assert not Path('/proc/1005097').exists() and not Path('/proc/1005100').exists()
    assert ticks(PID)==TICKS and sha('/proc/'+str(PID)+'/exe')==BINARY
    assert subprocess.check_output(['systemctl','--user','show','moloch-live.service','-p','MainPID','--value'],text=True).strip()==str(PID)
    assert sha(UPDATER)=='c30af610216c51d7f8bba378f3fce8f6aac04b71102f7b79fee809a482fa2953'
    with (F/'updater.lock').open('a') as lock:
        fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
        config=json.loads((F/'config-r66.json').read_bytes())
        assert config['workerPid']==1005097 and config['workerStartTicks']==56262531
        assert config['baseControlSha256']==BASE and sha(config['baseControl'])==BASE
        assert config['binarySha256']==BINARY and sha(config['binary'])==BINARY
        state_raw=(F/'state.json').read_bytes();state=json.loads(state_raw)
        pending_raw=(F/'pending.json').read_bytes();pending=json.loads(pending_raw)
        assert pending['expectedGenerationSha256']==WANTED and pending['previousGenerationSha256']==PREVIOUS
        assert state['generationSha256']==PREVIOUS and state['commits']==31
        staged=pending['staged'];request_path=Path(staged['controlPath']).parent/'request.json'
        assert sha(request_path)==WANTED and sha(staged['controlPath'])==staged['controlSha256']
        request=json.loads(request_path.read_bytes())
        expected={'workerPid':1005097,'workerStartTicks':56262531,'baseControlSha256':BASE,
            'previousGenerationSha256':PREVIOUS,'controlPath':staged['controlPath'],
            'controlSha256':staged['controlSha256'],'expiresAtMs':pending['expiresAtMs']}
        assert all(request[k]==v for k,v in expected.items())
        owner_path=R/'fundamental-reload/status.json';owner_raw=owner_path.read_bytes();owner=json.loads(owner_raw)
        assert owner['workerPid']==PID and owner['workerStartTicks']==TICKS and owner['baseControlSha256']==BASE
        assert owner['previousGenerationSha256']==PREVIOUS
        for key in ('bankManifestSha256','runId'):assert owner[key]==request[key]
        lease=json.loads((R/'account-queue/offline-account-queue.lock').read_bytes());assert lease['pid']==PID
        capture=(R.parent/'capture.lock').stat();capture_id=(capture.st_dev,capture.st_ino)
        opened=set()
        for fd in (Path('/proc')/str(PID)/'fd').iterdir():
            try:s=fd.stat();opened.add((s.st_dev,s.st_ino))
            except FileNotFoundError:pass
        assert capture_id in opened
        view=json.loads((R/'tartarus-current-native-state.json').read_bytes())
        assert view['generationSha256']==PREVIOUS
        expiry=request['expiresAtMs'];now=time.time_ns()//1000000
        assert now>expiry and owner_path.stat().st_mtime_ns//1000000>expiry
        assert int(datetime.fromisoformat(view['writtenAt'].replace('Z','+00:00')).timestamp()*1000)>expiry
        progress=json.loads((R/'progress.json').read_bytes())
        assert progress['pid']==PID and progress['phase']=='trading' and progress['realtimeHealthy'] is True
        assert not (R/'EXECUTION_STOP').exists()
        original=(R/'fundamental.ndjson').read_bytes();prefix=original[:original.rfind(b'\n')+1]
        generations=[]
        for line in prefix.splitlines():
            row=json.loads(line)
            assert row['manifestHash']==request['bankManifestSha256'] and row['runId']==request['runId']
            if row['event'].get('kind')=='review_generation':
                generations.append(row['event']['generation']['generationSha256'])
        assert generations and generations[-1]==PREVIOUS and WANTED not in generations
        assert owner_path.read_bytes()==owner_raw
        assert (F/'state.json').read_bytes()==state_raw and (F/'pending.json').read_bytes()==pending_raw
        assert ticks(PID)==TICKS
        resolution={'outcome':'expired_uncommitted','requestSha256':WANTED,
            'previousGenerationSha256':PREVIOUS,'committedGenerationSha256':PREVIOUS,
            'observedAtMs':now,'requestExpiresAtMs':expiry,'nativeViewWrittenAt':view['writtenAt'],
            'originalCohortPrefixBytes':len(prefix),'originalCohortPrefixSha256':hashlib.sha256(prefix).hexdigest(),
            'oldWorkerConfirmedDead':True,'restoredWorkerPid':PID,'restoredWorkerStartTicks':TICKS,
            'sameBinaryAndBaseControlVerified':True,'sameBankAndRunVerified':True,
            'requestResent':False,'accountRequests':0,'financialStateChanged':False}
        result={'observedUtc':datetime.now(timezone.utc).isoformat(),'resolution':resolution,'applied':False}
        if apply:
            spec=importlib.util.spec_from_file_location('checked_reference_updater',UPDATER)
            module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
            module.record_resolution(F,state,pending,resolution)
            config.update(workerPid=PID,workerStartTicks=TICKS)
            target=F/'config-r66-post-disk.json'
            with target.open('x') as f:
                json.dump(config,f,indent=2);f.flush();os.fsync(f.fileno())
            result.update(applied=True,newConfig=str(target),newConfigSha256=sha(target))
            with (F/'r66-post-disk-reconciliation.public.json').open('x') as f:
                json.dump(result,f,indent=2);f.flush();os.fsync(f.fileno())
        print(json.dumps(result))


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--apply',action='store_true')
    main(parser.parse_args().apply)
