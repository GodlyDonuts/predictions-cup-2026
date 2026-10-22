"""One-owner R80->R81 replacement with unchanged native engine and finance."""
from pathlib import Path
from datetime import datetime, timezone
import argparse, fcntl, hashlib, json, os, shutil, signal, subprocess, time

L = Path('/tmp/moloch-exit-audit-r81-20261022')
J = Path('/tmp/moloch-managed-age-r80-20261022')
R = Path('/home/archon/apps/moloch/shared/live-production-v1')
B = R / 'bank-all-markets-v1'
F = R.parent / 'fundamental-live-reference-20261021'
OLD = R.parent / 'moloch-live-tartarus-r80-managed-age-20261022.json'
OLD_SHA = '94fc09256728e09aed80f78775220446222641fdcdd820f55858fbe3f0572721'
OLD_PID = 1057647
OLD_TICKS = 60260332


def sha(path):
    with Path(path).open('rb') as f:
        return hashlib.file_digest(f, 'sha256').hexdigest()


def fresh(path, value):
    raw = value if isinstance(value, bytes) else (json.dumps(value, indent=2) + '\n').encode()
    with path.open('xb') as f:
        f.write(raw); f.flush(); os.fsync(f.fileno())


def system(*args):
    return subprocess.check_output(['systemctl', '--user', *args], text=True).strip()


def qualified():
    q = json.loads((L / 'qualified.public.json').read_bytes())
    assert sha(L / 'moloch-live-runtime') == q['binarySha256'] and q['cliLoads']
    assert len(q['tests']) == 3 and all('0 failed' in x and 'test result: ok.' in x for x in q['tests'])
    assert q['nativeEngineChanged'] is False
    assert sha(L/'reference-updater/run-live-reference-reload.py') == q['referenceUpdaterSha256']
    assert q['referenceUpdaterTestsPassed'] == 8
    assert sha(L / 'source/moloch/live-runtime/src/worker.rs') == q['sourceSha256']['moloch/live-runtime/src/worker.rs']
    previous = json.loads((J / 'qualified.public.json').read_bytes())
    assert q['nativeLibrarySha256'] == previous['nativeLibrarySha256'] and q['nativeEngineChanged'] is False
    assert sha(L/'libmoloch_live_runtime_release.rlib') == q['runtimeLibrarySha256']
    assert sha(L/'source/moloch/live-runtime/src/fundamental_policy.rs') == q['sourceSha256']['moloch/live-runtime/src/fundamental_policy.rs']
    source = (L / 'approved-source-commit.txt').read_text().strip()
    assert len(source) == 40 and all(c in '0123456789abcdef' for c in source)
    return q, source


def stop():
    q, source = qualified()
    p = Path('/proc') / str(OLD_PID)
    assert sha(p / 'exe') == OLD_SHA and int((p / 'stat').read_text().rsplit(') ', 1)[1].split()[19]) == OLD_TICKS
    assert not (R / 'EXECUTION_STOP').exists() and not (R.parent / 'EXECUTION_STOP').exists()
    config = F / 'config-r80.json'
    u = json.loads((J / 'deployment.public.json').read_bytes())['referenceUpdaterPid']
    progress = json.loads((R / 'progress.json').read_bytes())
    assert progress.get('pid') == OLD_PID and progress.get('phase') == 'trading', 'Do not interrupt original-ledger restoration'
    assert (J / 'fresh-live.public.json').exists(), 'Verify current release before replacement'
    proc = Path('/proc') / str(u)
    assert str(config).encode() in (proc / 'cmdline').read_bytes()
    deadline = time.monotonic() + 240
    while True:
        assert time.monotonic() < deadline, 'No quiet updater boundary; worker unchanged'
        if json.loads((F / 'pending.json').read_bytes()).get('resolved') is True:
            os.kill(u, signal.SIGSTOP)
            quiet = json.loads((F / 'pending.json').read_bytes()).get('resolved') is True
            quiet = quiet and not subprocess.run(['ps', '-o', 'pid=', '--ppid', str(u)], text=True, capture_output=True).stdout.strip()
            if quiet: break
            os.kill(u, signal.SIGCONT)
        time.sleep(.2)
    os.kill(u, signal.SIGTERM); os.kill(u, signal.SIGCONT)
    deadline = time.monotonic() + 10
    while proc.exists():
        assert time.monotonic() < deadline
        time.sleep(.1)
    with (F / 'updater.lock').open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        assert json.loads((F / 'pending.json').read_bytes()).get('resolved') is True
    request = dict(at=datetime.now(timezone.utc).isoformat(), reason='planned_R81_exit_audit_replacement', sourceCommit=source, previousPid=OLD_PID, previousStartTicks=OLD_TICKS)
    fresh(L / 'planned-stop.original.json', request); fresh(R / 'EXECUTION_STOP', request)
    print('PLANNED_STOP_SET', flush=True)
    deadline = time.monotonic() + 300
    while p.exists():
        assert time.monotonic() < deadline, 'No forced kill; worker must drain'
        time.sleep(.5)
    fresh(L / 'stopped.public.json', dict(observedUtc=datetime.now(timezone.utc).isoformat(), previousPid=OLD_PID, stop=json.loads((R / 'EXECUTION_STOP').read_bytes())))
    print('WORKER_DRAINED', flush=True)


