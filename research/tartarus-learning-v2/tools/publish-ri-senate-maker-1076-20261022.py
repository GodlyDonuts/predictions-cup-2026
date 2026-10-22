"""Publish one staged execution-owner cohort through the existing G12 owner.

Uses no account client. Original uncertain requests are reconciled, never resent.
The existing two-scope reference updater resumes after publication/reconciliation.
"""
from pathlib import Path
import argparse
import fcntl
import hashlib
import importlib.util
import json
import os
import signal
import struct
import subprocess
import time

REFERENCE = Path('/home/archon/apps/moloch/shared/fundamental-live-reference-20261021')
CONFIG = REFERENCE/'config-agent-maker-r101.json'


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def put(path, value):
    raw = (json.dumps(value, separators=(',', ':'))+'\n').encode()
    with path.open('xb') as f:
        f.write(raw)
        f.flush()
        os.fsync(f.fileno())
    return sha(raw)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--reference-updater-pid', type=int, required=True)
    args = parser.parse_args()
    root = args.root
    old_helper = args.reference_updater_pid
    assert root.is_absolute() and old_helper > 1
    assert not (root/'publication.public.json').exists()
    assert not (root/'generation-rebased').exists(), 'ONE_SHOT_DIRECTORY_ALREADY_USED'
    os.umask(0o077)
    config = json.loads(CONFIG.read_bytes())
    runtime = Path(config['workerRoot'])
    helper = Path(config['updaterScript'])
    assert sha(helper.read_bytes()) == config['updaterSha256']
    spec = importlib.util.spec_from_file_location('checked_reference', helper)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    process = Path('/proc')/str(old_helper)
    command = (process/'cmdline').read_bytes().split(b'\0')
    assert str(CONFIG).encode() in command and str(helper).encode() in command
    assert config['workerPid'] == 1131806 and not (runtime/'EXECUTION_STOP').exists()
    # Freeze only the source publisher at a resolved boundary; the account
    # worker keeps trading and owns every account reconciliation/HTTP action.
    deadline = time.monotonic()+180
    while True:
        assert time.monotonic() < deadline, 'NO_QUIET_REFERENCE_BOUNDARY'
        os.kill(old_helper, signal.SIGSTOP)
        try:
            quiet = json.loads((REFERENCE/'pending.json').read_bytes()).get('resolved') is True
            quiet = quiet and not subprocess.run(['ps','-o','pid=','--ppid',str(old_helper)], capture_output=True, text=True).stdout.strip()
        except BaseException:
            os.kill(old_helper, signal.SIGCONT)
            raise
        if quiet:
            break
        os.kill(old_helper, signal.SIGCONT)
        time.sleep(1)
    os.kill(old_helper, signal.SIGTERM)
    os.kill(old_helper, signal.SIGCONT)
    deadline = time.monotonic()+10
    while process.exists():
        assert time.monotonic() < deadline
        time.sleep(.1)
    result = {'workerPid':1131806, 'workerRestarted':False, 'newAccountClients':0,
              'monetaryCapsChanged':False, 'requestResent':False}
    try:
        with (REFERENCE/'updater.lock').open('a') as lock:
            fcntl.flock(lock, fcntl.LOCK_EX|fcntl.LOCK_NB)
            assert json.loads((REFERENCE/'pending.json').read_bytes()).get('resolved') is True
            state = json.loads((REFERENCE/'state.json').read_bytes())
            original = Path(state['controlPath']).read_bytes()
            assert sha(original) == state['controlSha256']
            control = json.loads(original)
            staged = json.loads((root/'generation/staged.json').read_bytes())
            prototype = json.loads(Path(staged['controlPath']).read_bytes())
            selected = next(x for x in prototype['fundamentalPolicy']['reviews']
                            if json.loads(Path(x['path']).read_bytes())['exchangeId'] == '1076')
            assert all(json.loads(Path(x['path']).read_bytes())['exchangeId'] != '1076'
                       for x in control['fundamentalPolicy']['reviews'])
            # Preserve every current unrelated review, including newer prices.
            control['fundamentalPolicy']['reviews'].append(selected)
            target = root/'generation-rebased'
            target.mkdir(mode=0o700)
            trust = [subprocess.check_output([config['reviewHelper'],'hash-review',x['path']], text=True).strip()
                     for x in control['fundamentalPolicy']['reviews']]
            authority = target/'authority.owner-reviewed.json'
            authority_hash = put(authority, {'version':1,'trustedReviewSha256':sorted(trust)})
            control['fundamentalPolicy'].update(authorityPath=str(authority),authoritySha256=authority_hash)
            cp = target/'control.owner-reviewed.json'
            control_hash = put(cp, control)
            validation = json.loads(subprocess.check_output([config['reviewHelper'],'validate-control',str(cp),str(time.time_ns()//1000000)], text=True))
            assert next(x for x in validation['reviews'] if x['exchangeId']=='1076')['current']
            staged = dict(controlPath=str(cp),controlSha256=control_hash)
            current = json.loads((runtime/'fundamental-reload/status.json').read_bytes())
            assert current['previousGenerationSha256'] == state['generationSha256']
            with module.connect_at_boundary(runtime, config, current) as channel:
                now = time.time_ns()//1000000
                request = {k:current[k] for k in ('version','workerPid','workerStartTicks','ownerLease','baseControlSha256','bankManifestSha256','runId','previousGenerationSha256')}
                request.update(requestId='Viserion-ri-existing-1076-'+str(now),controlPath=str(cp),controlSha256=control_hash,issuedAtMs=now,expiresAtMs=now+60000)
                raw = json.dumps(request,separators=(',',':')).encode()
                with (target/'request.json').open('xb') as request_file:
                    request_file.write(raw)
                    request_file.flush()
                    os.fsync(request_file.fileno())
                pending = dict(expectedGenerationSha256=sha(raw),previousGenerationSha256=state['generationSha256'],expiresAtMs=request['expiresAtMs'],staged=staged,selected=['1076'],refreshOpenTargetsOnce=False)
                module.put(REFERENCE/'pending.json',pending)
                channel.settimeout(75)
                channel.sendall(struct.pack('!I',len(raw))+raw)
                body = b''
                try:
                    while not body.endswith(b'\n'):
                        part = channel.recv(4096)
                        if not part:
                            break
                        body += part
                        assert len(body)<=65536
                    with (target/'reply.original.bin').open('xb') as reply_file:
                        reply_file.write(body)
                    json.loads(body)
                except (OSError,ValueError):
                    pass  # Original pending request remains authoritative.
            deadline = time.monotonic()+180
            resolution = None
            while time.monotonic()<deadline:
                resolution = module.reload_resolution(runtime,config,state,pending,time.time_ns()//1000000)
                if resolution is not None:
                    module.record_resolution(REFERENCE,state,pending,resolution)
                    break
                time.sleep(2)
            result.update(requestSha256=pending['expectedGenerationSha256'],
                          resolution=resolution, changedScopes=['1076'], validation=validation)
    finally:
        with (root/'reference-updater-resumed.log').open('x') as log:
            restarted = subprocess.Popen(['python3','-u',str(helper),'--config',str(CONFIG)],
                stdin=subprocess.DEVNULL,stdout=log,stderr=log,start_new_session=True)
        result['referenceUpdaterPid'] = restarted.pid
        put(root/'publication.public.json', result)
    print(json.dumps({'resolution':result.get('resolution'),'referenceUpdaterPid':restarted.pid}),flush=True)


if __name__ == '__main__':
    main()
