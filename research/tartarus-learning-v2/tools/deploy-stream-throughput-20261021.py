"""Repair the exact stopped R67 transport, preserving all original financial state."""
from pathlib import Path
from datetime import datetime,timezone
from collections import deque
import argparse,fcntl,hashlib,json,os,shutil,subprocess
D=Path('/tmp/moloch-stream-throughput-20261021')
P=Path('/tmp/moloch-fundamental-token-expiry-20261021')
R=Path('/home/archon/apps/moloch/shared/live-production-v1');B=R/'bank-all-markets-v1'
F=R.parent/'fundamental-live-reference-20261021';Q=R/'account-queue'
OLD=R.parent/'moloch-live-tartarus-r67-physical-token-20261021.json'
UPDATER=Path('/home/archon/apps/moloch/releases/live-reference-dcaaec81/run-live-reference-reload.py')
def sha(path):
    with Path(path).open('rb')as f:return hashlib.file_digest(f,'sha256').hexdigest()
def frozen(path,raw):
    with Path(path).open('xb')as f:f.write(raw);f.flush();os.fsync(f.fileno())
def encoded(value):return json.dumps(value,indent=2).encode()
def main(apply):
    os.umask(0o077);ready=json.loads((D/'COMPLETE.json').read_text());old=json.loads((P/'deployment.public.json').read_text())
    assert ready['financialEngineUnchanged']and ready['SDKUnchanged']
    binary_sha=sha(D/'moloch-live-runtime');assert binary_sha==ready['binarySha256']
    approved=(D/'approved-source-commit.txt').read_text().strip();assert len(approved)==40
    assert not Path('/proc/1014772').exists()and not Path('/proc/1014775').exists()
    assert subprocess.check_output(['systemctl','--user','show','moloch-live.service','-p','MainPID','--value'],text=True).strip()=='0'
    stop_raw=(R/'EXECUTION_STOP').read_bytes();assert json.loads(stop_raw)=={'action':'no automatic write retry or activation renewal','at':'2026-10-21T17:19:05.600Z','reason':'STREAM_CONNECT_ATTEMPTS_EXHAUSTED'}
    assert not(R.parent/'EXECUTION_STOP').exists()
    reference_raw=(F/'state.json').read_bytes();state=json.loads(reference_raw);pending_raw=(F/'pending.json').read_bytes()
    assert json.loads(pending_raw)['resolved']is True and state['commits']==35
    active_raw=Path(state['controlPath']).read_bytes();assert sha(state['controlPath'])==state['controlSha256']
    active=json.loads(active_raw);assert active['activation']['binarySha256']==old['binarySha256']
    assert sha(UPDATER)=='c30af610216c51d7f8bba378f3fce8f6aac04b71102f7b79fee809a482fa2953'
    report_raw=(B/'report.json').read_bytes();head_raw=(B/'head.json').read_bytes();report=json.loads(report_raw)
    prior=json.loads((P/'r67-first-sale.report.original.json').read_text());assert report['fills']==prior['fills']and len(report['fills'])==425
    for key in ('cash','ownedCost','realized','fees','positions'):assert report['report'][key]==prior['report'][key]
    bridge_raw=(B/'engine-upgrade-v1.json').read_bytes();assert json.loads(bridge_raw)['upgrades'][-1]['toEngineIdentity']==ready['engineIdentity']
    dispatch_raw=(R/'dispatch.ndjson').read_bytes();attempt=set();ack=set();closed=set();resolved=set()
    for raw in dispatch_raw.splitlines():
        e=json.loads(raw)['event'];qid=e['qid'];kind=e['kind']
        if kind=='attempt':attempt.add(qid)
        if kind in ('ack','recovered_ack'):ack.add(qid)
        if kind=='closed':closed.add(qid)
        if kind in ('local_final_guard_no_http','aborted_before_admission','expired_exposure_reconciled'):resolved.add(qid)
    assert not(attempt-ack-resolved)and len(ack)==1668 and len(closed)==1666
    m=json.loads((Q/'account-queue-manifest.json').read_text())
    with(Q/m['journal']).open()as f:queue=json.loads(deque(f,maxlen=1)[0])['state']
    assert queue['active']is None and not queue['unknown']and queue['manual_review']is False
    generations=[]
    for line in(R/'fundamental.ndjson').open():
        e=json.loads(line)['event']
        if e['kind']=='review_generation':generations.append(e['generation']['generationSha256'])
    assert generations[-1]==state['generationSha256']
    result=dict(observedUtc=datetime.now(timezone.utc).isoformat(),sourceCommit=approved,binarySha256=binary_sha,
        originalFills=425,cash=report['report']['cash'],ownedCost=report['report']['ownedCost'],realized=report['report']['realized'],
        originalNativeHead=json.loads(head_raw),bridgeSha256=hashlib.sha256(bridge_raw).hexdigest(),
        originalDispatchSha256=hashlib.sha256(dispatch_raw).hexdigest(),knownOpenOrders=sorted(ack-closed),
        originalFinancialStatePreserved=True,financialEngineUnchanged=True,SDKUnchanged=True,
        financialCapsLimiterAndWindowsChanged=False,streamAttemptsPreserved=True,orderRetry=False,freshTradingVerified=False,
        originalFinancialPrefixBytes=(B/'events.jsonl').stat().st_size,originalFinancialPrefixSha256=sha(B/'events.jsonl'))
    if not apply:print(json.dumps(result));return
    I=R.parent/'incidents'/('r68-stream-throughput-'+datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ'));I.mkdir(mode=0o700)
    for name,raw in [('active-control.original.json',active_raw),('reference-state.original.json',reference_raw),('reference-pending.original.json',pending_raw),('report.original.json',report_raw),('head.original.json',head_raw),('dispatch.original.ndjson',dispatch_raw),('stream-recovery-budget.original.json',(R/'stream-recovery-budget.json').read_bytes())]:frozen(I/name,raw)
    with open(active['ownerLock'],'r+')as owner,open(B/'writer.lock','r+')as writer,open(F/'updater.lock','a')as updater:
        for lock in (owner,writer,updater):fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
        assert(R/'EXECUTION_STOP').read_bytes()==stop_raw and(R/'dispatch.ndjson').read_bytes()==dispatch_raw
        assert(B/'head.json').read_bytes()==head_raw and(B/'report.json').read_bytes()==report_raw
        assert(F/'state.json').read_bytes()==reference_raw and(F/'pending.json').read_bytes()==pending_raw
        release=R.parent.parent/'releases'/('tartarus-r68-stream-throughput-'+binary_sha[:12]);release.mkdir()
        binary=release/'moloch-live-runtime';shutil.copy2(D/'moloch-live-runtime',binary);binary.chmod(0o700);assert sha(binary)==binary_sha
        control=R.parent/'moloch-live-tartarus-r68-stream-throughput-20261021.json';active['activation']['binarySha256']=binary_sha;frozen(control,encoded(active))
        U=Path('/home/archon/.config/systemd/user/moloch-live.service');unit=U.read_bytes();frozen(I/'unit.original',unit)
        old_binary='/home/archon/apps/moloch/releases/tartarus-r67-physical-token-'+old['binarySha256'][:12]+'/moloch-live-runtime'
        assert unit.count(old_binary.encode())==1 and unit.count(str(OLD).encode())==1
        changed=unit.replace(old_binary.encode(),str(binary).encode()).replace(str(OLD).encode(),str(control).encode())
        temporary=U.with_name('.moloch-live.r68.next');temporary.write_bytes(changed);temporary.chmod(0o600);os.replace(temporary,U)
        assert(B/'engine-upgrade-v1.json').read_bytes()==bridge_raw
        assert(R/'stream-recovery-budget.json').read_bytes()==(I/'stream-recovery-budget.original.json').read_bytes()
        os.rename(R/'EXECUTION_STOP',I/'EXECUTION_STOP.original');result.update(incident=str(I),controlSha256=sha(control))
        (D/'deployment.public.json').write_bytes(encoded(result))
    for cmd in (['daemon-reload'],['reset-failed','moloch-live.service'],['start','moloch-live.service']):subprocess.run(['systemctl','--user',*cmd],check=True)
    pid=int(subprocess.check_output(['systemctl','--user','show','moloch-live.service','-p','MainPID','--value'],text=True));proc=Path('/proc')/str(pid)
    assert pid>0 and sha(proc/'exe')==binary_sha and(proc/'cmdline').read_bytes().split(b'\0')[:-1]==[str(binary).encode(),b'run',str(control).encode()]
    ticks=int((proc/'stat').read_text().rsplit(') ',1)[1].split()[19])
    state.update(controlPath=str(control),controlSha256=sha(control));temporary=F/'state.r68.next';temporary.write_bytes(encoded(state));os.replace(temporary,F/'state.json')
    config=json.loads((F/'config-r67.json').read_text());config.update(sourceCommit=approved,baseControl=str(control),baseControlSha256=sha(control),workerPid=pid,workerStartTicks=ticks,binary=str(binary),binarySha256=binary_sha,initialGenerationSha256=state['generationSha256'])
    upath=F/'config-r68.json';frozen(upath,encoded(config))
    with(F/'r68-updater.log').open('x')as log:child=subprocess.Popen(['python3','-u',str(UPDATER),'--config',str(upath)],stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
    result.update(observedUtc=datetime.now(timezone.utc).isoformat(),pid=pid,startTicks=ticks,referenceUpdaterPid=child.pid,phase='restoring_original_ledger',actualExecutableAndArgumentsVerified=True)
    (D/'deployment.public.json').write_bytes(encoded(result));print(json.dumps(result))
if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--apply',action='store_true');main(parser.parse_args().apply)
