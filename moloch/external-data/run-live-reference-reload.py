"""Refresh reviewed values through the existing worker; never an account client.

Only immutable source/review files and the authenticated owner reload socket are
used. Restarts, STOP changes, financial control changes and order retries are not
implemented here. A changed worker or another generation writer requires review.
"""
import argparse
import fcntl
import gzip
import hashlib
import json
import os
import socket
import struct
from pathlib import Path
import subprocess
import time
import urllib.request
from decimal import Decimal, ROUND_FLOOR, ROUND_CEILING
from datetime import datetime


def sha(p):
    return hashlib.sha256(Path(p).read_bytes()).hexdigest()


def put(path, value):
    tmp = path.with_suffix('.pending')
    tmp.write_text(json.dumps(value, separators=(',', ':')), encoding='utf-8')
    tmp.replace(path)


def connect_at_boundary(root, config, expected_status, timeout=240):
    """Keep the authenticated client alive; mint TTL only when owner polls."""
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


def executable_ticks(review, midpoint):
    point = midpoint if review['side'] == 'YES' else 1 - midpoint
    allowance = Decimal(review['exceptionalProbabilityAllowance'])
    value = (1 - allowance) * point + allowance * Decimal(review['minimumExceptionalPayout'])
    tick = Decimal('0.005')
    entry = int(((value - Decimal(review['marginPerShare'])) / tick).to_integral_value(rounding=ROUND_FLOOR))
    passive = review.get('passiveValueExit')
    exit_tick = int((value / tick).to_integral_value(rounding=ROUND_CEILING)) if passive and not passive.get('pricingObjective') else None
    return entry, exit_tick


REVIEW_REFRESH_LEAD_MS = 300_000


def review_current_until(review):
    # Match the worker's review/source clocks. A quiet price still needs a
    # newly captured review before its earliest original source expires.
    return min([review['expiresAtMs']] + [
        s['observedAtMs'] + s['maximumAgeMs'] + 1 for s in review.get('sources', [])])


def execution_modes_policy(config):
    path, expected = config.get('executionModesPolicy'), config.get('executionModesPolicySha256')
    if not path and not expected:
        return {}
    if not path or not expected:
        raise ValueError('EXECUTION_MODES_POLICY_PIN')
    raw = Path(path).read_bytes()
    if hashlib.sha256(raw).hexdigest() != expected:
        raise ValueError('EXECUTION_MODES_POLICY_PIN')
    value = json.loads(raw)
    modes = value['exchanges']
    if value['version'] != 1 or not isinstance(modes, dict) or not set(modes) <= set(config['exchanges']):
        raise ValueError('EXECUTION_MODES_POLICY_SCOPE')
    for choices in modes.values():
        if (not isinstance(choices, list) or not choices or len(set(choices)) != len(choices)
                or not set(choices) <= {'joinBid','improveOneTick','boundedLimitMayRest'}):
            raise ValueError('EXECUTION_MODES_POLICY_CHOICES')
    return modes


def execution_margins_policy(config):
    modes = execution_modes_policy(config)
    if not config.get('executionModesPolicy'):
        return {}
    margins = json.loads(Path(config['executionModesPolicy']).read_bytes()).get('entryMargins', {})
    if not isinstance(margins, dict) or not set(margins) <= set(modes):
        raise ValueError('EXECUTION_MARGIN_POLICY_SCOPE')
    for value in margins.values():
        if (not isinstance(value, str) or len(value) != 8 or not value.startswith('0.')
                or any(c not in '0123456789' for c in value[2:]) or Decimal(value) <= 0):
            raise ValueError('EXECUTION_MARGIN_POLICY_VALUE')
    return margins


