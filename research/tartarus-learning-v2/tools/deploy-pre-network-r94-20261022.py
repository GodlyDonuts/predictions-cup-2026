"""One-owner R93->R94 proved local no-send recovery; native engine and original finances retained."""
from pathlib import Path
from datetime import datetime, timezone
import argparse, fcntl, hashlib, json, os, shutil, signal, subprocess, time

L = Path('/tmp/moloch-pre-network-r94-20261022')
J = Path('/tmp/moloch-rank-inventory-r93-20261022')
NATIVE = Path('/tmp/moloch-money-criterion-r88-20261022')
RUNTIME = L
R = Path('/home/archon/apps/moloch/shared/live-production-v1')
B = R / 'bank-all-markets-v1'
F = R.parent / 'fundamental-live-reference-20261021'
OLD = R.parent / 'moloch-live-tartarus-r93-rank-inventory-20261022.json'
OLD_SHA = json.loads((J/'deployment.public.json').read_bytes())['binarySha256']
OLD_PID = 1103623
OLD_TICKS = json.loads((J/'deployment.public.json').read_bytes())['startTicks']
RESUME_INCIDENT = None


def sha(path):
    with Path(path).open('rb') as f:
        return hashlib.file_digest(f, 'sha256').hexdigest()


def fresh(path, value):
    raw = value if isinstance(value, bytes) else (json.dumps(value, indent=2) + '\n').encode()
    if RESUME_INCIDENT is not None and path.exists():
        assert path.read_bytes() == raw, 'RESUME_EXISTING_FILE_CHANGED'
        return
    with path.open('xb') as f:
        f.write(raw); f.flush(); os.fsync(f.fileno())


def system(*args):
    return subprocess.check_output(['systemctl', '--user', *args], text=True).strip()


def qualified():
    q = json.loads((L / 'qualified.public.json').read_bytes())
    assert sha(L / 'moloch-live-runtime') == q['binarySha256'] and q['cliLoads']
    assert '270 passed; 0 failed' in q['checks']['worker-tests']
    assert '149 passed; 0 failed' in q['checks']['fundamental-tests']
    assert '2 passed; 0 failed' in q['checks']['pre-network-tests']
    for path, value in q['sourceHashes'].items(): assert sha(L/'source'/path)==value
    assert sha(NATIVE/'libmoloch_replay_release.rlib')==q['nativeLibrarySha256']
    assert q['nativeLibrarySha256']==json.loads((J/'qualified.public.json').read_bytes())['nativeLibrarySha256']
    assert sha(RUNTIME/'libmoloch_live_runtime_release.rlib')==q['runtimeLibrarySha256']
    assert q['nativeEngineUnchanged'] is True
    source = (L / 'approved-source-commit.txt').read_text().strip()
    assert len(source) == 40 and all(c in '0123456789abcdef' for c in source)
    return q, source


