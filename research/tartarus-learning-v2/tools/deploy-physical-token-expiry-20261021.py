"""Exact R66-to-R67 worker repair. No order retry or financial-ledger rewrite."""
from pathlib import Path
from collections import deque
from datetime import datetime,timezone
import argparse,fcntl,hashlib,json,os,shutil,subprocess

D=Path('/tmp/moloch-fundamental-token-expiry-20261021')
R=Path('/home/archon/apps/moloch/shared/live-production-v1');B=R/'bank-all-markets-v1'
F=R.parent/'fundamental-live-reference-20261021';Q=R/'account-queue'
OLD=R.parent/'moloch-live-tartarus-r66-stream-proof-recovery-20261021.json'
OLD_SHA='7b6ba68644b3e3749dbd78d73d1fefebb3b2c9cf1649fd8deeb660413c0b9ace'
UPDATER=Path('/home/archon/apps/moloch/releases/live-reference-dcaaec81/run-live-reference-reload.py')
def sha(path):
    with Path(path).open('rb')as f:return hashlib.file_digest(f,'sha256').hexdigest()
def frozen(path,value):
    with Path(path).open('x',encoding='utf-8')as f:json.dump(value,f,indent=2);f.flush();os.fsync(f.fileno())
def utc():return datetime.now(timezone.utc).isoformat()
def main(apply):
    os.umask(0o077)
    ready=json.loads((D/'COMPLETE.json').read_text());assert ready['workerTestsPassed']==224
    assert ready['preExistingFailures']==['worker::tartarus_snapshot_worker_tests::changed_requests_cannot_grow_unbounded_disk_attempts']
    assert ready['sameFailureReproducedOnUnchangedR66']is True
    assert ready['nativeEngineUnchanged']and ready['SDKUnchanged']
    binary_sha=sha(D/'moloch-live-runtime');assert binary_sha==ready['release']['binarySha256']
    source=(D/'approved-source-commit.txt').read_text().strip();assert len(source)==40
    assert not Path('/proc/1010157').exists()and not Path('/proc/1010746').exists()
    assert subprocess.check_output(['systemctl','--user','show','moloch-live.service','-p','MainPID','--value'],text=True).strip()=='0'
    stop_raw=(R/'EXECUTION_STOP').read_bytes();stop=json.loads(stop_raw)
    assert stop=={'action':'no automatic write retry or activation renewal','at':'2026-10-21T16:27:14.623Z','reason':'FUNDAMENTAL_PHYSICAL_TOKEN_EXPIRED_OR_REGRESSED'}
    assert not(R.parent/'EXECUTION_STOP').exists()
    state_raw=(F/'state.json').read_bytes();state=json.loads(state_raw)
    pending_raw=(F/'pending.json').read_bytes();assert json.loads(pending_raw)['resolved']is True
    active_raw=Path(state['controlPath']).read_bytes();assert sha(state['controlPath'])==state['controlSha256']
    active=json.loads(active_raw);assert active['activation']['binarySha256']==OLD_SHA and state['commits']==34
    assert sha(UPDATER)=='c30af610216c51d7f8bba378f3fce8f6aac04b71102f7b79fee809a482fa2953'
    report_raw=(B/'report.json').read_bytes();head_raw=(B/'head.json').read_bytes();bridge_sha=sha(B/'engine-upgrade-v1.json')
    report=json.loads(report_raw);assert len(report['fills'])==420 and report['report']['cash']=='9600.830000'
    prior=json.loads(Path('/tmp/moloch-stream-proof-recovery-20261021/r66-post-reference-fills.report.original.json').read_text())
    assert report['fills']==prior['fills']
    for key in ('cash','ownedCost','fees','positions'):assert report['report'].get(key)==prior['report'].get(key)
    target=[q for q in report['orders']if q['id']==20209];assert len(target)==1 and target[0]['retired']and target[0]['filled']==0
    assert not any(f['orderId']==20209 for f in report['fills'])
    original_dispatch=(R/'dispatch.ndjson').read_bytes();attempt=set();ack=set();closed=set();resolved=set();proof=None
    for line in original_dispatch.splitlines():
        e=json.loads(line)['event'];qid=e['qid'];k=e['kind']
        if k=='attempt':attempt.add(qid)
        if k in ('ack','recovered_ack'):ack.add(qid)
        if k=='closed':closed.add(qid)
        if k in ('local_final_guard_no_http','aborted_before_admission','expired_exposure_reconciled'):resolved.add(qid)
        if qid==20209 and k=='local_final_guard_no_http':proof=e['proof']
    assert proof and not(attempt-ack-resolved)and 20209 not in ack
    assert proof['originalError']['code']==stop['reason']and proof['originalError']['mayHaveExecuted']is False
    assert proof['retryPermitted']is False and proof['proofMode']=='durable_local_guard'
    assert sha(proof['sdkSourcePath'])==proof['sdkSourceSha256']
    assert proof['verifiedSDKHead']['globalUnknownCount']==0
    m=json.loads((Q/'account-queue-manifest.json').read_text())
    with(Q/m['journal']).open()as f:queue=json.loads(deque(f,maxlen=1)[0])['state']
    assert queue['active']is None and not queue['unknown']and queue['manual_review']is False
    generations=[]
    for line in(R/'fundamental.ndjson').open():
        e=json.loads(line)['event']
        if e['kind']=='review_generation':generations.append(e['generation']['generationSha256'])
    assert generations[-1]==state['generationSha256']
    result=dict(observedUtc=utc(),sourceCommit=source,binarySha256=binary_sha,originalFills=420,
        cash=report['report']['cash'],ownedCost=report['report']['ownedCost'],originalNativeHead=json.loads(head_raw),
        bridgeSha256=bridge_sha,originalDispatchSha256=hashlib.sha256(original_dispatch).hexdigest(),
        acknowledgedOrders=len(ack),terminalOrders=len(closed),knownOpenOrders=sorted(ack-closed),
        originalUnsentQid=20209,originalKeyRetried=False,sdkUnknown=0,sdkActive=None,
        originalFinancialStatePreserved=True,financialCapsLimiterAndWindowsChanged=False,
        nativeEngineUnchanged=True,SDKUnchanged=True,phase='checked_before_install',freshTradingVerified=False)
    if not apply:print(json.dumps(result));return
    I=R.parent/'incidents'/('r67-physical-token-'+datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ'));I.mkdir(mode=0o700)
    for name,value in [('active-control.original.json',active_raw),('reference-state.original.json',state_raw),('reference-pending.original.json',pending_raw),('report.original.json',report_raw),('head.original.json',head_raw),('dispatch.original.ndjson',original_dispatch)]:
        (I/name).write_bytes(value)
    prefix_bytes=(B/'events.jsonl').stat().st_size;prefix_sha=sha(B/'events.jsonl')
    result.update(originalFinancialPrefixBytes=prefix_bytes,originalFinancialPrefixSha256=prefix_sha)
    with open(active['ownerLock'],'r+')as owner,open(B/'writer.lock','r+')as writer,open(F/'updater.lock','a')as updater_lock:
        for lock in (owner,writer,updater_lock):fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
        assert(R/'EXECUTION_STOP').read_bytes()==stop_raw and(R/'dispatch.ndjson').read_bytes()==original_dispatch
        assert(B/'head.json').read_bytes()==head_raw and(B/'report.json').read_bytes()==report_raw
        assert(F/'state.json').read_bytes()==state_raw and(F/'pending.json').read_bytes()==pending_raw
        release=R.parent.parent/'releases'/('tartarus-r67-physical-token-'+binary_sha[:12]);release.mkdir()
        binary=release/'moloch-live-runtime';shutil.copy2(D/'moloch-live-runtime',binary);binary.chmod(0o700);assert sha(binary)==binary_sha
        control=R.parent/'moloch-live-tartarus-r67-physical-token-20261021.json'
        active['activation']['binarySha256']=binary_sha;frozen(control,active)
        U=Path('/home/archon/.config/systemd/user/moloch-live.service');unit=U.read_bytes();(I/'unit.original').write_bytes(unit)
        old_binary='/home/archon/apps/moloch/releases/tartarus-r66-stream-proof-recovery-7b6ba68644b3/moloch-live-runtime'
        assert unit.count(old_binary.encode())==1 and unit.count(str(OLD).encode())==1
        changed=unit.replace(old_binary.encode(),str(binary).encode()).replace(str(OLD).encode(),str(control).encode())
        pending_unit=U.with_name('.moloch-live.r67.next');pending_unit.write_bytes(changed);pending_unit.chmod(0o600);os.replace(pending_unit,U)
        assert sha(B/'engine-upgrade-v1.json')==bridge_sha
        os.rename(R/'EXECUTION_STOP',I/'EXECUTION_STOP.original')
        result.update(controlSha256=sha(control),phase='installed_before_start',incident=str(I))
        (D/'deployment.public.json').write_text(json.dumps(result,indent=2))
    for cmd in (['daemon-reload'],['reset-failed','moloch-live.service'],['start','moloch-live.service']):
        subprocess.run(['systemctl','--user',*cmd],check=True)
    pid=int(subprocess.check_output(['systemctl','--user','show','moloch-live.service','-p','MainPID','--value'],text=True));proc=Path('/proc')/str(pid)
    assert pid>0 and sha(proc/'exe')==binary_sha
    assert(proc/'cmdline').read_bytes().split(b'\0')[:-1]==[str(binary).encode(),b'run',str(control).encode()]
    ticks=int((proc/'stat').read_text().rsplit(') ',1)[1].split()[19])
    state.update(controlPath=str(control),controlSha256=sha(control));pending_state=F/'state.r67.next';pending_state.write_text(json.dumps(state));os.replace(pending_state,F/'state.json')
    upconfig=json.loads((F/'config-r66-post-disk.json').read_text());upconfig.update(sourceCommit=source,
        baseControl=str(control),baseControlSha256=sha(control),workerPid=pid,workerStartTicks=ticks,
        binary=str(binary),binarySha256=binary_sha,initialGenerationSha256=state['generationSha256'])
    upath=F/'config-r67.json';frozen(upath,upconfig)
    with(F/'r67-updater.log').open('x')as log:child=subprocess.Popen(['python3','-u',str(UPDATER),'--config',str(upath)],stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
    result.update(observedUtc=utc(),pid=pid,startTicks=ticks,referenceUpdaterPid=child.pid,phase='restoring_original_ledger',actualExecutableAndArgumentsVerified=True)
    (D/'deployment.public.json').write_text(json.dumps(result,indent=2));print(json.dumps(result))

if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--apply',action='store_true');main(parser.parse_args().apply)
