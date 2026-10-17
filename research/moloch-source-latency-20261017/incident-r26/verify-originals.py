from pathlib import Path
import os,json,zipfile,hashlib,importlib.util
repo=Path.cwd();t=Path(os.environ['TEMP']);p=t/'moloch-source-cow-incident-r26-20261017.zip';H=lambda b:hashlib.sha256(b).hexdigest()
assert p.stat().st_size==7606062 and H(p.read_bytes())=='8c09fd1aaa589dd0e4e55da09188b5b5aa7f19c9ea3378c779155da534dc02d3'
spec=importlib.util.spec_from_file_location('v',repo/'research/moloch-source-latency-20261017/compare_book_clocks.py');v=importlib.util.module_from_spec(spec);spec.loader.exec_module(v)
with zipfile.ZipFile(p)as z:
 files=json.loads(z.read('packet-manifest.json'))['files'];assert len(z.namelist())==len(set(z.namelist()))==len(files)+1
 for n,pin in files.items():raw=z.read(n);assert len(raw)==pin['bytes'] and H(raw)==pin['sha256']
 report,rt=v.V.sealed(z.read('report.original.json'));anchor,_=v.V.sealed(z.read('anchor-head.original.json'));head,_=v.V.sealed(z.read('head.original.json'))
 native=[];sequence=anchor['sequence'];previous=anchor['previous']
 for raw in z.read('native.suffix.jsonl').splitlines(keepends=True):
  row,_=v.V.sealed(raw);assert row['sequence']==sequence+1 and row['previous']==previous;sequence=row['sequence'];previous=row['hash'];native.append(row)
 assert sequence==head['sequence']==report['sequence']==489338 and previous==head['previous']==report['previous']
 dispatch=[];sequence=0;previous='0'*64
 for raw in z.read('dispatch.original.ndjson').splitlines(keepends=True):
  row,_=v.V.sealed(raw);assert row['sequence']==sequence+1 and row['previous']==previous;sequence=row['sequence'];previous=row['hash'];dispatch.append(row)
 with zipfile.ZipFile(t/'moloch-oregon-node-refresh-live-proof-20261017.zip')as old:
  assert z.read('anchor-head.original.json')==old.read('head.original.json')
  before,bt=v.V.sealed(old.read('report.original.json'));assert v.V.array_tokens(rt['fills'])==v.V.array_tokens(bt['fills']) and len(report['fills'])==382
  assert z.read('fundamental.original.jsonl').startswith(old.read('fundamental.original.jsonl'))
  assert max(q['id']for q in before['orders'])<15469
 q=next(q for q in report['orders']if q['id']==15469);assert q['exchangeId']=='851'and q['side']=='NO'and q['action']=='buy'and q['quantity']==680 and q['price']=='0.875000'and q['retired']is False and q['filled']==0
 assert not any(r['event'].get('qid')==15469 for r in dispatch)
 assert not any(r['event'].get('orderId')==15469 and r['event']['type'].startswith('actual_')for r in native)
 stop=json.loads(z.read('stop.original.json'));status=json.loads(z.read('status.original.json'));assert stop['reason']==status['reason']=='FUNDAMENTAL_NOT_ELIGIBLE'and status['pid']==651324 and status['mode']=='stopped'
 book=next(r['event']['book']for r in reversed(native)if r['event']['type']=='book'and r['event']['book']['exchangeId']=='851');at=v.N.instant(stop['at']);age={k:format((at-v.N.instant(book[k]))/1000000,'.7f')for k in ['sourceAt','requestStartedAt','receivedAt']};assert all(at-v.N.instant(book[k])>10_000_000_000 for k in ['sourceAt','requestStartedAt','receivedAt'])
 quiet=json.loads(z.read('queue.original.json'));assert quiet['globalUnknownCount']==0 and quiet['state']['active']is None and quiet['state']['manual_review']is False
 result={'result':'PASS_ORIGINAL_R26_PREINTENT_BOOK_EXPIRY_INCIDENT','privateArchiveSha256':H(p.read_bytes()),'privateArchiveBytes':p.stat().st_size,'originalFilesVerified':len(files),'nativeSuffixFirstSequence':anchor['sequence']+1,'nativeSuffixLastSequence':report['sequence'],'nativeSuffixRows':len(native),'dispatchRows':len(dispatch),'prior382RawFillTokensUnchanged':True,'fiveGenerationOriginalPrefixPreserved':True,'stopSha256':H(z.read('stop.original.json')),'stoppedAt':stop['at'],'reason':stop['reason'],'qid':15469,'noOriginalDispatchOrActualNativeOperationForQid':True,'queueUnknownCount':0,'queueActive':None,'queueManualReview':False,'finalOriginalBook':book,'bookAgeAtStopMs':age,'wholeNativePrefix':json.loads(z.read('capture.json'))['wholeNativePrefix'],'financialReplayPerformed':False,'newAccountRequests':0,'profitabilityEstablished':False}
 out=t/'moloch-source-cow-incident-root-verification-20261017.json'
 with out.open('xb')as f:f.write((json.dumps(result,indent=2)+'\n').encode())
 print(json.dumps({k:value for k,value in result.items()if k!='finalOriginalBook'},indent=2))
