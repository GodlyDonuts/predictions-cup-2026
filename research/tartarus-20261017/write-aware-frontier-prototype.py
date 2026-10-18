"""Dated research prototype: not a worker, model approval, order or live price.

Run from the repository with --input-directory pointing to retained private
study inputs. Requires scipy/numpy. Exact independent audit rejects numerical
incumbents that violate shared depth, FIFO, capital, write or market limits.
The candidate family buys only the selected world's winning sides; this is
neither exhaustive portfolio search nor a calibrated tournament forecast.
"""
from pathlib import Path
from decimal import Decimal
import argparse,sys,json,copy,time,math
sys.path.insert(0,str(Path(__file__).resolve().parents[2]))
from moloch.tartarus.sequence_audit import audit
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--input-directory',type=Path,required=True)
parser.add_argument('--output',type=Path,required=True)
args=parser.parse_args()
D=args.input_directory.resolve(strict=True)
import numpy as np
from scipy.optimize import milp,Bounds,LinearConstraint
from scipy.sparse import coo_matrix

depth=json.loads((D/'dated-sig-depth.private.json').read_text());c=json.loads((D/'full-universe-expanded/census.private.json').read_text());g=json.loads((D/'global-projection-study/model-band-1000.private.json').read_text());fin=json.loads((D/'financial-lots-with-inherited-capacity-v3-574174.private.json').read_text());byex={r['exchangeId']:r for r in c['rows']};u=lambda x:int(Decimal(x)*1000000)
UNIT=1000000
def levels(ex,side,buy):
 b=depth['books'][ex]['book'];src=b['asks' if buy==(side=='YES') else 'bids'];return [(u(x['price']) if side=='YES' else UNIT-u(x['price']),int(x['quantity'])) for x in src]
