from pathlib import Path
from decimal import Decimal as N
from copy import deepcopy
from datetime import datetime,timezone
import json,hashlib
D=Path('/tmp/moloch-stream-throughput-20261021');R=Path('/home/archon/apps/moloch/shared/live-production-v1')
deployment=json.loads((D/'deployment.public.json').read_text())
b=json.loads((Path(deployment['incident'])/'report.original.json').read_bytes())
raw=(D/'r68-reconciled-report.original.json').read_bytes();a=json.loads(raw)
assert len(b['fills'])==425 and a['fills'][:425]==b['fills']
fills=a['fills'][425:];assert len(fills)==5 and all(x['orderId']==20215 and x['actualOrderId']=='54136675' and x['action']=='sell' and x['exchangeId']=='872' and x['side']=='YES' for x in fills)
receipts=[]
for rh in sorted(set(x['receiptHash']for x in fills)):
 fr=(R/'receipts'/f'{rh}.json').read_bytes();f=json.loads(fr);assert f['status']==200 and f['endpoint']=='/orders/54136675/fills?limit=200'
 rows={str(x['id']):x for x in f['data']['data']}
 for x in [x for x in fills if x['receiptHash']==rh]:
  y=rows[x['sourceId']];assert y['quantity']==x['quantity'] and N(str(y['price']))==N(x['price']) and y['side'].upper()==x['side'] and x['counterfactual']is False and y['filledAt']==x['executedAt']
 receipts.append(dict(nativeReceiptHash=rh,fileSha256=hashlib.sha256(fr).hexdigest(),endpoint=f['endpoint'],receivedAt=f['receivedAt'],fills=f['data']['data'],positiveOfficialYesQuantitiesVerified=True))
events={}
for line in (R/'dispatch.ndjson').open():
 row=json.loads(line)
 if row['event'].get('qid')==20215:events[row['event']['kind']]=row
assert {'intent','attempt','ack'}<=set(events)
ack=events['ack']['event']['receipt'];assert ack['status']==200 and str(ack['data']['orderId'])=='54136675'
quantity=sum(x['quantity']for x in fills);proceeds=sum(N(x['price'])*x['quantity']for x in fills)
pre=b['report'];post=a['report'];expected_positions=deepcopy(pre['positions']);lots=expected_positions['872']['YES'];left=quantity;released=N(0)
while left:
 lot=lots[0];used=min(left,lot['quantity']);released+=N(lot['price'])*used;left-=used;lot['quantity']-=used
 if not lot['quantity']:lots.pop(0)
assert expected_positions==post['positions']
assert N(pre['ownedCost'])-N(post['ownedCost'])==released
delta=proceeds-released;assert N(post['realized'])-N(pre['realized'])==delta
assert N(post['cash'])-N(pre['cash'])==proceeds
assert pre['inheritedRealizedGross']==post['inheritedRealizedGross'] and pre['fees']==post['fees']
assert N(post['newTradingRealized'])-N(pre['newTradingRealized'])==delta
proof=dict(observedUtc=datetime.now(timezone.utc).isoformat(),executingRelease='R67',workerPid=1014772,
 original425FillPrefixUnchanged=True,reconcilingRelease='R68',reconcilingWorkerPid=1018590,executedBeforeR67Shutdown=True,actualFills=len(a['fills']),nativeFills=fills,officialFillReceipts=receipts,
 officialOrderReceipt=ack,dispatchEvidence={k:dict(sequence=v['sequence'],hash=v['hash'])for k,v in events.items()},
 originalReportHash=b['hash'],endingReportHash=a['hash'],sharesSold=quantity,admittedQuantity=ack['data']['quantity'],
 terminalConfirmed='closed'in events,cashBefore=pre['cash'],cashAfter=post['cash'],proceeds=str(proceeds),
 fifoCostReleased=str(released),realizedGrossDelta=str(delta),remainingFifoCost=post['ownedCost'],
 totalAttributedRealizedGross=post['realized'],newTradingRealized=post['newTradingRealized'],
 inheritedRealizedUnchanged=True,exactRemainingFifoLotsVerified=True,wholeAccountProfitabilityEstablished=False,additionalAccountRequests=0)
for name,content in [('r67-second-sale-reconciled.public.json',json.dumps(proof,indent=2).encode()),('r67-second-sale.report.audited.json',raw)]:
 with (D/name).open('xb')as f:f.write(content)
print(json.dumps({k:proof[k]for k in ['sharesSold','proceeds','fifoCostReleased','realizedGrossDelta','cashAfter','admittedQuantity','terminalConfirmed']}))
