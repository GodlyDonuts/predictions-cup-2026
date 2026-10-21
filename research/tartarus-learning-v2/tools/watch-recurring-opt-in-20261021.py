"""Finite file-only coordinator; opt fresh quiet markets in through one worker."""
import argparse
import datetime
import json
from pathlib import Path
import subprocess
import time


def run(release, config_path):
    runtime = Path('/home/archon/apps/moloch/shared/live-production-v1')
    reference = config_path.parent
    config = json.loads(config_path.read_bytes())
    targets = set(config['exchanges'])
    start = time.monotonic()
    batches = []
    last_report = 0.0
    unresolved = None
    unsuccessful = 0
    while time.monotonic() - start < 2400:
        if (runtime / 'EXECUTION_STOP').exists() or (release / 'fresh-live.failed.json').exists():
            raise RuntimeError('Current worker stopped; no further activation attempted')
        phase, selected, enabled = 'waiting_for_fresh_worker', [], []
        if (release / 'fresh-live.public.json').exists():
            phase = 'waiting_for_quiet_fresh_reviews'
            pending = json.loads((reference / 'pending.json').read_bytes())
            if unresolved is not None:
                resolution_path = reference / ('reload-resolution-' + unresolved + '.json')
                if pending.get('resolved') is not True or not resolution_path.exists():
                    time.sleep(.25)
                    continue
                resolution = json.loads(resolution_path.read_bytes())
                assert resolution['requestSha256'] == unresolved
                assert resolution['outcome'] in ['committed', 'expired_uncommitted']
                assert resolution['requestResent'] is False
                if resolution['outcome'] == 'expired_uncommitted':
                    unsuccessful += 1
                    assert unsuccessful < 3, 'Three confirmed uncommitted attempts; inspect owner timing'
                unresolved = None
            if pending.get('resolved') is True:
                state = json.loads((reference / 'state.json').read_bytes())
                control = json.loads(Path(state['controlPath']).read_bytes())
                native = json.loads((runtime / 'tartarus-current-native-state.json').read_bytes())
                occupied = {o['exchangeId'] for o in native['activeNativeOrders']}
                now = time.time_ns() // 1_000_000
                for item in control['fundamentalPolicy']['reviews']:
                    review = json.loads(Path(item['path']).read_bytes())
                    exchange = review['exchangeId']
                    if exchange not in targets:
                        continue
                    version = review.get('sameSideTopUps', {}).get('version')
                    if version == 3:
                        enabled.append(exchange)
                    elif version == 1 and review['expiresAtMs'] > now + 180_000 and exchange not in occupied:
                        selected.append(exchange)
                if set(enabled) == targets:
                    proof = dict(observedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),
                        workerPid=config['workerPid'], exchanges=sorted(enabled), allRecurringReviewsEnabled=True,
                        batches=batches, accountRequests=0)
                    (release / 'recurring-complete.public.json').write_text(json.dumps(proof, indent=2) + '\n')
                    return
                # Fully closed V1 cohorts cannot start new entries or covered
                # exits while waiting. Activate those first; other markets are
                # changed individually to contain races with new exit orders.
                positions = native['account']['positions']
                flat = [ex for ex in selected if all(not lots for lots in positions.get(ex, {}).values())]
                selected = flat if flat else sorted(selected)[:1]
        if selected:
            batch = release / ('recurring-batch-' + str(time.time_ns()))
            with batch.with_suffix('.log').open('x') as log:
                result = subprocess.run(['python3', str(release / 'enable-recurring-reviews-20261021.py'),
                    '--release-dir', str(release), '--reference-config', str(config_path),
                    '--exchanges', ','.join(sorted(selected))], stdout=log, stderr=subprocess.STDOUT)
            if result.returncode != 0:
                raise RuntimeError('Activation failed; inspect ' + str(batch.with_suffix('.log')))
            proof = json.loads((release / 'recurring-opt-in.public.json').read_bytes())
            assert set(proof['exchanges']) == set(selected)
            batch.with_suffix('.json').write_text(json.dumps(proof, indent=2) + '\n')
            batches.append(str(batch.with_suffix('.json')))
            if proof['recurringReviewsOptedIn']:
                unsuccessful = 0
                phase = 'confirmed_batch'
            else:
                # No new request until the original updater proves this exact
                # outcome from owner identity, current view and durable log.
                unresolved = proof['expectedGenerationSha256']
                phase = 'waiting_for_original_request_resolution'
        status = dict(at=datetime.datetime.now(datetime.timezone.utc).isoformat(), phase=phase,
            enabled=sorted(enabled), selected=sorted(selected), elapsedSeconds=int(time.monotonic() - start))
        if selected or time.monotonic() - last_report >= 5:
            (release / 'opt-in-watch.status.json').write_text(json.dumps(status))
            print(json.dumps(status), flush=True)
            last_report = time.monotonic()
        # The existing updater can begin its next capture after one second.
        # Observe that file-only quiet interval without competing for its lock.
        time.sleep(.25)
    raise RuntimeError('Finite activation window expired; no further changes attempted')


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--release-dir', required=True, type=Path)
    parser.add_argument('--reference-config', required=True, type=Path)
    args = parser.parse_args()
    run(args.release_dir, args.reference_config)
