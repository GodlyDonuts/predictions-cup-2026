"""Frozen historical census; never an order, authority, or additive profit claim."""
import json, hashlib, datetime, re, collections
from decimal import Decimal as D, ROUND_CEILING
from pathlib import Path
ROOT=Path(__file__).resolve().parent
SRC=ROOT/'actual-exit-audit-v4'
def sha(b):return hashlib.sha256(b).hexdigest()
def pin(p):
 h=hashlib.sha256();n=0
 with p.open('rb') as f:
  for b in iter(lambda:f.read(1024*1024),b''):h.update(b);n+=len(b)
 return {'bytes':n,'sha256':h.hexdigest()}
def rawbody(raw,row):
 needle=b',"hash":"'+row['hash'].encode()+b'"';body=raw.rstrip(b'\n');assert body.count(needle)==1
 body=body.replace(needle,b'',1);assert json.loads(body)=={k:v for k,v in row.items() if k!='hash'};return body
def t(s):
 m=re.fullmatch(r'(\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d)(?:\.(\d+))?(Z|[+-]\d\d:\d\d)',s);assert m
 whole=datetime.datetime.fromisoformat(m[1]+m[3].replace('Z','+00:00'))
 return D(int(whole.timestamp()))+D('0.'+(m[2] or '0'))
def fee(p,n):return ((p*D(1000000)*n*P['feeBps']/10000).to_integral_value(rounding=ROUND_CEILING)+D(P['feePerShare'])*1000000*n)/1000000
P={'feeBps':0,'feePerShare':'0.000000','inventoryMinimumGain':'0.005000','bookAgeMs':10000,'quantity':10000,'inventoryExitDepthBps':5000}
expected=json.loads((SRC/'SHA256SUMS.json').read_bytes())
for name in ['current.original.json','native-through-snapshot.original.jsonl','dispatch-full-original.jsonl']:
 assert pin(SRC/name)==expected[name],name
r=json.loads((SRC/'current.original.json').read_bytes());p=r['report'];now=t(p['at']);assert r['sequence']==452287 and p['fills']==380
assert 'livePackages' not in p
managed=set(p['ordinaryEntryExclusions']['managedExchangeIds'])|set(p['managedInventoryPlanning']['managedExchangeIds']);assert len(managed)==11
assert p['profitableDepthMode']['enabled'] and p['profitableDepthMode']['depthBps']==5000
books={};latest={};gaps={};coverage={};previous=None;lastseq=None;firstseq=None;native_rows=0
with (SRC/'native-through-snapshot.original.jsonl').open('rb') as f:
 for raw in f:
  row=json.loads(raw);assert sha(rawbody(raw,row))==row['hash']
  if previous is not None:assert row['previous']==previous and row['sequence']==lastseq+1
  previous=row['hash'];lastseq=row['sequence'];firstseq=firstseq or lastseq;native_rows+=1;e=row['event'];assert t(e['processedAt'])<=now
  if e['type']=='gap':
   for ex in e.get('exchangeIds',[e.get('exchangeId')]):books.pop(ex,None);gaps[ex]=lastseq
  elif e['type']=='book':
   b=e['book'];ex=b['exchangeId'];latest[ex]={'book':b,'rowSha256':row['hash'],'rowSequence':lastseq,'processedAt':e['processedAt'],'sourceRef':e.get('sourceRef')}
   if b.get('invalid'):books.pop(ex,None);gaps[ex]=lastseq
   else:books[ex]=latest[ex]
  elif e['type']=='coverage':coverage[e.get('exchangeId')]=dict(e)
assert lastseq==r['sequence'] and previous==r['previous']
history=collections.defaultdict(set);previous=None;lastseq=0;dispatch_rows=0
with (SRC/'dispatch-full-original.jsonl').open('rb') as f:
 for raw in f:
  row=json.loads(raw);assert sha(rawbody(raw,row))==row['hash'];assert row['sequence']==lastseq+1
  if previous is not None:assert row['previous']==previous
  previous=row['hash'];lastseq=row['sequence'];dispatch_rows+=1;e=row['event'];history[e.get('qid')].add(e['kind'])
