from pathlib import Path
import json,hashlib,re,decimal,datetime,argparse
parser=argparse.ArgumentParser();parser.add_argument('--input',required=True,type=Path);parser.add_argument('--boundary',required=True,type=Path);parser.add_argument('--output',required=True,type=Path);args=parser.parse_args()
D=decimal.Decimal;decimal.getcontext().prec=40
root=Path('/home/archon/apps/moloch/shared/live-production-v1');p=args.input;raw=p.read_bytes();r=json.loads(raw);b=json.loads(args.boundary.read_bytes())
assert hashlib.sha256(raw).hexdigest()==b['snapshot']['reportSha256'] and r['sequence']==409030
u,n=re.subn(rb'"hash":"'+r['hash'].encode()+rb'",',b'',raw.strip(),count=1);assert n==1 and hashlib.sha256(u).hexdigest()==r['hash'];q=r['report'];costs=[]
for ex,sides in q['positions'].items():
 for side,lots in sides.items():
  assert side in ['YES','NO'] and isinstance(lots,list)
  value=D(0);quantity=0
  for lot in lots:
   assert isinstance(lot['quantity'],int) and lot['quantity']>0
   price=D(lot['price']);assert D(0)<=price<=D(1);quantity+=lot['quantity'];value+=price*lot['quantity']
  if quantity:costs.append((value,ex,side,quantity))
costs.sort(reverse=True);total=sum((row[0] for row in costs),D(0));assert total==D(q['ownedCost']) and total>0
d={'observedAt':datetime.datetime.now(datetime.timezone.utc).isoformat(),'sourceFinancialSequence':r['sequence'],'sourceFinancialReportRawSha256':hashlib.sha256(raw).hexdigest(),'sourceFinancialReportSealedHash':r['hash'],'boundaryRawSha256':hashlib.sha256(args.boundary.read_bytes()).hexdigest(),'holdingCount':len(costs),'ownedCost':str(total),'cash':q['cash'],'topFiveOwnedCost':str(sum((x[0] for x in costs[:5]),D(0))),'topTenOwnedCost':str(sum((x[0] for x in costs[:10]),D(0))),'topFivePercentOfOwnedCost':str(sum((x[0] for x in costs[:5]),D(0))*100/total),'topTenPercentOfOwnedCost':str(sum((x[0] for x in costs[:10]),D(0))*100/total),'ownedCostPercentOfCashPlusCost':str(total*100/(total+D(q['cash']))),'basis':'exact remaining native lot quantity times acquisition price, sums exactly to ownedCost; no supplied CSV used','notLiquidationOrExpectedTerminalValue':True,'doesNotMeasureJointElectionScenarioConcentration':True,'isNotProfit':True,'noAPIOrFinancialMutation':True}
out=args.output;assert not out.exists();out.write_text(json.dumps(d,indent=2)+'\n');print('\nCOST_CONCENTRATION',json.dumps(d))
