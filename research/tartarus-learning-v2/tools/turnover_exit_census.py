"""File-only FIFO/bid-depth experiment. No orders, account client or price authority.

The zero-fee path is deliberately limited to matched original fills and wallet
lots. Inherited/mixed/unmatched positions and nonzero/unknown fees are reported
without inventing net profit. Displayed depth is not a future fill guarantee.
"""
from collections import deque
from datetime import datetime, timezone
from decimal import Decimal as D
import argparse
import hashlib
import json
from pathlib import Path
import time

from turnover_opportunity_census import clock, levels, load, recent_books


def reconstruct(fills, exchange, side, quantity):
    # Under FIFO, remaining units are a suffix of original BUY units on this
    # side. Match that suffix exactly to the current wallet below. Earlier
    # opposite-side activity or disposed inherited lots are not new cost basis.
    lots = deque()
    left = quantity
    for f in reversed(fills):
        if not left:
            break
        if f['exchangeId'] != exchange or f['side'] != side or f['action'] != 'buy':
            continue
        if f.get('counterfactual') is not False or D(f['fee']) != 0:
            raise ValueError('simulated_or_nonzero_fee_remaining_lot')
        n = min(left, f['quantity']); left -= n
        lots.appendleft(dict(quantity=n, price=f['price'], executedAt=f['executedAt']))
    if left:
        raise ValueError('unattributed_or_inherited_fifo')
    return list(lots)


def bid_prefix(lots, bids, cap, gain):
    """Largest FIFO prefix supported by actual displayed depth and one limit.

    The worst original unit cost must remain profitable even when any subset
    fills at the marginal limit. Never skip an expensive leading lot.
    """
    queue = deque((D(l['price']), int(l['quantity'])) for l in lots)
    n, cost, proceeds, worst, limit = 0, D(0), D(0), D(0), None
    for price, depth in bids:
        while depth and queue and n < cap:
            unit, remaining = queue[0]
            candidate_worst = max(worst, unit)
            if price < candidate_worst + gain:
                return dict(quantity=n, cost=cost, displayedProceeds=proceeds, limit=limit,
                            maximumUnitCost=worst)
            take = min(depth, remaining, cap-n)
            n += take; depth -= take; remaining -= take
            cost += unit*take; proceeds += price*take
            worst, limit = candidate_worst, price
            queue.popleft()
            if remaining:
                queue.appendleft((unit, remaining))
    return dict(quantity=n, cost=cost, displayedProceeds=proceeds, limit=limit,
                maximumUnitCost=worst)


def sample(runtime, reference, pid):
    now = time.time_ns()//1000000
    view = load(runtime/'tartarus-current-native-state.json')
    report_raw = (runtime/'bank-all-markets-v1/report.json').read_bytes()
    report = json.loads(report_raw)
    account = report['report']
    assert account['positions'] == view['account']['positions'], 'snapshot_positions_changed'
    assert len(report['fills']) == view['account']['fillCount'], 'snapshot_fills_changed'
    control = load(load(reference/'state.json')['controlPath'])
    books = recent_books(runtime, pid)
    occupied = {o['exchangeId'] for o in view['activeNativeOrders']}
    rows = []
    for spec in control['fundamentalPolicy']['reviews']:
        review = load(spec['path']); ex, side = review['exchangeId'], review['side']
        policy = review.get('passiveValueExit') or {}
        objective = policy.get('pricingObjective') or {}
        if objective.get('kind') != 'fifoProfitTurnover':
            continue
        row = dict(exchangeId=ex, side=side, reasons=[], executionAuthority=False)
        rows.append(row)
        lots = account['positions'].get(ex, {}).get(side, [])
        row['heldQuantity'] = sum(l['quantity'] for l in lots)
        try:
            assert not review.get('existingOrigin'), 'inherited_origin'
            assert reconstruct(report['fills'], ex, side, row['heldQuantity']) == lots, 'original_fifo_mismatch'
            assert view['policy']['feeBps'] == 0 and D(view['policy']['feePerShare']) == 0, 'nonzero_exit_fees'
        except (ValueError, AssertionError) as e:
            row['reasons'].append(str(e)); continue
        row['originalZeroFeeFifoMatched'] = True
        if not lots:
            row['reasons'].append('flat'); continue
        if ex in occupied:
            row['reasons'].append('existing_order')
        if now < review['reviewedAtMs'] or now >= review['expiresAtMs'] or any(
                not 0 <= now-s['observedAtMs'] <= s['maximumAgeMs'] for s in review['sources']):
            row['reasons'].append('review_not_current')
        book = books.get(ex)
        if not book:
            row['reasons'].append('missing_book'); continue
        row.update(bookReceivedAt=book['receivedAt'], bookSourceAt=book['sourceAt'],
                   bookSequence=book['sequence'], bookAgeMs=now-clock(book['receivedAt']))
        if not all(0 <= now-clock(book[k]) <= 10000 for k in ['receivedAt','sourceAt']):
            row['reasons'].append('stale_book')
        bids, asks = levels(book, side, 'sell'), levels(book, side, 'buy')
        if book['marketId'] != review['marketId'] or not bids or not asks or bids[0][0] >= asks[0][0]:
            row['reasons'].append('invalid_book'); continue
        cap = min(view['policy']['quantity'], policy['maximumQuantity'], review['maximumQuantity'])
        gain = D(objective['minimumNetGain'])
        proposal = bid_prefix(lots, bids, cap, gain)
        row.update(bestBid=str(bids[0][0]), bestAsk=str(asks[0][0]), minimumNetGain=str(gain),
                   profitableBidPrefix={k:str(v) if isinstance(v,D) else v for k,v in proposal.items()})
        n = proposal['quantity']
        if n:
            old_limit = max(asks[0][0]-D('0.005'), proposal['maximumUnitCost']+gain)
            row.update(currentAskRelativeLimitForSamePrefix=str(old_limit),
                       minimumLimitGain=str(proposal['limit']*n-proposal['cost']),
                       displayedDepthGain=str(proposal['displayedProceeds']-proposal['cost']),
                       askRelativePolicyMissesProfitableBid=old_limit > bids[0][0])
        row['currentResearchNomination'] = n > 0 and not row['reasons']
    return dict(observedUtc=datetime.fromtimestamp(now/1000, timezone.utc).isoformat(),
                workerPid=pid, nativeViewWrittenAt=view['writtenAt'],
                nativeViewAgeMs=now-clock(view['writtenAt']), reportSha256=hashlib.sha256(report_raw).hexdigest(),
                fillCount=len(report['fills']), rows=rows, accountRequests=0, executionAuthority=False,
                hypotheticalQuotesNotFills=True, futureFillProbabilityEstimated=False)


def main():
    p=argparse.ArgumentParser(); p.add_argument('--runtime',required=True,type=Path)
    p.add_argument('--reference',required=True,type=Path); p.add_argument('--pid',required=True,type=int)
    p.add_argument('--output',required=True,type=Path); a=p.parse_args()
    result=sample(a.runtime,a.reference,a.pid)
    a.output.write_text(json.dumps(result,indent=2)+'\n',encoding='utf-8')
    print(json.dumps({'observedUtc':result['observedUtc'],'rows':len(result['rows']),
        'currentNominations':sum(x.get('currentResearchNomination',False) for x in result['rows'])}))


if __name__=='__main__': main()
