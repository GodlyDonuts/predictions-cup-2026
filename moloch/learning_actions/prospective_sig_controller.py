"""Sequential public capture after the original historical account reader.

Production stays stopped. Each bounded segment owns the existing owner lock
and SDK queue, and restores the original NETWORK_DISABLED inode on exit.
"""
import argparse
from datetime import datetime, timezone, timedelta
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time

SHARED=Path('/home/archon/apps/moloch/shared')
ROOT=SHARED/'live-production-v1'
CONTROL_SHA='bed178634e699f969a344bb3c1aa3a595c37ef40db1198ee9935a87cce00467a'
STOP_SHA='1b1b24f1379d9e3a11c13991afd588d085dc16d99c392eda2a3920769c0b2043'


def require(ok,reason):
    if not ok:raise ValueError(reason)


def sha(path):
    with Path(path).open('rb') as source:return hashlib.file_digest(source,'sha256').hexdigest()


def fingerprint(path):
    s=path.stat()
    require(path.is_file() and not path.is_symlink(),'ORIGINAL_REGULAR_CONTROL')
    return {'inode':s.st_ino,'size':s.st_size,'mtimeNs':s.st_mtime_ns,'sha256':sha(path)}


def store(path,value):
    temp=path.with_suffix('.pending')
    with temp.open('w') as out:
        json.dump(value,out,indent=2);out.flush();os.fsync(out.fileno())
    temp.replace(path)


def segment_end(now,end):
    require(now.tzinfo is not None and end.tzinfo is not None and now<end,
            'AWARE_FUTURE_CAPTURE_BOUNDARY')
    return min(now+timedelta(minutes=10),end).isoformat().replace('+00:00','Z')


def wait_history(job,stop):
    while not stop.exists():
        state=json.loads((job/'acquisition-status.json').read_text())
        if state['phase'] in ('complete','failed'):
            require(state['executionStopBefore']==state['executionStopAfter']
                    and state['networkMarkerBefore']==state['networkMarkerAfter'],
                    'HISTORY_CONTROL_RESTORATION_REQUIRED')
            pid=state['readerPid']
            require(not Path(f'/proc/{pid}').exists(),'HISTORY_READER_MUST_HAVE_EXITED')
            return state
        pid=state.get('readerPid');cmd=Path(f'/proc/{pid}/cmdline')
        require(type(pid)is int and cmd.exists() and str(job/'sig-history-read').encode()
                in cmd.read_bytes().split(b'\0'),'HISTORY_READER_LOST_WITHOUT_FINAL_STATE')
        time.sleep(10)
    raise ValueError('CAPTURE_JOB_STOPPED')


