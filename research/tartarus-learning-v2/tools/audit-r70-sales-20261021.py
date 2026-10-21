"""File-only R70 fill/FIFO audit; uses original SIG receipts, no account client."""
from pathlib import Path
from decimal import Decimal as N
from copy import deepcopy
from datetime import datetime, timezone
import hashlib
import json

ROOT = Path('/home/archon/apps/moloch/shared/live-production-v1')
BASELINE = ROOT.parent / 'incidents/r70-history-bound-20261021T185643Z/report.json'
OUT = Path('/tmp/moloch-recurring-cohort-20261021/r70-sales.public.json')
before = json.loads(BASELINE.read_bytes())
raw = (ROOT / 'bank-all-markets-v1/report.json').read_bytes()
after = json.loads(raw)
count = len(before['fills'])
assert count == 433 and after['fills'][:count] == before['fills']
positions = deepcopy(before['report']['positions'])
orders, receipt_hashes = {}, {}
for fill in after['fills'][count:]:
    assert fill['counterfactual'] is False and fill['action'] == 'sell'
    receipt_raw = (ROOT / 'receipts' / (fill['receiptHash'] + '.json')).read_bytes()
    receipt = json.loads(receipt_raw)
    assert receipt['status'] == 200
    assert receipt['endpoint'] == f"/orders/{fill['actualOrderId']}/fills?limit=200"
    rows = {str(row['id']): row for row in receipt['data']['data']}
    row = rows[fill['sourceId']]
    assert row['filledAt'] == fill['executedAt'] and abs(row['quantity']) == fill['quantity']
    assert row['side'].upper() == fill['side'] and N(str(row['price'])) == N(fill['price'])
    receipt_hashes[fill['receiptHash']] = hashlib.sha256(receipt_raw).hexdigest()
    group = orders.setdefault(str(fill['orderId']), dict(
        actualOrderId=fill['actualOrderId'], exchangeId=fill['exchangeId'], side=fill['side'],
        shares=0, fillReceipts=0, proceeds=N(0), releasedFifoCost=N(0), fees=N(0),
        firstExecutedAt=fill['executedAt'], lastExecutedAt=fill['executedAt']))
    group['shares'] += fill['quantity']
    group['fillReceipts'] += 1
    group['lastExecutedAt'] = max(group['lastExecutedAt'], fill['executedAt'])
    group['fees'] += N(fill['fee'])
    group['proceeds'] += fill['quantity'] * N(fill['price'])
    lots = positions[fill['exchangeId']][fill['side']]
    left = fill['quantity']
    while left:
        lot = lots[0]
        take = min(left, lot['quantity'])
        group['releasedFifoCost'] += take * N(lot['price'])
        left -= take
        lot['quantity'] -= take
        if not lot['quantity']:
            lots.pop(0)
for group in orders.values():
    group['realizedGross'] = group['proceeds'] - group['releasedFifoCost']
    group['realizedNet'] = group['realizedGross'] - group['fees']
total = lambda key: sum((group[key] for group in orders.values()), N(0))
pre, post = before['report'], after['report']
assert positions == post['positions']
assert N(pre['ownedCost']) - N(post['ownedCost']) == total('releasedFifoCost')
assert N(post['cash']) - N(pre['cash']) == total('proceeds') - total('fees')
assert N(post['realized']) - N(pre['realized']) == total('realizedGross')
assert pre['inheritedRealizedGross'] == post['inheritedRealizedGross']
today = [f for f in after['fills'] if f.get('executedAt', '').startswith('2026-10-21')]
proof = dict(observedUtc=datetime.now(timezone.utc).isoformat(), workerPid=1022216, release='R70',
    originalFillCount=count, currentFillCount=len(after['fills']), originalFillsUnchanged=True,
    remainingFifoExactlyMatches=True, allNewFillsMatchOriginalOfficialReceipts=True,
    reportSha256=hashlib.sha256(raw).hexdigest(), reportAt=post['at'], orders=orders,
    financial={k: post[k] for k in ['cash', 'ownedCost', 'realized', 'newTradingRealized', 'inheritedRealizedGross']},
    incrementalRealized=total('realizedGross'), officialReceiptFileHashes=receipt_hashes,
    utcDay='2026-10-21', dayFillReceipts=len(today),
    dayDistinctFilledOrders=len({f['actualOrderId'] for f in today}),
    noAccountRequestsByAudit=True, wholeAccountProfitEstablished=False)
OUT.write_text(json.dumps(proof, default=str, indent=2) + '\n')
print(json.dumps({k: proof[k] for k in ['observedUtc', 'currentFillCount', 'incrementalRealized', 'financial', 'dayDistinctFilledOrders']}, default=str))
