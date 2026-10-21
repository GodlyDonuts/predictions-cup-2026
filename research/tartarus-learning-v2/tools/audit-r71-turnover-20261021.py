"""Audit actual BUY/SELL turnover from saved official receipts; no network.

Fails if non-fill financial activity makes this incremental FIFO replay
incomplete. Fill receipts, filled orders and turnover are counted separately.
"""
from pathlib import Path
from decimal import Decimal as N
from copy import deepcopy
from datetime import datetime, timezone
import argparse
import hashlib
import json

ROOT = Path('/home/archon/apps/moloch/shared/live-production-v1')
BASELINE = ROOT.parent / 'incidents/r71-recurring-20261021T194753Z/report.json'
OUT = Path('/tmp/moloch-recurring-cohort-20261021/r71-turnover.public.json')


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
            lots.append(dict(quantity=fill['quantity'], price=fill['price']))
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
    proof = dict(observedUtc=datetime.now(timezone.utc).isoformat(), baselineIncident=baseline.parent.name,
        baselineReportSha256=hashlib.sha256(before_raw).hexdigest(),
        originalFillCount=count, currentFillCount=len(after['fills']), originalFillsUnchanged=True,
        remainingFifoExactlyMatches=True, allNewFillsMatchOriginalOfficialReceipts=True,
        reportSha256=hashlib.sha256(raw).hexdigest(), reportAt=post['at'], orders=orders,
        financial={k: post[k] for k in ['cash', 'ownedCost', 'realized', 'newTradingRealized', 'inheritedRealizedGross']},
        incrementalRealizedNet=total('realizedNet'), incrementalTurnover=total('notional'),
        incrementalShares=total('shares'), incrementalDistinctFilledOrders=len(orders),
        officialReceiptFileHashes=receipt_hashes, utcDay='2026-10-21', dayFillReceipts=len(today),
        dayDistinctFilledOrders=len({f['actualOrderId'] for f in today}),
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
