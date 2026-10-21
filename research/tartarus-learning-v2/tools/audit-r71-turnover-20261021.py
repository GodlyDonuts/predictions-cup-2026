"""Audit actual BUY/SELL turnover from saved official receipts; no network.

Fails if non-fill financial activity makes this incremental FIFO replay
incomplete. Fill receipts, filled orders and turnover are counted separately.
"""
from pathlib import Path
from decimal import Decimal as N
from copy import deepcopy
from datetime import datetime, timezone, timedelta
import argparse
import hashlib
import json

ROOT = Path('/home/archon/apps/moloch/shared/live-production-v1')
BASELINE = ROOT.parent / 'incidents/r71-recurring-20261021T194753Z/report.json'
OUT = Path('/tmp/moloch-recurring-cohort-20261021/r71-turnover.public.json')


def filled_order_cadence(fills, observed):
    """Count each physical order once, at its first execution, across all history.

    Later partial receipts cannot manufacture a new trade in another minute.
    These are observed executions, not an assertion of complete venue coverage.
    """
    first = {}
    for fill in fills:
        assert fill.get('counterfactual') is False
        actual = fill['actualOrderId']
        at = datetime.fromisoformat(fill['executedAt'].replace('Z', '+00:00'))
        assert at.tzinfo is not None and at <= observed
        first[actual] = min(first.get(actual, at), at)
    windows = {}
    for minutes in (1, 5, 15, 60):
        start = observed - timedelta(minutes=minutes)
        executions = sorted(at for at in first.values() if start < at <= observed)
        count = len(executions)
        anchors = [start, *executions, observed]
        maximum_gap = max((b-a).total_seconds() for a, b in zip(anchors, anchors[1:]))
        completed_end = observed.replace(second=0, microsecond=0)
        complete_counts = [sum(completed_end-timedelta(minutes=i+1) <= at
                               < completed_end-timedelta(minutes=i) for at in first.values())
                           for i in range(minutes)]
        windows[str(minutes)] = dict(distinctFirstFilledOrders=count,
            filledOrdersPerMinute=count / minutes, targetFilledOrders=minutes,
            averageTargetMet=count >= minutes, maximumGapWithinWindowSeconds=maximum_gap,
            completedUtcMinutesObserved=minutes,
            completedUtcMinutesWithNoNewFilledOrder=sum(n == 0 for n in complete_counts),
            rollingMinuteCadenceMet=count >= minutes and maximum_gap <= 60
                and bool(executions) and (observed-executions[-1]).total_seconds() < 60)
    last = max(first.values()) if first else None
    return dict(observedUtc=observed.isoformat(), targetFilledOrdersPerMinute=1,
        counting='Each actual order once at first official execution; partial fills do not reset the clock',
        windowsMinutes=windows, latestNewFilledOrderUtc=last.isoformat() if last else None,
        secondsSinceNewFilledOrder=(observed - last).total_seconds() if last else None,
        accountRequests=0, executionAuthority=False)


def take(lots, quantity):
    cost = N(0)
    while quantity:
        lot = lots[0]
        n = min(quantity, lot['quantity'])
        cost += n * N(lot['price'])
        quantity -= n
        lot['quantity'] -= n
        if not lot['quantity']:
            lots.pop(0)
    return cost


