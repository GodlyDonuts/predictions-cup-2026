"""Current Atlas inputs from exact original primary-source bytes and receipts.
No forecast probability is converted to a poll. No existing source clock renewed.
"""
import os
os.environ.setdefault('OPENBLAS_NUM_THREADS','1')
import argparse,datetime,hashlib,json,re
from html.parser import HTMLParser
from pathlib import Path
import model
DAY=86400000
TTL=6*3600000  # Explicit research freshness policy, not a publisher expiration.
SPECS=[
 ('emerson-kansas.html','df44ade35d3c2f2dc0df43b2dd9961aa653fb60f6a1f7f5574b261bd10d90259','KS','senate','Emerson College Polling','2026-09-15','2026-09-16','https://emersoncollegepolling.com/kansas-2026/'),
 ('surveyusa-wral-release.html','7dbb0e578e9c1fa78c073dda831ea7a5d626c7fefc47af294e7ffe38a19cf0ca','NC','senate','SurveyUSA','2026-10-07','2026-10-11','https://www.wral.com/news/nccapitol/wral-nc-us-senate-poll-cooper-whatley-tillis-trump-stein-approval-midterm-october-2026/'),
 ('emerson-iowa-october.html','0493d0414b438f125b785c7995dc754e5309bb914d2d29fb9f8bd3a89fe0fc71','IA','senate','Emerson College Polling','2026-10-05','2026-10-06','https://emersoncollegepolling.com/iowa-2026-poll-u-s-senate-race-tightens-sand-maintains-edge-for-governor/'),
 ('emerson-new-york-october.html','f70762ad432f2b28d07a38bec722d47565dfb1e5637995ed2ed0c04f4bd4919a','NY','governor','Emerson College Polling','2026-10-03','2026-10-05','https://emersoncollegepolling.com/new-york-2026-poll-governor-hochul-leads-blakeman-by-12/'),
]
RECEIPT_SHA={
 'emerson-kansas.html':'82b7243a2c4e7e58103514c59b71ed478f110d5aab1ee538f90ff9f5a996a445',
 'surveyusa-wral-release.html':'61878268e0b6bbc3540cc29fbabfe5db42675f0b14f1c3354cf0c68f59831282',
 'emerson-iowa-october.html':'377aaf44d9361c2363ab08c180cdcbb1ca88ef77babe6416b625bb63cdf74be9',
 'emerson-new-york-october.html':'e82a135d07d2dd32412b20873e2f1cc7256c2fc004888b241f44a17a40d8a138',
}
def ms(s):
 d=datetime.datetime.fromisoformat(s.replace('Z','+00:00'))
 if d.tzinfo is None:raise ValueError('unknown source timezone')
 return int(d.timestamp()*1000)
class Text(HTMLParser):
 def __init__(self):super().__init__();self.parts=[];self.skip=0
 def handle_starttag(self,t,a):
  if t in ('script','style'):self.skip+=1
 def handle_endtag(self,t):
  if t in ('script','style'):self.skip=max(0,self.skip-1)
 def handle_data(self,d):
  if not self.skip:self.parts.append(d)
def match(pattern,text):
 m=re.search(pattern,text,re.I)
 if not m:raise ValueError('original primary extraction pattern unavailable')
 return m.groups()
def source(path):
 if path.is_symlink() or not path.is_file() or path.stat().st_size>8*1024*1024:raise ValueError('unsafe primary source')
 return path.read_bytes()