def changed_scopes(capture, current_control, open_exchanges, now, *, refresh_open=False, execution_modes=None, entry_margins=None):
    inputs = {r['exchangeId']: r for r in capture['rows']}
    selected = []
    for file in current_control['fundamentalPolicy']['reviews']:
        review = json.loads(Path(file['path']).read_bytes())
        ex = review['exchangeId']
        if ex not in inputs or ex in open_exchanges and not refresh_open:
            continue
        if ex in open_exchanges and refresh_open:
            selected.append(ex)
            continue
        wanted = (execution_modes or {}).get(ex)
        if wanted and review.get('executionPolicy') != {'version': 1, 'allowedModes': wanted}:
            selected.append(ex)
            continue
        margin = (entry_margins or {}).get(ex)
        if margin and Decimal(review['marginPerShare']) != Decimal(margin):
            selected.append(ex)
            continue
        old = Decimal(review['ordinaryProbability'])
        old_midpoint = old if review['side'] == 'YES' else 1 - old
        new_midpoint = Decimal(inputs[ex]['yesMidpoint'])
        if review_current_until(review) - now <= REVIEW_REFRESH_LEAD_MS or executable_ticks(review, old_midpoint) != executable_ticks(review, new_midpoint):
            selected.append(ex)
    return selected


def reload_resolution(root, config, state, pending, now):
    """File-only proof of a late commit or an expired, uncommitted request.

    Time alone never resolves a request. Require the exact request, live-owner
    binding, matching later owner/native views and the original cohort prefix.
    Returning None means wait without submitting another generation.
    """
    root = Path(root)
    staged = pending['staged']
    request_path = Path(staged['controlPath']).parent / 'request.json'
    wanted = pending['expectedGenerationSha256']
    raw = request_path.read_bytes()
    if hashlib.sha256(raw).hexdigest() != wanted:
        raise ValueError('RELOAD_RECONCILIATION_REQUEST_CHANGED')
    request = json.loads(raw)
    previous = pending['previousGenerationSha256']
    bindings = {'workerPid': config['workerPid'], 'workerStartTicks': config['workerStartTicks'],
        'baseControlSha256': config['baseControlSha256'], 'previousGenerationSha256': previous,
        'controlPath': staged['controlPath'], 'controlSha256': staged['controlSha256'],
        'expiresAtMs': pending['expiresAtMs']}
    if any(request.get(k) != v for k, v in bindings.items()) or state['generationSha256'] not in (previous, wanted):
        raise ValueError('RELOAD_RECONCILIATION_BINDING_CHANGED')
    if sha(staged['controlPath']) != staged['controlSha256']:
        raise ValueError('RELOAD_RECONCILIATION_CONTROL_CHANGED')
    owner_path = root / 'fundamental-reload/status.json'
    owner_raw = owner_path.read_bytes()
    owner = json.loads(owner_raw)
    owner_mtime = owner_path.stat().st_mtime_ns
    for key in ('workerPid', 'workerStartTicks', 'baseControlSha256'):
        if owner.get(key) != config[key]:
            raise ValueError('RELOAD_RECONCILIATION_OWNER_CHANGED')
    for key in ('ownerLease', 'bankManifestSha256', 'runId'):
        if owner.get(key) != request[key]:
            raise ValueError('RELOAD_RECONCILIATION_OWNER_SCOPE_CHANGED')
    view = json.loads((root / 'tartarus-current-native-state.json').read_bytes())
    generation = owner['previousGenerationSha256']
    if generation not in (previous, wanted):
        raise ValueError('RELOAD_RECONCILIATION_EXTERNAL_GENERATION')
    if view['generationSha256'] != generation:
        return None
    original = (root / 'fundamental.ndjson').read_bytes()
    prefix = original[:original.rfind(b'\n') + 1]
    generations = []
    for line in prefix.splitlines():
        row = json.loads(line)
        if row['manifestHash'] != request['bankManifestSha256'] or row['runId'] != request['runId']:
            raise ValueError('RELOAD_RECONCILIATION_COHORT_SCOPE')
        event = row['event']
        if event.get('kind') == 'review_generation':
            generations.append(event['generation']['generationSha256'])
    if not generations or generations[-1] != generation:
        return None
    if owner_path.read_bytes() != owner_raw or owner_path.stat().st_mtime_ns != owner_mtime:
        return None
    if generation == wanted:
        outcome = 'committed'
    else:
        expiry = request['expiresAtMs']
        written = datetime.fromisoformat(view['writtenAt'].replace('Z', '+00:00'))
        if written.tzinfo is None:
            raise ValueError('RELOAD_RECONCILIATION_UTC_VIEW_REQUIRED')
        if (now <= expiry or owner_mtime // 1_000_000 <= expiry
                or int(written.timestamp() * 1000) <= expiry or wanted in generations):
            return None
        outcome = 'expired_uncommitted'
    return {'outcome': outcome, 'requestSha256': wanted, 'previousGenerationSha256': previous,
        'committedGenerationSha256': generation, 'observedAtMs': now,
        'requestExpiresAtMs': request['expiresAtMs'], 'nativeViewWrittenAt': view['writtenAt'],
        'originalCohortPrefixBytes': len(prefix), 'originalCohortPrefixSha256': hashlib.sha256(prefix).hexdigest(),
        'requestResent': False, 'accountRequests': 0, 'financialStateChanged': False}


