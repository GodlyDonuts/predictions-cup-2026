"""Read original receipts/FIFO and publish aggregates; no account access."""
from pathlib import Path
from decimal import Decimal as N
from copy import deepcopy
from datetime import datetime, timezone
import hashlib, json

D = Path('/tmp/moloch-native-checked-lookup-20261021')
R = Path('/home/archon/apps/moloch/shared/live-production-v1')
before = json.loads((D/'frozen-original/report.original.json').read_bytes())
raw = (R/'bank-all-markets-v1/report.json').read_bytes()
after = json.loads(raw)
assert len(before['fills']) == 430 and after['fills'][:430] == before['fills']
fills = after['fills'][430:]
assert len(fills) == 3 and all(f['orderId'] == 20263 and f['actualOrderId'] == '54228551' and f['exchangeId'] == '851' and f['side'] == 'NO' and f['action'] == 'sell' and f['counterfactual'] is False for f in fills)
receipt_hash = '4fe5c4b6a826d0241fbddd8fc808ccff58db99e04473487408d069e5f5e8b1ed'
receipt_raw = (R/'receipts'/f'{receipt_hash}.json').read_bytes()
receipt = json.loads(receipt_raw)
assert receipt['status'] == 200 and receipt['endpoint'] == '/orders/54228551/fills?limit=200'
rows = {str(row['id']):row for row in receipt['data']['data']}
for fill in fills:
    row = rows[fill['sourceId']]
    assert fill['receiptHash'] == receipt_hash and row['filledAt'] == fill['executedAt']
    assert abs(row['quantity']) == fill['quantity'] and N(str(row['price'])) == N(fill['price'])
    assert row['side'].upper() == fill['side']
quantity = sum(f['quantity'] for f in fills)
proceeds = sum(N(f['price'])*f['quantity'] for f in fills)
pre, post = before['report'], after['report']
positions = deepcopy(pre['positions']); lots = positions['851']['NO']; left = quantity; basis = N(0)
while left:
    lot = lots[0]; used = min(left, lot['quantity']); basis += N(lot['price'])*used
    left -= used; lot['quantity'] -= used
    if not lot['quantity']: lots.pop(0)
assert positions == post['positions']
assert N(pre['ownedCost'])-N(post['ownedCost']) == basis
assert N(post['cash'])-N(pre['cash']) == proceeds
assert N(post['realized'])-N(pre['realized']) == proceeds-basis
assert pre['inheritedRealizedGross'] == post['inheritedRealizedGross'] and pre['fees'] == post['fees']
preflight_raw = (R/'preflight.json').read_bytes(); observed = json.loads(preflight_raw)
cash = N(str(observed['cashBalance']['data']['balance']))
market_value = N(str(observed['positions']['data']['summary']['totalMarketValue']))
initial = N(str(observed['account']['data']['initialBalance']))
day_fills = [f for f in after['fills'] if f.get('executedAt','').startswith('2026-10-21')]
proof = dict(observedUtc=datetime.now(timezone.utc).isoformat(), executingRelease='R68', workerPid=1018590,
    original430FillPrefixPreserved=True, actualFillCount=433, exactRemainingFifoVerified=True,
    actualOrderId='54228551', nativeOrderId=20263, executedAt=fills[0]['executedAt'],
    sharesSold=quantity, proceeds=str(proceeds), fifoCostReleased=str(basis), realizedGrossDelta=str(proceeds-basis),
    officialFillReceiptSha256=hashlib.sha256(receipt_raw).hexdigest(), nativeReceiptHash=receipt_hash,
    originalReportHash=before['hash'], endingReportHash=after['hash'],
    utcDay='2026-10-21', dayFillCount=len(day_fills), dayDistinctFilledOrders=len(set(f['actualOrderId'] for f in day_fills)),
    officialAccountObservedAt=observed['at'], officialAccountReceiptSha256=hashlib.sha256(preflight_raw).hexdigest(),
    cash=str(cash), officialMarkedPositions=str(market_value), officialMarkedEquity=str(cash+market_value),
    markedChangeFromInitial=str(cash+market_value-initial), markedRoiFromInitialPct=str((cash+market_value-initial)/initial*100),
    valuationBasis='official portfolio marks; not executable whole-position liquidation value',
    openOrdersAtObservation=len(observed['openOrders']['data']['data']), noAccountRequestsByAudit=True,
    wholeAccountProfitabilityEstablished=False)
(D/'r68-sale.public.json').write_text(json.dumps(proof,indent=2)+'\n')
print(json.dumps(proof))
