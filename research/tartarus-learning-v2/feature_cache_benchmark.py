import os,json,time,subprocess,types,statistics
from pathlib import Path
import numpy as np
from moloch.learning_actions.disk_tape import DiskFlowTape,sha
from moloch.learning_actions.full_depth import FullDepthTape,extend_observation
from moloch.learning_actions.engine import ApiEpisode
from moloch.learning_actions.snapshot_clock import prepare
from moloch.learning_actions.contract import UNIT
from moloch.learning_realism.protocol import Split,DAY
from moloch.learning_v2.run_experiment import load_mechanics
from moloch.learning_actions.state_encoding import SIGNED_LOG
from moloch.learning_actions.batched import inputs
import torch
root=Path(os.environ['LOCALAPPDATA'])/'Moloch/learning-v2'
base='893ece427f77435bcca052af87704af942c08791'
def old(name):
 text=subprocess.check_output(['git','show',base+':moloch/learning_actions/'+name+'.py'],text=True)
 m=types.ModuleType('moloch.learning_actions._baseline_'+name);m.__package__='moloch.learning_actions';exec(compile(text,name+'.py','exec'),m.__dict__);return m
old_engine,old_depth=old('engine'),old('full_depth')
c=json.loads((root/'handoffs/population-014-to-015-v2/next-config.json').read_text());lo,hi=c['development_train_ms']
cache=root/'data/three-venue-disk-cache-001';d=DiskFlowTape(cache,sha(cache/'manifest.json'),root/'data/three-venue-longer-raw-flow-003')
t=FullDepthTape.from_loaded(d,split=Split(d.times[0],d.times[-1]+9*DAY),role='train');t.counterparty_information_bounds=(lo,hi);t.counterparty_replay_enabled=True;t.world_book_lifetime_ms=60000;t.global_state_encoding=SIGNED_LOG
prepare(t,(lo,hi),role_prefix_initial_state=True)
e=t.episode(load_mechanics(Path(c['mechanics'])),lo+3600000,hi,initial_cash_micros=100000*UNIT,target_wealth_micros=110000*UNIT,burnin_ms=0)
for key in [k for k in e.keys['SIG']if k[0]=='book']:
 ready=e.budget.ready_at(e.now,False)
 if ready>e.now:e.advance(ready)
 e.request_observation('SIG',key);e.advance(e.busy_until)
def legacy():return old_depth.extend_observation(e,old_engine.ApiEpisode.observe(e))
def current():return extend_observation(e,ApiEpisode.observe(e))
def equal():
 a,b=legacy(),current();assert a['entities']==b['entities']
 for k in('contracts','global_state','present'):np.testing.assert_array_equal(a[k],b[k])
 for k in a['masks']:np.testing.assert_array_equal(a['masks'][k],b['masks'][k])
 return b
for i in range(6):
 e.advance(e.now+17);e._delivered_depth_cache={};equal()
samples={'baseline':[],'cached':[]}
for rep in range(8):
 for name,f in ([('baseline',legacy),('cached',current)]if rep%2==0 else[('cached',current),('baseline',legacy)]):
  for _ in range(5):f()
  start=time.perf_counter()
  for _ in range(100):f()
  samples[name].append((time.perf_counter()-start)/100)
a=equal();packed=inputs([a]*8,'cpu');n=len(a['entities'])
for i in range(8):np.testing.assert_array_equal(packed['contracts'][i].numpy(),a['contracts'])
r={'classification':'real_source_cpu_observation_benchmark_not_training_or_roi','baselineCommit':base,'sourceManifestSha256':d.manifest_sha256,'actorBooks':len(e.actor_books),'entities':n,'virtualTimeFrozenDuringTiming':True,'observationsPerSample':100,'secondsPerObservation':samples,'medianSpeedRatio':statistics.median(samples['baseline'])/statistics.median(samples['cached']),'exactObservationParity':True,'eightWorldCpuPackingParity':True,'newAccountRequests':0,'independentTestUsed':False,'productionChanged':False,'activeTrainingChanged':False}
Path('research/tartarus-learning-v2/feature-cache-benchmark-001.json').write_text(json.dumps(r,indent=2)+'\n');d.disk_store.close();print(json.dumps(r))