def record_resolution(out, state, pending, resolution):
    """Preserve the old request; make metadata recovery idempotent across crashes."""
    wanted = pending['expectedGenerationSha256']
    for name, value in [('reload-request-' + wanted + '.json', pending),
                        ('reload-resolution-' + wanted + '.json', resolution)]:
        path = out / name
        if not path.exists():
            put(path, value)
    if state.get('lastResolvedReloadRequestSha256') != wanted:
        if resolution['outcome'] == 'committed' and state['generationSha256'] != wanted:
            state.update(generationSha256=wanted, controlPath=pending['staged']['controlPath'],
                controlSha256=pending['staged']['controlSha256'], commits=state['commits'] + 1)
        state.update(index=state['index'] + 1, lastResolvedReloadRequestSha256=wanted)
        put(out / 'state.json', state)
    put(out / 'pending.json', {'resolved': True, 'generationSha256': state['generationSha256'],
        'resolvedRequestSha256': wanted, 'outcome': resolution['outcome']})


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--config', type=Path, required=True)
    parser.add_argument('--refresh-open-targets-once', action='store_true',
        help='Capture genuine new evidence for open targets once; the official worker owns any resulting cancellations/reconciliation.')
    a = parser.parse_args()
    refresh_open_once = a.refresh_open_targets_once
    c = json.loads(a.config.read_bytes())
    modes = execution_modes_policy(c)
    margins = execution_margins_policy(c)
    os.umask(0o077)
    out = Path(c['outputRoot'])
    out.mkdir(parents=True, exist_ok=True)
    lock = (out / 'updater.lock').open('a')
    fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    base, root = Path(c['baseControl']), Path(c['workerRoot'])
    assert sha(base) == c['baseControlSha256']
    assert sha(c['reviewHelper']) == c['reviewHelperSha256']
    assert sha(c['stageScript']) == c['stageScriptSha256']
    assert sha(c['binary']) == c['binarySha256']
    state_path = out / 'state.json'
    state = json.loads(state_path.read_bytes()) if state_path.exists() else {
        'generationSha256': c['initialGenerationSha256'], 'controlPath': str(base),
        'controlSha256': c['baseControlSha256'], 'commits': 0, 'index': 0}
    retained = json.loads((out / 'pending.json').read_bytes()) if (out / 'pending.json').exists() else None
    pending = retained if retained is not None and retained.get('resolved') is not True else None

    def report(phase, **fields):
        value = {'atMs': time.time_ns() // 1_000_000, 'pid': os.getpid(), 'phase': phase,
            'commits': state['commits'], 'officialSigClientCreated': False, 'stopChanged': False,
            'financialControlChanged': False, 'mlFitOrSelectionUse': False, **fields}
        put(out / 'status.json', value)
        print(json.dumps(value), flush=True)

    while True:
        started = time.monotonic()
        try:
            status = json.loads((root / 'fundamental-reload/status.json').read_bytes())
            progress = json.loads((root / 'progress.json').read_bytes())
            process = Path('/proc') / str(c['workerPid'])
            if not process.exists():
                report('worker_exited'); return
            ticks = int((process / 'stat').read_text().rsplit(') ', 1)[1].split()[19])
            if ticks != c['workerStartTicks'] or sha(base) != c['baseControlSha256']:
                report('worker_or_control_changed'); return
            if status.get('workerPid') != c['workerPid'] or progress.get('pid') != c['workerPid'] or progress.get('phase') != 'trading' or not progress.get('realtimeHealthy'):
                report('waiting_for_current_worker'); time.sleep(5); continue
            if status['baseControlSha256'] != c['baseControlSha256']:
                report('worker_base_binding_changed'); return
            if pending is not None:
                resolution = reload_resolution(root, c, state, pending, time.time_ns() // 1_000_000)
                if resolution is None:
                    report('waiting_for_reload_reconciliation'); time.sleep(5); continue
                record_resolution(out, state, pending, resolution)
                if resolution['outcome'] == 'committed' and pending.get('refreshOpenTargetsOnce'):
                    refresh_open_once = False
                report('reload_reconciled', outcome=resolution['outcome'], requestSha256=resolution['requestSha256'])
                pending = None
                continue  # Capture new values on a new pass; never resend the old request.
            if status['previousGenerationSha256'] != state['generationSha256']:
                report('external_generation_changed'); return
            if sha(state['controlPath']) != state['controlSha256']:
                report('active_review_file_changed'); return
            url = c['captureUrl'] + '?exchanges=' + ','.join(c['exchanges'])
            with urllib.request.urlopen(url, timeout=20) as response:
                raw = response.read(2_000_001)
                if len(raw) > 2_000_000:
                    raise ValueError('PUBLIC_CAPTURE_BOUND')
                if response.headers.get('Content-Encoding') == 'gzip':
                    raw = gzip.decompress(raw)
            if len(raw) > 4_000_000:
                raise ValueError('PUBLIC_CAPTURE_DECOMPRESSED_BOUND')
            capture = json.loads(raw)
            if {x['exchangeId'] for x in capture['rows']} != set(c['exchanges']):
                raise ValueError('EXACT_CAPTURE_RESPONSE_SCOPE')
            # The original refresh changed review hashes even when all actual
            # order-price ticks were identical. That consumed account reads and
            # displaced pending plans without new economic information.
            view = json.loads((root / 'tartarus-current-native-state.json').read_bytes())
            unclosed = set(view['knownUnclosedAcknowledgments'])
            open_exchanges = {q['exchangeId'] for q in view['activeNativeOrders'] if q['id'] in unclosed}
            current_control = json.loads(Path(state['controlPath']).read_bytes())
            selected = changed_scopes(capture, current_control, open_exchanges, time.time_ns() // 1_000_000,
                refresh_open=refresh_open_once, execution_modes=execution_modes_policy(c), entry_margins=execution_margins_policy(c))
            if not selected:
                report('fresh_reference_no_price_tick_change', capturedAtMs=capture['capturedAtMs'], openTargets=len(open_exchanges))
                time.sleep(max(1, c['intervalSeconds'] - (time.monotonic() - started)))
                continue
            capture['rows'] = [x for x in capture['rows'] if x['exchangeId'] in selected]
            raw = json.dumps(capture, separators=(',', ':')).encode()
            stamp = str(time.time_ns())
            capture_path = out / ('capture-' + stamp + '.json')
            capture_path.write_bytes(raw)
            generation = out / ('generation-' + stamp)
            command = ['python3', c['stageScript'], '--base-control', state['controlPath'],
                '--base-sha256', state['controlSha256'], '--capture', str(capture_path), '--output', str(generation),
                '--review-helper', c['reviewHelper']]
            if modes:
                command.extend(['--execution-modes-policy', c['executionModesPolicy'],
                    '--execution-modes-sha256', c['executionModesPolicySha256']])
            result = subprocess.run(command, capture_output=True, text=True, timeout=40)
            if result.returncode:
                raise ValueError('STAGE_REJECTED:' + result.stderr[-500:])
            staged = json.loads((generation / 'staged.json').read_bytes())
            # Recheck the exact current live owner's generation after file work.
            current = json.loads((root / 'fundamental-reload/status.json').read_bytes())
            if current['previousGenerationSha256'] != state['generationSha256'] or current['workerPid'] != c['workerPid']:
                report('generation_changed_before_request'); return
            # Before this change the65s CLI could die in the owner's queue,
            # invalidating peer authentication and consuming the whole request
            # lifetime before the worker even read it. No request is issued or
            # renewed while waiting for this exact owner's poll boundary.
            with connect_at_boundary(root, c, current) as channel:
                now = time.time_ns() // 1_000_000
                request = {k: current[k] for k in ('version', 'workerPid', 'workerStartTicks', 'ownerLease',
                    'baseControlSha256', 'bankManifestSha256', 'runId', 'previousGenerationSha256')}
                request.update(requestId='live-reference-' + stamp, controlPath=staged['controlPath'],
                    controlSha256=staged['controlSha256'], issuedAtMs=now, expiresAtMs=now + 60_000)
                request_path = generation / 'request.json'
                raw_request = json.dumps(request, separators=(',', ':')).encode()
                request_path.write_bytes(raw_request)
                wanted = sha(request_path)
                pending = {'expectedGenerationSha256': wanted, 'previousGenerationSha256': state['generationSha256'],
                    'expiresAtMs': request['expiresAtMs'], 'staged': staged, 'selected': selected,
                    'refreshOpenTargetsOnce': refresh_open_once}
                put(out / 'pending.json', pending)
                response, reply_code = {}, 1
                try:
                    channel.settimeout(75)
                    channel.sendall(struct.pack('!I', len(raw_request)) + raw_request)
                    body = b''
                    while not body.endswith(b'\n'):
                        part = channel.recv(4096)
                        if not part:
                            break
                        body += part
                        if len(body) > 65536:
                            raise ValueError('OWNER_REPLY_BOUND')
                    response = json.loads(body)
                    reply_code = 0
                except (OSError, ValueError) as error:
                    # A send/receive timeout never implies nonacceptance.
                    # Preserve pending; existing exact file proofs decide it.
                    report('reload_reply_uncertain', reason=type(error).__name__)
            observed = json.loads((root / 'fundamental-reload/status.json').read_bytes())
            if observed['previousGenerationSha256'] == wanted:
                state.update(generationSha256=wanted, controlPath=staged['controlPath'],
                    controlSha256=staged['controlSha256'], commits=state['commits'] + 1)
                put(state_path, state)
                report('committed', generationSha256=wanted, exchanges=selected)
                if pending.get('refreshOpenTargetsOnce'):
                    refresh_open_once = False
                pending = None
            else:
                phase = response.get('state', '')
                if phase.startswith(('rejected_', 'deferred_')) and observed['previousGenerationSha256'] == state['generationSha256']:
                    if response.get('reason') == 'FUNDAMENTAL_GENERATION_TARGET_OPEN':
                        # This runtime cannot revise a bound open target. Do
                        # not repeatedly occupy its reload boundary trying it.
                        refresh_open_once = False
                    report('deferred', exchanges=selected, reason=response.get('reason', phase)); pending = None
                else:
                    report('reload_outcome_requires_reconciliation', exitCode=reply_code)
            if pending is None:
                state['index'] += 1
                put(state_path, state)
                put(out / 'pending.json', {'resolved': True, 'generationSha256': state['generationSha256']})
        except Exception as e:
            report('error', reason=str(e)[-700:], unresolvedRequest=pending is not None)
            if pending is not None and not isinstance(e, subprocess.TimeoutExpired):
                return
        time.sleep(max(1, c['intervalSeconds'] - (time.monotonic() - started)))


if __name__ == '__main__':
    main()
