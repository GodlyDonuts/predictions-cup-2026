"""File-only live opportunity census. No API client, credentials or order writer.

Reviewed probabilities are conditional operator assumptions, NOT calibrated
fair value or settlement equivalence. A positive result is a research nomination.
The owner must still reconcile current account/contract/book state before trading.
"""
import argparse
from collections import Counter
from datetime import datetime, timezone
from decimal import Decimal
import hashlib
import json
from pathlib import Path
import time

D = Decimal


def clock(value):
    return int(datetime.fromisoformat(value.replace('Z', '+00:00')).timestamp() * 1000)


def load(path):
    return json.loads(Path(path).read_bytes())


def levels(book, side, action):
    if side == 'YES':
        rows = book['asks' if action == 'buy' else 'bids']
        result = [(D(str(r['price'])), int(r['quantity'])) for r in rows]
    else:
        rows = book['bids' if action == 'buy' else 'asks']
        result = [(1-D(str(r['price'])), int(r['quantity'])) for r in rows]
    return sorted(result, reverse=action == 'sell')


def assess(review, book, account, occupied, now, maximum_age_ms=10000):
    ex, side = review['exchangeId'], review['side']
    probability = D(review['ordinaryProbability'])
    allowance = D(review['exceptionalProbabilityAllowance'])
    conditional = (1-allowance)*probability + allowance*D(review['minimumExceptionalPayout'])
    ceiling = conditional-D(review['marginPerShare'])
    lots = account['positions'].get(ex, {}).get(side, [])
    held = sum(int(l['quantity']) for l in lots)
    cost = sum((D(l['price'])*int(l['quantity']) for l in lots), D(0))
    row = dict(exchangeId=ex, side=side, reviewSha256=hashlib.sha256(
        json.dumps(review, sort_keys=True, separators=(',', ':')).encode()).hexdigest(),
        probability=str(probability), allowance=str(allowance), conditionalPayout=str(conditional),
        entryCeiling=str(ceiling), heldQuantity=held, heldCost=str(cost),
        recurringVersion=review.get('sameSideTopUps', {}).get('version'),
        reasons=[], conditionalCandidate=False, executionAuthority=False)
    if now < review['reviewedAtMs'] or now >= review['expiresAtMs']:
        row['reasons'].append('review_not_current')
    if any(not 0 <= now-s['observedAtMs'] <= s['maximumAgeMs'] for s in review['sources']):
        row['reasons'].append('source_not_current')
    if ex in occupied:
        row['reasons'].append('existing_physical_or_native_liability')
    if book is None:
        row['reasons'].append('missing_book')
        return row
    row['bookAgeMs'] = now-clock(book['receivedAt'])
    row['sourceAgeMs'] = now-clock(book['sourceAt'])
    row['bookSequence'] = book['sequence']
    if book['exchangeId'] != ex or book['marketId'] != review['marketId']:
        row['reasons'].append('scope_mismatch')
    if not all(0 <= row[k] <= maximum_age_ms for k in ['bookAgeMs', 'sourceAgeMs']):
        row['reasons'].append('stale_book')
    asks, bids = levels(book, side, 'buy'), levels(book, side, 'sell')
    if not asks or not bids or bids[0][0] >= asks[0][0]:
        row['reasons'].append('invalid_or_crossed_book')
        return row
    row.update(bestAsk=str(asks[0][0]), bestBid=str(bids[0][0]),
        conditionalEdgeAtAsk=str(conditional-asks[0][0]),
        zeroAllowanceCeiling=str(probability-D(review['marginPerShare'])),
        allowanceIsDecisive=ceiling < asks[0][0] <= probability-D(review['marginPerShare']))
    available = max(D(0), min(D(account['cash'])-D(account['reservedCash']),
        D(review['maximumPrincipal'])-cost))
    capacity = max(0, min(10000, review['maximumQuantity']-held))
    quantity, spend, edge = 0, D(0), D(0)
    for price, depth in asks:
        if price <= 0 or price > ceiling:
            break
        q = min(depth//2, capacity-quantity, int((available-spend)/price))
        if q <= 0:
            break
        quantity += q
        spend += price*q
        edge += (conditional-price)*q
    if asks[0][0] > ceiling:
        row['reasons'].append('price_above_conditional_ceiling')
    if quantity == 0:
        row['reasons'].append('no_size_after_depth_and_capital')
    if edge < D('0.5'):
        row['reasons'].append('conditional_gain_below_0.5')
    row.update(proposedQuantity=quantity, proposedCost=str(spend), conditionalGrossGain=str(edge),
        conditionalCandidate=not row['reasons'])
    return row


def recent_books(runtime, pid):
    """Read only bounded retained recorder tails; missing coverage is not healed."""
    books = {}
    paths = sorted(runtime.glob('breadth-'+str(pid)+'-*.jsonl'), key=lambda p:p.stat().st_mtime)[-2:]
    for path in paths:
        with path.open('rb') as stream:
            offset = max(0, path.stat().st_size-8*1024*1024)
            stream.seek(offset)
            if offset:
                stream.readline()
            for line in stream:
                if not line.endswith(b'\n'):
                    continue
                record = json.loads(line)['event']
                if record['event'] == 'stream_reset':
                    books.clear()
                if record['event'] != 'markets_batch':
                    continue
                for market in record['payload']['markets']:
                    batch = market['batch']
                    if batch.get('resyncRequired'):
                        books = {k:v for k,v in books.items() if v['marketId'] != str(market['marketId'])}
                        continue
                    for b in batch.get('books', []):
                        ex = str(b['exchangeId'])
                        books[ex] = dict(exchangeId=ex, marketId=str(market['marketId']),
                            bids=b['bids'], asks=b['asks'], receivedAt=record['receivedAt'],
                            sourceAt=b['asOf']['at'], sequence=str(b['asOf']['sequence']))
    return books


def sample(runtime, reference, pid):
    now = time.time_ns()//1000000
    state = load(reference/'state.json')
    control = load(state['controlPath'])
    view = load(runtime/'tartarus-current-native-state.json')
    books = recent_books(runtime, pid)
    occupied = {o['exchangeId'] for o in view['activeNativeOrders']}
    rows = [assess(load(r['path']), books.get(load(r['path'])['exchangeId']),
        view['account'], occupied, now) for r in control['fundamentalPolicy']['reviews']]
    return dict(observedAt=datetime.fromtimestamp(now/1000, timezone.utc).isoformat(),
        observedAtMs=now, workerPid=pid, nativeViewAgeMs=now-clock(view['writtenAt']),
        reviewedMarkets=len(rows), observedBooks=len(books), rows=rows,
        reasons=dict(Counter(reason for r in rows for reason in r['reasons'])),
        conditionalCandidates=sum(r['conditionalCandidate'] for r in rows),
        officialAccountRequests=0, executionAuthority=False, accountEligibilityVerified=False,
        calibratedProbabilityClaimed=False, settlementEquivalenceClaimed=False)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--runtime', required=True, type=Path)
    parser.add_argument('--reference', required=True, type=Path)
    parser.add_argument('--pid', required=True, type=int)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--seconds', type=int, default=600)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    start, active, episodes = time.monotonic(), set(), 0
    with (args.output/'observations.jsonl').open('a', encoding='utf8') as log:
        while True:
            value = sample(args.runtime, args.reference, args.pid)
            # Price-version updates and partial fills do not mint new episodes.
            current = {(r['exchangeId'],r['side']) for r in value['rows'] if r['conditionalCandidate']}
            # Stale/missing data cannot prove the prior economic episode ended.
            observed = {(r['exchangeId'],r['side']) for r in value['rows']
                if not any(x in r['reasons'] for x in ['stale_book','missing_book','scope_mismatch','invalid_or_crossed_book'])}
            new = current-active
            episodes += len(new)
            active = (active-observed)|current
            elapsed = time.monotonic()-start
            value.update(distinctConditionalSignalEpisodes=episodes,
                elapsedSeconds=round(elapsed,3), newEpisodes=sorted(new),
                filledOrdersClaimed=0)
            raw = json.dumps(value, separators=(',', ':'))
            log.write(raw+'\n');log.flush()
            temporary=args.output/'latest.pending.json'; temporary.write_text(raw+'\n',encoding='utf8')
            temporary.replace(args.output/'latest.json')
            if elapsed >= args.seconds:
                break
            time.sleep(5)


if __name__ == '__main__':
    main()
