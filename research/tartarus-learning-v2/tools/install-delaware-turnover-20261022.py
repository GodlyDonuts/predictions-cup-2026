"""One same-worker scope reload after R83's measured window; no account client.

Only the public-reference helper is replaced. The trading worker, binary,
financial controls, original ledgers and limiter are not restarted or changed.
An uncertain owner reply retains exact pending metadata for the existing resolver.
"""
import fcntl
import gzip
import importlib.util
import json
import os
from pathlib import Path
import signal
import struct
import subprocess
import time
import urllib.request

ROOT=Path('/home/archon/apps/moloch/shared/fundamental-live-reference-20261021')
WORK=Path('/tmp/moloch-delaware-turnover-20261022')
R83=Path('/tmp/moloch-local-retirement-sweep-r83-20261022')
CONFIG=ROOT/'config-r83.json'


def main():
    c=json.loads(CONFIG.read_bytes())
    spec=importlib.util.spec_from_file_location('existing_reference_updater',c['updaterScript'])
    m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
    assert m.sha(c['updaterScript'])==c['updaterSha256']
    assert m.sha(c['reviewHelper'])==c['reviewHelperSha256']
    assert m.sha(WORK/'stage.py')=='e198802788cab131beb25c6385a9942eaf8b7f981cf05034768b5a8d5b79c9ae'
    runtime=Path(c['workerRoot']);proc=Path('/proc')/str(c['workerPid'])
    assert c['workerPid']==1063142 and c['workerStartTicks']==61099963
    assert int((proc/'stat').read_text().rsplit(') ',1)[1].split()[19])==c['workerStartTicks']
    assert m.sha(proc/'exe')==c['binarySha256'] and m.sha(c['baseControl'])==c['baseControlSha256']
    assert (R83/'fresh-live.public.json').exists()
    assert json.loads((R83/'cadence/complete.json').read_bytes())['elapsedSeconds']>=900
    progress=json.loads((runtime/'progress.json').read_bytes())
    assert progress['pid']==c['workerPid'] and progress['phase']=='trading' and progress['realtimeHealthy']
    assert not (runtime/'EXECUTION_STOP').exists() and not (runtime.parent/'EXECUTION_STOP').exists()
    pending=json.loads((ROOT/'pending.json').read_bytes())
    assert pending.get('resolved') is True, 'Resolve existing helper request first'
    old_status=json.loads((ROOT/'status.json').read_bytes());old_pid=old_status['pid']
    assert old_pid==1063145, 'Unexpected reference helper; do not signal'
    helper=Path('/proc')/str(old_pid)
    assert (helper/'cmdline').read_bytes().split(b'\0')==[
        b'python3',b'-u',c['updaterScript'].encode(),b'--config',str(CONFIG).encode(),b'']
    old_ticks=int((helper/'stat').read_text().rsplit(') ',1)[1].split()[19])
    # Check relay coverage before interrupting even the non-account helper.
    with urllib.request.urlopen(c['captureUrl']+'?exchanges=1075',timeout=20) as response:
        raw=response.read(2_000_001)
        assert len(raw)<=2_000_000
        if response.headers.get('Content-Encoding')=='gzip':raw=gzip.decompress(raw)
    assert len(raw)<=4_000_000 and [x['exchangeId'] for x in json.loads(raw)['rows']]==['1075']
    stamp=str(time.time_ns());out=ROOT/('delaware-turnover-'+stamp);out.mkdir()
    (out/'capture.json').write_bytes(raw)
    next_config=ROOT/'config-r83-delaware.json'
    assert not next_config.exists()
    next_c=dict(c,exchanges=c['exchanges']+['1075'])
    assert len(next_c['exchanges'])==len(set(next_c['exchanges']))
    m.put(next_config,next_c)
    lock=None
    result=dict(workerPid=c['workerPid'],oldUpdaterPid=old_pid,oldUpdaterStartTicks=old_ticks,
        stagedOnly=True,reloadSent=False,committed=False,tradingWorkerRestarted=False,accountClientsCreated=0)
    os.kill(old_pid,signal.SIGTERM)
    try:
        deadline=time.monotonic()+15
        while helper.exists():
            assert time.monotonic()<deadline, 'Prior helper remains alive'
            time.sleep(.1)
        lock=(ROOT/'updater.lock').open('a');fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
        assert json.loads((ROOT/'pending.json').read_bytes()).get('resolved') is True, 'Helper was in flight; original resolver must finish'
        state=json.loads((ROOT/'state.json').read_bytes())
        subprocess.run(['python3',str(WORK/'stage.py'),'--base-control',state['controlPath'],
            '--base-sha256',state['controlSha256'],'--capture',str(out/'capture.json'),
            '--ballot',str(WORK/'ballot.html'),'--ballot-receipt',str(WORK/'ballot-receipt.json'),
            '--contract',str(WORK/'contract.json'),'--output',str(out/'generation'),
            '--review-helper',c['reviewHelper']],check=True,timeout=45)
        staged=json.loads((out/'generation/staged.json').read_bytes())
        current=json.loads((runtime/'fundamental-reload/status.json').read_bytes())
        assert current['previousGenerationSha256']==state['generationSha256']
        with m.connect_at_boundary(runtime,c,current) as channel:
            now=time.time_ns()//1_000_000
            request={k:current[k] for k in ('version','workerPid','workerStartTicks','ownerLease',
                'baseControlSha256','bankManifestSha256','runId','previousGenerationSha256')}
            request.update(requestId='delaware-turnover-'+stamp,controlPath=staged['controlPath'],
                controlSha256=staged['controlSha256'],issuedAtMs=now,expiresAtMs=now+60000)
            request_path=out/'generation/request.json'
            request_raw=json.dumps(request,separators=(',',':')).encode();request_path.write_bytes(request_raw)
            wanted=m.sha(request_path)
            m.put(ROOT/'pending.json',dict(expectedGenerationSha256=wanted,
                previousGenerationSha256=state['generationSha256'],expiresAtMs=request['expiresAtMs'],
                staged=staged,selected=['1075']))
            result.update(stagedOnly=False,reloadSent=True,requestSha256=wanted,controlPath=staged['controlPath'])
            reply={}
            try:
                channel.settimeout(75);channel.sendall(struct.pack('!I',len(request_raw))+request_raw)
                body=b''
                while not body.endswith(b'\n'):
                    part=channel.recv(4096)
                    if not part:break
                    body+=part
                    assert len(body)<=65536
                reply=json.loads(body)
            except (OSError,ValueError,AssertionError) as error:
                result['replyUncertain']=type(error).__name__
            observed=json.loads((runtime/'fundamental-reload/status.json').read_bytes())
            if observed['previousGenerationSha256']==wanted:
                state.update(generationSha256=wanted,controlPath=staged['controlPath'],
                    controlSha256=staged['controlSha256'],commits=state['commits']+1,index=state['index']+1)
                m.put(ROOT/'state.json',state)
                m.put(ROOT/'pending.json',dict(resolved=True,generationSha256=wanted))
                result['committed']=True
            elif reply.get('state','').startswith(('rejected_','deferred_')) and observed['previousGenerationSha256']==state['generationSha256']:
                m.put(ROOT/'pending.json',dict(resolved=True,generationSha256=state['generationSha256']))
                result['rejection']=reply
            result['response']=reply
    finally:
        if lock is not None:lock.close()
        if not helper.exists():
            with (out/'updater.log').open('xb') as log:
                new=subprocess.Popen(['python3','-u',c['updaterScript'],'--config',str(next_config)],
                    stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
            result['newUpdaterPid']=new.pid
        result['observedAtMs']=time.time_ns()//1_000_000
        m.put(out/'installation.public.json',result)
        print(json.dumps(result))


if __name__=='__main__':main()
