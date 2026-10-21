"""Install the checked R70 history repair at the exact stopped R69 boundary."""
from pathlib import Path
from datetime import datetime, timezone
import argparse, fcntl, hashlib, json, os, shutil, subprocess

D = Path('/tmp/moloch-upgrade-bound-r70-20261021')
P = Path('/tmp/moloch-native-checked-lookup-20261021')
R = Path('/home/archon/apps/moloch/shared/live-production-v1')
B = R / 'bank-all-markets-v1'
F = R.parent / 'fundamental-live-reference-20261021'
Q = R / 'account-queue'
OLD_SHA = '8b903763c346b805f8468eafe612fbee712988dae17fedc34635a5cb4d8b9c70'
NEW_SHA = json.loads((D/'RELEASE-VERIFIED.json').read_text())['binarySha256']
SOURCE = '91cb79503632a383eedcf037e9e21b71d2927318'
UPDATER = Path('/home/archon/apps/moloch/releases/live-reference-dcaaec81/run-live-reference-reload.py')

def sha(path):
    with Path(path).open('rb') as handle:
        return hashlib.file_digest(handle, 'sha256').hexdigest()

def write_new(path, raw):
    with path.open('xb') as handle:
        handle.write(raw)
        handle.flush()
        os.fsync(handle.fileno())

def encoded(value):
    return (json.dumps(value, indent=2) + '\n').encode()

