from pathlib import Path
import json,hashlib,zipfile,datetime,re,decimal
P=Path;D=decimal.Decimal;root=P('/home/archon/apps/moloch/shared/live-production-v1');boundary=json.loads(P('/tmp/moloch-throughput-capital-r19-20261017-boundary.json').read_bytes());s=boundary['snapshot']
z=zipfile.ZipFile('/tmp/moloch-actual-realized-attribution-20261017.zip');names=[n for n in z.namelist() if n.endswith('financial-report-private.json')];assert len(names)==1;raw=z.read(names[0]);assert hashlib.sha256(raw).hexdigest()==s['reportSha256']=='0e3d3f84f2557ee63ab426ec556cb3934edae81a39e543efda7aca996f33d491';v=json.loads(raw)
candidates=[q for q in v['orders'] if q['exchangeId']=='948' and q['side']=='NO' and q['action']=='sell' and q.get('executionStyle')=='inventoryAON' and q.get('inventoryExitReason') is None and q['filled']==0]
assert candidates;ids={q['id'] for q in candidates};events={n:[] for n in ids};h=hashlib.sha256();prev='0'*64;seq=0
with (root/'dispatch.ndjson').open('rb') as f:
 while seq<s['dispatch']['sequence']:
  line=f.readline();assert line.endswith(b'\n');h.update(line);row=json.loads(line);unsigned,n=re.subn(rb'"hash":"'+row['hash'].encode()+rb'",',b'',line.strip(),count=1);assert n==1 and hashlib.sha256(unsigned).hexdigest()==row['hash'] and row['sequence']==seq+1 and row['previous']==prev;seq+=1;prev=row['hash'];e=row['event']
  if e.get('qid') in events:events[e['qid']].append({'kind':e['kind'],'rowSha256':hashlib.sha256(line).hexdigest(),'proof':e.get('proof')})
assert prev==s['dispatch']['hash'] and h.hexdigest()==s['pins'][str(root/'dispatch.ndjson')]
prefix_bytes=s['prefixBytes'];hbank=hashlib.sha256()
with (root/'bank-all-markets-v1/events.jsonl').open('rb') as f:
 left=prefix_bytes
 while left:
  b=f.read(min(left,4*1024*1024));assert b;hbank.update(b);left-=len(b)
 assert hbank.hexdigest()==s['prefixSha256']
 start=max(0,prefix_bytes-24000000);f.seek(start);f.readline();slice_start=f.tell();tail=f.read(prefix_bytes-slice_start)
clock=lambda t:int(datetime.datetime.fromisoformat(t.replace('Z','+00:00')).timestamp()*1000)
observations=[]
for line in tail.splitlines(keepends=True):
 row=json.loads(line);e=row['event']
 if e.get('type')=='book' and e['book']['exchangeId']=='948':
  b=e['book'];observations.append((clock(e.get('processedAt',e['receivedAt'])),row['sequence'],'book',b,hashlib.sha256(line).hexdigest()))
 if e.get('type')=='gap' and e.get('exchangeId')=='948':observations.append((clock(e.get('processedAt',e['receivedAt'])),row['sequence'],'gap',None,hashlib.sha256(line).hexdigest()))
lots=v['report']['positions']['948']['NO'];owned=sum(l['quantity'] for l in lots);worst=max(D(l['price']) for l in lots);assert owned==7603 and worst==D('.895')
rows=[]
for q in candidates:
 kinds=[e['kind'] for e in events[q['id']]]
 assert not any(x in kinds for x in ['attempt','ack','recovered_ack'])
 out={'nativeQid':q['id'],'quantity':q['quantity'],'limit':q['price'],'createdAtMs':q['createdAt'],'originalLifetimeMs':q['expiresAt']-q['createdAt'],'minimumGrossAtLimit':str((D(q['price'])-worst)*q['quantity']),'durableNotSent':any(e['kind']=='not_sent' and e['proof']['networkAttempts']==0 for e in events[q['id']]),'physicalAttempted':False}
 preceding=[x for x in observations if x[0]<q['createdAt']]
 if preceding:
  at,seq,kind,b,rsha=preceding[-1];out['lastStrictlyEarlierNativeObservation']={'sequence':seq,'kind':kind,'rowSha256':rsha}
  if kind=='book':
   fresh=not b.get('invalid') and all(0<=q['createdAt']-clock(b[k])<=10000 for k in ['sourceAt','requestStartedAt','receivedAt']);depth=sum(l['quantity'] for l in b['asks'] if 1-D(l['price'])>=D(q['price']))
   out['lastStrictlyEarlierNativeObservation'].update({'freshAtQuoteCreation':fresh,'bookSourceAt':b['sourceAt'],'bookReceivedAt':b['receivedAt'],'qualifyingNoBidDepthAtFrozenLimit':depth,'fullFrozenQuantityDisplayed':depth>=q['quantity']})
 rows.append(out)
out={'observedAt':datetime.datetime.now(datetime.timezone.utc).isoformat(),'sourceFinancialSequence':v['sequence'],'sourceFinancialReportSha256':hashlib.sha256(raw).hexdigest(),'sourceDispatchPrefixSequence':s['dispatch']['sequence'],'sourceDispatchPrefixSha256':h.hexdigest(),'sourceNativePrefixBytes':prefix_bytes,'sourceNativePrefixSha256':hbank.hexdigest(),'nativeSliceStartByte':slice_start,'nativeSliceBytes':len(tail),'nativeSliceSha256':hashlib.sha256(tail).hexdigest(),'exchangeId':'948','side':'NO','ownedSharesAtFrozenBoundary':owned,'ownedWorstLotPrice':str(worst),'feeAssumptionAtSource':'0.000000','expiredProfitOnlyDrafts':rows,'originalMinimumGain':'0.005000','noActualFillOrProfitClaim':True,'overlappingOpportunitiesMustNotBeAdded':True,'bookSnapshotsDoNotGuaranteeCounterfactualFills':True,'codeBarrier':'maximumAge-only read-hint and refreshed candidate identity excludes profit-only inventoryAON drafts','runtimeChangedByAudit':False}
folder=P('/tmp/moloch-profitable-exit-refresh-original-evidence-20261017');folder.mkdir(exist_ok=False);path=folder/'original-evidence.json';path.write_text(json.dumps(out,indent=2)+'\n');archive=folder.with_suffix('.zip')
with zipfile.ZipFile(archive,'w',zipfile.ZIP_DEFLATED) as z:z.write(path,path.name)
hh=hashlib.sha256(archive.read_bytes()).hexdigest()
print('\nAUDIT',json.dumps({'draftCount':len(rows),'allNeverAttempted':True,'withNotSent':sum(r['durableNotSent'] for r in rows),'freshPriorFullDepth':sum(r.get('lastStrictlyEarlierNativeObservation',{}).get('freshAtQuoteCreation',False) and r.get('lastStrictlyEarlierNativeObservation',{}).get('fullFrozenQuantityDisplayed',False) for r in rows),'latest':rows[-2:]}),flush=True)
for i in range(4):print('H'+str(i)+':'+hh[i*16:(i+1)*16],flush=True)
