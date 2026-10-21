"""Install checked recurring-cohort release at an independently verified quiet boundary.

The old worker/updater must already be stopped. This script cannot select a STOP
to clear: stop-verified.json pins the exact operator-inspected planned-stop bytes.
It never submits or retries an order. Startup replays every original bridge.
"""
from pathlib import Path
from datetime import datetime, timezone
import argparse
import fcntl
import hashlib
import json
import os
import shutil
import subprocess

H = Path('/tmp/moloch-recurring-cohort-20261021')
R = Path('/home/archon/apps/moloch/shared/live-production-v1')
B = R / 'bank-all-markets-v1'
F = R.parent / 'fundamental-live-reference-20261021'
Q = R / 'account-queue'
OLD_SHA = '80c6094916c766dea93138669662df7ad0f2479a186c4ee64c8c3e08db8a3884'
SOURCE = '90fbfd9f8ef6759eaf24357fcc848e04725eafaa'
UPDATER = Path('/home/archon/apps/moloch/releases/live-reference-dcaaec81/run-live-reference-reload.py')

def sha(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()

def encoded(value):
    return (json.dumps(value, indent=2) + '\n').encode()

def fresh(path, raw):
    with path.open('xb') as stream:
        stream.write(raw)
        stream.flush()
        os.fsync(stream.fileno())

def system(*args):
    return subprocess.check_output(['systemctl', '--user', *args], text=True).strip()

def main(apply):
    os.umask(0o077)
    release = json.loads((H / 'RELEASE-COMPLETE.json').read_bytes())
    new_sha = sha(H / 'moloch-live-runtime')
    assert new_sha == release['binarySha256'] and release['cliLoads']
    for name, count in [('fundamental_policy', 137), ('ledger_snapshot', 3), ('alpha_proposal', 19), ('native', 131)]:
        assert f'{count} passed; 0 failed;' in (H / (name + '-tests.log')).read_text()
    assert sha(UPDATER) == 'c30af610216c51d7f8bba378f3fce8f6aac04b71102f7b79fee809a482fa2953'
    if not apply:
        print(json.dumps(dict(binarySha256=new_sha, sourceCommit=SOURCE, checksPassed=290, productionChanged=False)))
        return
    verified = json.loads((H / 'stop-verified.json').read_bytes())
    assert verified['previousPid'] == 1022216 and verified['plannedReplacement'] is True
    assert not Path('/proc/1022216').exists() and not Path('/proc/1022219').exists()
    assert system('show', 'moloch-live.service', '-p', 'MainPID', '--value') == '0'
    stop_raw = (R / 'EXECUTION_STOP').read_bytes()
    assert sha(R / 'EXECUTION_STOP') == verified['stopSha256']
    assert not (R.parent / 'EXECUTION_STOP').exists()
    state_raw, pending_raw = (F / 'state.json').read_bytes(), (F / 'pending.json').read_bytes()
    state, pending = json.loads(state_raw), json.loads(pending_raw)
    assert pending['resolved'] is True
    active_raw = Path(state['controlPath']).read_bytes()
    assert sha(state['controlPath']) == state['controlSha256']
    active = json.loads(active_raw)
    assert active['activation']['binarySha256'] == OLD_SHA
    control = R.parent / 'moloch-live-tartarus-r71-recurring-20261021.json'
    active['activation']['binarySha256'] = new_sha
    fresh(control, encoded(active))
    with (H / 'preflight.log').open('x') as log:
        subprocess.run([str(H / 'moloch-live-runtime'), 'preflight', str(control)], check=True, stdout=log, stderr=subprocess.STDOUT)
    assert 'Read-only preflight complete' in (H / 'preflight.log').read_text()
    manifest = json.loads((Q / 'account-queue-manifest.json').read_bytes())
    journal = Q / manifest['journal']
    with journal.open('rb') as stream:
        stream.seek(max(0, journal.stat().st_size - 16 * 1024 * 1024))
        tail = stream.read()
    assert tail.endswith(b'\n')
    queue = json.loads(tail.splitlines()[-1])['state']
    assert queue['active'] is None and not queue['unknown'] and queue['manual_review'] is False
    dispatch_raw = (R / 'dispatch.ndjson').read_bytes()
    attempted, ack, closed, resolved = set(), set(), set(), set()
    for line in dispatch_raw.splitlines():
        event = json.loads(line)['event']
        qid, kind = event['qid'], event['kind']
        if kind == 'attempt': attempted.add(qid)
        if kind in ('ack', 'recovered_ack'): ack.add(qid)
        if kind == 'closed': closed.add(qid)
        if kind in ('aborted_before_admission', 'local_final_guard_no_http', 'expired_exposure_reconciled'): resolved.add(qid)
    assert not (attempted - ack - resolved)
    proposal_path = H / 'upgrade.proposed.json'
    subprocess.run([str(H / 'propose-upgrade'), str(B), OLD_SHA, datetime.now(timezone.utc).isoformat(), str(proposal_path)], check=True)
    assert proposal_path.stat().st_size <= 256 * 1024 * 1024
    proposal = json.loads(proposal_path.read_bytes())
    prior_raw = (B / 'engine-upgrade-v1.json').read_bytes()
    prior = json.loads(prior_raw)
    assert proposal['upgrades'][:-1] == prior['upgrades']
    last = proposal['upgrades'][-1]
    head_raw, report_raw = (B / 'head.json').read_bytes(), (B / 'report.json').read_bytes()
    report = json.loads(report_raw)
    assert last['oldHead'] == json.loads(head_raw) and last['oldReportEnvelope'] == report
    assert last['journalPrefix'] == dict(bytes=(B / 'events.jsonl').stat().st_size, sha256=sha(B / 'events.jsonl'))
    assert last['approval']['oldBinarySha256'] == OLD_SHA
    incident = R.parent / 'incidents' / ('r71-recurring-' + datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ'))
    incident.mkdir(mode=0o700)
    for name, raw in [('active-control.json', active_raw), ('reference-state.json', state_raw), ('reference-pending.json', pending_raw),
                      ('report.json', report_raw), ('head.json', head_raw), ('dispatch.ndjson', dispatch_raw), ('engine-upgrade-v1.json', prior_raw)]:
        fresh(incident / name, raw)
    unit_path = Path('/home/archon/.config/systemd/user/moloch-live.service')
    unit = unit_path.read_bytes()
    fresh(incident / 'unit.original', unit)
    release_dir = R.parent.parent / 'releases' / ('tartarus-r71-recurring-' + new_sha[:12])
    release_dir.mkdir()
    binary = release_dir / 'moloch-live-runtime'
    shutil.copy2(H / 'moloch-live-runtime', binary)
    binary.chmod(0o700)
    assert sha(binary) == new_sha
    with open(active['ownerLock'], 'r+') as owner, open(B / 'writer.lock', 'r+') as writer, open(F / 'updater.lock', 'a') as updater:
        for lock in (owner, writer, updater): fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        for path, raw in [(R / 'EXECUTION_STOP', stop_raw), (B / 'head.json', head_raw), (B / 'report.json', report_raw),
                          (R / 'dispatch.ndjson', dispatch_raw), (F / 'state.json', state_raw), (F / 'pending.json', pending_raw), (B / 'engine-upgrade-v1.json', prior_raw)]:
            assert path.read_bytes() == raw
        temporary = B / 'engine-upgrade-v1.r71.next'
        fresh(temporary, proposal_path.read_bytes())
        os.replace(temporary, B / 'engine-upgrade-v1.json')
        old_binary = R.parent.parent / 'releases/tartarus-r70-history-bound-80c6094916c7/moloch-live-runtime'
        old_control = R.parent / 'moloch-live-tartarus-r70-history-bound-20261021.json'
        assert unit.count(str(old_binary).encode()) == 1 and unit.count(str(old_control).encode()) == 1
        changed = unit.replace(str(old_binary).encode(), str(binary).encode()).replace(str(old_control).encode(), str(control).encode())
        temporary = unit_path.with_name('.moloch-live.r71.next')
        fresh(temporary, changed)
        os.replace(temporary, unit_path)
        os.rename(R / 'EXECUTION_STOP', incident / 'EXECUTION_STOP.original')
        for directory in [incident, B, R, release_dir, R.parent, unit_path.parent]:
            fd = os.open(directory, os.O_RDONLY); os.fsync(fd); os.close(fd)
    for args in [('daemon-reload',), ('reset-failed', 'moloch-live.service'), ('start', 'moloch-live.service')]: system(*args)
    pid = int(system('show', 'moloch-live.service', '-p', 'MainPID', '--value'))
    process = Path('/proc') / str(pid)
    assert pid > 0 and sha(process / 'exe') == new_sha
    assert (process / 'cmdline').read_bytes().split(b'\0')[:-1] == [str(binary).encode(), b'run', str(control).encode()]
    ticks = int((process / 'stat').read_text().rsplit(') ', 1)[1].split()[19])
    state.update(controlPath=str(control), controlSha256=sha(control))
    temporary = F / 'state.r71.next'; fresh(temporary, encoded(state)); os.replace(temporary, F / 'state.json')
    config = json.loads((F / 'config-r70.json').read_bytes())
    config.update(sourceCommit=SOURCE, baseControl=str(control), baseControlSha256=sha(control), workerPid=pid,
        workerStartTicks=ticks, binary=str(binary), binarySha256=new_sha, initialGenerationSha256=state['generationSha256'])
    config_path = F / 'config-r71.json'; fresh(config_path, encoded(config))
    with (F / 'r71-updater.log').open('x') as log:
        child = subprocess.Popen(['python3', '-u', str(UPDATER), '--config', str(config_path)], stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
    result = dict(observedUtc=datetime.now(timezone.utc).isoformat(), sourceCommit=SOURCE, binarySha256=new_sha,
        pid=pid, startTicks=ticks, referenceUpdaterPid=child.pid, controlSha256=sha(control),
        engineIdentity=last['toEngineIdentity'], incident=str(incident), originalFinancialPrefix=last['journalPrefix'],
        originalNativeHead=json.loads(head_raw), originalReportHash=report['hash'], originalFills=len(report['fills']),
        knownOpenOrders=sorted(ack - closed), allPriorEngineBridgesPreserved=True, upgradeBytes=proposal_path.stat().st_size,
        monetaryCapsLimiterAndWindowsChanged=False, sdkUnknown=0, sdkActive=None, ordersSubmittedByDeployment=0,
        actualExecutableAndArgumentsVerified=True, phase='restoring_original_ledger', freshTradingVerified=False,
        recurringReviewsOptedIn=False)
    (H / 'deployment.public.json').write_bytes(encoded(result))
    print(json.dumps(result))

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--apply', action='store_true')
    main(parser.parse_args().apply)