def audit(baseline, output):
    before_raw = baseline.read_bytes()
    before = json.loads(before_raw)
    raw = (ROOT / 'bank-all-markets-v1/report.json').read_bytes()
    after = json.loads(raw)
    count = len(before['fills'])
    assert after['fills'][:count] == before['fills']
    wanted = {f['orderId'] for f in after['fills'][count:]}
    acknowledgments = {}
    if wanted:
        with (ROOT / 'dispatch.ndjson').open() as stream:
            for line in stream:
                event = json.loads(line)['event']
                if event.get('kind') == 'ack' and event.get('qid') in wanted:
                    assert event['qid'] not in acknowledgments
                    receipt = event['receipt']
                    assert receipt['status'] == 200 and receipt['endpoint'] == '/orders'
                    acknowledgments[event['qid']] = receipt['data']
        assert set(acknowledgments) == wanted, 'original normal order acknowledgment required'
    positions = deepcopy(before['report']['positions'])
    orders, receipt_hashes, identities = {}, {}, set()
    for fill in after['fills'][count:]:
        assert fill['counterfactual'] is False and fill['action'] in ['buy', 'sell']
        ack = acknowledgments[fill['orderId']]
        assert str(ack['orderId']) == fill['actualOrderId']
        assert ack['action'] == fill['action'] and ack['exchangeId'] == fill['exchangeId']
        assert ack['side'].upper() == fill['side']
        identity = (fill['actualOrderId'], fill['sourceId'])
        assert identity not in identities
        identities.add(identity)
        receipt_raw = (ROOT / 'receipts' / (fill['receiptHash'] + '.json')).read_bytes()
        receipt = json.loads(receipt_raw)
        assert receipt['status'] == 200
        assert receipt['endpoint'] == f"/orders/{fill['actualOrderId']}/fills?limit=200"
        rows = [row for row in receipt['data']['data'] if str(row['id']) == fill['sourceId']]
        assert len(rows) == 1
        row = rows[0]
        assert row['filledAt'] == fill['executedAt'] and abs(row['quantity']) == fill['quantity']
        # SIG signs fill quantities by outcome: YES positive, NO negative.
        # Buy/sell direction comes from the original bound order, not this sign.
        assert (row['quantity'] > 0) == (fill['side'] == 'YES')
        assert row['side'].upper() == fill['side'] and N(str(row['price'])) == N(fill['price'])
        receipt_hashes[fill['receiptHash']] = hashlib.sha256(receipt_raw).hexdigest()
        group = orders.setdefault(str(fill['orderId']), dict(
            actualOrderId=fill['actualOrderId'], exchangeId=fill['exchangeId'], side=fill['side'],
            action=fill['action'], shares=0, fillReceipts=0, notional=N(0),
            cashDelta=N(0), ownedCostDelta=N(0), realizedGross=N(0), fees=N(0),
            firstExecutedAt=fill['executedAt'], lastExecutedAt=fill['executedAt']))
        assert all(group[k] == fill[k] for k in ['actualOrderId', 'exchangeId', 'side', 'action'])
        group['shares'] += fill['quantity']
        group['fillReceipts'] += 1
        group['firstExecutedAt'] = min(group['firstExecutedAt'], fill['executedAt'])
        group['lastExecutedAt'] = max(group['lastExecutedAt'], fill['executedAt'])
        group['fees'] += N(fill['fee'])
        value = fill['quantity'] * N(fill['price'])
        group['notional'] += value
        sides = positions.setdefault(fill['exchangeId'], dict(YES=[], NO=[]))
        lots = sides[fill['side']]
        if fill['action'] == 'sell':
            cost = take(lots, fill['quantity'])
            group['cashDelta'] += value - N(fill['fee'])
            group['ownedCostDelta'] -= cost
            group['realizedGross'] += value - cost
        else:
            lots.append(dict(quantity=fill['quantity'], price=fill['price'], executedAt=fill['executedAt']))
            group['cashDelta'] -= value + N(fill['fee'])
            group['ownedCostDelta'] += value
            other = sides['NO' if fill['side'] == 'YES' else 'YES']
            pairs = min(sum(l['quantity'] for l in lots), sum(l['quantity'] for l in other))
            if pairs:
                cost = take(lots, pairs) + take(other, pairs)
                group['cashDelta'] += N(pairs)
                group['ownedCostDelta'] -= cost
                group['realizedGross'] += N(pairs) - cost
    for group in orders.values():
        group['realizedNet'] = group['realizedGross'] - group['fees']
    total = lambda key: sum((g[key] for g in orders.values()), N(0))
    pre, post = before['report'], after['report']
    assert positions == post['positions'], 'non-fill position activity or incorrect FIFO'
    assert N(post['ownedCost']) - N(pre['ownedCost']) == total('ownedCostDelta')
    assert N(post['cash']) - N(pre['cash']) == total('cashDelta')
    assert N(post['realized']) - N(pre['realized']) == total('realizedNet')
    today = [f for f in after['fills'] if f.get('executedAt', '').startswith('2026-10-21')]
    observed = datetime.now(timezone.utc)
    proof = dict(observedUtc=observed.isoformat(), baselineIncident=baseline.parent.name,
        baselineReportSha256=hashlib.sha256(before_raw).hexdigest(),
        originalFillCount=count, currentFillCount=len(after['fills']), originalFillsUnchanged=True,
        remainingFifoExactlyMatches=True, allNewFillsMatchOriginalOfficialReceipts=True,
        reportSha256=hashlib.sha256(raw).hexdigest(), reportAt=post['at'], orders=orders,
        financial={k: post[k] for k in ['cash', 'ownedCost', 'realized', 'newTradingRealized', 'inheritedRealizedGross']},
        incrementalRealizedNet=total('realizedNet'), incrementalTurnover=total('notional'),
        incrementalShares=total('shares'), incrementalDistinctFilledOrders=len(orders),
        officialReceiptFileHashes=receipt_hashes, utcDay='2026-10-21', dayFillReceipts=len(today),
        dayDistinctFilledOrders=len({f['actualOrderId'] for f in today}),
        cadence=filled_order_cadence(after['fills'], observed),
        noAccountRequestsByAudit=True, wholeAccountProfitEstablished=False)
    output.write_text(json.dumps(proof, default=str, indent=2) + '\n', encoding='utf-8')
    print(json.dumps({k: proof[k] for k in ['observedUtc', 'currentFillCount', 'incrementalRealizedNet',
        'incrementalTurnover', 'incrementalDistinctFilledOrders', 'financial']}, default=str))


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--baseline', type=Path, default=BASELINE)
    parser.add_argument('--output', type=Path, default=OUT)
    args = parser.parse_args()
    audit(args.baseline, args.output)
