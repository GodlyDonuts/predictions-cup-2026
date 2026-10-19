import os,json,time,hashlib,bisect,ctypes,copy
from pathlib import Path
import numpy as np
from moloch.learning_actions.disk_tape import DiskFlowTape,sha
from moloch.learning_actions.full_depth import FullDepthTape
from moloch.learning_actions.snapshot_clock import prepare
from moloch.learning_actions.contract import OrderSpec,UNIT
from moloch.learning_flow.episodes import FlowTape
from moloch.learning_realism.protocol import Split,DAY
from moloch.learning_v2.run_experiment import load_mechanics
from moloch.learning_actions.state_encoding import SIGNED_LOG
root=Path(os.environ['LOCALAPPDATA'])/'Moloch/learning-v2';cache=root/'data/three-venue-disk-cache-001';source=root/'data/three-venue-longer-raw-flow-003'
cache_sha=sha(cache/'manifest.json');started=time.perf_counter();d=DiskFlowTape(cache,cache_sha,source);loaded=time.perf_counter()-started
c=json.loads((root/'handoffs/population-014-to-015-v2/next-config.json').read_text());lo,hi=c['development_train_ms'];mechanics=load_mechanics(Path(c['mechanics']))
sp=Split(d.times[0],d.times[-1]+9*DAY)
def tape(original):
 t=FullDepthTape.from_loaded(original,split=sp,role='train');t.counterparty_information_bounds=(lo,hi)
 t.counterparty_replay_enabled=True;t.world_book_lifetime_ms=60000;t.global_state_encoding=SIGNED_LOG
 prepare(t,(lo,hi),role_prefix_initial_state=True);return t
def world(t,start,end):return t.episode(mechanics,start,end,initial_cash_micros=100000*UNIT,target_wealth_micros=110000*UNIT,burnin_ms=0)
started=time.perf_counter();disk=tape(d);view_time=time.perf_counter()-started;before_decoded=d.disk_store.decoded_rows
started=time.perf_counter();late=world(disk,lo+3600000,lo+3660000);prefix_time=time.perf_counter()-started
prefix={'initialState':late.initial_market_state,'actorBooks':len(late.actor_books),'actorReferences':len(late.reference_cache),
'decodedSourceRows':d.disk_store.decoded_rows-before_decoded,'sourceCursors':sum(e[3]=='api_source_cursor'for e in late.clock._events),
'queuedEvents':len(late.clock._events)}
# Materialize only the already-inspected first62s, not a second full dataset.
end=lo+60000;a=0;b=bisect.bisect_left(d.times,end+2000)
e=FlowTape.__new__(FlowTape);e.rows=tuple({k:v for k,v in r.items()if k!='_disk_record'}for r in d.rows[a:b]);e.times=tuple(r['available_at_ms']for r in e.rows)
e.exchanges=d.exchanges;e.index=d.index;e.flow_rows=tuple({k:v for k,v in r.items()if k!='_disk_record'}for r in d.flow_rows);e.flow_times=tuple(r['available_at_ms']for r in e.flow_rows)
e.manifest=d.manifest;e.manifest_sha256=d.manifest_sha256
before=world(tape(e),lo,end);after=world(disk,lo,end)
def equal():
 x,y=before.observe(),after.observe();assert x['entities']==y['entities']
 for k in('contracts','global_state','present'):np.testing.assert_array_equal(x[k],y[k])
 for k in x['masks']:np.testing.assert_array_equal(x['masks'][k],y['masks'][k])
 assert (before.cash,before.lots,before.api_fills,before.realized,before.now,before.api_orders)==(after.cash,after.lots,after.api_fills,after.realized,after.now,after.api_orders)
checks=0;started=time.perf_counter()
for offset in(3000,10000,20000,30000,40000,50000):
 for env in(before,after):env.advance(lo+offset)
 equal();checks+=1
 key=next(k for k in before.keys['SIG']if k[0]=='book')
 for env in(before,after):env.request_observation('SIG',key);env.advance(env.busy_until)
 equal();checks+=1
 if offset==3000:
  for env in(before,after):
   env.dispatch('ORDER',[OrderSpec(key[1],'YES','BUY',1,None)],idempotency_key='engineering-one-share')
   env.advance(env.busy_until)
  equal();checks+=1
 elapsed=time.perf_counter()-started
class Memory(ctypes.Structure):
 _fields_=[('cb',ctypes.c_uint32),('faults',ctypes.c_uint32)]+[(k,ctypes.c_size_t)for k in('peak','working','quota_peak_paged','quota_paged','quota_peak_nonpaged','quota_nonpaged','pagefile','peak_pagefile','private')]
m=Memory();m.cb=ctypes.sizeof(m);kernel=ctypes.WinDLL('kernel32');kernel.GetCurrentProcess.restype=ctypes.c_void_p
psapi=ctypes.WinDLL('psapi');psapi.GetProcessMemoryInfo.argtypes=[ctypes.c_void_p,ctypes.c_void_p,ctypes.c_uint32]
if not psapi.GetProcessMemoryInfo(kernel.GetCurrentProcess(),ctypes.byref(m),m.cb):raise OSError('MEMORY_PROBE_FAILED')
r={'classification':'file_backed_real_source_engineering_parity_and_memory_not_roi',
'cacheManifestSha256':cache_sha,'sourceManifestSha256':d.manifest_sha256,'cacheCounts':d.disk_store.manifest['counts'],
'cacheDatabaseBytes':d.disk_store.manifest['databaseBytes'],'openAndSourceHashVerificationSeconds':loaded,'roleIndexSeconds':view_time,
'oneHourSameRolePrefixSeconds':prefix_time,'oneHourPrefix':prefix,'actualFirstMinuteParityCheckpoints':checks,
'actualFirstMinuteReplayAndChecksSeconds':elapsed,'rangeMaterializedRows':len(e.rows),'rangeMaterializedAllTradeRows':len(e.flow_rows),
'exactObservationsCashOrdersFillsAndClocks':True,'peakProcessWorkingSetBytes':m.peak,'processPrivateBytes':m.private,
'simulatedFills':len(after.api_fills),'newAccountRequests':0,'independentTestUsed':False,'activeTrainingChanged':False,
'productionChanged':False,'independentProfitableRoiEstablished':False,
'limits':['single finite CPU engineering probe, not8world GPU capacity','first-minute eager range is not a second full corpus',
'cache built during source development; final checked-source integration still required','no additional historical coverage or execution calibration']}
Path('research/tartarus-learning-v2/disk-replay-real-source-probe-001.json').write_text(json.dumps(r,indent=2)+'\n');d.disk_store.close()
print(json.dumps(r))