def build(base):
 rows=[];sources=[]
 for name,expected,state,office,pollster,start,end,url in SPECS:
  raw=source(base/name);receipt_bytes=source(base/(name+'.receipt.json'));receipt=json.loads(receipt_bytes)
  if hashlib.sha256(receipt_bytes).hexdigest()!=RECEIPT_SHA[name]:raise ValueError('original receipt changed; clock renewal is forbidden')
  if hashlib.sha256(raw).hexdigest()!=expected or receipt.get('sha256')!=expected or receipt.get('url')!=url or receipt.get('status')!=200:raise ValueError('original source/receipt/hash changed')
  acquired=ms(receipt.get('receivedAt') or receipt.get('observedAt'));requested=ms(receipt['requestStartedAt'])
  if requested>acquired:raise ValueError('source request clock order')
  html=raw.decode('utf-8',errors='strict');t=Text();t.feed(html);text=' '.join(' '.join(t.parts).split())
  dates=set(re.findall(r'"datePublished"\s*:\s*"([^"]+)"',html))
  if len(dates)!=1:raise ValueError('ambiguous actual publication time')
  published=ms(dates.pop())
  if published>acquired:raise ValueError('source not yet published at actual observation')
  if state=='KS':
   d,r=map(int,match(r'finds (\d+)% of Kansas voters support Democrat .*? and (\d+)% support incumbent Republican',text));n=int(match(r'n=(\d+)',text)[0]);match(r'conducted September 15-16, 2026',text)
  elif state=='NC':
   d,r=map(int,match(r'(\d+)% percent said they would likely vote for Cooper, while (\d+)% said they',text));n=int(match(r'of whom (\d+) had either',text)[0]);match(r'conducted from Oct\. 7-11',text)
  elif state=='IA':
   r,d=map(int,match(r'finds (\d+)% of Iowa voters support Republican .*? while (\d+)% support Democrat',text));n=int(match(r'n=(\d+)',text)[0]);match(r'conducted October 5-6, 2026',text)
  else:
   d,r=map(int,match(r'Governor Kathy Hochul with a 12-point lead .*? (\d+)% to (\d+)%',text));n=int(match(r'n=(\d+)',text)[0]);match(r'conducted October 3-5, 2026',text)
  first=ms(start+'T12:00:00Z');last=ms(end+'T12:00:00Z');field=(first+last)//2;election=ms('2026-11-03T12:00:00Z')
  if not field<=published<=acquired or n<=0:raise ValueError('field/publication/availability order')
  record={'file':name,'sha256':expected,'url':url,'bytes':len(raw),'originalBytesVerified':True,'requestStartedAtMs':requested,'receivedAtMs':acquired,'publishedAtMs':published,'receiptSha256':hashlib.sha256(receipt_bytes).hexdigest(),'sourceKind':'primary_poll_release','population':'likely_voters','nominalSampleSize':n}
  if 'connection' in receipt:record['connection']=receipt['connection']
  sources.append(record);rows.append({'id':f'2026:{office}:{state}:{expected}','raceId':f'2026:{office}:{state}','year':2026,'state':state,'office':office,'pollsterId':'current-primary:'+pollster.lower().replace(' ',''),'pollster':pollster,'fieldAtMs':field,'fieldStartDate':start,'fieldEndDate':end,'fieldClockPrecision':'date_only_midpoint_UTC_noon_encoding_not_observed_time','electionAtMs':election,'electionClockPrecision':'configured_date_only_UTC_noon_not_settlement','availableAtMs':acquired,'publishedAtMs':published,'publicationClockKnown':True,'population':'likely_voters','sampleSize':n,'effectiveDesignSampleSize':None,'democratic':d,'republican':r,'pollMargin':d-r,'horizonDays':(election-field)/DAY,'sourceSha256':expected})
 observed=max(s['receivedAtMs'] for s in sources);expires=min(s['receivedAtMs']+TTL for s in sources)
 if expires<=observed:raise ValueError('no shared fresh interval; source reacquisition needed, not timestamp editing')
 return {'schemaVersion':1,'rows':rows,'sources':sources,'observedAtMs':observed,'expiresAtMs':expires,'researchReceiptTtlMs':TTL,'notes':['Original publication timestamps parsed from retained publisher structured metadata; first-known availability is actual original receipt, not publication or field date.','Field ranges are original dates; numeric model clocks are declared midpoint encodings, not fabricated intraday field observations.','Nominal sample sizes are actual LV samples; design effects/effective sample sizes are unknown.','One race per respondent survey retained; same-survey governor outcomes are withheld rather than assuming independent measurement errors.','ECU default withheld: original PDF fieldSep29-Oct2 differs officialreleaseSep30-Oct3; Q5A49.5/42.8 vs forced-choiceQ5B52.5/45.5 are the same810respondents, not independent polls.','Kansas is a real September15-16 poll observed today; its old field window is not refreshed.','CTM/99chambers forecast probabilities are not poll observations and are not converted to fake margins.']}
def write_new(path,value):
 fd=os.open(path,os.O_CREAT|os.O_EXCL|os.O_WRONLY,0o600)
 with os.fdopen(fd,'wb') as f:f.write(model.canonical(value)+b'\n');f.flush();os.fsync(f.fileno())
def export(base,training_path,out):
 if not all(p.is_absolute() for p in [base,training_path,out]) or out.exists():raise ValueError('absolute private sources/training and NEW output directory required')
 current=build(base)
 if ms(datetime.datetime.now(datetime.timezone.utc).isoformat())>=current['expiresAtMs']:raise ValueError('original shared research interval expired; acquire genuinely new source evidence, do not retimestamp')
 training_bytes=source(training_path);training=json.loads(training_bytes);fit=model.fit(training['rows']);validation=model.validate(training['rows']);provenance=dict(training['provenance']);provenance['sources']=provenance['sources']+current['sources']+[{'sha256':hashlib.sha256(training_bytes).hexdigest(),'role':'exact normalized training rows'},{'sha256':hashlib.sha256(Path(model.__file__).read_bytes().replace(b'\r\n',b'\n')).hexdigest(),'role':'Atlas model source normalized Git LF'},{'sha256':hashlib.sha256(Path(__file__).read_bytes().replace(b'\r\n',b'\n')).hexdigest(),'role':'actual current primary extraction source normalized Git LF'}]
 a=model.artifact(fit,current['rows'],provenance,validation,current['observedAtMs'],current['expiresAtMs'])
 # The base sample builder could not verify originals. This different producer
 # DID re-read each exact original and receipt above. No qualification promotion.
 a['qualification']['currentSourceOriginalsVerified']=True;a['qualification']['currentFieldClocksDateOnly']=True;a['qualification']['currentDesignEffectsKnown']=False
 a['limitations']=[s for s in a['limitations'] if not s.startswith('Current retained poll source originals')]+current['notes'];a.pop('modelSha256');a['modelSha256']=model.sha(a)
 out.mkdir(mode=0o700);write_new(out/'current-input.json',current);write_new(out/'current-atlas.json',a)
 probabilities={r['raceId']:sum(s['weightPpm'] for s in a['scenarios'] if s['raceOutcomes'][i]=='DEM') for i,r in enumerate(a['races'])}
 diagnostics={'schemaVersion':1,'observedAtMs':current['observedAtMs'],'expiresAtMs':current['expiresAtMs'],'sourceFilesVerified':len(current['sources']),'races':len(a['races']),'jointScenarios':len(a['scenarios']),'ordinaryDemocraticProbabilityPpm':probabilities,'modelSha256':a['modelSha256'],'qualification':a['qualification'],'excludedEcu':True,'networkRequestsByExporter':0,'executionAuthority':False}
 write_new(out/'current-diagnostics.json',diagnostics);return diagnostics
if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('--sources',type=Path,required=True);p.add_argument('--training',type=Path,required=True);p.add_argument('--out',type=Path,required=True);a=p.parse_args();print(json.dumps(export(a.sources,a.training,a.out),indent=2))
