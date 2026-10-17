"""Finite native-reservation census. No account access or execution authority."""
from pathlib import Path
from decimal import Decimal
from datetime import datetime,timezone
import argparse,hashlib,json,zipfile,collections
EXPECTED='ddf08bca7c2f15d20875983ae0ffb414cddc06de38a04d94d97b050555bc48ee'
def census(path):
 sha=lambda b:hashlib.sha256(b).hexdigest()
 assert sha(path.read_bytes())==EXPECTED
 with zipfile.ZipFile(path)as z:
  manifest=json.loads(z.read('packet-manifest.json'))
  assert len(z.namelist())==len(set(z.namelist()))==len(manifest)+1
  for n,pin in manifest.items():assert len(z.read(n))==pin['bytes'] and sha(z.read(n))==pin['sha256']
  raw=z.read('report.original.json');report=json.loads(raw,parse_float=Decimal)
  assert report['sequence']==463299 and report['report']['fills']==380
  cash=Decimal(report['report']['cash']);reserved=Decimal(report['report']['reservedCash'])
  quotes=[q for q in report['orders'] if q['retired'] is False and q['action']=='buy']
  rows=[];pairs=collections.defaultdict(list)
  for q in quotes:
   assert q['filled']==0 and q.get('reviewedFundamentalEntry') is None and q.get('reviewedInventoryExit') is None
   cost=Decimal(q['price'])*(q['quantity']-q['filled'])
   row={'nativeQid':q['id'],'exchangeId':q['exchangeId'],'marketId':q['marketId'],'side':q['side'],'quantity':q['quantity'],'filled':q['filled'],'limit':q['price'],'limitPrincipal':format(cost,'.6f'),'createdAt':datetime.fromtimestamp(q['createdAt']/1000,timezone.utc).isoformat(),'expiresAt':datetime.fromtimestamp(q['expiresAt']/1000,timezone.utc).isoformat(),'nativeReviewedEntryMarker':False,'nativeReviewedExitMarker':False}
   rows.append(row);pairs[q['exchangeId']].append(q)
  assert len(quotes)==4 and len(pairs)==2
  for rows_for_market in pairs.values():
   assert len(rows_for_market)==2 and {q['side']for q in rows_for_market}=={'YES','NO'} and len({q['quantity']for q in rows_for_market})==1 and len({q['barrier']for q in rows_for_market})==1
  principal=sum(Decimal(x['limitPrincipal'])for x in rows)
  assert principal==reserved==Decimal('3593.940000') and cash==Decimal('3597.585000')
  return {'sourcePrivateArchiveSha256':EXPECTED,'originalPacketFilesChecked':len(manifest),'rawReportSha256':sha(raw),'financialSequence':report['sequence'],'engineAt':report['report']['at'],'officialFillRows':380,'cash':format(cash,'.6f'),'ownedCost':report['report']['ownedCost'],'nativeReservedCash':format(reserved,'.6f'),'cashLessNativeReserved':format(cash-reserved,'.6f'),'unreviewedNativeBuyLimitPrincipal':format(principal,'.6f'),'nativeBuyDrafts':rows,'pairedExchangeCount':2,'comparisonSource':'moloch/live-runtime/src/alpha_proposal.rs:77-85','sourceBehavior':'The existing comparison deducts native reservedCash before allocating reviewed candidates. The four paired ordinary drafts exactly account for that limit principal at this frozen boundary.','positiveNeverAttemptedProofEstablished':False,'physicalReclaimableCashEstablished':False,'newTradeOpportunityOrProfitEstablished':False,'quantityCauseOfEarlierOneShareTradeEstablished':False,'limitations':['Native marker absence does not establish no physical intent, attempt, ACK, native binding or package liability. The current authenticated DispatchLog and actual worker checks must positively prove each lifecycle before retirement.','This is a frozen planning-accounting observation, not a current account cash receipt, balance mutation or spending permission.','The analysis does not change existing reservations or show that reviewed candidates have positive calibrated EV. It does not prove why the earlier one-share order was sized that way.','Reported native reservation equals summed unfilled limit principal under the existing deployed zero-fee policy; actual physical liabilities must remain independently covered.']}
if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('archive',type=Path);p.add_argument('--output',type=Path);a=p.parse_args()
 result=census(a.archive);encoded=json.dumps(result,indent=2)+'\n'
 if a.output:
  with a.output.open('x',encoding='utf-8')as f:f.write(encoded)
 print(encoded)