def main(apply):
    os.umask(0o077)
    assert sha(D/'moloch-live-runtime') == NEW_SHA
    assert json.loads((D/'RELEASE-VERIFIED.json').read_text())['binarySha256'] == NEW_SHA
    parity = json.loads((P/'PARITY-COMPLETE.json').read_text())
    assert parity['originalReportFills'] == 430 and parity['productionChanged'] is False
    assert '1 passed; 0 failed' in (P/'fresh-prefix-parity.log').read_text()
    assert '17 passed; 0 failed' in (P/'sdk-pre-admission-tests.log').read_text()
    assert '228 passed; 0 failed' in (P/'worker-tests.log').read_text()
    assert '131 passed; 0 failed' in (D/'native-tests.log').read_text()
    assert '1 passed; 0 failed' in (D/'chain-checked.log').read_text()
    assert 'Read-only preflight complete' in (D/'r70-preflight.log').read_text()
    assert subprocess.check_output(['systemctl','--user','show','moloch-live.service','-p','MainPID','--value'], text=True).strip() == '0'
    assert not Path('/proc/1021520').exists() and not Path('/proc/1021523').exists()
    stop_raw = (R/'EXECUTION_STOP').read_bytes()
    assert json.loads(stop_raw) == {'action':'no automatic write retry or activation renewal','at':'2026-10-21T18:43:28.650Z','reason':'UPGRADE_FILE_BOUND'}
    assert not (R.parent/'EXECUTION_STOP').exists()
    state_raw = (F/'state.json').read_bytes()
    state = json.loads(state_raw)
    pending_raw = (F/'pending.json').read_bytes()
    assert json.loads(pending_raw)['resolved'] is True and state['commits'] == 38
    active_raw = Path(state['controlPath']).read_bytes()
    assert sha(state['controlPath']) == state['controlSha256']
    active = json.loads(active_raw)
    assert active['activation']['binarySha256'] == OLD_SHA
    assert sha(UPDATER) == 'c30af610216c51d7f8bba378f3fce8f6aac04b71102f7b79fee809a482fa2953'
    report_raw = (B/'report.json').read_bytes()
    head_raw = (B/'head.json').read_bytes()
    report = json.loads(report_raw)
    assert len(report['fills']) == 433 and json.loads(head_raw)['sequence'] == 773995
    bridge_raw = (B/'engine-upgrade-v1.json').read_bytes()
    old_bridge = json.loads(bridge_raw)
    proposed_raw = (D/'upgrade.proposed.json').read_bytes()
    assert len(proposed_raw) <= 256*1024*1024, 'R70 upgrade parser byte bound'
    proposed = json.loads(proposed_raw)
    assert proposed['upgrades'][:-1] == old_bridge['upgrades']
    last = proposed['upgrades'][-1]
    assert last['oldHead'] == json.loads(head_raw) and last['oldReportEnvelope'] == report
    assert last['approval']['oldBinarySha256'] == OLD_SHA
    assert last['toEngineIdentity'] == json.loads((D/'RELEASE-VERIFIED.json').read_text())['engineIdentity']
    assert last['journalPrefix'] == {'bytes': (B/'events.jsonl').stat().st_size, 'sha256': sha(B/'events.jsonl')}
    dispatch_raw = (R/'dispatch.ndjson').read_bytes()
    attempted, acknowledged, closed, resolved = set(), set(), set(), set()
    recovered = []
    for raw in dispatch_raw.splitlines():
        event = json.loads(raw)['event']; qid = event['qid']; kind = event['kind']
        if kind == 'attempt': attempted.add(qid)
        if kind in ('ack','recovered_ack'): acknowledged.add(qid)
        if kind == 'closed': closed.add(qid)
        if kind in ('local_final_guard_no_http','aborted_before_admission','expired_exposure_reconciled'): resolved.add(qid)
        if kind == 'aborted_before_admission' and qid == 20264: recovered.append(event)
    assert len(recovered) == 1 and not (attempted - acknowledged - resolved) and 20264 not in acknowledged
    recovery = json.loads((P/'recovery.public.json').read_text())
    assert recovery['recoveredOriginalQids'] == [20264] and recovery['retainedExposureQidsRequireStartupVerification'] == [18533]
    manifest = json.loads((Q/'account-queue-manifest.json').read_text())
    with (Q/manifest['journal']).open('rb') as handle:
        handle.seek(max(0, (Q/manifest['journal']).stat().st_size - 16777216))
        tail = handle.read()
    assert tail.endswith(b'\n')
    queue = json.loads(tail.splitlines()[-1])['state']
    assert queue['active'] is None and not queue['unknown'] and queue['manual_review'] is False
    result = dict(observedUtc=datetime.now(timezone.utc).isoformat(), sourceCommit=SOURCE,
        sourceComposition='R69 checked source plus checked Session upgrade-history parsing repair', binarySha256=NEW_SHA,
        originalFinancialPrefix=last['journalPrefix'], originalNativeHead=json.loads(head_raw),
        originalReportHash=report['hash'], originalFills=433, nativeEngineIdentity=last['toEngineIdentity'],
        knownOpenOrders=sorted(acknowledged-closed), acknowledgedOrders=len(acknowledged), terminalOrders=len(closed),
        allPriorEngineBridgesPreserved=True, originalFifoAndHistoryPreserved=True,
        exactActiveControlOverlayPreserved=True, monetaryCapsLimiterAndWindowsChanged=False,
        originalUnsentQid=20264, sameIntentRetried=False, retainedExposureRequiresStartupVerification=[18533],
        sdkUnknown=0, sdkActive=None, ordersSubmittedByDeployment=0, freshTradingVerified=False)
    if not apply:
        print(json.dumps(result)); return
    incident = R.parent/'incidents'/('r70-history-bound-'+datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ'))
    incident.mkdir(mode=0o700)
    originals = {'active-control.json':active_raw, 'reference-state.json':state_raw, 'reference-pending.json':pending_raw,
        'report.json':report_raw, 'head.json':head_raw, 'dispatch.ndjson':dispatch_raw, 'engine-upgrade-v1.json':bridge_raw,
        'stream-recovery-budget.json':(R/'stream-recovery-budget.json').read_bytes()}
    for name, raw in originals.items(): write_new(incident/name, raw)
    with open(active['ownerLock'],'r+') as owner, open(B/'writer.lock','r+') as writer, open(F/'updater.lock','a') as updater:
        for lock in (owner,writer,updater): fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
        for path, raw in [(R/'EXECUTION_STOP',stop_raw),(R/'dispatch.ndjson',dispatch_raw),(B/'report.json',report_raw),(B/'head.json',head_raw),(F/'state.json',state_raw),(F/'pending.json',pending_raw),(B/'engine-upgrade-v1.json',bridge_raw)]: assert path.read_bytes() == raw
        release = R.parent.parent/'releases'/('tartarus-r70-history-bound-'+NEW_SHA[:12]); release.mkdir()
        binary = release/'moloch-live-runtime'; shutil.copy2(D/'moloch-live-runtime',binary); binary.chmod(0o700)
        assert sha(binary) == NEW_SHA
        control = R.parent/'moloch-live-tartarus-r70-history-bound-20261021.json'
        active['activation']['binarySha256'] = NEW_SHA
        write_new(control,encoded(active))
        temporary = B/'engine-upgrade-v1.r70.next'; write_new(temporary,proposed_raw); os.replace(temporary,B/'engine-upgrade-v1.json')
        unit_path = Path('/home/archon/.config/systemd/user/moloch-live.service')
        unit = unit_path.read_bytes(); write_new(incident/'unit.original',unit)
        old_binary = R.parent.parent/'releases'/('tartarus-r69-throughput-'+OLD_SHA[:12])/'moloch-live-runtime'
        old_control = R.parent/'moloch-live-tartarus-r69-throughput-20261021.json'
        assert unit.count(str(old_binary).encode()) == 1 and unit.count(str(old_control).encode()) == 1
        changed = unit.replace(str(old_binary).encode(),str(binary).encode()).replace(str(old_control).encode(),str(control).encode())
        temporary = unit_path.with_name('.moloch-live.r70.next'); write_new(temporary,changed); os.replace(temporary,unit_path)
        os.rename(R/'EXECUTION_STOP',incident/'EXECUTION_STOP.original')
        for directory in [incident,B,R,release,R.parent,unit_path.parent]:
            fd = os.open(directory,os.O_RDONLY); os.fsync(fd); os.close(fd)
        result.update(incident=str(incident),controlSha256=sha(control),phase='installed_before_start')
        (D/'deployment.public.json').write_bytes(encoded(result))
    for command in (['daemon-reload'],['reset-failed','moloch-live.service'],['start','moloch-live.service']): subprocess.run(['systemctl','--user',*command],check=True)
    pid = int(subprocess.check_output(['systemctl','--user','show','moloch-live.service','-p','MainPID','--value'],text=True))
    process = Path('/proc')/str(pid)
    assert pid > 0 and sha(process/'exe') == NEW_SHA
    assert (process/'cmdline').read_bytes().split(b'\0')[:-1] == [str(binary).encode(),b'run',str(control).encode()]
    ticks = int((process/'stat').read_text().rsplit(') ',1)[1].split()[19])
    state.update(controlPath=str(control),controlSha256=sha(control))
    temporary = F/'state.r70.next'; write_new(temporary,encoded(state)); os.replace(temporary,F/'state.json')
    config = json.loads((F/'config-r69.json').read_text())
    config.update(sourceCommit=SOURCE,baseControl=str(control),baseControlSha256=sha(control),workerPid=pid,workerStartTicks=ticks,binary=str(binary),binarySha256=NEW_SHA,initialGenerationSha256=state['generationSha256'])
    updater_config = F/'config-r70.json'; write_new(updater_config,encoded(config))
    with (F/'r70-updater.log').open('x') as log: child = subprocess.Popen(['python3','-u',str(UPDATER),'--config',str(updater_config)],stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
    result.update(observedUtc=datetime.now(timezone.utc).isoformat(),pid=pid,startTicks=ticks,referenceUpdaterPid=child.pid,phase='restoring_original_ledger',actualExecutableAndArgumentsVerified=True)
    (D/'deployment.public.json').write_bytes(encoded(result)); print(json.dumps(result))

if __name__ == '__main__':
    parser = argparse.ArgumentParser(); parser.add_argument('--apply',action='store_true'); main(parser.parse_args().apply)
