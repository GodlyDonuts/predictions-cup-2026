"""Finite owner-directed model-review loop; only the existing worker trades.

The explicit publish flag selects the checked, declared five-model monetary
policy under the human's existing Moloch authority. These are uncalibrated
model assumptions, not proven alpha. No account API, second worker, implicit
expiry renewal, risk increase or replacement of an unresolved order.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import subprocess
import time


def completion(root, view, raw_sha):
    """Bounded nomination hint; the publisher independently checks full history."""
    path = root / 'dispatch.ndjson'
    size = path.stat().st_size
    with path.open('rb') as stream:
        offset = max(0, size-64*1024*1024)
        stream.seek(offset)
        if offset:
            stream.readline()
        data = stream.read(size-stream.tell())
    intents, closed = set(), set()
    for line in data.splitlines(keepends=True):
        if not line.endswith(b'\n'):
            continue
        event = json.loads(line)['event']
        if event['kind'] == 'intent' and (event.get('rankCampaign') or {}).get('rawSha256') == raw_sha:
            intents.add(event['qid'])
        if event['kind'] == 'closed':
            closed.add(event['qid'])
    candidates = intents & closed
    assert len(intents) <= 1, 'NOMINATION_HAS_MULTIPLE_INTENTS'
    live = set(view['knownUnclosedAcknowledgments']) | {q['id'] for q in view['activeNativeOrders']}
    return next(iter(candidates-live), None)


def run(a):
    os.umask(0o077)
    assert a.publish_reviewed_model_policy, 'EXPLICIT_OWNER_MODEL_POLICY_SELECTION_REQUIRED'
    assert 1 <= a.cycles <= 1000 and 1 <= a.minutes <= 1440
    assert len(a.source_commit) == 40
    a.output.mkdir(mode=0o700, parents=True, exist_ok=False)
    deployed = json.loads(a.deployment.read_bytes())
    end = time.monotonic()+a.minutes*60
    completed = 0
    while completed < a.cycles and time.monotonic() < end:
        assert Path('/proc', str(deployed['pid'])).exists(), 'ORIGINAL_WORKER_EXITED'
        assert hashlib.sha256(Path('/proc', str(deployed['pid']), 'exe').read_bytes()).hexdigest() == deployed['binarySha256']
        assert not (a.runtime/'EXECUTION_STOP').exists() and not (a.runtime.parent/'EXECUTION_STOP').exists()
        progress = json.loads((a.runtime/'progress.json').read_bytes())
        if progress.get('pid') != deployed['pid'] or progress.get('phase') != 'trading' or not progress.get('realtimeHealthy'):
            time.sleep(5)
            continue
        view = json.loads((a.runtime/'tartarus-current-native-state.json').read_bytes())
        assert not view['unresolvedOrdinaryAttempt'], 'UNKNOWN_ACCEPTANCE_REQUIRES_RECONCILIATION'
        owner = a.runtime/'rank-campaign-owner'
        raw = (owner/'nomination.json').read_bytes()
        old = json.loads(raw)
        qid = completion(a.runtime, view, hashlib.sha256(raw).hexdigest())
        stale_unused = old['account']['actualFillsSha256'] != view['account']['actualFillsSha256'] and not any(q.get('reviewedRankCampaign') for q in view['activeNativeOrders'])
        if int(time.time()*1000) < old['validUntilMs'] and qid is None and not stale_unused:
            time.sleep(5)
            continue
        output = a.output/f'cycle-{completed+1:04d}'
        args = ['python3', str(a.tools/'prepare-live-money-research.py')]
        for key, value in dict(source=a.source, tools=a.tools, runtime=a.runtime,
                               reference=a.reference, deployment=a.deployment,
                               field=a.field, output=output).items():
            args += ['--'+key, str(value)]
        args += ['--horizon-ms', '900000', '--wait-seconds', '60']
        with (a.output/f'cycle-{completed+1:04d}.log').open('x') as log:
            subprocess.run(args, stdout=log, stderr=subprocess.STDOUT, check=True)
            prepared = output/'prepared'
            summary = json.loads((prepared/'prepared.public.json').read_bytes())
            assert summary['objective'] == 'declared_positive_expected_money'
            assert summary['probabilityModels'] == 5 and not summary['actualFillPriceClaimed']
            assert summary['weakestDeclaredExpectedChange'] > 0
            assert float(summary['ordinaryMinimumExpectedChange']) > 0
            args = ['python3', str(a.tools/'publish-current-money-review.py'),
                    '--runtime', str(a.runtime), '--prepared', str(prepared),
                    '--expected-pid', str(deployed['pid']),
                    '--expected-binary-sha256', deployed['binarySha256'],
                    '--expected-raw-sha256', summary['rawSha256'],
                    '--source-commit', a.source_commit]
            if qid is not None:
                args += ['--completed-qid', str(qid)]
            elif stale_unused:
                args += ['--supersede-filled-account']
            subprocess.run(args, stdout=log, stderr=subprocess.STDOUT, check=True)
        completed += 1
        print(json.dumps(dict(at=datetime.now(timezone.utc).isoformat(),
            state='fresh_review_published', cycle=completed, nomination=summary,
            accountClientsCreated=0, ordersSubmittedBySupervisor=0)), flush=True)
        time.sleep(1)


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    for name in ('source', 'tools', 'runtime', 'reference', 'deployment', 'field', 'output'):
        p.add_argument('--'+name, type=Path, required=True)
    p.add_argument('--source-commit', required=True)
    p.add_argument('--cycles', type=int, default=12)
    p.add_argument('--minutes', type=int, default=120)
    p.add_argument('--publish-reviewed-model-policy', action='store_true')
    run(p.parse_args())
