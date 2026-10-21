"""Resolve one expired reload bound to the confirmed-dead R68 process, offline."""
from pathlib import Path
import fcntl, hashlib, json, os, subprocess, time

R = Path('/home/archon/apps/moloch/shared/live-production-v1')
F = R.parent/'fundamental-live-reference-20261021'
D = Path('/tmp/moloch-native-checked-lookup-20261021')
sha = lambda raw: hashlib.sha256(raw).hexdigest()
raw = (F/'pending.json').read_bytes(); pending = json.loads(raw)
state_raw = (F/'state.json').read_bytes(); state = json.loads(state_raw)
config = json.loads((F/'config-r68.json').read_text())
control = json.loads(Path(state['controlPath']).read_text())
assert state['commits'] == 38 and config['workerPid'] == 1018590
assert not Path('/proc/1018590').exists() and not Path('/proc/1018593').exists()
assert subprocess.check_output(['systemctl','--user','show','moloch-live.service','-p','MainPID','--value'],text=True).strip() == '0'
assert (R/'EXECUTION_STOP').exists()
with open(control['ownerLock'],'r+') as owner, open(F/'updater.lock','a') as updater:
    for lock in (owner,updater): fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
    request_raw = (Path(pending['staged']['controlPath']).parent/'request.json').read_bytes()
    request = json.loads(request_raw); wanted = pending['expectedGenerationSha256']
    assert sha(request_raw) == wanted and state['generationSha256'] == pending['previousGenerationSha256']
    for key in ('workerPid','workerStartTicks','baseControlSha256'): assert request[key] == config[key]
    assert request['previousGenerationSha256'] == state['generationSha256']
    assert request['expiresAtMs'] == pending['expiresAtMs'] < time.time_ns()//1_000_000
    assert request['controlPath'] == pending['staged']['controlPath'] and request['controlSha256'] == sha(Path(request['controlPath']).read_bytes())
    cohort = (R/'fundamental.ndjson').read_bytes(); assert cohort.endswith(b'\n')
    generations = []
    for line in cohort.splitlines():
        row = json.loads(line)
        assert row['manifestHash'] == request['bankManifestSha256'] and row['runId'] == request['runId']
        if row['event'].get('kind') == 'review_generation': generations.append(row['event']['generation']['generationSha256'])
    assert generations[-1] == state['generationSha256'] and wanted not in generations
    view = json.loads((R/'tartarus-current-native-state.json').read_text())
    assert view['generationSha256'] == state['generationSha256']
    proof = dict(outcome='expired_uncommitted_stopped_owner',requestSha256=wanted,
        previousGenerationSha256=state['generationSha256'],committedGenerationSha256=state['generationSha256'],
        observedAtMs=time.time_ns()//1_000_000,workerPid=1018590,workerStartTicks=config['workerStartTicks'],
        originalCohortPrefixBytes=len(cohort),originalCohortPrefixSha256=sha(cohort),
        originalPendingSha256=sha(raw),originalStateSha256=sha(state_raw),
        requestExpired=True,originalWorkerConfirmedDead=True,exclusiveOwnerAndUpdaterLocksHeld=True,
        originalRequestRetained=True,requestResent=False,accountRequests=0,financialStateChanged=False)
    for name,data in [('reload-request-'+wanted+'.json',raw),('reload-resolution-'+wanted+'.json',json.dumps(proof,indent=2).encode()),('r68-state-before-resolution.json',state_raw)]:
        with (F/name).open('xb') as output: output.write(data); output.flush(); os.fsync(output.fileno())
    assert (F/'pending.json').read_bytes() == raw and (F/'state.json').read_bytes() == state_raw
    state.update(index=state['index']+1,lastResolvedReloadRequestSha256=wanted)
    resolved={'resolved':True,'generationSha256':state['generationSha256'],'resolvedRequestSha256':wanted,'outcome':proof['outcome']}
    for name,data in [('state.json',state),('pending.json',resolved)]:
        temporary=F/(name+'.stopped-r68-next')
        with temporary.open('xb') as output: output.write(json.dumps(data,indent=2).encode()); output.flush(); os.fsync(output.fileno())
        os.replace(temporary,F/name)
    (D/'reference-resolution.public.json').write_text(json.dumps(proof,indent=2)+'\n')
print(json.dumps(proof))
