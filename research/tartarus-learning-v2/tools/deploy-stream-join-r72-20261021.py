"""Replace the stopped R71 worker; preserve its native engine and financial state."""
from pathlib import Path
from datetime import datetime, timezone
import argparse
import fcntl
import hashlib
import json
import os
import shutil
import signal
import subprocess
import time

J = Path('/tmp/moloch-stream-join-r72-20261021')
H = Path('/tmp/moloch-recurring-cohort-20261021')
R = Path('/home/archon/apps/moloch/shared/live-production-v1')
B = R / 'bank-all-markets-v1'
F = R.parent / 'fundamental-live-reference-20261021'
OLD = R.parent / 'moloch-live-tartarus-r71-recurring-20261021.json'
UPDATER = Path('/home/archon/apps/moloch/releases/live-reference-dcaaec81/run-live-reference-reload.py')
OLD_SHA = 'e5e1cdf14c3181b159b79d87ca5f7f12f4180ce272b5851f6a17fbc8c1abcdc7'


def sha(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def fresh(path, value):
    raw = value if isinstance(value, bytes) else (json.dumps(value, indent=2) + '\n').encode()
    with path.open('xb') as stream:
        stream.write(raw)
        stream.flush()
        os.fsync(stream.fileno())


def system(*args):
    return subprocess.check_output(['systemctl', '--user', *args], text=True).strip()


def main(apply):
    os.umask(0o077)
    ready = json.loads((J / 'qualified.public.json').read_bytes())
    new_sha = sha(J / 'moloch-live-runtime')
    assert ready['binarySha256'] == new_sha and ready['cliLoads'] and ready['testsPassed'] == 5
    assert '5 passed; 0 failed;' in (J / 'join-test-result.log').read_text()
    assert sha(H / 'libmoloch_replay_release.rlib') == ready['nativeLibrarySha256']
    assert sha(H / 'libmoloch_live_runtime_release.rlib') == ready['runtimeLibrarySha256']
    source = (J / 'approved-source-commit.txt').read_text().strip()
    assert len(source) == 40
    assert not Path('/proc/1028075').exists() and system('show', 'moloch-live.service', '-p', 'MainPID', '--value') == '0'
    stop = (R / 'EXECUTION_STOP').read_bytes()
    assert json.loads(stop) == dict(action='no automatic write retry or activation renewal', at='2026-10-21T20:06:41.093Z', reason='LIVE_STREAM_JOIN_FAILED')
    assert not (R.parent / 'EXECUTION_STOP').exists()
    assert sha(UPDATER) == 'c30af610216c51d7f8bba378f3fce8f6aac04b71102f7b79fee809a482fa2953'
    state_raw, pending_raw = (F / 'state.json').read_bytes(), (F / 'pending.json').read_bytes()
    state = json.loads(state_raw)
    assert json.loads(pending_raw)['resolved'] is True
    active_raw = Path(state['controlPath']).read_bytes()
    assert sha(state['controlPath']) == state['controlSha256']
    active = json.loads(active_raw)
    assert active['activation']['binarySha256'] == OLD_SHA
    head_raw, report_raw = (B / 'head.json').read_bytes(), (B / 'report.json').read_bytes()
    report = json.loads(report_raw)
    original = json.loads((R.parent / 'incidents/r71-recurring-20261021T194753Z/report.json').read_bytes())
    assert report['fills'] == original['fills'] and len(report['fills']) == 452
    for key in ['cash', 'positions', 'ownedCost', 'realized', 'fees']:
        assert report['report'][key] == original['report'][key]
    bridge_sha = sha(B / 'engine-upgrade-v1.json')
    dispatch = (R / 'dispatch.ndjson').read_bytes()
    attempts, ack, closed, resolved = set(), set(), set(), set()
    for line in dispatch.splitlines():
        e = json.loads(line)['event']
        qid, kind = e['qid'], e['kind']
        if kind == 'attempt': attempts.add(qid)
        if kind in ['ack', 'recovered_ack']: ack.add(qid)
        if kind == 'closed': closed.add(qid)
        if kind in ['aborted_before_admission', 'local_final_guard_no_http', 'expired_exposure_reconciled']: resolved.add(qid)
    assert not attempts - ack - resolved
    result = dict(observedUtc=datetime.now(timezone.utc).isoformat(), sourceCommit=source, binarySha256=new_sha,
        originalFills=452, originalNativeHead=json.loads(head_raw), originalReportHash=report['hash'],
        bridgeSha256=bridge_sha, nativeEngineUnchanged=True, runtimeLibraryUnchanged=True,
        originalFinancialStatePreserved=True, knownOpenOrders=sorted(ack - closed),
        monetaryCapsLimiterAndWindowsChanged=False, accountClientsCreated=0, ordersSubmittedByDeployment=0,
        freshTradingVerified=False, recurringReviewsOptedIn=False, phase='checked_before_install')
    if not apply:
        print(json.dumps(result)); return
    system('stop', 'moloch-live.service')
    updater = Path('/proc/1028078')
    if updater.exists():
        assert (updater / 'cmdline').read_bytes().split(b'\0')[:-1] == [b'python3', b'-u', str(UPDATER).encode(), b'--config', str(F / 'config-r71.json').encode()]
        os.kill(1028078, signal.SIGTERM)
        for _ in range(50):
            if not updater.exists(): break
            time.sleep(.1)
        assert not updater.exists()
    control = R.parent / 'moloch-live-tartarus-r72-stream-join-20261021.json'
    active['activation']['binarySha256'] = new_sha
    fresh(control, active)
    with (J / 'preflight.log').open('x') as log:
        subprocess.run([str(J / 'moloch-live-runtime'), 'preflight', str(control)], check=True, stdout=log, stderr=subprocess.STDOUT)
    assert 'Read-only preflight complete' in (J / 'preflight.log').read_text()
    queue_root = R / 'account-queue'
    manifest = json.loads((queue_root / 'account-queue-manifest.json').read_bytes())
    journal = queue_root / manifest['journal']
    with journal.open('rb') as stream:
        stream.seek(max(0, journal.stat().st_size - 16 * 1024 * 1024)); tail = stream.read()
    assert tail.endswith(b'\n')
    queue = json.loads(tail.splitlines()[-1])['state']
    assert queue['active'] is None and not queue['unknown'] and queue['manual_review'] is False
    incident = R.parent / 'incidents' / ('r72-stream-join-' + datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ'))
    incident.mkdir(mode=0o700)
    for name, raw in [('active-control.json', active_raw), ('reference-state.json', state_raw), ('reference-pending.json', pending_raw),
                      ('report.json', report_raw), ('head.json', head_raw), ('dispatch.ndjson', dispatch)]:
        fresh(incident / name, raw)
    prefix = dict(bytes=(B / 'events.jsonl').stat().st_size, sha256=sha(B / 'events.jsonl'))
    release = R.parent.parent / 'releases' / ('tartarus-r72-stream-join-' + new_sha[:12])
    release.mkdir(); binary = release / 'moloch-live-runtime'
    shutil.copy2(J / 'moloch-live-runtime', binary); binary.chmod(0o700)
    assert sha(binary) == new_sha
    with open(active['ownerLock'], 'r+') as owner, open(B / 'writer.lock', 'r+') as writer, open(F / 'updater.lock', 'a') as lock:
        for handle in [owner, writer, lock]: fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
        for path, raw in [(R / 'EXECUTION_STOP', stop), (B / 'head.json', head_raw), (B / 'report.json', report_raw),
                          (R / 'dispatch.ndjson', dispatch), (F / 'state.json', state_raw), (F / 'pending.json', pending_raw)]:
            assert path.read_bytes() == raw
        assert sha(B / 'engine-upgrade-v1.json') == bridge_sha
        unit_path = Path('/home/archon/.config/systemd/user/moloch-live.service')
        unit = unit_path.read_bytes(); fresh(incident / 'unit.original', unit)
        old_binary = R.parent.parent / 'releases/tartarus-r71-recurring-e5e1cdf14c31/moloch-live-runtime'
        assert unit.count(str(old_binary).encode()) == 1 and unit.count(str(OLD).encode()) == 1
        changed = unit.replace(str(old_binary).encode(), str(binary).encode()).replace(str(OLD).encode(), str(control).encode())
        temporary = unit_path.with_name('.moloch-live.r72.next'); fresh(temporary, changed); os.replace(temporary, unit_path)
        os.rename(R / 'EXECUTION_STOP', incident / 'EXECUTION_STOP.original')
        result.update(incident=str(incident), originalFinancialPrefix=prefix, controlSha256=sha(control), sdkUnknown=0, sdkActive=None)
        fresh(J / 'installed.public.json', result)
    for args in [('daemon-reload',), ('reset-failed', 'moloch-live.service'), ('start', 'moloch-live.service')]: system(*args)
    pid = int(system('show', 'moloch-live.service', '-p', 'MainPID', '--value'))
    process = Path('/proc') / str(pid)
    assert pid > 0 and sha(process / 'exe') == new_sha
    assert (process / 'cmdline').read_bytes().split(b'\0')[:-1] == [str(binary).encode(), b'run', str(control).encode()]
    ticks = int((process / 'stat').read_text().rsplit(') ', 1)[1].split()[19])
    state.update(controlPath=str(control), controlSha256=sha(control))
    temporary = F / 'state.r72.next'; fresh(temporary, state); os.replace(temporary, F / 'state.json')
    config = json.loads((F / 'config-r71.json').read_bytes())
    config.update(sourceCommit=source, baseControl=str(control), baseControlSha256=sha(control), workerPid=pid,
        workerStartTicks=ticks, binary=str(binary), binarySha256=new_sha, initialGenerationSha256=state['generationSha256'])
    config_path = F / 'config-r72.json'; fresh(config_path, config)
    with (F / 'r72-updater.log').open('x') as log:
        child = subprocess.Popen(['python3', '-u', str(UPDATER), '--config', str(config_path)], stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
    result.update(observedUtc=datetime.now(timezone.utc).isoformat(), pid=pid, startTicks=ticks,
        referenceUpdaterPid=child.pid, actualExecutableAndArgumentsVerified=True, phase='restoring_original_ledger')
    (J / 'deployment.public.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(); parser.add_argument('--apply', action='store_true'); main(parser.parse_args().apply)
