#!/usr/bin/env python3
"""Finite original-file hedge audit; no network, engine replay or trading operations."""
import argparse
from collections import Counter, defaultdict, deque
import gzip
import hashlib
import importlib.util
import json
from pathlib import Path
import statistics

ROOT = Path(__file__).resolve().parents[2]
MAKER = ROOT / 'research/moloch-maker-markout-20261016/analyze.py'
spec = importlib.util.spec_from_file_location('original_maker_audit', MAKER)
A = importlib.util.module_from_spec(spec)
spec.loader.exec_module(A)
POLICY_SHA = 'a2664a76823975c43575b5e88c8ba07fce7241e28c1d68d6f88de26c09182597'
MAX_V7_DECODED = 128 * 1024**2


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def audit(directory, manifest_sha, v7_path, v7_sha):
    directory, v7_path = Path(directory), Path(v7_path)
    A.pinned(directory / 'manifest.json', manifest_sha)
    manifest = json.loads((directory / 'manifest.json').read_bytes())
    files = {Path(f['path']).name: f for f in manifest['files']}
    for name in ('report.json', 'dispatch.ndjson'):
        A.pinned(directory / name, files[name]['sha256'], files[name]['bytes'])
    A.pinned(directory / 'policy-extract.json', POLICY_SHA)
    policy = json.loads((directory / 'policy-extract.json').read_bytes())['policy']
    snapshot = A.RAW.native_sealed_object((directory / 'report.json').read_bytes())
    fills, excluded, duplicate_count = A.fill_inputs(snapshot)
    dispatch_stats = {}
    dispatch = list(A.rows(directory / 'dispatch.ndjson', '0' * 64, dispatch_stats))
    intents, acks = A.attribution(dispatch, fills)
    attempted = {str(r['event']['qid']) for r in dispatch if r['event'].get('kind') == 'attempt'}
    orders = {str(q['id']): q for q in snapshot['orders']}
    ordinary = [f for f in fills if f['action'] == 'buy' and f['qid'] not in excluded
                and f['qid'] in intents and intents[f['qid']].get('fundamental') is None
                and f['qid'] in acks and acks[f['qid']] <= f['executed']]
    ordinary_qids = {f['qid'] for f in ordinary}
    first = {}
    for f in ordinary:
        first.setdefault(f['qid'], f)
    lifecycle, delays = Counter(), []
    for f in first.values():
        q = f['order']
        mates = [o for o in orders.values() if o['exchangeId'] == q['exchangeId']
                 and o['marketId'] == q['marketId'] and o['action'] == 'buy'
                 and o['side'] != q['side'] and o['createdAt'] == q['createdAt']]
        lifecycle['ordinaryIntents'] += 1
        if len(mates) != 1:
            lifecycle['noUniqueOriginalMate'] += 1
            continue
        o = mates[0]
        lifecycle['uniqueOriginalMate'] += 1
        observed = f['observed'] // A.MS
        if A.units(o['price']) + f['price'] < A.RAW.SCALE:
            lifecycle['mateLimitPlusFirstActualBuyBelowOne'] += 1
        ever_attempted = str(o['id']) in attempted
        lifecycle['mateEverAttempted' if ever_attempted else 'mateNeverAttemptedInPrefix'] += 1
        if o['expiresAt'] <= observed:
            lifecycle['mateExpiredBeforeFirstNativeFillObservation'] += 1
        if o.get('cancelEffectiveAt') is not None:
            delta = o['cancelEffectiveAt'] - observed
            delays.append(delta)
            lifecycle['mateCancelExactlyObservedFillPlus3000ms' if delta == 3000 else
                      'mateCancelEffectiveBeforeFirstObservation' if delta < 0 else
                      'mateCancelLaterOtherDelay'] += 1
        if (not ever_attempted and o['quantity'] <= f['n'] and o['expiresAt'] > observed
                and o.get('cancelEffectiveAt') == observed + 3000):
            lifecycle['neverAttemptedWholeMateCoveredByFirstFillAndCanceled3000msLater'] += 1

    def opening_lots():
        out = defaultdict(deque)
        for row in snapshot['report']['inheritedInventory']['sourcePositions']:
            for lot in row['lots']:
                out[(row['exchangeId'], lot['side'])].append([int(lot['quantity']), 'inherited'])
        return out

    def apply(lots, f, counts):
        ex, side, n = f['exchange'], f['side'], f['n']
        opposite = 'NO' if side == 'YES' else 'YES'

        def consume(key, n):
            while n:
                A.need(bool(lots[key]), 'NEGATIVE_INVENTORY')
                lot = lots[key][0]
                take = min(n, lot[0]); n -= take; lot[0] -= take
                if not lot[0]:
                    lots[key].popleft()

        if f['action'] == 'sell':
            consume((ex, side), n)
        else:
            net = min(n, sum(l[0] for l in lots[(ex, opposite)]))
            if net:
                counts['oppositeBuyNettingFills'] += 1
                counts['oppositeBuyNettingShares'] += net
                if f['qid'] in ordinary_qids:
                    counts['ordinaryOppositeBuyNettingFills'] += 1
            consume((ex, opposite), net)
            if n > net:
                lots[(ex, side)].append([n - net, 'ordinary' if f['qid'] in ordinary_qids else 'reviewed'])

    netting, lots = Counter(), opening_lots()
    for f in fills:
        apply(lots, f, netting)
    expected = {(ex, side): sum(l['quantity'] for l in ls)
                for ex, sides in snapshot['report']['positions'].items() for side, ls in sides.items()}
    A.need(all(sum(l[0] for l in lots[k]) == expected.get(k, 0)
               for k in set(lots) | set(expected)), 'ENDING_POSITION_QUANTITY_PARITY')

    A.pinned(v7_path, v7_sha)
    A.need(v7_path.stat().st_size <= MAX_V7_DECODED, 'V7_WRAPPER_BOUND')
    A.need(all(a['observed'] <= b['observed'] for a, b in zip(fills, fills[1:])), 'NATIVE_OBSERVATION_ORDER')
    kinds, overlap = Counter(), Counter()
    lots, applied, decoded, digest, decisions = opening_lots(), 0, 0, hashlib.sha256(), []
    with gzip.open(v7_path, 'rb') as source:
        for index in range(100_001):
            raw = source.readline(A.RAW.MAX_ROW_BYTES + 1)
            if not raw:
                break
            decoded += len(raw)
            A.need(index < 100_000 and len(raw) <= A.RAW.MAX_ROW_BYTES and decoded <= MAX_V7_DECODED, 'V7_DECODED_BOUND')
            digest.update(raw)
            row = json.loads(raw)
            kinds[row['kind']] += 1
            if row['kind'] != 'decision_features':
                continue
            t = A.clock(row['decisionAt'])
            A.need(row['decisionMs'] == t // A.MS and (not decisions or t >= decisions[-1]), 'V7_DECISION_CLOCK')
            decisions.append(t)
            while applied < len(fills) and fills[applied]['observed'] <= t:
                apply(lots, fills[applied], Counter()); applied += 1
            for side in ('YES', 'NO'):
                held = lots[(str(row['exchangeId']), side)]
                if any(l[1] == 'ordinary' for l in held):
                    overlap['decisionExchangeSidesWithAnyObservedOrdinaryInventory'] += 1
                    if all(l[1] == 'ordinary' for l in held):
                        overlap['decisionExchangeSidesWithEntirelyObservedOrdinaryInventory'] += 1
    A.need(bool(decisions), 'NO_V7_DECISIONS')
    # Recheck immutable input pins after analysis; no moving-bank inference.
    for name in ('report.json', 'dispatch.ndjson'):
        A.pinned(directory / name, files[name]['sha256'], files[name]['bytes'])
    A.pinned(directory / 'manifest.json', manifest_sha)
    A.pinned(directory / 'policy-extract.json', POLICY_SHA)
    A.pinned(v7_path, v7_sha)
    return {
        'schemaVersion': 1, 'sourceRevisionInspected': 'e5fa7d6f601a9c5ac507d6e17d0f7feb3b7cc9bb',
        'reportCutoff': snapshot['report']['at'], 'bankSequence': snapshot['sequence'],
        'reportEnvelopeSeal': snapshot['hash'],
        'sources': {'manifestSha256': manifest_sha, 'reportSha256': files['report.json']['sha256'],
                    'reportBytes': files['report.json']['bytes'], 'dispatchSha256': files['dispatch.ndjson']['sha256'],
                    'dispatchBytes': files['dispatch.ndjson']['bytes'], 'policyExtractSha256': POLICY_SHA,
                    'v7WrapperSha256': v7_sha, 'v7WrapperBytes': v7_path.stat().st_size,
                    'v7DecodedSha256': digest.hexdigest(), 'v7DecodedBytes': decoded,
                    'makerAnalyzerSourceSha256': sha(MAKER), 'rawSealParserSourceSha256': sha(A.RAW_PATH),
                    'auditScriptSourceSha256': sha(__file__)},
        'checks': {'reportSealVerified': True, 'dispatchRowsChainVerified': dispatch_stats['verifiedRows'],
                   'endingPositionQuantityParity': True, 'duplicateFillObservations': duplicate_count,
                   'v7OwnJournalChainVerified': False, 'v7ReferencedSourceMembersVerified': False,
                   'engineReplayPerformed': False, 'accountRequests': 0},
        'policy': {k: policy.get(k) for k in ('retainProfitableHedge', 'repriceProfitableHedge',
                   'maximumHedgeOvershoot', 'maximumInventoryAgeMs', 'inventoryAgeClock',
                   'inventoryExitMode', 'inventoryMinimumGain', 'inventoryTakerAfterMs', 'feeBps', 'feePerShare')},
        'uniqueFills': len(fills), 'ordinaryBuyFills': len(ordinary), 'ordinaryBuyShares': sum(f['n'] for f in ordinary),
        'pairLifecycle': {**dict(lifecycle), 'neverAttemptedWholeMateCoveredByFirstFillAndCanceled3000msLater': lifecycle['neverAttemptedWholeMateCoveredByFirstFillAndCanceled3000msLater']},
        'mateCancelMinusFirstNativeObservationMs': {'count': len(delays), 'minimum': min(delays),
                                                   'median': statistics.median(delays), 'maximum': max(delays)},
        'actualNetting': {k: netting[k] for k in ('oppositeBuyNettingFills', 'oppositeBuyNettingShares',
                                                'ordinaryOppositeBuyNettingFills')},
        'v7': {'rowsByKind': dict(kinds), 'firstDecisionMs': decisions[0] // A.MS,
               'lastDecisionMs': decisions[-1] // A.MS,
               'decisionExchangeSidesWithAnyObservedOrdinaryInventory': overlap['decisionExchangeSidesWithAnyObservedOrdinaryInventory'],
               'decisionExchangeSidesWithEntirelyObservedOrdinaryInventory': overlap['decisionExchangeSidesWithEntirelyObservedOrdinaryInventory'],
               'fullDepthOrPhysicalPreflightProof': False},
        'missedProfitableHedgeEstablished': False, 'executionSourceChanged': False,
        'limitations': ['Original quote mate matching uses same creation clock, exchange, market and opposite side.',
                        'Ever-attempted and cancellation times describe the finite prefix, not independent venue causation.',
                        'Whole-mate coverage test uses the first actual fill only; it is not a complete historical eligibility replay.',
                        'Only inventory quantities are replayed here; exact financial/FIFO-cost audit remains separate.',
                        'V7 feature clocks/quantities are derived observations, without full original order/preflight/current-cash proof.',
                        'No matching observation is missing coverage, not proof no opportunity existed.',
                        'No maker fill, avoided loss, profitability or policy activation is inferred.']}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--input-directory', required=True)
    p.add_argument('--manifest-sha256', required=True)
    p.add_argument('--sealed-v7', required=True)
    p.add_argument('--sealed-v7-sha256', required=True)
    p.add_argument('--output', required=True)
    args = p.parse_args()
    report = audit(args.input_directory, args.manifest_sha256, args.sealed_v7, args.sealed_v7_sha256)
    Path(args.output).write_text(json.dumps(report, sort_keys=True, indent=2) + '\n', encoding='utf-8')
    print(json.dumps({'uniqueFills': report['uniqueFills'], 'ordinaryBuyIntents': report['pairLifecycle']['ordinaryIntents'],
                      'actualOppositeNettingFills': report['actualNetting']['oppositeBuyNettingFills'],
                      'v7MatchingOrdinaryObservations': report['v7']['decisionExchangeSidesWithAnyObservedOrdinaryInventory']}))


if __name__ == '__main__':
    main()
