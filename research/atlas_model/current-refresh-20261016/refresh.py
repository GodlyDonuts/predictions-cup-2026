"""Two genuinely different primary YouGov polls; no old input clock renewal.
Original PDFs/receipts stay private. No fetching, account I/O or activation.
"""
import os
os.environ.setdefault('OPENBLAS_NUM_THREADS','1')
import argparse,datetime,hashlib,json,re,sys
from pathlib import Path
from pypdf import PdfReader
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import model
TTL=21600000
SPECS=[
 ('KS','yougov-kansas-october.pdf','2f9f21d7b589b6dbeefe80a7370a7aecb7b0fb4085a704d2dc37b99c04cf7062','3a5ef2a2703518a5ff66637f25f1a01444af62a82eb1dd8cfe39a322a96a9495','https://d3nkl3psvxxpe9.cloudfront.net/documents/ttw_ks_20260921_lv.pdf','2026-09-24','2026-10-04',2255,48,45,'Adam Hamilton','Roger Marshall'),
 ('IA','yougov-iowa-oct14.pdf','86968315dabd82f898119fa650f277257dce2e37915cdea0772a3d4c4d4090d9','ab11b8f4e098f6eb8b24d6e4467b14303cf3cf6817dad441f8e2d9bc5864008d','https://d3nkl3psvxxpe9.cloudfront.net/documents/ttw_ia_20261006_lv.pdf','2026-10-06','2026-10-12',2245,47,46,'Josh Turek','Ashley Hinson')]
def ms(x):
 d=datetime.datetime.fromisoformat(x.replace('Z','+00:00'))
 if d.tzinfo is None:raise ValueError('unknown timezone')
 return int(d.timestamp()*1000)
def sha(b):return hashlib.sha256(b).hexdigest()
def read(p):
 if not p.is_absolute() or p.is_symlink() or not p.is_file() or p.stat().st_size>8*1024*1024:raise ValueError('unsafe original')
 return p.read_bytes()
def build(base):
 rows=[];sources=[]
 for state,name,expected,receipt_sha,url,start,end,n,d,r,dem,rep in SPECS:
  raw=read(base/name);rb=read(base/(name+'.receipt.json'));receipt=json.loads(rb)
  if sha(raw)!=expected or sha(rb)!=receipt_sha or receipt['sha256']!=expected or receipt['status']!=200 or receipt['url']!=url:raise ValueError('original source or receipt changed; retimestamp forbidden')
  text=' '.join(PdfReader(base/name).pages[0].extract_text().split());page=' '.join(PdfReader(base/name).pages[1].extract_text().split()).replace('T urek','Turek')
  if 'Likely Voters' not in text or f'{n:,}' not in text or dem not in page or rep not in page:raise ValueError('source scope/sample/named nominees changed')
  if not re.search(re.escape(dem)+r' \(Democrat\).*?'+str(d)+r'%',page) or not re.search(re.escape(rep)+r' \(Republican\).*?'+str(r)+r'%',page):raise ValueError('exact Senate percentages changed')
  received=ms(receipt['receivedAt']);requested=ms(receipt['requestStartedAt']);first=ms(start+'T12:00:00Z');last=ms(end+'T12:00:00Z');field=(first+last)//2;election=ms('2026-11-03T12:00:00Z')
  if not first<=last<=received or requested>received:raise ValueError('source chronology')
  sources.append({'sha256':expected,'receiptSha256':receipt_sha,'url':url,'file':name,'requestStartedAtMs':requested,'receivedAtMs':received,'publishedAtMs':None,'publicationClockKnown':False,'httpLastModified':receipt.get('lastModified'),'originalBytesVerified':True,'sourceKind':'primary_poll_topline','nominalSampleSize':n,'effectiveDesignSampleSize':None,'population':'likely_voters','fieldStartDate':start,'fieldEndDate':end})
  rows.append({'id':f'2026:senate:{state}:{expected}','raceId':f'2026:senate:{state}','year':2026,'state':state,'office':'senate','pollsterId':'current-primary:yougov','pollster':'YouGov','fieldAtMs':field,'fieldStartDate':start,'fieldEndDate':end,'fieldClockPrecision':'date_only_midpoint_UTC_noon_encoding','electionAtMs':election,'horizonDays':(election-field)/86400000,'availableAtMs':received,'publishedAtMs':None,'publicationClockKnown':False,'sampleSize':n,'effectiveDesignSampleSize':None,'democratic':d,'republican':r,'pollMargin':d-r,'sourceSha256':expected,'population':'likely_voters','questionTreatment':'undecided preference allocated; not identical to unallocated Emerson'})
 return {'schemaVersion':1,'rows':rows,'sources':sources,'observedAtMs':max(s['receivedAtMs'] for s in sources),'expiresAtMs':min(s['receivedAtMs']+TTL for s in sources),'researchReceiptTtlMs':TTL,'notes':['Both PDFs are newly observed different poll data, not old primary HTML re-fetched to renew a clock.','No NC/NY old observations included or retimestamped.','HTTP Last-Modified is server metadata, not a verified publication instant.','Question allocates undecided preferences; cross-poll shifts are not pure time trends.','Shared YouGov house uncertainty retained across races; samples/design effects and respondent overlap are not invented.']}