def install():
    q, source = qualified(); new_sha = q['binarySha256']
    assert not Path('/proc/' + str(OLD_PID)).exists()
    recovery = json.loads((R/'pre-network-recovery.json').read_bytes())
    assert recovery['qid']==21832 and recovery['sdkTransitionVerified'] is True and recovery['stopPreserved'] is True and recovery['networkRequests']==0 and recovery['sameIntentRetryAuthorized'] is False
    system('stop', 'moloch-live.service')
    assert system('show', 'moloch-live.service', '-p', 'MainPID', '--value') == '0'
    stop_raw = (R / 'EXECUTION_STOP').read_bytes()
    stopped = json.loads(stop_raw)
    assert stopped['reason']=='LIVE_WRITE_NOT_CONFIRMED_NO_RETRY' and stopped['at']=='2026-10-22T14:12:56.980Z'
    assert not (R.parent / 'EXECUTION_STOP').exists()
    state_raw, pending_raw = (F / 'state.json').read_bytes(), (F / 'pending.json').read_bytes()
    state = json.loads(state_raw); assert json.loads(pending_raw).get('resolved') is True
    active_raw = Path(state['controlPath']).read_bytes(); assert sha(state['controlPath']) == state['controlSha256']
    active = json.loads(active_raw); assert active['activation']['binarySha256'] == OLD_SHA
    report_raw, head_raw = (B / 'report.json').read_bytes(), (B / 'head.json').read_bytes()
    report = json.loads(report_raw); dispatch = (R / 'dispatch.ndjson').read_bytes()
    attempted, ack, resolved, closed = set(), set(), set(), set()
    for line in dispatch.splitlines():
        e = json.loads(line)['event']; qid = e['qid']; kind = e['kind']
        if kind == 'attempt': attempted.add(qid)
        if kind in ('ack', 'recovered_ack'): ack.add(qid)
        if kind == 'closed': closed.add(qid)
        if kind in ('aborted_before_admission', 'local_final_guard_no_http', 'expired_exposure_reconciled', 'admitted_pre_network_not_sent'): resolved.add(qid)
    assert not attempted - ack - resolved, 'Original unknown acceptance must be resolved first'
    bridge_sha = sha(B / 'engine-upgrade-v1.json')
    prefix = dict(bytes=(B / 'events.jsonl').stat().st_size, sha256=sha(B / 'events.jsonl'))
    engine_identity=json.loads((J/'deployment.public.json').read_bytes())['engineIdentity']

    incident = RESUME_INCIDENT or R.parent / 'incidents' / ('r94-pre-network-' + datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ'))
    if RESUME_INCIDENT is None:
        incident.mkdir(mode=0o700)
        shutil.copyfile(B/'engine-upgrade-v1.json',incident/'engine-upgrade-v1.json')
    else:
        assert incident.is_absolute() and incident.resolve().parent == (R.parent/'incidents').resolve()
        assert incident.name.startswith('r94-pre-network-') and not incident.is_symlink()
        assert not (L/'installed.public.json').exists() and not (L/'deployment.public.json').exists()
        for name, raw in [('active-control.json',active_raw),('reference-state.json',state_raw),('reference-pending.json',pending_raw),('report.json',report_raw),('head.json',head_raw),('dispatch.ndjson',dispatch)]:
            assert (incident/name).read_bytes()==raw, 'RESUME_ORIGINAL_BOUNDARY_CHANGED'
    assert sha(incident/'engine-upgrade-v1.json')==bridge_sha
    for name, raw in [('active-control.json', active_raw), ('reference-state.json', state_raw), ('reference-pending.json', pending_raw), ('report.json', report_raw), ('head.json', head_raw), ('dispatch.ndjson', dispatch)]: fresh(incident / name, raw)
    release = R.parent.parent / 'releases' / ('tartarus-r94-pre-network-' + new_sha[:12]); release.mkdir(exist_ok=RESUME_INCIDENT is not None)
    binary = release / 'moloch-live-runtime'
    if binary.exists(): assert RESUME_INCIDENT is not None and sha(binary)==new_sha
    else: shutil.copy2(L/'moloch-live-runtime',binary)
    binary.chmod(0o700); assert sha(binary) == new_sha
    control = R.parent / 'moloch-live-tartarus-r94-pre-network-20261022.json'
    active['activation']['binarySha256'] = new_sha; fresh(control, active)
    if RESUME_INCIDENT is not None:
        assert 'Read-only preflight complete' in (L/'preflight.log').read_text(), 'RESUME_REQUIRES_COMPLETED_PREFLIGHT'
    else:
        with (L / 'preflight.log').open('x') as log:
            subprocess.run([str(binary), 'preflight', str(control)], check=True, stdout=log, stderr=subprocess.STDOUT)
    assert 'Read-only preflight complete' in (L / 'preflight.log').read_text()
    queue_root = R / 'account-queue'; manifest = json.loads((queue_root / 'account-queue-manifest.json').read_bytes())
    with (queue_root / manifest['journal']).open('rb') as f:
        f.seek(max(0, (queue_root / manifest['journal']).stat().st_size - 16 * 1024 * 1024)); tail = f.read()
    queue = json.loads(tail.splitlines()[-1])['state']; assert queue['active'] is None and not queue['unknown'] and queue['manual_review'] is False
    result = dict(observedUtc=datetime.now(timezone.utc).isoformat(), sourceCommit=source, binarySha256=new_sha, controlSha256=sha(control), originalFills=len(report['fills']), originalReportHash=report['hash'], originalFinancialPrefix=prefix, originalNativeHead=json.loads(head_raw), bridgeSha256=bridge_sha, incident=str(incident), nativeEngineUnchanged=True, originalFinancialStatePreserved=True, knownOpenOrders=sorted(ack-closed), monetaryCapsLimiterAndWindowsChanged=False, accountClientsCreated=0, ordersSubmittedByDeployment=0, freshTradingVerified=False, phase='installed_before_start')
    with open(active['ownerLock'], 'r+') as owner, open(B / 'writer.lock', 'r+') as writer, open(F / 'updater.lock', 'a') as lock:
        for f in (owner, writer, lock): fcntl.flock(f, fcntl.LOCK_EX | fcntl.LOCK_NB)
        for path, raw in [(R/'EXECUTION_STOP', stop_raw), (B/'report.json', report_raw), (B/'head.json', head_raw), (R/'dispatch.ndjson', dispatch), (F/'state.json', state_raw), (F/'pending.json', pending_raw)]: assert path.read_bytes() == raw
        assert sha(B/'engine-upgrade-v1.json') == bridge_sha
        unit_path = Path('/home/archon/.config/systemd/user/moloch-live.service'); unit = unit_path.read_bytes(); fresh(incident/'unit.original', unit)
        old_binary = R.parent.parent/'releases'/('tartarus-r93-rank-inventory-'+OLD_SHA[:12])/'moloch-live-runtime'
        assert unit.count(str(old_binary).encode()) == 1 and unit.count(str(OLD).encode()) == 1
        changed = unit.replace(str(old_binary).encode(), str(binary).encode()).replace(str(OLD).encode(), str(control).encode())
        temp = unit_path.with_name('.moloch-live.r94.next'); fresh(temp, changed)
        os.replace(temp, unit_path)
        os.rename(R/'EXECUTION_STOP', incident/'EXECUTION_STOP.original')
        result.update(engineIdentity=engine_identity,allPriorEngineBridgesPreserved=True,newBridgeSha256=bridge_sha)
        fresh(L/'installed.public.json', result)

        for directory in [incident,B,R,release,R.parent,unit_path.parent]:
            fd=os.open(directory,os.O_RDONLY);os.fsync(fd);os.close(fd)
    # Release copied report/dispatch before startup. The native chain is unchanged.
    del report, dispatch, report_raw
    import gc
    gc.collect()
    for args in [('daemon-reload',), ('reset-failed','moloch-live.service'), ('start','moloch-live.service')]: system(*args)
    pid = int(system('show','moloch-live.service','-p','MainPID','--value')); proc = Path('/proc')/str(pid)
    assert pid > 0 and sha(proc/'exe') == new_sha
    assert (proc/'cmdline').read_bytes().split(b'\0')[:-1] == [str(binary).encode(),b'run',str(control).encode()]
    ticks = int((proc/'stat').read_text().rsplit(') ',1)[1].split()[19])
    state.update(controlPath=str(control),controlSha256=sha(control)); temp=F/'state.r94.next'; fresh(temp,state); os.replace(temp,F/'state.json')
    config=json.loads((F/'config-r93.json').read_bytes()); updater=Path(config['updaterScript']); assert sha(updater)==config['updaterSha256']
    previous_updater=updater; updater=release/'run-live-reference-reload.py'; shutil.copy2(previous_updater,updater); assert sha(updater)==config['updaterSha256']
    config.update(sourceCommit=source,baseControl=str(control),baseControlSha256=sha(control),workerPid=pid,workerStartTicks=ticks,binary=str(binary),binarySha256=new_sha,initialGenerationSha256=state['generationSha256'],updaterScript=str(updater),updaterSha256=sha(updater))
    config_path=F/'config-r94.json'; fresh(config_path,config)
    with (F/'r94-updater.log').open('x') as log: child=subprocess.Popen(['python3','-u',str(updater),'--config',str(config_path)],stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
    result.update(observedUtc=datetime.now(timezone.utc).isoformat(),pid=pid,startTicks=ticks,referenceUpdaterPid=child.pid,actualExecutableAndArgumentsVerified=True,phase='restoring_original_ledger')
    fresh(L/'deployment.public.json',result); print(json.dumps(result),flush=True)


if __name__ == '__main__':
    os.umask(0o077); parser=argparse.ArgumentParser(); parser.add_argument('phase',choices=['install']); parser.add_argument('--resume-incident',type=Path); args=parser.parse_args(); RESUME_INCIDENT=args.resume_incident
    assert RESUME_INCIDENT is None or args.phase=='install'
    install()