source_market={x['exchangeId']:x['marketId'] for x in p['inheritedInventory']['sourcePositions']}
rows=[];excluded=[]
for ex,sides in p['positions'].items():
 for side,lots in sides.items():
  if not lots:continue
  if ex in managed:excluded.append({'exchangeId':ex,'side':side,'reason':'managed'});continue
  if any(l.get('executedAt') is not None for l in lots):excluded.append({'exchangeId':ex,'side':side,'reason':'clocked_or_mixed_lots'});continue
  if ex not in source_market:excluded.append({'exchangeId':ex,'side':side,'reason':'inherited_origin_not_bound'});continue
  n=sum(l['quantity'] for l in lots);worst=max(D(l['price']) for l in lots);market=source_market[ex]
  live=[q for q in r['orders'] if not q['retired'] and q['exchangeId']==ex]
  physical=[q['id'] for q in live if history[q['id']]&{'intent','attempt','ack','cancel_attempt'}]
  out={'exchangeId':ex,'marketId':market,'side':side,'heldQuantity':n,'worstLotPrice':str(worst),'activeNativeQuoteIds':[q['id'] for q in live],'nativeSellReservations':sum(q['quantity']-q['filled'] for q in live if q['action']=='sell' and q['side']==side),'nativeOppositeBuyReservations':sum(q['quantity']-q['filled'] for q in live if q['action']=='buy' and q['side']!=side),'activeIntentOrPhysicalQids':physical,'nonAdditiveReplacementDiagnostic':True}
  b=books.get(ex)
  if b is None:
   out['classification']='invalidated_by_gap_or_invalid_book' if ex in gaps else 'missing_book_in_finite_suffix';rows.append(out);continue
  raw=b['book'];assert raw['marketId']==market and raw['tournamentId']==p['ordinaryEntryExclusions']['bankScope']['tournamentId']
  out.update({'bookRowSha256':b['rowSha256'],'bookRowSequence':b['rowSequence'],'bookOriginalClocks':{k:raw[k] for k in ['sourceAt','requestStartedAt','receivedAt']},'bookTransport':raw.get('transport','REST'),'lastObservedGapSequence':gaps.get(ex),'currentNativeValidAfterLastGap':True,'independentCompleteTapeProof':False})
  clock=raw['sourceAt'] if raw.get('transport')=='official_sig_realtime' else raw['requestStartedAt']
  ages={k:str((now-t(raw[k]))*1000) for k in ['sourceAt','requestStartedAt','receivedAt']};fresh=all(t(raw[k])<=now for k in ages) and (now-t(clock))*1000<=P['bookAgeMs']
  out['originalAgeMs']=ages;out['freshAtFrozenEngineClock']=fresh
  ys=raw['bids'] if side=='YES' else raw['asks'];levels=[(D(x['price']) if side=='YES' else 1-D(x['price']),x['quantity']) for x in ys]
  assert all(0<a<1 and isinstance(n,int) and n>0 for a,n in levels);assert all(levels[i][0]>levels[i+1][0] for i in range(len(levels)-1))
  if not levels:out['classification']='empty_executable_side';rows.append(out);continue
  eligible=[]
  for price,size in levels:
   if price-worst-fee(worst,1)-fee(price,1)<D(P['inventoryMinimumGain']):break
   eligible.append((price,size))
  capacity=sum(k for _,k in eligible);qty=min(n,P['quantity'],capacity*P['inventoryExitDepthBps']//10000)
  out.update({'bestOwnBid':str(levels[0][0]),'bestMarginalNetOverWorstLot':str(levels[0][0]-worst-fee(worst,1)-fee(levels[0][0],1)),'profitableCapturedDepth':capacity,'depthSliceQuantity':qty})
  out['classification']='positive_depth_current_observation' if fresh and qty else 'stale_book' if not fresh else 'minimum_gain_or_depth_failure'
  out['edgeTestPassIgnoringFreshness']=qty>0
  if qty:
   remain=qty;proceeds=D(0);selected=[];limit=None
   for price,size in levels:
    k=min(remain,size)
    if k:selected.append({'quantity':k,'price':str(price)});proceeds+=price*k-fee(price,k);limit=price;remain-=k
    if not remain:break
   assert remain==0
   left=qty;basis=D(0)
   for lot in lots:
    k=min(left,lot['quantity']);basis+=D(lot['price'])*k;left-=k
    if not left:break
   out.update({'selectedLevels':selected,'conservativeLimit':str(limit),'depthProceedsAtObservedLevels':str(proceeds),'principalReleasedFIFO':str(basis),'markedNetAboveFIFO':str(proceeds-basis),'worstLotGainLowerBoundAtLimit':str((limit-worst)*qty-fee(worst,qty)-fee(limit,qty)),'remainingHeldAfterHypotheticalSale':n-qty})
  rows.append(out)
result={'kind':'historical_non_additive_inherited_profit_depth_census','asOfOriginalEngineAt':p['at'],'nativeSequence':r['sequence'],'nativeFillRows':p['fills'],'sourcePins':{name:expected[name] for name in ['current.original.json','native-through-snapshot.original.jsonl','dispatch-full-original.jsonl']},'verifiedNativeSuffix':{'firstSequence':firstseq,'lastSequence':r['sequence'],'rows':native_rows,'tailMatchedReport':True},'verifiedDispatchRows':dispatch_rows,'policyFields':P,'policyProvenance':{'source':'root independently observed pinned actual R22 Control; full normalized policy not in ZIP','controlRawSha256':'81ddfa2d30bc4385e709dbeea3af0250cb8a808df8c60fe89cb13acfc9595b51','reportPolicySha256':p['policyHash'],'fullPolicyHashRecomputed':False},'managedExchangeIds':sorted(managed),'nativePackageSummaryAbsentMeansEmpty':True,'classificationCounts':dict(collections.Counter(x['classification'] for x in rows)),'positiveCandidates':[x for x in rows if x['classification']=='positive_depth_current_observation'],'allInheritedUnmanagedSides':rows,'excluded':excluded,'limitations':['Finite original suffix accepted-native-book reconstruction, with every subsequent gap/invalid book invalidating; no independent raw receipt/tape completeness certification.','All prices/clocks historical asynchronous observations at engine clock, not current execution authority.','One counterfactual per exchange/side, not additive across timestamps or existing native quotes; existing SELL reservations are displayed, not released.','Captured dispatch chain is operationally paired with report but has untimestamped closed rows; cannot establish exact wall-clock closure cutoffs independently.','No simulated fills, guaranteed proceeds/profit, fresh account authority, or permission to retire/replace/submit.']}
(ROOT/'census-result.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps({'counts':result['classificationCounts'],'positive':result['positiveCandidates'],'excluded':len(excluded),'resultSha256':pin(ROOT/'census-result.json')['sha256']},indent=2))