def export(base,fit_path,validation_path,prior_path,out):
 if out.exists() or not out.is_absolute():raise ValueError('new private absolute output directory required')
 current=build(base)
 if ms(datetime.datetime.now(datetime.timezone.utc).isoformat())>=current['expiresAtMs']:raise ValueError('original evidence expired')
 fit=json.loads(read(fit_path));validation=json.loads(read(validation_path));prior=json.loads(read(prior_path))
 if model.sha(fit)!=prior['declaredModel']['fittedParameterSha256'] or fit['races']!=416 or fit['pollsterRaceObservations']!=1291:raise ValueError('checked historical fit changed')
 provenance={'sources':current['sources']+[{'sha256':sha(read(fit_path)),'role':'original checked cached historical fit'},{'sha256':sha(read(validation_path)),'role':'original checked historical validation'},{'sha256':sha(read(prior_path)),'role':'prior research artifact fit identity only; no old observations reused'}]}
 a=model.artifact(fit,current['rows'],provenance,validation,current['observedAtMs'],current['expiresAtMs']);a['qualification']['currentSourceOriginalsVerified']=True;a['qualification']['currentPublicationClocksKnown']=False;a['qualification']['currentDesignEffectsKnown']=False;a['limitations']+=current['notes'];a.pop('modelSha256');a['modelSha256']=model.sha(a)
 races,means,cov=model.posterior(fit,current['rows']);sens=[]
 for i,race in enumerate(races):
  cases={str(shift):float(model.probabilities(means+shift,cov)[i]) for shift in [-5,-3,0,3,5]};wide={str(shift):float(model.probabilities(means+shift,cov*2)[i]) for shift in [-3,0,3]};sens.append({'raceId':race['raceId'],'meanMarginPoints':float(means[i]),'posteriorStdMarginPoints':float(cov[i,i]**.5),'democraticProbabilityBySharedBiasShiftPoints':cases,'doubleCovarianceDemocraticProbabilityBySharedBiasShiftPoints':wide})
 diag={'schemaVersion':1,'observedAtMs':current['observedAtMs'],'expiresAtMs':current['expiresAtMs'],'modelSha256':a['modelSha256'],'ordinaryDemocraticProbabilityPpm':{r['raceId']:sum(s['weightPpm'] for s in a['scenarios'] if s['raceOutcomes'][i]=='DEM') for i,r in enumerate(a['races'])},'sensitivity':sens,'sensitivityIsNotCalibratedConfidenceBound':True,'qualification':a['qualification'],'executionAuthority':False}
 out.mkdir(mode=0o700)
 for name,value in [('current-input.json',current),('current-atlas.json',a),('diagnostics.json',diag)]:
  with (out/name).open('xb') as f:f.write(model.canonical(value)+b'\n')
 return diag
if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('--sources',type=Path,required=True);p.add_argument('--fit',type=Path,required=True);p.add_argument('--validation',type=Path,required=True);p.add_argument('--prior',type=Path,required=True);p.add_argument('--out',type=Path,required=True);a=p.parse_args();print(json.dumps(export(a.sources,a.fit,a.validation,a.prior,a.out),indent=2))
