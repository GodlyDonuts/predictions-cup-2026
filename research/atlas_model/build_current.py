"""Build a reproducible CURRENT-INPUT RESEARCH fixture from retained public studies.
No URLs are fetched; original primary publication/contract semantics not invented.
"""
import datetime,hashlib,json,sys
from pathlib import Path
DAY=86400000
def ms(x):return int(datetime.datetime.fromisoformat(x.replace('Z','+00:00')).timestamp()*1000)
def source(p):
 b=p.read_bytes();d=json.loads(b);return d,{'file':'research/'+str(p).replace('\\','/').split('/research/',1)[1],'sha256':hashlib.sha256(b).hexdigest(),'observedAt':d['at'],'originalBytesVerified':False,'role':'retained poll transcription/source manifest, not reloaded primary publisher bytes'}
def build(root):
 study,a=source(root/'research/current-polls-v2/assessments-20261005-v2.json');texas,b=source(root/'research/current-polls-v3/texas-governor-assessment-20261005-v3.json')
 races=[(r['state'],'senate',r['polls'],r['at'],r['sources']) for r in study['assessments']]
 races.append(('TX','governor',texas['polls'],texas['at'],[]))
 rows=[];election=ms('2026-11-03T12:00:00Z')
 for state,office,polls,at,sources in races:
  by_file={s['file']:s for s in sources}
  for i,p in enumerate(polls):
   src=by_file.get(p['file']);available=ms(src['observedAt'] if src else at);field=ms(p['fieldEndAt'])
   rows.append({'id':f"2026:{office}:{state}:{p['file']}",'raceId':f'2026:{office}:{state}','year':2026,'state':state,'office':office,'pollsterId':'current:'+p['pollster'].lower().replace(' ',''),'pollster':p['pollster'],'fieldAtMs':field,'electionAtMs':election,'availableAtMs':available,'publicationClockKnown':False,'population':p['population'],'sampleSize':None,'democratic':p['democratic'],'republican':p['republican'],'pollMargin':p['democratic']-p['republican'],'horizonDays':(election-field)/DAY,'source':src or {'file':p['file'],'observedAt':at,'role':'retained transcription clock only'}})
 return {'schemaVersion':1,'rows':rows,'sources':[a,b],'observedAtMs':max(ms(at) for _,_,_,at,_ in races),'expiresAtMs':max(ms(at) for _,_,_,at,_ in races)+21600000,'qualification':'research_only_retained_transcriptions_missing_original_primary_bytes','notes':['electionAtMs is configured ordinary general-election timing, never settlement timing.','FieldEnd is a retained date transcription; original publication clock is unavailable.','No current sample size is imputed; model empirical fallback measurement variance is explicitly used.','No historical pollster identity is guessed from a current short name; unseen houses retain pooled variance.']}
if __name__=='__main__':
 root=Path(__file__).resolve().parents[2];out=Path(sys.argv[1]);payload=build(root)
 with out.open('x',encoding='utf-8') as f:json.dump(payload,f,sort_keys=True,indent=2);f.write('\n')
 print(json.dumps({'races':len({p['raceId'] for p in payload['rows']}),'polls':len(payload['rows']),'output':str(out)}))