def chunks(ex,side,buy,quantity=None,skip=0):
 out=[];n=0;left=quantity;budget=10000000000 if buy else None
 for price,available in levels(ex,side,buy):
  used=min(skip,available);skip-=used;available-=used
  if left is not None:available=min(available,left)
  while available:
   take=min(available,10000-n);n+=take;available-=take
   if left is not None:left-=take
   if n==10000:
    if buy:
     q=min(n,budget//price)
     if q:out.append((q,price));budget-=q*price
     if q<n:return out
    else:out.append((n,price))
    n=0
  if left==0:break
 if n:
  q=min(n,budget//price) if buy else n
  if q:out.append((q,price))
 return out
def build(s):
 columns=[];sales={};buys={}
 for ex,sides in fin['positions'].items():
  for side,lots in sides.items():
   quantity=sum(x['quantity'] for x in lots)
   if not quantity:continue
   original=copy.deepcopy(lots);idx=0
   for n,price in chunks(ex,side,False,quantity):
    left=n;basis=0
    for lot in original:
     take=min(left,lot['quantity']);left-=take;lot['quantity']-=take;basis+=take*u(lot['price'])
     if not left:break
    assert not left
    j=len(columns);columns.append(dict(action='SELL',ex=ex,side=side,n=n,price=price,basis=basis,previous=sales.get((ex,side),[None])[-1]));sales.setdefault((ex,side),[]).append(j)
 for ex,r in byex.items():
  if ex not in depth['books']:continue
  side='YES' if s['payouts'][r['contractId']]=='resolved_yes' else 'NO';opp='NO' if side=='YES' else 'YES';opposite=sum(x['quantity'] for x in fin['positions'].get(ex,{}).get(opp,[]));dep=sales.get((ex,opp),[])
  if opposite and (not dep or sum(columns[j]['n'] for j in dep)!=opposite):continue
  for n,price in chunks(ex,side,True,skip=opposite):
   j=len(columns);columns.append(dict(action='BUY',ex=ex,side=side,n=n,price=price,previous=buys.get(ex,[None])[-1],opposite_sale=dep[-1] if opposite else None));buys.setdefault(ex,[]).append(j)
 return columns,sales,buys
def run(s,seconds=5,target_wealth=0):
 cols,sales,buys=build(s);N=len(cols);buy_indices=[j for j,v in enumerate(cols) if v['action']=='BUY'];z={j:N+k for k,j in enumerate(buy_indices)};fresh={ex:N+len(z)+k for k,ex in enumerate(sorted((set(buys)|set(fin['positions']))-set(fin['inheritedExchanges'])))};V=N+len(z)+len(fresh)+1; tcol=V-1
 lo=np.zeros(V);hi=np.ones(V);lo[tcol]=-10000000;hi[tcol]=10000000;integrality=np.ones(V);integrality[tcol]=0
 for j in buy_indices:hi[j]=cols[j]['n']
 rows=[];low=[];high=[]
 def row(values,l=-np.inf,h=np.inf):rows.append(values);low.append(l);high.append(h)
 cash={};owned={};market={};writes={};objective=np.zeros(V);base=u(fin['cash'])
 for ex,sides in fin['positions'].items():
  for side,lots in sides.items():
   if s['payouts'][byex[ex]['contractId']]==('resolved_yes' if side=='YES' else 'resolved_no'):base+=sum(x['quantity'] for x in lots)*UNIT
 for j,v in enumerate(cols):
  ex=v['ex'];p=v['price'];n=v['n'];prev=v['previous'];market.setdefault(ex,{})
  if v['action']=='SELL':
   cash[j]=-p*n/UNIT;owned[j]=-v['basis']/UNIT;market[ex][j]=-v['basis']/UNIT;writes[j]=1
   payout=UNIT if s['payouts'][byex[ex]['contractId']]==('resolved_yes' if v['side']=='YES' else 'resolved_no') else 0;objective[j]=-n*(p-payout)/UNIT
   if prev is not None:row({j:1,prev:-1},h=0)
  else:
   cash[j]=p/UNIT;owned[j]=p/UNIT;market[ex][j]=p/UNIT;writes[z[j]]=1;objective[j]=-(UNIT-p)/UNIT
   row({j:1,z[j]:-n},h=0);row({j:1,z[j]:-1},l=0)
   if prev is not None:row({prev:1,z[j]:-cols[prev]['n']},l=0)
   if v['opposite_sale'] is not None:row({z[j]:1,v['opposite_sale']:-1},h=0)
   if ex in fresh:row({z[j]:1,fresh[ex]:-1},h=0)

 for ex,aidx in fresh.items():
  original_sides=fin['positions'].get(ex,{})
  original_quantity=sum(x['quantity'] for lots0 in original_sides.values() for x in lots0)
  if original_quantity:
   sale_indices=[j for (sx,side),indices in sales.items() if sx==ex for j in indices]
   if sale_indices and sum(cols[j]['n'] for j in sale_indices)==original_quantity:row({aidx:1,sale_indices[-1]:1},l=1)
   else:row({aidx:1},l=1)
 row(cash,h=u(fin['cash'])/UNIT);row(owned,h=(u(fin['maximumCommitted'])-u(fin['ownedCost']))/UNIT);row(writes,h=129);row({j:1 for j in fresh.values()},h=12)
 for ex,values in market.items():
  original=sum(x['quantity']*u(x['price']) for lots in fin['positions'].get(ex,{}).values() for x in lots)
  row(values,h=(10000000000-original)/UNIT)

 target_objective=objective.copy()
 if target_wealth:row({j:float(target_objective[j]) for j in range(N)},h=base/UNIT-target_wealth)
 global_model=json.loads((D/'global-five-model.private.json').read_text())['model']
 mean_coefficients=[]
 for model in global_model['probabilityModels']:
  coeff={tcol:1.0}
  for j,v in enumerate(cols):
   expected=0;cid=byex[v['ex']]['contractId']
   for world,w in zip(global_model['scenarios'],model['weightsPpm']):
    payoff=world['payouts'][cid]
    if payoff=='refund_principal':paid=v['basis']/v['n'] if v['action']=='SELL' else v['price']
    else:paid=UNIT if payoff==('resolved_yes' if v['side']=='YES' else 'resolved_no') else 0
    expected+=w*paid
   expected/=UNIT
   gain=v['n']*(v['price']-expected)/UNIT if v['action']=='SELL' else (expected-v['price'])/UNIT
   coeff[j]=-gain
  row(coeff,h=0);mean_coefficients.append((model['id'],coeff))
 objective=np.zeros(V);objective[tcol]=-1
 rr=[];cc=[];data=[]
 for i,values in enumerate(rows):
  for j,value in values.items():
   if value:rr.append(i);cc.append(j);data.append(value)
 matrix=coo_matrix((data,(rr,cc)),shape=(len(rows),V)).tocsc();t=time.monotonic();r=milp(objective,integrality=integrality,bounds=Bounds(lo,hi),constraints=LinearConstraint(matrix,low,high),options={'time_limit':seconds,'mip_rel_gap':0.0,'presolve':True})
 out={'scenario':s['id'],'variables':V,'status':int(r.status),'message':str(r.message),'elapsed':round(time.monotonic()-t,3),'exactOptimalityClaimed':False}
 if r.x is None:return out
 legs=[]
 for j,v in enumerate(cols):
  q=int(round(r.x[j]))
  if not q:continue
  n=v['n'] if v['action']=='SELL' else q
  legs.append(dict(v,quantity=n))
 # SELL columns already precede BUY columns; every prefix is fully funded.
 cash=u(fin['cash']);owned=u(fin['ownedCost']);lots=copy.deepcopy(fin['positions']);used_fresh=set();used_market={ex:sum(x['quantity']*u(x['price']) for ls in sides.values() for x in ls) for ex,sides in lots.items()}
 for v in legs:
  ex,side,n,p=v['ex'],v['side'],v['quantity'],v['price'];assert 0<n<=10000
  if v['action']=='SELL':
   left=n;released=0
   for lot in lots[ex][side]:
    take=min(left,lot['quantity']);left-=take;lot['quantity']-=take;released+=take*u(lot['price'])
    if not left:break
   assert not left and released==v['basis'];cash+=n*p;owned-=released;used_market[ex]-=released
  else:
   assert not any(x['quantity'] for x in lots.get(ex,{}).get('NO' if side=='YES' else 'YES',[]));cash-=n*p;owned+=n*p;used_market[ex]=used_market.get(ex,0)+n*p;lots.setdefault(ex,{}).setdefault(side,[]).append({'quantity':n,'price':str(Decimal(p)/UNIT)})
   if ex not in fin['positions']:used_fresh.add(ex)
  assert cash>=0 and owned<=u(fin['maximumCommitted']) and used_market[ex]<=10000000000
 assert len(legs)<=129 and len(used_fresh)<=12
 wealth=cash+sum(lot['quantity']*UNIT for ex,sides in lots.items() for side,ls in sides.items() for lot in ls if s['payouts'][byex[ex]['contractId']]==('resolved_yes' if side=='YES' else 'resolved_no'))
 assert not target_wealth or wealth>=int(target_wealth*UNIT)
 model_changes={mid:sum(-co[j]*round(r.x[j]) for j in range(N)) for mid,co in mean_coefficients}
 out.update(targetWealth=target_wealth,modelExpectedWealthChanges=model_changes,minimumModelExpectedChange=min(model_changes.values()),exactWealthMicros=wealth,trades=len(legs),sales=sum(v['action']=='SELL' for v in legs),freshMarkets=len(used_fresh),cashMicros=cash,ownedCostMicros=owned,legs=legs,exactFundingAndFifoChecked=True,executionAuthority=False)
 return out


s=next(s for s in g['model']['scenarios'] if s['id']=='price_lower_common:0')
out=[]
for target in [0,250000,500000,925300,1081242]:
 try:
  v=run(s,12,target)
  if 'legs' in v:
   views=json.loads((D/'global-five-model.private.json').read_text())
   checked=audit(fin,c['rows'],depth['books'],views['model'],views['fields'],v['legs'],assumptions={
    'zeroFees':True,'zeroTerminalDebt':True,
    'conditionalReservationTreatment':'Conditional retirement of proved unadmitted drafts; not performed here',
    'datedBooksAreFutureLiquidityHypotheses':True})
   v['independentExactAudit']=checked
 except (ValueError,AssertionError) as error:
  v={'targetWealth':target,'status':'incumbent_rejected_by_exact_audit',
     'reason':str(error) or 'NUMERICAL_INCUMBENT_FAILED_EXACT_RECHECK','executionAuthority':False}
 out.append(v)
 args.output.write_text(json.dumps(out,indent=2))
 print(json.dumps({k:x for k,x in v.items() if k not in ['legs','independentExactAudit']}),flush=True)
