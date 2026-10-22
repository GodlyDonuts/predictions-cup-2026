"""Recover the confirmed policy-log-cap exit, retaining every original journal."""
from pathlib import Path
from datetime import datetime, timezone
import fcntl
import hashlib
import json
import os
import shutil
import subprocess

L=Path('/tmp/moloch-policy-log-r104-20261022')
J=Path('/tmp/moloch-generation-pins-r103-20261022')
R=Path('/home/archon/apps/moloch/shared/live-production-v1')
B=R/'bank-all-markets-v1'
F=R.parent/'fundamental-live-reference-20261021'


def sha(p):
    with Path(p).open('rb') as f: return hashlib.file_digest(f,'sha256').hexdigest()


def fresh(p,v):
    b=v if isinstance(v,bytes) else (json.dumps(v,indent=2)+'\n').encode()
    with p.open('xb') as f: f.write(b); f.flush(); os.fsync(f.fileno())


def system(*args):
    return subprocess.check_output(['systemctl','--user',*args],text=True).strip()


def main():
    os.umask(0o077); os.nice(15); Path('/proc/self/oom_score_adj').write_text('1000')
    q=json.loads((L/'qualified.public.json').read_bytes()); old=json.loads((J/'deployment.public.json').read_bytes())
    assert '149 passed; 0 failed' in q['checks']['fundamental-tests'] and q['cliLoads']
    assert sha(L/'moloch-live-runtime')==q['binarySha256']
    assert sha(L/'source'/q['changedSource'])==q['sourceSha256']
    assert q['originalHistoryChanged'] is False and q['nativeEngineChanged'] is False and q['riskCapsChanged'] is False
    source=(L/'approved-source-commit.txt').read_text().strip()
    assert len(source)==40 and all(x in '0123456789abcdef' for x in source)
    assert not Path('/proc/'+str(old['pid'])).exists() and not Path('/proc/1136220').exists()
    status=json.loads((R/'status.json').read_bytes()); stop=(R/'EXECUTION_STOP').read_bytes()
    assert status['pid']==old['pid'] and status['reason']=='FUNDAMENTAL_LOG_BOUND'
    assert json.loads(stop)['reason']=='FUNDAMENTAL_LOG_BOUND' and not (R.parent/'EXECUTION_STOP').exists()
    assert q['priorLogBoundBytes']-65536 < (R/'fundamental.ndjson').stat().st_size <= q['priorLogBoundBytes'] < q['logBoundBytes']
    state_raw=(F/'state.json').read_bytes(); pending=(F/'pending.json').read_bytes(); state=json.loads(state_raw)
    assert json.loads(pending).get('resolved') is True
    active_raw=Path(state['controlPath']).read_bytes(); assert sha(state['controlPath'])==state['controlSha256']
    active=json.loads(active_raw); assert active['activation']['binarySha256']==old['binarySha256']
    originals={n:(R/n).read_bytes() for n in ['dispatch.ndjson','fundamental.ndjson','fundamental.initialized.json']}
    originals.update({'report.json':(B/'report.json').read_bytes(),'head.json':(B/'head.json').read_bytes()})
    attempted=set(); acknowledged=set(); resolved=set(); closed=set()
    for line in originals['dispatch.ndjson'].splitlines():
        e=json.loads(line)['event']; k=e['kind']; i=e['qid']
        if k=='attempt': attempted.add(i)
        if k in ['ack','recovered_ack']: acknowledged.add(i)
        if k=='closed': closed.add(i)
        if k in ['aborted_before_admission','local_final_guard_no_http','expired_exposure_reconciled','admitted_pre_network_not_sent']: resolved.add(i)
    assert not attempted-acknowledged-resolved, 'UNKNOWN_ACCEPTANCE_REQUIRES_RECONCILIATION'
    assert not any(json.loads(x)['event'].get('qid')==23400 for x in originals['dispatch.ndjson'].splitlines()), 'RESERVED_Q23400_UNEXPECTED_DISPATCH'
    bridge=sha(B/'engine-upgrade-v1.json'); prefix=dict(bytes=(B/'events.jsonl').stat().st_size,sha256=sha(B/'events.jsonl'))
    incident=R.parent/'incidents'/('r104-policy-log-'+datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ')); incident.mkdir(mode=0o700)
    for n,b in originals.items(): fresh(incident/n,b)
    for n,b in [('active-control.json',active_raw),('reference-state.json',state_raw),('reference-pending.json',pending)]: fresh(incident/n,b)
    release=R.parent.parent/'releases'/('tartarus-r104-policy-log-'+q['binarySha256'][:12]); release.mkdir(mode=0o700)
    binary=release/'moloch-live-runtime'; shutil.copyfile(L/'moloch-live-runtime',binary); binary.chmod(0o700)
    control=R.parent/'moloch-live-tartarus-r104-policy-log-20261022.json'
    active['activation']['binarySha256']=q['binarySha256']; fresh(control,active)
    with (L/'preflight.log').open('x') as log:
        subprocess.run([str(binary),'preflight',str(control)],check=True,stdout=log,stderr=subprocess.STDOUT)
    assert 'Read-only preflight complete' in (L/'preflight.log').read_text()
    Q=R/'account-queue'; m=json.loads((Q/'account-queue-manifest.json').read_bytes()); path=Q/m['journal']
    with path.open('rb') as f:
        f.seek(max(0,path.stat().st_size-16*1024*1024)); queue=json.loads(f.read().splitlines()[-1])['state']
    assert queue['active'] is None and not queue['unknown'] and queue['manual_review'] is False
    result=dict(observedUtc=datetime.now(timezone.utc).isoformat(),sourceCommit=source,binarySha256=q['binarySha256'],
        controlSha256=sha(control),incident=str(incident),originalFills=len(json.loads(originals['report.json'])['fills']),
        originalFinancialPrefix=prefix,originalFundamentalSha256=hashlib.sha256(originals['fundamental.ndjson']).hexdigest(),
        originalFundamentalBytes=len(originals['fundamental.ndjson']),bridgeSha256=bridge,engineIdentity=old['engineIdentity'],
        nativeEngineUnchanged=True,knownOpenOrders=sorted(acknowledged-closed),priorReservedQidNotDispatched=23400,
        priorReviewRenewed=False,monetaryCapsLimiterAndWindowsChanged=False,ordersSubmittedByDeployment=0,freshTradingVerified=False)
    with open(active['ownerLock'],'r+') as owner, open(B/'writer.lock','r+') as writer, open(F/'updater.lock','a') as updater:
        for f in [owner,writer,updater]: fcntl.flock(f,fcntl.LOCK_EX|fcntl.LOCK_NB)
        assert (R/'EXECUTION_STOP').read_bytes()==stop and (F/'state.json').read_bytes()==state_raw and (F/'pending.json').read_bytes()==pending
        for n,b in originals.items(): assert ((B if n in ['report.json','head.json'] else R)/n).read_bytes()==b
        assert sha(B/'engine-upgrade-v1.json')==bridge
        unit=Path('/home/archon/.config/systemd/user/moloch-live.service'); raw=unit.read_bytes(); fresh(incident/'unit.original',raw)
        old_binary=R.parent.parent/'releases'/('tartarus-r103-generation-pins-'+old['binarySha256'][:12])/'moloch-live-runtime'
        old_control=R.parent/'moloch-live-tartarus-r103-generation-pins-20261022.json'
        assert raw.count(str(old_binary).encode())==1 and raw.count(str(old_control).encode())==1
        changed=raw.replace(str(old_binary).encode(),str(binary).encode()).replace(str(old_control).encode(),str(control).encode())
        temp=unit.with_name('.moloch-live.r104.next'); fresh(temp,changed); os.replace(temp,unit)
        os.rename(R/'EXECUTION_STOP',incident/'EXECUTION_STOP.original'); fresh(L/'installed.public.json',result)
        for d in [R,B,incident,release,unit.parent]:
            fd=os.open(d,os.O_RDONLY); os.fsync(fd); os.close(fd)
    del originals
    for args in [('daemon-reload',),('reset-failed','moloch-live.service'),('start','moloch-live.service')]: system(*args)
    pid=int(system('show','moloch-live.service','-p','MainPID','--value')); proc=Path('/proc')/str(pid)
    assert pid>0 and sha(proc/'exe')==q['binarySha256']
    assert (proc/'cmdline').read_bytes().split(b'\0')[:-1]==[str(binary).encode(),b'run',str(control).encode()]
    ticks=int((proc/'stat').read_text().rsplit(') ',1)[1].split()[19])
    state.update(controlPath=str(control),controlSha256=sha(control)); temp=F/'state.r104.next'; fresh(temp,state); os.replace(temp,F/'state.json')
    cfg=json.loads((F/'config-agent-maker-r103-window-009.json').read_bytes()); script=Path(cfg['updaterScript'])
    assert sha(script)==cfg['updaterSha256']
    cfg.update(sourceCommit=source,baseControl=str(control),baseControlSha256=sha(control),workerPid=pid,workerStartTicks=ticks,
               binary=str(binary),binarySha256=q['binarySha256'],initialGenerationSha256=state['generationSha256'])
    cfg_path=F/'config-agent-maker-r104-window-012.json'; fresh(cfg_path,cfg)
    with (F/'r104-updater.log').open('x') as log:
        child=subprocess.Popen(['python3','-u',str(script),'--config',str(cfg_path)],stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
    result.update(pid=pid,startTicks=ticks,referenceUpdaterPid=child.pid,phase='restoring_original_ledger',actualExecutableAndArgumentsVerified=True)
    fresh(L/'deployment.public.json',result); print(json.dumps(result),flush=True)


if __name__=='__main__': main()
