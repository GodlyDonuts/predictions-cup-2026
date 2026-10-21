"""Opt fresh reviews into recurring acquisition through the verified worker.

Requires verified fresh startup and the existing reference updater's quiet
boundary. The same updater resumes with durable pending metadata, including
after an uncertain local-channel outcome. No account client is constructed.
"""
from pathlib import Path
from datetime import datetime, timezone
import fcntl
import argparse
import hashlib
import json
import os
import signal
import socket
import struct
import subprocess
import time

H = Path('/tmp/moloch-recurring-cohort-20261021')
R = Path('/home/archon/apps/moloch/shared/live-production-v1')
F = R.parent / 'fundamental-live-reference-20261021'
UPDATER = Path('/home/archon/apps/moloch/releases/live-reference-dcaaec81/run-live-reference-reload.py')

def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()

def put(path, value):
    temporary = path.with_name(path.name + '.recurring-next')
    with temporary.open('xb') as stream:
        stream.write((json.dumps(value, indent=2) + '\n').encode())
        stream.flush()
        os.fsync(stream.fileno())
    os.replace(temporary, path)

def connect_at_boundary(root, config, expected_status, timeout=240):
    """Wait for the existing owner to poll, before starting request lifetime."""
    status_path = root / 'fundamental-reload/status.json'
    before = status_path.stat().st_mtime_ns
    channel = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    try:
        channel.connect(str(root / 'fundamental-reload/owner.sock'))
        pid, uid, _gid = struct.unpack('3i', channel.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12))
        assert pid == config['workerPid'] and uid == os.geteuid()
        proc = Path('/proc') / str(pid)
        assert int((proc / 'stat').read_text().rsplit(') ', 1)[1].split()[19]) == config['workerStartTicks']
        deadline = time.monotonic() + timeout
        while status_path.stat().st_mtime_ns == before:
            assert time.monotonic() < deadline, 'Owner reload boundary timeout; no request sent'
            assert proc.exists() and not (root / 'EXECUTION_STOP').exists()
            time.sleep(.005)
        assert json.loads(status_path.read_bytes()) == expected_status
        assert not (root / 'EXECUTION_STOP').exists()
        return channel
    except BaseException:
        channel.close()
        raise