def run(spec):
    import fcntl
    require(os.name=='posix' and ROOT.resolve()==ROOT,'ORIGINAL_LINUX_STATE_ROOT')
    p=Path(spec['job']);p.mkdir(exist_ok=False)
    stop=p/'STOP';end=datetime.fromisoformat(spec['until'].replace('Z','+00:00'))
    now=datetime.now(timezone.utc)
    require(now<end<=now+timedelta(days=6),'FINITE_PROSPECTIVE_CAPTURE_PERIOD')
    binary=Path(spec['binary']);stream=Path(spec['streamEntry']);archiver=Path(spec['archiver'])
    for file,key in ((binary,'binarySha256'),(stream,'streamSha256'),(archiver,'archiverSha256')):
        require(sha(file)==spec[key],'PINNED_CAPTURE_SOURCE')
    history=Path(spec['historyJob'])
    store(p/'status.json',{'phase':'waiting_for_original_history_reader','pid':os.getpid()})
    previous=wait_history(history,stop)
    store(p/'history-terminal.json',previous)
    base=json.loads((history/'backfill-plan.json').read_text());base.pop('backfill',None)
    control=Path(base['controlFile']);marker=SHARED/'NETWORK_DISABLED';execution=ROOT/'EXECUTION_STOP'
    require(sha(control)==CONTROL_SHA and sha(execution)==STOP_SHA,'ORIGINAL_FINANCIAL_CONTROLS')
    initial_marker=fingerprint(marker);initial_stop=fingerprint(execution)
    number=0
    while datetime.now(timezone.utc)<end and not stop.exists():
        for file,key in ((binary,'binarySha256'),(stream,'streamSha256'),(archiver,'archiverSha256')):
            require(sha(file)==spec[key],'PINNED_CAPTURE_SOURCE')
        require(fingerprint(marker)==initial_marker and fingerprint(execution)==initial_stop,
                'ORIGINAL_STOP_AND_NETWORK_MARKER')
        if shutil.disk_usage(ROOT).free<12*1024**3:
            with (p/f'preservation-{number:05d}.log').open('x') as log:
                subprocess.run([sys.executable,str(archiver),'--output',str(p/f'preserved-{number:05d}')],
                    stdout=log,stderr=subprocess.STDOUT,check=True,timeout=1800)
        session=p/f'segment-{number:05d}';session.mkdir()
        saved=session/'NETWORK_DISABLED.original'
        with (SHARED/'capture.lock').open('rb') as lease:
            fcntl.flock(lease,fcntl.LOCK_EX|fcntl.LOCK_NB)
            for service in ('moloch-live.service','moloch-capture.service'):
                require(subprocess.check_output(['systemctl','--user','show',service,'-p','MainPID','--value'],
                    text=True).strip()=='0','EXISTING_ACCOUNT_SERVICE_ACTIVE')
            require(not (ROOT/'account-queue/offline-account-queue.lock').exists(),'EXISTING_SDK_WORKER')
            require(sha(control)==CONTROL_SHA and fingerprint(execution)==initial_stop
                    and fingerprint(marker)==initial_marker,'ORIGINAL_CONTROLS_CHANGED')
            plan=dict(base,binarySha256=spec['binarySha256'],jobStop=str(stop),forward={
                'output':str(session/'capture'),'until':segment_end(datetime.now(timezone.utc),end),
                'maximumRequests':1000,'node':spec['node'],'streamEntry':str(stream),
                'streamSha256':spec['streamSha256']})
            store(session/'plan.json',plan)
            with (session/'preflight.json').open('x') as out:
                subprocess.run([str(binary),'--plan',str(session/'plan.json')],stdout=out,check=True)
            marker.rename(saved)
            try:
                with (session/'stdout.json').open('x') as out,(session/'stderr.log').open('x') as err:
                    child=subprocess.Popen([str(binary),'--plan',str(session/'plan.json'),'--execute-read-only'],
                        stdout=out,stderr=err)
                    store(p/'status.json',{'phase':'capturing','pid':os.getpid(),'readerPid':child.pid,
                        'segment':number,'startedUtc':datetime.now(timezone.utc).isoformat(),
                        'financialExecution':False,'independentTestConsumed':False})
                    try:code=child.wait(timeout=780)
                    except subprocess.TimeoutExpired:
                        stop.touch(exist_ok=False)
                        child.terminate();child.wait(timeout=60);raise
            finally:
                require(not os.path.lexists(marker),'NEW_NETWORK_MARKER_MUST_NOT_BE_OVERWRITTEN')
                saved.rename(marker)
                require(fingerprint(marker)==initial_marker and fingerprint(execution)==initial_stop,
                        'EXACT_CONTROL_RESTORATION')
        result={'segment':number,'exitCode':code,'finishedUtc':datetime.now(timezone.utc).isoformat(),
                'networkMarkerRestored':True,'financialExecution':False}
        store(session/'result.json',result);store(p/'status.json',dict(result,phase='between_segments',pid=os.getpid()))
        if code:
            error=(session/'stderr.log').read_text()
            require(not any(x in error for x in ('DURABILITY','UNKNOWN','MANUAL','LOCK','STOPPED','HEADROOM')),
                    'CAPTURE_STATE_FAILURE_REQUIRES_DIAGNOSIS')
            time.sleep(60)
        number+=1
    store(p/'status.json',{'phase':'stopped','segments':number,'networkMarkerRestored':True,
                          'financialExecution':False,'independentTestConsumed':False})


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--spec',required=True)
    args=parser.parse_args();spec=json.loads(Path(args.spec).read_text())
    try:run(spec)
    except Exception as error:
        p=Path(spec['job'])
        if p.is_dir():store(p/'controller-failure.json',{'errorType':type(error).__name__,'reason':str(error)})
        raise
