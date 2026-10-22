"""Exact-depth, hindsight quote-cycle diagnostics from original SIG recorder rows.

Receiver clocks are not reconstructed worker execution clocks. No orders,
passive fills, market impact, independent opportunities or strategy ROI are
inferred. The best historical entry uses hindsight and is not a trading policy.
"""
import argparse
from collections import deque
from datetime import datetime, timezone
from decimal import Decimal
from hashlib import sha256
import json
from pathlib import Path
import time

import research_book_observations as observations


def sweep(book, side, quantity, buy):
    key = ('asks' if side == 'YES' else 'bids') if buy else ('bids' if side == 'YES' else 'asks')
    left, value = quantity, 0
    for level in book[key]:
        p = int(Decimal(level['price'])*1000000)
        if side == 'NO':
            p = 1000000-p
        take = min(left, level['quantity'])
        value += take*p
        left -= take
        if not left:
            return value
    return None


def best_cycle(points, separation_ms, window_ms=300000):
    eligible, start, segment = deque(), 0, None
    best = dict(maximumGrossSurplusMicros=0, candidateObservations=0,
                minimumQuoteSeparationMs=separation_ms, maximumHoldingWindowMs=window_ms)
    for i, point in enumerate(points):
        if point['segment'] != segment:
            eligible.clear(); start = i; segment = point['segment']
        at = point['receivedAtMs']
        while start < i and points[start]['receivedAtMs'] <= at-separation_ms:
            old = points[start]
            if old['askCostMicros'] is not None:
                while eligible and points[eligible[-1]]['askCostMicros'] >= old['askCostMicros']:
                    eligible.pop()
                eligible.append(start)
            start += 1
        while eligible and points[eligible[0]]['receivedAtMs'] < at-window_ms:
            eligible.popleft()
        if not eligible or point['bidProceedsMicros'] is None:
            continue
        entry = points[eligible[0]]
        surplus = point['bidProceedsMicros']-entry['askCostMicros']
        if surplus > 0:
            best['candidateObservations'] += 1
        if surplus > best['maximumGrossSurplusMicros']:
            best.update(maximumGrossSurplusMicros=surplus,
                entryReceivedAtMs=entry['receivedAtMs'], exitReceivedAtMs=at,
                entryAskCostMicros=entry['askCostMicros'], exitBidProceedsMicros=point['bidProceedsMicros'])
    return best


def run(a):
    view_raw = (a.runtime/'tartarus-current-native-state.json').read_bytes()
    view = json.loads(view_raw)
    targets = {}
    for text in a.target:
        ex, side, n = text.split(':')
        assert ex in view['scope']['exchanges'] and side in ('YES', 'NO') and 0 < int(n) <= 1000000
        targets[ex, side] = int(n)
    mapping = {ex: view['scope']['exchanges'][ex] for ex, _ in targets}
    segments = {ex: 0 for ex in mapping}
    revisions, points, last_signature = {}, {key: [] for key in targets}, {}
    paths = sorted(a.runtime.glob(f'breadth-{a.pid}-*.jsonl'), key=lambda p:p.stat().st_mtime_ns)
    assert paths, 'ORIGINAL_RECORDER_REQUIRED'
    sources, last_hash, last_sequence, gaps, rows = [], None, None, 0, 0
    for path in paths:
        limit = path.stat().st_size
        prefix, count, consumed = sha256(), 0, 0
        with path.open('rb') as stream:
            while stream.tell() < limit:
                raw = stream.readline(limit-stream.tell())
                if not raw.endswith(b'\n'):
                    break
                record = json.loads(raw)
                event = observations.event(raw)
                if last_hash is not None and (record['previous'] != last_hash or record['sequence'] != last_sequence+1):
                    gaps += 1
                    for ex in segments:
                        segments[ex] += 1
                last_hash, last_sequence = record['hash'], record['sequence']
                prefix.update(raw); consumed += len(raw); count += 1; rows += 1
                if event['event'] == 'stream_reset':
                    for ex in segments:
                        segments[ex] += 1
                    revisions.clear()
                    continue
                if event['event'] in ('book_dirty', 'market_settled'):
                    mid = str(event['payload']['marketId'])
                    for ex, market in mapping.items():
                        if market == mid:
                            segments[ex] += 1
                    continue
                if event['event'] != 'markets_batch':
                    continue
                at = observations.clock(event['receivedAt'])
                books = observations.row_books(raw, mapping, view['scope']['tournamentId'], at)
                for market in event['payload']['markets']:
                    mid, batch = str(market['marketId']), market['batch']
                    relevant = [ex for ex, m in mapping.items() if m == mid]
                    if not relevant:
                        continue
                    delivery = batch['delivery']
                    gap = mid in revisions and delivery['previousRevision'] != revisions[mid]
                    revisions[mid] = delivery['revision']
                    dirty = {str(b['exchangeId']) for b in batch.get('bookDirty', [])}
                    present = {str(b['exchangeId']) for b in batch.get('books', [])}
                    for ex in relevant:
                        if gap or batch.get('resyncRequired') or batch.get('marketSettled') or (ex not in books and ex in dirty|present):
                            segments[ex] += 1
                for (ex, side), n in targets.items():
                    if ex not in books:
                        continue
                    book = books[ex]
                    bid, ask = sweep(book, side, n, False), sweep(book, side, n, True)
                    signature = book['sequence'], bid, ask, segments[ex]
                    if last_signature.get((ex, side)) == signature:
                        continue
                    previous = points[ex, side][-1] if points[ex, side] else None
                    if previous and (at < previous['receivedAtMs'] or at-previous['receivedAtMs'] > 60000):
                        segments[ex] += 1
                    last_signature[ex, side] = signature
                    points[ex, side].append(dict(receivedAtMs=at, sourceAt=book['sourceAt'],
                        sequence=book['sequence'], segment=segments[ex],
                        bidProceedsMicros=bid, askCostMicros=ask))
        sources.append(dict(path=str(path), originalCompletePrefixBytes=consumed,
                            prefixSha256=prefix.hexdigest(), validatedRows=count))
        print(json.dumps(dict(phase='original_prefix_scanned', path=path.name, bytes=consumed, rows=count)), flush=True)
    result = dict(observedUtc=datetime.now(timezone.utc).isoformat(), workerPid=a.pid,
        sources=sources, originalRowsValidated=rows, recorderChainGaps=gaps,
        originalViewSha256=sha256(view_raw).hexdigest(), maximumSourceDeliveryAgeMs=observations.MAX_AGE_MS,
        targets=[dict(exchangeId=ex, side=side, quantity=targets[ex, side], samples=len(series),
                      diagnostics=[best_cycle(series, lag) for lag in (1000, 10000, 60000)])
                 for (ex, side), series in points.items()],
        hindsightSelection=True, independentOpportunitiesClaimed=False,
        feesIncluded=False, passiveFillsOrMarketImpactModeled=False,
        workerProcessingLatencyReconstructed=False, strategyRoiClaimed=False,
        newAccountClients=0, executionAuthority=False)
    with a.output.open('x') as f:
        json.dump(result, f, indent=2); f.write('\n')
    print(json.dumps(dict(phase='completed', rows=rows, targets=result['targets'], executionAuthority=False)), flush=True)
    return result


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--runtime', type=Path, required=True)
    p.add_argument('--pid', type=int, required=True)
    p.add_argument('--target', action='append', required=True)
    p.add_argument('--output', type=Path, required=True)
    run(p.parse_args())
