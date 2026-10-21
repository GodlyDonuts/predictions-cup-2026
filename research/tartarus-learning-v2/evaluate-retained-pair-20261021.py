"""Matched CPU full-window development evaluation; never an independent test."""
from pathlib import Path
from datetime import datetime,timezone
import argparse,os,sys,json,time,gc,hashlib,traceback

def main():
 parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--job',type=Path,required=True);args=parser.parse_args();j=args.job.resolve();plan=json.loads((j/'PLAN.json').read_bytes());root=Path(plan['sourceRoot']);sys.path.insert(0,str(root));os.chdir(root)
 import torch
 from moloch.learning_actions.continue_population import verify_source
 from moloch.learning_actions.experiment import Experiment
 from moloch.learning_actions.training import load_checkpoint
 from moloch.learning_actions.evaluation_batch import EvaluationBatch
 from moloch.learning_v2.train_inner import fingerprint
 sha=lambda p:hashlib.sha256(Path(p).read_bytes()).hexdigest()
 assert sha(j/'config.json')==plan['configSha256'];assert sha(j/'calibration.json')==plan['calibrationSha256'];assert sha(root/'source-snapshot.json')==plan['sourceManifestSha256'];assert verify_source(root)['commit']==plan['sourceCommit']
 c=json.loads((j/'config.json').read_bytes());assert c['device']=='cpu' and c['cpu_threads']==1;assert c['batched_evaluation'] is True and c['batched_evaluation_action_heads'] is True;assert c['process_source_worlds'] is False
 assert not torch.cuda.is_initialized();e=None;started=time.perf_counter();label=None;original=EvaluationBatch.run_chunk
 def progress(batch,*a,**kw):
  n=original(batch,*a,**kw);d=dict(observedUtc=datetime.now(timezone.utc).isoformat(),policy=label,decisions=batch.decisions,virtualMs=[x.now for x in batch.envs],endMs=[x.end for x in batch.envs],wallSeconds=time.perf_counter()-started,optimizerRun=False,independentTestUsed=False);q=j/'progress.pending';q.write_text(json.dumps(d));q.replace(j/'progress.json');return n
 EvaluationBatch.run_chunk=progress
 try:
  e=Experiment(c,j/'experiment');assert e.dataset_sha==plan['datasetManifestSha256'];assert list(e.evolution_bounds)==plan['evaluationBoundsMs'];e.calibration=json.loads((j/'calibration.json').read_bytes());assert e.calibration['decision_latency_ms']==plan['frozenActorCostMs']
  profiles=None;results={}
  for label in ['parent','candidate']:
   cp=plan[label];assert sha(cp['path'])==cp['sha256'];model,bundle=load_checkpoint(cp['path'],cp['sha256'],'cpu');assert bundle['split_specification_sha256']==plan['experimentSha256']
   current={k:bundle.get(k) for k in ['architecture','hidden_size','historical_memory_profile','read_target_sampling','managed_data_profile','price_parameterization','acting_validation','global_state_encoding','observation_protocol']}
   if profiles is None:profiles=current
   else:assert current==profiles
   before=fingerprint(model);model.eval();t=time.perf_counter();selection=e.evaluate(model,cp,confirmation=True);assert fingerprint(model)==before and not torch.cuda.is_initialized()
   assert len(selection['cases'])==2 and all(x['D']==2 and x['start_ms']==plan['evaluationBoundsMs'][0] and x['end_ms']==plan['evaluationBoundsMs'][1] for x in selection['cases'])
   value=dict(policy=label,checkpointSha256=cp['sha256'],parameterSha256=before,selection=selection,wallSeconds=time.perf_counter()-t,weightsUnchanged=True,optimizerRun=False)
   (j/(label+'-RESULT.json')).write_text(json.dumps(value,indent=2)+'\n');results[label]=value;del model,bundle;gc.collect()
  e.close();e=None;verify_source(root);result=dict(observedUtc=datetime.now(timezone.utc).isoformat(),classification='paired_CPU_full_two_day_development_not_independent_ROI',sourceCommit=plan['sourceCommit'],sourceManifestSha256=plan['sourceManifestSha256'],planSha256=sha(j/'PLAN.json'),datasetManifestSha256=plan['datasetManifestSha256'],candidateTrainingUpdates=plan['candidate']['updates'],candidateTrainingDecisions=plan['candidate']['decisions'],device='cpu',frozenActorCostMs=plan['frozenActorCostMs'],sameHistoricalSourceAndCaseSeeds=True,nominalAndStressedPathsDiffer=True,bitEquivalentToGPU=False,results=results,wallSeconds=time.perf_counter()-started,optimizerRun=False,activeTrainerChanged=False,independentTestUsed=False,productionChanged=False,physicalAccountRequests=0)
  (j/'RESULT.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps({k:dict(reward=v['selection']['reward'],markedROI=v['selection']['marked_roi_min'])for k,v in results.items()}),flush=True)
 except BaseException as ex:
  (j/'FAILED.json').write_text(json.dumps(dict(type=type(ex).__name__,reason=str(ex))));traceback.print_exc();raise
 finally:
  EvaluationBatch.run_chunk=original
  if e is not None:e.close()
if __name__=='__main__':main()
