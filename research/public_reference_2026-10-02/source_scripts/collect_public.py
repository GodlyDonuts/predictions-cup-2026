"""Anonymous bounded public reads only. No credentials or account/order endpoints."""
import urllib.request,urllib.parse,urllib.error,json,time,datetime,pathlib,re,sys
ROOT=pathlib.Path(__file__).parent
BASE='https://external-api.kalshi.com/trade-api/v2/'
LOG=ROOT/'request_log.jsonl'
def now():return datetime.datetime.now(datetime.timezone.utc).isoformat()
def get(path,params,label):
 rows=[json.loads(x) for x in LOG.read_text().splitlines()] if LOG.exists() else []
 n=len(rows)+1
 if n>34:raise RuntimeError('34 API-request cap reserves six documentation requests within 40 total public requests')
 if rows and rows[-1].get('status') in [403,429]:raise RuntimeError('Access/rate block, no retries or route bypass')
 if rows:
  delay=3-(datetime.datetime.now(datetime.timezone.utc)-datetime.datetime.fromisoformat(rows[-1]['receipt_utc'])).total_seconds()
  if delay>0:time.sleep(delay)
 u=BASE+path+('?' + urllib.parse.urlencode(params) if params else '')
 sent=now(); status=None; headers={}; raw=b''
 try:
  req=urllib.request.Request(u,headers={'Accept':'application/json','User-Agent':'PublicElectionReference/1.0'})
  with urllib.request.urlopen(req,timeout=60) as r: status=r.status; headers=dict(r.headers);raw=r.read()
 except urllib.error.HTTPError as e:
  status=e.code;headers=dict(e.headers);raw=e.read()
 except Exception as e:
  receipt=now();rec={'sequence':n,'url':u,'sent_utc':sent,'receipt_utc':receipt,'status':None,'error':str(e)}
  with LOG.open('a') as f:f.write(json.dumps(rec)+'\n')
  raise
 receipt=now();name=f'raw/{n:03d}_{label}.json';(ROOT/name).write_bytes(raw)
 rec={'sequence':n,'url':u,'sent_utc':sent,'receipt_utc':receipt,'status':status,'headers':headers,'file':name}
 with LOG.open('a') as f:f.write(json.dumps(rec)+'\n')
 print(n,status,label,'bytes',len(raw),'receipt',receipt,flush=True)
 if status!=200:raise RuntimeError(f'HTTP {status}; no bypass or automatic retry; Retry-After={headers.get("Retry-After")}')
 return json.loads(raw),rec
if __name__=='__main__':
 mode=sys.argv[1]
 if mode=='initial':
  series=json.loads((ROOT/'eligible_series_catalog.json').read_text())
  # '-26' is a candidate event convention, not a claim of a valid event. Only API-returned events are included.
  candidates=sorted(s['ticker']+'-26' for s in series if not s['ticker'].startswith('KX'))
  (ROOT/'candidate_event_ids.json').write_text(json.dumps({'method':'catalog series ticker + known -26 candidate convention; API validates existence','candidates':candidates},indent=2))
  for i in range(0,len(candidates),50):
   d,r=get('events',{'tickers':','.join(candidates[i:i+50]),'with_nested_markets':'true','limit':200},f'candidate_events_{i//50+1}')
   print('events',len(d.get('events',[])),'cursor',d.get('cursor'),flush=True)
  d,r=get('events',{'series_ticker':'KXHOUSERACE','with_nested_markets':'true','limit':200},'kxhouserace_events_1')
  print('kx events',len(d.get('events',[])),'cursor',d.get('cursor'),'sample',[(e['event_ticker'],e['title']) for e in d.get('events',[])[:5]],flush=True)
