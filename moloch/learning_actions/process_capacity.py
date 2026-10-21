"""Actual spawned-world model/optimizer admission, not strategy qualification.

The handoff caller must first retain/stop the exact old GPU owner. The short
inspected training-role window measures allocation and one real PPO update;
it is never a complete-day reward, a parent checkpoint or a profitability test.
"""
import argparse,hashlib,json,time,gc
from pathlib import Path
import torch
from .experiment import Experiment
from .process_worlds import ProcessCohort,CapacityFactory,prime_books
from .economics import GrowthLedger
from .batched import BatchStream
from .episode_training import release_worlds
from .episode_tape import financial_digest,tensor_digest,observation_fields
from .stream import calibrate,tensors
from .training import update
from .selection import frozen
from .behavior_guard import identity


def sha(path):return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def run(config_path,output,*,worlds=8,steps=8,start_offset_ms=1200000,
        priming_reads=8,memory_fraction=.8,guard='tensor_versions_v1',checkpoint_encoder=True):
    if type(worlds)is not int or worlds<1 or type(steps)is not int or steps<1:
        raise ValueError('POSITIVE_CAPACITY_WORLD_AND_STEP_COUNTS')
    config=json.loads(Path(config_path).read_text(encoding='utf-8-sig'))
    if config.get('process_source_worlds')is not True:
        raise ValueError('PROCESS_CAPACITY_REQUIRES_PROCESS_PROFILE')
    if not 0<memory_fraction<=1:raise ValueError('VALID_CAPACITY_MEMORY_FRACTION')
    config=dict(config,cuda_memory_fraction=memory_fraction,allow_tf32=False)
    owner=None;cohort=None;local=[];output=Path(output)
    try:
        owner=Experiment(config,output);torch.manual_seed(191104)
        model=owner.make_policy()
        if config.get('initial_short_alarm_prior',True):owner.initialize_alarm_prior(model)
        device=next(model.parameters()).device
        a,b=owner.train_bounds;start=a+start_offset_ms
        bounds=[(start+i*500,start+i*500+60000)for i in range(worlds)]
        if bounds[0][0]<a or bounds[-1][1]>b:raise ValueError('CAPACITY_DIAGNOSTIC_OUTSIDE_TRAIN_ROLE')
        local=[owner.world('train',lo,hi)for lo,hi in bounds]
        for env in local:prime_books(env,priming_reads)
        calibration=calibrate(local[0],model)
        ledgers=[GrowthLedger(env,development=True,historical_last_mark=True)for env in local]
        settings=dict(calibration=calibration,behavior_guard=guard,compact_finished=True,
                      rollout_only=config.get('compact_rollout_statistics',False))
        cpu_rng=torch.get_rng_state();gpu_rng=torch.cuda.get_rng_state(device)if device.type=='cuda'else None
        sync=lambda:torch.cuda.synchronize(device)if device.type=='cuda'else None
        sync();started=time.perf_counter()
        baseline=BatchStream(local,model,ledgers,**settings);baseline_chunk=baseline.collect(steps)
        sync();baseline_seconds=time.perf_counter()-started
        baseline_frames=[(tensor_digest(observation_fields(f)),tensor_digest({k:f[k]for k in
            ('actions','old_logprob','old_value','initial_hidden','reset','terminal','learn')}),
            f['economic_reward'],f['elapsed_ms'])for f in baseline_chunk.frames]
        baseline_hidden=baseline.hidden.detach().clone()
        baseline_states=[(financial_digest(e),tensor_digest(tensors(e.observe(),'cpu')),
                          list(e.feed_requests),list(e.feed_receipts),dict(e.feed_request_counts))for e in local]
        del baseline_chunk,baseline
        release_worlds(local);local=[];del ledgers;gc.collect()
        started=time.perf_counter()
        cohort=ProcessCohort([(CapacityFactory(config,str(output/('source-%03d'%i)),lo,hi,priming_reads),())
                             for i,(lo,hi)in enumerate(bounds)])
        startup=time.perf_counter()-started
        torch.set_rng_state(cpu_rng)
        if gpu_rng is not None:torch.cuda.set_rng_state(gpu_rng,device)
        stream=BatchStream(cohort.worlds,model,[None]*worlds,world_backend=cohort,**settings)
        sync();started=time.perf_counter();chunk=stream.collect(steps);sync();process_seconds=time.perf_counter()-started
        frames=[(tensor_digest(observation_fields(f)),tensor_digest({k:f[k]for k in
            ('actions','old_logprob','old_value','initial_hidden','reset','terminal','learn')}),
            f['economic_reward'],f['elapsed_ms'])for f in chunk.frames]
        if frames!=baseline_frames:raise ValueError('PROCESS_CAPACITY_ACTION_OBSERVATION_OR_REWARD_DRIFT')
        if not torch.equal(stream.hidden,baseline_hidden):raise ValueError('PROCESS_CAPACITY_HIDDEN_STATE_DRIFT')
        for view,expected in zip(cohort.worlds,baseline_states):
            actual=cohort.request(view,'audit',None)
            state=(actual['financialSha256'],actual['observationSha256'],actual['feedRequests'],
                   actual['feedReceipts'],actual['paidRequests'])
            if state!=expected:raise ValueError('PROCESS_CAPACITY_FINANCIAL_OR_RECEIPT_DRIFT')
        optimizer=owner.make_optimizer(model,rate=1e-6);before=identity(model,guard)
        sync();started=time.perf_counter()
        reports=update(model,optimizer,chunk,[f['economic_reward']for f in chunk.frames],role='train',epochs=1,
                       checkpoint_encoder=checkpoint_encoder,
                       entropy_coefficient=optimizer.param_groups[0]['tartarus_entropy_coefficient'])
        sync();update_seconds=time.perf_counter()-started
        changed=identity(model,guard)!=before
        if not any(p['accepted']for p in reports)or not changed:raise ValueError('PROCESS_GPU_UPDATE_REQUIRED')
        source=Path(__file__).resolve().parents[2]/'source-snapshot.json'
        result={'classification':'short_process_GPU_allocation_and_update_not_full_reward_history_or_ROI',
            'device':str(device),'processSourceWorlds':True,'sourceWorkerCount':worlds,
            'sourceWorkersCPUOnly':all(x['cudaInitialized']is False for x in cohort.identities),
            'workerIdentities':cohort.identities,'configurationSha256':sha(config_path),
            'sourceManifestSha256':sha(source)if source.exists()else None,
            'identical_wallets_orders_fills_and_clocks':True,'exactObservationActionsHiddenRewardsAndReceipts':True,
            'on_policy_guard_changed_after_optimizer_step':changed,'ppo':reports,
            'capacityDiagnosticBoundsMs':bounds,'fullDayRewardQualification':False,
            'effectiveCudaMemoryFraction':memory_fraction,
            'compute_calibration':calibration,'offlineIPCIncludedInVirtualActorCost':False,
            'localCollectionSeconds':baseline_seconds,'processStartupSeconds':startup,
            'processCollectionSeconds':process_seconds,'optimizerSeconds':update_seconds,
            'cuda_peak_allocated_bytes':torch.cuda.max_memory_allocated(device)if device.type=='cuda'else 0,
            'dataset_manifest_sha256':owner.dataset_sha,'production_changed':False,
            'independent_test_used':False,'http_requests':0,'parentOrProductionCheckpointCreated':False}
        frozen(output/'RESULT.json',result);return result
    finally:
        if cohort is not None:cohort.close()
        if local:release_worlds(local)
        if owner is not None:owner.close()


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--config',type=Path,required=True);p.add_argument('--output',type=Path,required=True)
    p.add_argument('--worlds',type=int,default=8);p.add_argument('--steps',type=int,default=8)
    p.add_argument('--start-offset-ms',type=int,default=1200000);p.add_argument('--prime-books',type=int,default=8)
    p.add_argument('--memory-fraction',type=float,default=.8)
    p.add_argument('--guard',choices=('sha256','tensor_versions_v1'),default='tensor_versions_v1')
    p.add_argument('--checkpoint-encoder',action='store_true');a=p.parse_args()
    print(json.dumps(run(a.config,a.output,worlds=a.worlds,steps=a.steps,start_offset_ms=a.start_offset_ms,
        priming_reads=a.prime_books,memory_fraction=a.memory_fraction,guard=a.guard,
        checkpoint_encoder=a.checkpoint_encoder)),flush=True)


if __name__=='__main__':main()
