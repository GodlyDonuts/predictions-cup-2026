"""Recorded-data process/serial source comparison; no inference or ROI claim.
Copy to a fresh private job containing PLAN/config and a checked source snapshot.
"""
from pathlib import Path
import argparse,os,sys,json,time,math,statistics,hashlib,traceback

def main():
 p=argparse.ArgumentParser();p.add_argument('--job',type=Path,default=Path(__file__).parent);a=p.parse_args()
 j=a.job.resolve();root=j/'checked-source';sys.path.insert(0,str(root));os.chdir(root)
 from moloch.learning_actions.continue_population import verify_source
 from moloch.learning_actions.experiment import Experiment
 from moloch.learning_actions.process_worlds import ProcessCohort,ExperimentFactory
 from moloch.learning_actions.world_step import transition
 from moloch.learning_actions.episode_training import release_worlds
 from moloch.learning_actions.episode_tape import financial_digest,tensor_digest
 from moloch.learning_actions.stream import tensors
 from moloch.learning_actions.training_reward import ledger
 from moloch.learning_actions.realtime import RECONCILE_REFRESH
 from datetime import datetime,timezone
 owner=None;backend=None;serial=[]
 try:
  plan=json.loads((j/'PLAN.json').read_bytes());source=verify_source(root);assert source['commit']==plan['sourceCommit']
  c=json.loads((j/'config.json').read_bytes());assert c['sig_realtime_refresh_mode']==RECONCILE_REFRESH
  assert c['development_train_ms'][1]<=1792540800000
  t=time.perf_counter();owner=Experiment(c,j/'serial-experiment');assert owner.dataset_sha==plan['datasetManifestSha256']
  assert owner.original.disk_store.cache_manifest_sha256==plan['cacheManifestSha256']
  start,end=owner.train_bounds;serial=[owner.world('train',start,end)for _ in range(plan['worlds'])]
  ledgers=[ledger(e,c,development=owner.development)for e in serial];serial_start=time.perf_counter()-t
  t=time.perf_counter();backend=ProcessCohort([(ExperimentFactory(c,str(j/('process-world-%03d'%i)),start,end),())for i in range(plan['worlds'])]);parallel_start=time.perf_counter()-t
  def action(now,obs,duration):
   market=next(i for i in range(len(obs['entities']))if obs['masks']['family'][i,0])
   after=now+12;span=max(1,253402300799999-after-1)
   return dict(family=0,market=market,target_fraction=0.,child_fraction=math.log1p(duration)/math.log1p(span),patience=1,side=0,price=1)
  def step_serial(duration):
   t=time.perf_counter();results=[]
   for e,l in zip(serial,ledgers):
    e.advance(e.now);o=e.observe();results.append(transition(e,l,action(e.now,o,duration),o,12))
   return time.perf_counter()-t,results
  def step_parallel(duration):
   t=time.perf_counter();observed=backend.prepare(backend.worlds,[True]*len(serial));actions=[action(e.now,o,duration)for e,o in zip(backend.worlds,observed)]
   result=backend.transitions(backend.worlds,[True]*len(serial),actions,12);result=[{k:x[k]for k in('reward','terminal','elapsed_ms','final')}for x in result];return time.perf_counter()-t,result
  def audit():
   hashes=[]
   for local,remote in zip(serial,backend.worlds):
    observed=tensor_digest(tensors(local.observe(),'cpu'));state=financial_digest(local);r=backend.request(remote,'audit',None)
    assert state==r['financialSha256'] and observed==r['observationSha256']
    assert local.now==remote.now and local.end==remote.end==end
    assert dict(local.feed_request_counts)==r['paidRequests']
    assert list(local.feed_requests)==r['feedRequests'] and list(local.feed_receipts)==r['feedReceipts']
    assert local.cash==c.get('initial_cash_micros',100000000000) and not local.api_orders and not local.api_fills and remote.details['fill_count']==0
    hashes.append(dict(virtualMs=local.now,financialSha256=state,observationSha256=observed,paidReadCounts=r['paidRequests']))
   return hashes
  warm_a=step_serial(plan['initialUnmeasuredWarmupMs']);warm_b=step_parallel(plan['initialUnmeasuredWarmupMs']);assert warm_a[1]==warm_b[1];audit();rounds=[]
  for k in range(plan['measuredRounds']):
   if k%2:a1,b1=step_parallel(plan['roundAdvanceMs']),step_serial(plan['roundAdvanceMs']);p1,s1=a1,b1
   else:s1,p1=step_serial(plan['roundAdvanceMs']),step_parallel(plan['roundAdvanceMs'])
   assert s1[1]==p1[1]
   rounds.append(dict(round=k,serialSeconds=s1[0],parallelSeconds=p1[0],exactTransitionResultsEqual=True,states=audit()))
   (j/('round-%03d.json'%k)).write_text(json.dumps(rounds[-1],indent=2)+'\n')
   (j/'progress.json').write_text(json.dumps(dict(completedRounds=len(rounds),worlds=plan['worlds'])))
  memories=[dict(ident)for ident in backend.identities];memory_error=None
  try:
   import subprocess
   ids=','.join(str(int(x['pid']))for x in memories)
   command='Get-Process -Id '+ids+' | Select-Object Id,WorkingSet64 | ConvertTo-Json -Compress'
   values=json.loads(subprocess.check_output(['powershell.exe','-NoProfile','-Command',command],text=True))
   if isinstance(values,dict):values=[values]
   rss={x['Id']:x['WorkingSet64']for x in values}
   for x in memories:x['rssBytes']=rss[x['pid']]
  except Exception as error:memory_error=type(error).__name__

  verify_source(root);result=dict(observedUtc=datetime.now(timezone.utc).isoformat(),classification='recorded_fit_source_parallel_advance_not_whole_training_speed_or_ROI',sourceCommit=source['commit'],datasetManifestSha256=plan['datasetManifestSha256'],cacheManifestSha256=plan['cacheManifestSha256'],fitBoundsMs=[start,end],worlds=plan['worlds'],originalFinancialHorizonsPreserved=True,firstWarmupExcluded=True,serialFirstCohortStartupSeconds=serial_start,parallelFirstCohortStartupSeconds=parallel_start,rounds=rounds,sourceAdvanceMedianSpeedRatio=statistics.median(x['serialSeconds']for x in rounds)/statistics.median(x['parallelSeconds']for x in rounds),workers=memories,optionalMemoryMetricError=memory_error,calibration=dict(decision_latency_ms=12,scope='same_frozen_virtual_cost_not_IPC_calibration'),controlledRawAlarmTrace=True,modelInference=False,optimizerRun=False,active033Changed=False,queued034Changed=False,productionChanged=False,independentTestUsed=False,physicalAccountRequests=0)
  (j/'RESULT.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps({k:result[k]for k in ['observedUtc','worlds','sourceAdvanceMedianSpeedRatio']}),flush=True)
 except BaseException as error:
  (j/'FAILED.json').write_text(json.dumps(dict(type=type(error).__name__,reason=str(error))));traceback.print_exc();raise
 finally:
  if backend is not None:backend.close()
  if serial:release_worlds(serial)
  if owner is not None:owner.close()

if __name__=='__main__':main()
