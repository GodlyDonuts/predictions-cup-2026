"""Submit one pinned staged review through the existing owner's reload socket.

The recurring updater must be stopped first. Its same lock and durable pending
record serialize this operation. An uncertain reply is retained for exact
reconciliation by the ordinary updater, never treated as rejection or retried.
"""
import argparse
import fcntl
import hashlib
import importlib.util
import json
from pathlib import Path
import socket
import struct
import time


def sha(path):
    with Path(path).open('rb') as f:
        return hashlib.file_digest(f, 'sha256').hexdigest()


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--config', type=Path, required=True)
    p.add_argument('--staged', type=Path, required=True)
    a = p.parse_args()
    c = json.loads(a.config.read_bytes())
    if sha(c['updaterScript']) != c['updaterSha256']:
        raise ValueError('UPDATER_SOURCE_CHANGED')
    spec = importlib.util.spec_from_file_location('reload_owner', c['updaterScript'])
    helper = importlib.util.module_from_spec(spec); spec.loader.exec_module(helper)
    out = Path(c['outputRoot']); root = Path(c['workerRoot'])
    with (out/'updater.lock').open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        state = json.loads((out/'state.json').read_bytes())
        pending = json.loads((out/'pending.json').read_bytes())
        if pending.get('resolved') is not True:
            raise ValueError('PRIOR_REQUEST_REQUIRES_RECONCILIATION')
        staged = json.loads(a.staged.read_bytes())
        if sha(staged['controlPath']) != staged['controlSha256'] or staged['baseSha256'] != state['controlSha256']:
            raise ValueError('STAGED_BASE_CHANGED')
        old = json.loads(Path(state['controlPath']).read_bytes())
        new = json.loads(Path(staged['controlPath']).read_bytes())
        old.pop('fundamentalPolicy'); new.pop('fundamentalPolicy')
        if old != new:
            raise ValueError('FINANCIAL_OR_RUNTIME_CONTROL_CHANGE_FORBIDDEN')
        pid = c['workerPid']; process = Path('/proc')/str(pid)
        ticks = int((process/'stat').read_text().rsplit(') ',1)[1].split()[19])
        if ticks != c['workerStartTicks'] or sha(process/'exe') != c['binarySha256']:
            raise ValueError('OWNER_CHANGED')
        for stop in (root/'EXECUTION_STOP', root.parent/'EXECUTION_STOP'):
            if stop.exists():
                raise ValueError('EXECUTION_STOP_PRESENT')
        current = json.loads((root/'fundamental-reload/status.json').read_bytes())
        if (current['workerPid'] != pid or current['previousGenerationSha256'] != state['generationSha256']
                or current['baseControlSha256'] != c['baseControlSha256']):
            raise ValueError('CURRENT_OWNER_GENERATION_CHANGED')
        with helper.connect_at_boundary(root,c,current) as channel:
            now = time.time_ns()//1_000_000
            request = {k:current[k] for k in ('version','workerPid','workerStartTicks','ownerLease',
                'baseControlSha256','bankManifestSha256','runId','previousGenerationSha256')}
            request.update(requestId='flat-maker-'+str(now),controlPath=staged['controlPath'],
                controlSha256=staged['controlSha256'],issuedAtMs=now,expiresAtMs=now+60_000)
            raw = json.dumps(request,separators=(',',':')).encode()
            request_path = a.staged.parent/'request.json'
            with request_path.open('xb') as f:
                f.write(raw); f.flush()
                import os
                os.fsync(f.fileno())
            wanted = sha(request_path)
            pending = dict(expectedGenerationSha256=wanted,previousGenerationSha256=state['generationSha256'],
                expiresAtMs=request['expiresAtMs'],staged=staged,selected=[x['exchangeId'] for x in staged['changes']],
                refreshOpenTargetsOnce=False)
            helper.put(out/'pending.json',pending)
            response = {}
            try:
                channel.settimeout(75)
                channel.sendall(struct.pack('!I',len(raw))+raw)
                body = b''
                while not body.endswith(b'\n'):
                    part = channel.recv(4096)
                    if not part: break
                    body += part
                    if len(body)>65536: raise ValueError('OWNER_REPLY_BOUND')
                response = json.loads(body)
            except (OSError,ValueError) as e:
                print(json.dumps(dict(state='reply_uncertain',reason=type(e).__name__,requestSha256=wanted,
                    pendingRetained=True,requestResent=False)))
        observed = json.loads((root/'fundamental-reload/status.json').read_bytes())
        if observed['previousGenerationSha256'] == wanted:
            resolution = dict(outcome='committed',requestSha256=wanted,observedAtMs=time.time_ns()//1_000_000)
            helper.record_resolution(out,state,pending,resolution)
            result = dict(state='committed',generationSha256=wanted,selected=pending['selected'],
                workerPid=pid,workerRestarted=False,accountClientCreated=False)
        elif response.get('state','').startswith(('rejected_','deferred_')) and observed['previousGenerationSha256']==state['generationSha256']:
            resolution = dict(outcome='rejected_uncommitted',requestSha256=wanted,response=response,
                observedAtMs=time.time_ns()//1_000_000)
            helper.record_resolution(out,state,pending,resolution)
            result = dict(state='rejected',response=response,requestSha256=wanted)
        else:
            result = dict(state='requires_reconciliation',requestSha256=wanted,pendingRetained=True,requestResent=False)
        (a.staged.parent/'reload-result.json').write_text(json.dumps(result,indent=2))
        print(json.dumps(result))


if __name__ == '__main__':
    main()
