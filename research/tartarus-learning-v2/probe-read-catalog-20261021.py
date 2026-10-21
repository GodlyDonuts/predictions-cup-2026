"""Recorded source advance, ordered key-index equivalence and bounded CPU timing."""
from pathlib import Path
import argparse,sys,os,json,time,hashlib,statistics,importlib.util,traceback

def main():
 p=argparse.ArgumentParser();p.add_argument('--job',type=Path,required=True);j=p.parse_args().job.resolve();plan=json.loads((j/'PLAN.json').read_bytes());root=Path(plan['sourceRoot']);sys.path.insert(0,str(root));os.chdir(root)
 from moloch.learning_actions.continue_population import verify_source
 from moloch.learning_actions.experiment import Experiment
 from moloch.learning_actions.episode_training import release_worlds
 from moloch.learning_actions.episode_tape import financial_digest,tensor_digest
 from moloch.learning_actions.stream import tensors
 from collections import defaultdict
 sha=lambda p:hashlib.sha256(Path(p).read_bytes()).hexdigest();assert sha(j/'read_catalog.py')==plan['catalogSha256'];assert verify_source(root)['commit']==plan['sourceCommit']
 spec=importlib.util.spec_from_file_location('measured_read_catalog',j/'read_catalog.py');module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module);Catalog=module.ReadCatalog
 owner=None;envs=[]
 try:
  c=json.loads((j/'config.json').read_bytes());assert c['device']=='cpu' and c['cpu_threads']==1
  owner=Experiment(c,j/'experiment');assert owner.dataset_sha==plan['datasetManifestSha256'];role=plan.get('role','train');assert role in ('train','evolution');role_start,end=owner.train_bounds if role=='train' else owner.evolution_bounds;start=plan.get('startMs',role_start);assert role_start<=start<end<=1792540800000
  envs=[owner.world(role,start,end)for _ in range(2)];envs[1].keys=defaultdict(Catalog,((v,Catalog(keys))for v,keys in envs[1].keys.items()));assert all(e.managed_data_service.enabled for e in envs)
  def audit():
   a,b=envs;assert a.now==b.now;assert list(a.feed_requests)==list(b.feed_requests) and list(a.feed_receipts)==list(b.feed_receipts);assert dict(a.feed_request_counts)==dict(b.feed_request_counts);assert a.keys==b.keys
   fa,fb=financial_digest(a),financial_digest(b);assert fa==fb;oa,ob=[tensor_digest(tensors(e.observe(),'cpu'))for e in envs];assert oa==ob
   return dict(virtualMs=a.now,financialSha256=fa,observationSha256=oa,paidRequests=dict(a.feed_request_counts))
  for e in envs:e.advance(start+60000)
  audit();rounds=[]
  if plan.get('requireThreeVenueReads',False):assert all(envs[0].feed_request_counts[v]>0 for v in ['SIG','Kalshi','Polymarket'])
  for k in range(6):
   target=start+60000+(k+1)*300000;times={}
   for i in ([0,1]if k%2==0 else[1,0]):
    t=time.perf_counter();envs[i].advance(target);times[i]=time.perf_counter()-t
   rounds.append(dict(round=k,listSeconds=times[0],indexedSeconds=times[1],state=audit()));(j/'progress.json').write_text(json.dumps(dict(completedRounds=len(rounds))));(j/f'round-{k:03d}.json').write_text(json.dumps(rounds[-1],indent=2))
  result=dict(classification='six_paired_recorded_role_source_rounds_not_whole_training_or_ROI',sourceCalendarRole=role,sourceBoundsMs=[start,end],sourceCommit=plan['sourceCommit'],catalogSha256=plan['catalogSha256'],datasetManifestSha256=owner.dataset_sha,rounds=rounds,medianSourceSpeedRatio=statistics.median(x['listSeconds']for x in rounds)/statistics.median(x['indexedSeconds']for x in rounds),exactObservationFinanceCatalogRequestsAndReceipts=True,optimizerRun=False,modelInference=False,productionChanged=False,independentTestUsed=False,physicalAccountRequests=0);verify_source(root);(j/'RESULT.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(dict(medianSourceSpeedRatio=result['medianSourceSpeedRatio'])))
 except BaseException as ex:
  (j/'FAILED.json').write_text(json.dumps(dict(type=type(ex).__name__,reason=str(ex))));traceback.print_exc();raise
 finally:
  if envs:release_worlds(envs)
  if owner is not None:owner.close()
if __name__=='__main__':main()
