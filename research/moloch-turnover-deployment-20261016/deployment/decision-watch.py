import pathlib,time,json,datetime
r=pathlib.Path('/home/archon/apps/moloch/shared/live-production-v1');o=pathlib.Path('/tmp/moloch-turnover-decision-watch-20261016.ndjson');last={};end=time.monotonic()+1800
with o.open('x') as f:
 while time.monotonic()<end:
  for n in ['fundamental-entry.json','fundamental-actions.json','fundamental-cohort.json']:
   p=r/n
   try:
    raw=p.read_bytes()
    if raw==last.get(n):continue
    d=json.loads(raw);last[n]=raw
    if n=='fundamental-actions.json':
     v=d.get('result',{});d={'at':d.get('at'),'exchangeId':d.get('exchangeId'),'comparisonState':v.get('state'),'comparisonReason':v.get('reason'),'candidateModes':[x.get('mode') for x in (v.get('comparison') or {}).get('candidates',[])]}
    f.write(json.dumps({'observedAt':datetime.datetime.now(datetime.timezone.utc).isoformat(),'file':n,'data':d})+'\n');f.flush()
   except (FileNotFoundError,json.JSONDecodeError):pass
  time.sleep(.2)
