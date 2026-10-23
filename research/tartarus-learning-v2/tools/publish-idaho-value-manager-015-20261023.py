"""Publish a source-backed Idaho manager through the existing owner; never an account client."""
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
from review_generation_window import publication_boundary, active_managed_review

REFERENCE = Path('/home/archon/apps/moloch/shared/fundamental-live-reference-20261021')
CONFIG = REFERENCE/'config-agent-maker-r104-window-012.json'


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
    parser.add_argument('--config',type=Path,required=True)
    parser.add_argument('--next-config',type=Path,required=True)
    parser.add_argument('--restoration-proof',type=Path,required=True)
    parser.add_argument('--source-commit',required=True)
    args = parser.parse_args()
    assert args.next_config.is_absolute() and not args.next_config.exists()
    assert len(args.source_commit)==40 and all(c in '0123456789abcdef' for c in args.source_commit)
    global CONFIG
    CONFIG=args.config
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
    assert config['workerPid'] == 1138184 and not (runtime/'EXECUTION_STOP').exists()
    deadline=time.monotonic()+2700
    while not args.restoration_proof.exists():
        assert Path('/proc',str(config['workerPid'])).exists() and not (runtime/'EXECUTION_STOP').exists()
        assert time.monotonic()<deadline, 'FINITE_RESTORATION_WAIT_EXPIRED'
        time.sleep(5)
    proof=json.loads(args.restoration_proof.read_bytes())
    assert proof['pid']==config['workerPid'] and proof['freshTradingVerified'] and proof['fullOriginalFinancialPrefixPreserved'] and proof['fullOriginalFundamentalPrefixPreserved']
    with publication_boundary(runtime,wait_seconds=240):
        deadline=time.monotonic()+360
        while True:
            state=json.loads((REFERENCE/'state.json').read_bytes())
            current_control=json.loads(Path(state['controlPath']).read_bytes())
            hold=active_managed_review(runtime,current_control,state['generationSha256'],time.time_ns()//1000000)
            if not hold: break
            assert time.monotonic()<deadline, 'ACTIVE_REVIEW_REQUIRES_LATER_SOURCE_HANDOFF'
            time.sleep(1)
        # Refresh before selecting the new cohort; preserve every unrelated current review.
        subprocess.run(['python3',str(root.parent/'stage.py'),'--config',str(CONFIG),
            '--sources',str(root.parent),'--source','/tmp/moloch-rank-current-r99-20261022',
            '--output',str(root/'generation-fresh')],check=True)
        # Freeze only the source publisher at a resolved boundary; the account
        # worker keeps trading and owns every account reconciliation/HTTP action.
        deadline = time.monotonic()+180
        while True:
            assert time.monotonic() < deadline, 'NO_QUIET_REFERENCE_BOUNDARY'
            assert str(CONFIG).encode() in (process/'cmdline').read_bytes() and str(helper).encode() in (process/'cmdline').read_bytes(), 'SOURCE_UPDATER_IDENTITY_CHANGED'
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
        result = {'workerPid':1138184, 'workerRestarted':False, 'newAccountClients':0,
                  'monetaryCapsChanged':False, 'requestResent':False}
        try:
            with (REFERENCE/'updater.lock').open('a') as lock:
                fcntl.flock(lock, fcntl.LOCK_EX|fcntl.LOCK_NB)
                assert json.loads((REFERENCE/'pending.json').read_bytes()).get('resolved') is True
                state = json.loads((REFERENCE/'state.json').read_bytes())
                original = Path(state['controlPath']).read_bytes()
                assert sha(original) == state['controlSha256']
                control = json.loads(original)
                staged = json.loads((root/'generation-fresh/generation/staged.json').read_bytes())
                prototype = json.loads(Path(staged['controlPath']).read_bytes())
                selected = next(x for x in prototype['fundamentalPolicy']['reviews']
                                if json.loads(Path(x['path']).read_bytes())['exchangeId'] == '859')
                assert all(json.loads(Path(x['path']).read_bytes())['exchangeId'] != '859'
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
                assert next(x for x in validation['reviews'] if x['exchangeId']=='859')['current']
                staged = dict(controlPath=str(cp),controlSha256=control_hash)
                current = json.loads((runtime/'fundamental-reload/status.json').read_bytes())
                assert current['previousGenerationSha256'] == state['generationSha256']
                with module.connect_at_boundary(runtime, config, current) as channel:
                    now = time.time_ns()//1000000
                    request = {k:current[k] for k in ('version','workerPid','workerStartTicks','ownerLease','baseControlSha256','bankManifestSha256','runId','previousGenerationSha256')}
                    request.update(requestId='Moloch-idaho-flat-859-'+str(now),controlPath=str(cp),controlSha256=control_hash,issuedAtMs=now,expiresAtMs=now+60000)
                    raw = json.dumps(request,separators=(',',':')).encode()
                    with (target/'request.json').open('xb') as request_file:
                        request_file.write(raw)
                        request_file.flush()
                        os.fsync(request_file.fileno())
                    pending = dict(expectedGenerationSha256=sha(raw),previousGenerationSha256=state['generationSha256'],expiresAtMs=request['expiresAtMs'],staged=staged,selected=['859'],refreshOpenTargetsOnce=False)
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
                              resolution=resolution, changedScopes=['859'], validation=validation)
        finally:
            resume_config=CONFIG
            if (result.get('resolution') or {}).get('outcome')=='committed':
                next_value=json.loads((root/'generation-fresh/maintenance-config.unapproved.json').read_bytes())
                next_value.update(sourceCommit=args.source_commit,initialGenerationSha256=json.loads((REFERENCE/'state.json').read_bytes())['generationSha256'])
                put(args.next_config,next_value)
                resume_config=args.next_config
            with (root/'reference-updater-resumed.log').open('x') as log:
                restarted = subprocess.Popen(['python3','-u',str(helper),'--config',str(resume_config)],
                    stdin=subprocess.DEVNULL,stdout=log,stderr=log,start_new_session=True)
            result['referenceUpdaterPid'] = restarted.pid
            result['referenceUpdaterConfig'] = str(resume_config)
            result['accountWorkerRestarted'] = False
            result['sourceCommit'] = args.source_commit
            put(root/'publication.public.json', result)
    print(json.dumps({'resolution':result.get('resolution'),'referenceUpdaterPid':restarted.pid}),flush=True)


if __name__ == '__main__':
    main()