def main(config_path, exchanges):
    deployment = json.loads((H / 'deployment.public.json').read_bytes())
    live = json.loads((H / 'fresh-live.public.json').read_bytes())
    config = json.loads(config_path.read_bytes())
    assert live['pid'] == config['workerPid'] == deployment['pid']
    assert live['fullOriginalPrefixVerified'] and live['originalFillsUnchanged']
    assert not (R / 'EXECUTION_STOP').exists()
    process = Path('/proc') / str(config['workerPid'])
    assert int((process / 'stat').read_text().rsplit(') ', 1)[1].split()[19]) == config['workerStartTicks']
    assert sha(process / 'exe') == config['binarySha256']
    assert sha(UPDATER) == 'c30af610216c51d7f8bba378f3fce8f6aac04b71102f7b79fee809a482fa2953'
    assert sha(config['reviewHelper']) == config['reviewHelperSha256']
    pending = json.loads((F / 'pending.json').read_bytes())
    assert pending.get('resolved') is True
    state = json.loads((F / 'state.json').read_bytes())
    assert sha(state['controlPath']) == state['controlSha256']
    control = json.loads(Path(state['controlPath']).read_bytes())
    selected = set(exchanges.split(',')) if exchanges else set(config['exchanges'])
    assert selected and selected <= set(config['exchanges'])
    now = time.time_ns() // 1_000_000
    seen = set()
    for item in control['fundamentalPolicy']['reviews']:
        assert sha(item['path']) == item['fileSha256']
        review = json.loads(Path(item['path']).read_bytes())
        if review['exchangeId'] in selected:
            assert review['expiresAtMs'] > now + 120_000
            assert review['sameSideTopUps']['version'] == 1
            seen.add(review['exchangeId'])
    assert seen == selected
    resumed = H / 'reference-updater-resumed.json'
    updater_pid = json.loads(resumed.read_bytes())['pid'] if resumed.exists() else deployment['referenceUpdaterPid']
    updater = Path('/proc') / str(updater_pid)
    assert (updater / 'cmdline').read_bytes().split(b'\0')[:-1] == [b'python3', b'-u', str(UPDATER).encode(), b'--config', str(config_path).encode()]
    result = None
    try:
        os.kill(updater_pid, signal.SIGTERM)
        for _ in range(50):
            if not updater.exists(): break
            time.sleep(.1)
        assert not updater.exists()
        with (F / 'updater.lock').open('a') as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            assert json.loads((F / 'pending.json').read_bytes()).get('resolved') is True
            assert json.loads((F / 'state.json').read_bytes()) == state
            # Keep approved review generations in the durable shared tree,
            # alongside ordinary forecast generations, rather than build /tmp.
            stage = F / ('recurring-generation-' + str(time.time_ns()))
            subprocess.run(['python3', str(H / 'stage-recurring-reviews-20261021.py'),
                '--base-control', state['controlPath'], '--base-sha256', state['controlSha256'],
                '--output', str(stage), '--review-helper', config['reviewHelper'],
                '--exchanges', ','.join(sorted(selected))], check=True, capture_output=True, text=True)
            staged = json.loads((stage / 'staged.json').read_bytes())
            status = json.loads((R / 'fundamental-reload/status.json').read_bytes())
            assert status['workerPid'] == config['workerPid'] and status['workerStartTicks'] == config['workerStartTicks']
            assert status['previousGenerationSha256'] == state['generationSha256']
            assert status['baseControlSha256'] == config['baseControlSha256']
            request = {k: status[k] for k in ['version', 'workerPid', 'workerStartTicks', 'ownerLease',
                'baseControlSha256', 'bankManifestSha256', 'runId', 'previousGenerationSha256']}
            old_sidecar = (R / 'fundamental.ndjson').read_bytes()
            # The existing CLI's 65s response timer previously elapsed while
            # waiting in the owner's socket queue. Its dead peer then failed
            # /proc authentication. Keep this same-UID client alive, and mint
            # the unchanged 60s request only at the owner's actual poll.
            with connect_at_boundary(R, config, status) as channel:
                now = time.time_ns() // 1_000_000
                request.update(requestId='recurring-v3-' + str(now), controlPath=staged['controlPath'],
                    controlSha256=staged['controlSha256'], issuedAtMs=now, expiresAtMs=now + 60_000)
                request_path = stage / 'request.json'
                raw = json.dumps(request, separators=(',', ':')).encode()
                request_path.write_bytes(raw)
                wanted = sha(request_path)
                put(F / 'pending.json', dict(expectedGenerationSha256=wanted, previousGenerationSha256=state['generationSha256'],
                    expiresAtMs=request['expiresAtMs'], staged=staged, selected=sorted(selected)))
                channel.settimeout(75)
                channel.sendall(struct.pack('!I', len(raw)) + raw)
                try:
                    response = b''
                    while not response.endswith(b'\n'):
                        part = channel.recv(4096)
                        if not part:
                            break
                        response += part
                        assert len(response) <= 65536
                    reply_data = dict(response=json.loads(response))
                except (TimeoutError, ValueError, OSError) as error:
                    reply_data = dict(uncertain=True, reason=type(error).__name__)
            observed = json.loads((R / 'fundamental-reload/status.json').read_bytes())
            confirmed = observed['previousGenerationSha256'] == wanted
            current = (R / 'fundamental.ndjson').read_bytes()
            assert current.startswith(old_sidecar)
            if confirmed:
                generations = [json.loads(line)['event']['generation']['generationSha256'] for line in current.splitlines() if json.loads(line)['event'].get('kind') == 'review_generation']
                assert generations[-1] == wanted
                state.update(generationSha256=wanted, controlPath=staged['controlPath'], controlSha256=staged['controlSha256'], commits=state['commits'] + 1)
                put(F / 'state.json', state)
                put(F / 'pending.json', dict(resolved=True, generationSha256=wanted))
            result = dict(observedUtc=datetime.now(timezone.utc).isoformat(), workerPid=config['workerPid'],
                binarySha256=config['binarySha256'], expectedGenerationSha256=wanted,
                observedGenerationSha256=observed['previousGenerationSha256'], recurringReviewsOptedIn=confirmed,
                exchanges=sorted(selected), staged=staged, reply=reply_data, previousSidecarBytes=len(old_sidecar),
                previousSidecarSha256=hashlib.sha256(old_sidecar).hexdigest(), fullPreviousSidecarPreserved=True,
                monetaryCapsLimiterAndWindowsChanged=False, accountClientsCreated=0)
    finally:
        if not updater.exists():
            log_path = F / ('recurring-updater-resumed-' + str(time.time_ns()) + '.log')
            with log_path.open('x') as log:
                child = subprocess.Popen(['python3', '-u', str(UPDATER), '--config', str(config_path)], stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
            (H / 'reference-updater-resumed.json').write_text(json.dumps(dict(pid=child.pid, log=str(log_path), at=datetime.now(timezone.utc).isoformat())))
    assert result is not None
    result['referenceUpdaterPid'] = child.pid
    (H / 'recurring-opt-in.public.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result))

if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--release-dir', type=Path, default=H)
    parser.add_argument('--reference-config', type=Path, default=F / 'config-r71.json')
    parser.add_argument('--exchanges', help='Optional subset; native reload still checks current open orders.')
    args = parser.parse_args()
    H = args.release_dir
    main(args.reference_config, args.exchanges)
