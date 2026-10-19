import os,json,hashlib,importlib.util
from pathlib import Path
from collections import Counter,defaultdict
from datetime import datetime,timezone
from moloch.learning_actions.breadth_source import Decoder,frames
root=Path(os.environ['LOCALAPPDATA'])/'Moloch/learning-v2';source=root/'data/raw-breadth-authorized-002'
manifest=json.loads((source/'source-manifest.json').read_text());archive_sha=hashlib.sha256((source/'original-slices.jsonl.gz').read_bytes()).hexdigest();manifest_sha=hashlib.sha256((source/'source-manifest.json').read_bytes()).hexdigest()
pinned=root/'handoffs/population-014-to-015-v2/checked-source';original=pinned/'moloch/learning_actions/breadth_source.py';pins=json.loads((pinned/'source-snapshot.json').read_text());assert hashlib.sha256(original.read_bytes()).hexdigest()==pins['files']['moloch/learning_actions/breadth_source.py']
spec=importlib.util.spec_from_file_location('old_breadth_decoder',original);oldmod=importlib.util.module_from_spec(spec);spec.loader.exec_module(oldmod)
tournament='bda92870-621e-47b0-bc3c-3602c5c26f55';old=oldmod.Decoder(tournament);new=Decoder(tournament)
counts=Counter();times=defaultdict(list);differences=[];digests={k:hashlib.sha256()for k in('old','new')}
for event,digest,at,role in frames(source,archive_sha,manifest_sha):
 before,bp=old.decode(event,digest,at,role);after,ap=new.decode(event,digest,at,role)
 assert bp==ap,'PUBLIC_PRINTS_CHANGED'
 counts[role+'_frames']+=1;counts['prints']+=len(ap)
 for label,rows in(('old',before),('new',after)):
  for row in rows:
   counts[label+'_'+role+'_'+row['kind']]+=1
   digests[label].update((json.dumps(row,sort_keys=True,separators=(',',':'))+'\n').encode())
   if row['kind']=='book':times[(label,role)].append(at)
 if before!=after:
  counts['frames_changed']+=1
  if len(differences)<8:differences.append({'atMs':at,'role':role,'event':event['event'],
   'oldRows':len(before),'newRows':len(after),'oldBookRows':sum(r['kind']=='book'for r in before),
   'newBookRows':sum(r['kind']=='book'for r in after)})
 assert all(row['available_at_ms']==at and row['received_at_ms']==at for row in after)
coverage={}
for (label,role),values in times.items():
 coverage[label+'_'+role]={'firstAtMs':values[0],'lastAtMs':values[-1],
 'maximumBetweenObservedBookFramesGapMs':max((b-a for a,b in zip(values,values[1:])),default=0)}
result={'classification':'streaming_existing_inspected_fit_selection_decoder_comparison_not_new_history_or_roi',
'observedUtc':datetime.now(timezone.utc).isoformat(),'originalArchiveSha256':archive_sha,'sourceManifestSha256':manifest_sha,
'oldSourceCommit':pins['commit'],'oldDecoderSha256':hashlib.sha256(original.read_bytes()).hexdigest(),
'newDecoderSha256':hashlib.sha256(Path('moloch/learning_actions/breadth_source.py').read_bytes()).hexdigest(),
'counts':dict(counts),'oldDecoderCounters':dict(old.counts),'newDecoderCounters':dict(new.counts),
'rowStreamSha256':{k:v.hexdigest()for k,v in digests.items()},'aggregateBookCoverage':coverage,
'firstDifferenceExamples':differences,'originalTradeRowsIdentical':True,'originalReceiptClocksUnchanged':True,
'newAccountRequests':0,'independentTestUsed':False,'active015OrPending016Changed':False,'productionChanged':False,
'fullCompetitionHistoryReady':False,'independentProfitableRoiEstablished':False}
Path('research/tartarus-learning-v2/breadth-decoder-real-source-audit-001.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps({'counts':result['counts'],'oldCounters':result['oldDecoderCounters'],'newCounters':result['newDecoderCounters'],'coverage':coverage}))