def install():
    q, source = qualified(); new_sha = q['binarySha256']
    assert (L / 'stopped.public.json').exists() and not Path('/proc/' + str(OLD_PID)).exists()
    system('stop', 'moloch-live.service')
    assert system('show', 'moloch-live.service', '-p', 'MainPID', '--value') == '0'
    stop_raw = (R / 'EXECUTION_STOP').read_bytes()
    stopped = json.loads(stop_raw)
    assert stopped['reason'] in ['planned_R81_exit_audit_replacement', 'LIVE_EXECUTION_STOP', 'LIVE_EXIT_REFRESH_CONTROL_OR_STOP', 'LIVE_PROFITABLE_DEPTH_CONTROL_OR_STOP', 'LIVE_STREAM_RECOVERY_CONTROL_OR_STOP', 'LIVE_READ_RECOVERY_CONTROL_OR_STOP']
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
        if kind in ('aborted_before_admission', 'local_final_guard_no_http', 'expired_exposure_reconciled'): resolved.add(qid)
    assert not attempted - ack - resolved, 'Original unknown acceptance must be resolved first'
    bridge_sha = sha(B / 'engine-upgrade-v1.json')
    prefix = dict(bytes=(B / 'events.jsonl').stat().st_size, sha256=sha(B / 'events.jsonl'))
    incident = R.parent / 'incidents' / ('r81-exit-audit-' + datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ')); incident.mkdir(mode=0o700)
    for name, raw in [('active-control.json', active_raw), ('reference-state.json', state_raw), ('reference-pending.json', pending_raw), ('report.json', report_raw), ('head.json', head_raw), ('dispatch.ndjson', dispatch)]: fresh(incident / name, raw)
    release = R.parent.parent / 'releases' / ('tartarus-r81-exit-audit-' + new_sha[:12]); release.mkdir()
    binary = release / 'moloch-live-runtime'; shutil.copy2(L / 'moloch-live-runtime', binary); binary.chmod(0o700); assert sha(binary) == new_sha
    control = R.parent / 'moloch-live-tartarus-r81-exit-audit-20261022.json'
    active['activation']['binarySha256'] = new_sha; fresh(control, active)
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
        old_binary = R.parent.parent/'releases/tartarus-r80-managed-age-94fc09256728/moloch-live-runtime'
        assert unit.count(str(old_binary).encode()) == 1 and unit.count(str(OLD).encode()) == 1
        changed = unit.replace(str(old_binary).encode(), str(binary).encode()).replace(str(OLD).encode(), str(control).encode())
        temp = unit_path.with_name('.moloch-live.r81.next'); fresh(temp, changed); os.replace(temp, unit_path)
        os.rename(R/'EXECUTION_STOP', incident/'EXECUTION_STOP.original'); fresh(L/'installed.public.json', result)
    for args in [('daemon-reload',), ('reset-failed','moloch-live.service'), ('start','moloch-live.service')]: system(*args)
    pid = int(system('show','moloch-live.service','-p','MainPID','--value')); proc = Path('/proc')/str(pid)
    assert pid > 0 and sha(proc/'exe') == new_sha
    assert (proc/'cmdline').read_bytes().split(b'\0')[:-1] == [str(binary).encode(),b'run',str(control).encode()]
    ticks = int((proc/'stat').read_text().rsplit(') ',1)[1].split()[19])
    state.update(controlPath=str(control),controlSha256=sha(control)); temp=F/'state.r81.next'; fresh(temp,state); os.replace(temp,F/'state.json')
    config=json.loads((F/'config-r80.json').read_bytes()); updater=Path(config['updaterScript']); assert sha(updater)==config['updaterSha256']
    updater=release/'run-live-reference-reload.py'; shutil.copy2(L/'reference-updater/run-live-reference-reload.py',updater); assert sha(updater)==q['referenceUpdaterSha256']
    config.update(sourceCommit=source,baseControl=str(control),baseControlSha256=sha(control),workerPid=pid,workerStartTicks=ticks,binary=str(binary),binarySha256=new_sha,initialGenerationSha256=state['generationSha256'],updaterScript=str(updater),updaterSha256=sha(updater))
    config_path=F/'config-r81.json'; fresh(config_path,config)
    with (F/'r81-updater.log').open('x') as log: child=subprocess.Popen(['python3','-u',str(updater),'--config',str(config_path)],stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
    result.update(observedUtc=datetime.now(timezone.utc).isoformat(),pid=pid,startTicks=ticks,referenceUpdaterPid=child.pid,actualExecutableAndArgumentsVerified=True,phase='restoring_original_ledger')
    fresh(L/'deployment.public.json',result); print(json.dumps(result),flush=True)


if __name__ == '__main__':
    os.umask(0o077); parser=argparse.ArgumentParser(); parser.add_argument('phase',choices=['stop','install']); args=parser.parse_args()
    stop() if args.phase=='stop' else install()
