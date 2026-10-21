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
from pathlib import Path
import subprocess
import time
import urllib.request
from decimal import Decimal, ROUND_FLOOR, ROUND_CEILING


def sha(p):
    return hashlib.sha256(Path(p).read_bytes()).hexdigest()


def put(path, value):
    tmp = path.with_suffix('.pending')
    tmp.write_text(json.dumps(value, separators=(',', ':')), encoding='utf-8')
    tmp.replace(path)


def executable_ticks(review, midpoint):
    point = midpoint if review['side'] == 'YES' else 1 - midpoint
    allowance = Decimal(review['exceptionalProbabilityAllowance'])
    value = (1 - allowance) * point + allowance * Decimal(review['minimumExceptionalPayout'])
    tick = Decimal('0.005')
    entry = int(((value - Decimal(review['marginPerShare'])) / tick).to_integral_value(rounding=ROUND_FLOOR))
    passive = review.get('passiveValueExit')
    exit_tick = int((value / tick).to_integral_value(rounding=ROUND_CEILING)) if passive and not passive.get('pricingObjective') else None
    return entry, exit_tick


def changed_scopes(capture, current_control, open_exchanges, now):
    inputs = {r['exchangeId']: r for r in capture['rows']}
    selected = []
    for file in current_control['fundamentalPolicy']['reviews']:
        review = json.loads(Path(file['path']).read_bytes())
        ex = review['exchangeId']
        if ex not in inputs or ex in open_exchanges:
            continue
        old = Decimal(review['ordinaryProbability'])
        old_midpoint = old if review['side'] == 'YES' else 1 - old
        new_midpoint = Decimal(inputs[ex]['yesMidpoint'])
        if review['expiresAtMs'] - now <= 120_000 or executable_ticks(review, old_midpoint) != executable_ticks(review, new_midpoint):
            selected.append(ex)
    return selected


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--config', type=Path, required=True)
    a = parser.parse_args()
    c = json.loads(a.config.read_bytes())
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
    if (out / 'pending.json').exists() and json.loads((out / 'pending.json').read_bytes()).get('resolved') is not True:
        raise ValueError('PRIOR_RELOAD_OUTCOME_REQUIRES_RECONCILIATION')
    pending = None

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
            selected = changed_scopes(capture, current_control, open_exchanges, time.time_ns() // 1_000_000)
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
            result = subprocess.run(['python3', c['stageScript'], '--base-control', state['controlPath'],
                '--base-sha256', state['controlSha256'], '--capture', str(capture_path), '--output', str(generation),
                '--review-helper', c['reviewHelper']], capture_output=True, text=True, timeout=40)
            if result.returncode:
                raise ValueError('STAGE_REJECTED:' + result.stderr[-500:])
            staged = json.loads((generation / 'staged.json').read_bytes())
            # Recheck the exact current live owner's generation after file work.
            current = json.loads((root / 'fundamental-reload/status.json').read_bytes())
            if current['previousGenerationSha256'] != state['generationSha256'] or current['workerPid'] != c['workerPid']:
                report('generation_changed_before_request'); return
            now = time.time_ns() // 1_000_000
            request = {k: current[k] for k in ('version', 'workerPid', 'workerStartTicks', 'ownerLease',
                'baseControlSha256', 'bankManifestSha256', 'runId', 'previousGenerationSha256')}
            request.update(requestId='live-reference-' + stamp, controlPath=staged['controlPath'],
                controlSha256=staged['controlSha256'], issuedAtMs=now, expiresAtMs=now + 60_000)
            request_path = generation / 'request.json'
            request_path.write_text(json.dumps(request, separators=(',', ':')), encoding='utf-8')
            wanted = sha(request_path)
            pending = {'expectedGenerationSha256': wanted, 'previousGenerationSha256': state['generationSha256'],
                'expiresAtMs': request['expiresAtMs'], 'staged': staged, 'selected': selected}
            put(out / 'pending.json', pending)
            reply = subprocess.run([c['binary'], 'fundamental-reload', str(base), str(request_path), wanted],
                capture_output=True, text=True, timeout=75)
            observed = json.loads((root / 'fundamental-reload/status.json').read_bytes())
            if observed['previousGenerationSha256'] == wanted:
                state.update(generationSha256=wanted, controlPath=staged['controlPath'],
                    controlSha256=staged['controlSha256'], commits=state['commits'] + 1)
                put(state_path, state)
                report('committed', generationSha256=wanted, exchanges=selected)
                pending = None
            else:
                response = json.loads(reply.stdout) if reply.returncode == 0 else {}
                phase = response.get('state', '')
                if phase.startswith(('rejected_', 'deferred_')) and observed['previousGenerationSha256'] == state['generationSha256']:
                    report('deferred', exchanges=selected, reason=response.get('reason', phase)); pending = None
                else:
                    report('reload_outcome_requires_reconciliation', exitCode=reply.returncode); return
            state['index'] += 1
            put(state_path, state)
            put(out / 'pending.json', {'resolved': True, 'generationSha256': state['generationSha256']})
        except Exception as e:
            report('error', reason=str(e)[-700:], unresolvedRequest=pending is not None)
            if pending is not None:
                return
        time.sleep(max(1, c['intervalSeconds'] - (time.monotonic() - started)))


if __name__ == '__main__':
    main()
