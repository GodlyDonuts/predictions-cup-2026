"""Controlled raw-action serial/batched collection on inspected history."""
import argparse,json,time,statistics
from pathlib import Path
import torch
from .experiment import Experiment
from .stream import Stream,calibrate
from .batched import BatchStream,inputs
from .economics import GrowthLedger
from .policy import ApiPolicy
from .training import collect_chunk,update
from .selection import frozen


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--config',type=Path,required=True)
    p.add_argument('--output',type=Path,required=True);p.add_argument('--worlds',type=int,default=8)
    p.add_argument('--steps',type=int,default=32)
    p.add_argument('--guard',choices=('sha256','tensor_versions_v1'),default='sha256')
    p.add_argument('--checkpoint-encoder',action='store_true')
    p.add_argument('--start-offset-ms',type=int,default=120000)
    p.add_argument('--prime-books',type=int,default=0)
    p.add_argument('--finished-worlds',type=int,default=0)
    p.add_argument('--memory-fraction',type=float,default=1.);args=p.parse_args()
    if args.prime_books<0:raise ValueError('NONNEGATIVE_BENCHMARK_PRIMING_READS')
    if not 0<=args.finished_worlds<args.worlds:raise ValueError('BENCHMARK_REQUIRES_ONE_ACTIVE_WORLD')
    config=json.loads(args.config.read_text(encoding='utf-8-sig'))
    if not 0<args.memory_fraction<=1:raise ValueError('VALID_BENCHMARK_MEMORY_FRACTION')
    config['cuda_memory_fraction']=args.memory_fraction
    # Compare mathematical batching without TF32's shape-dependent rounding.
    torch.backends.cuda.matmul.allow_tf32=False;torch.backends.cudnn.allow_tf32=False
    experiment=Experiment(config,args.output);torch.manual_seed(191104)
    model=experiment.make_policy()
    if config.get('initial_short_alarm_prior',True):experiment.initialize_alarm_prior(model)
    def worlds():
        a,b=experiment.train_bounds;start=a+args.start_offset_ms
        envs=[experiment.world('train',start+i*500,start+i*500+60000)for i in range(args.worlds)]
        for env in envs:
            targets=[k for k in env.keys['SIG']if k[0]=='book'][:args.prime_books]
            if len(targets)!=args.prime_books:raise ValueError('BENCHMARK_PRIMING_SOURCE_TARGETS_UNAVAILABLE')
            for key in targets:
                while True:
                    result=env.request_observation('SIG',key)
                    if result['status']=='IN_FLIGHT':env.advance(env.busy_until);break
                    if result['status']not in('BUSY','RATE_WAIT'):raise ValueError('BENCHMARK_PRIMING_READ_FAILED')
                    at=result['ready_ms']
                    if not env.now<at<env.end:raise ValueError('BENCHMARK_PRIMING_EXCEEDS_HORIZON')
                    env.advance(at)
        for env in envs[-args.finished_worlds:]if args.finished_worlds else[]:env.advance(env.end)
        return envs
    calibration=calibrate(worlds()[0],model)
    def sync():
        if next(model.parameters()).is_cuda:torch.cuda.synchronize()
    warm=worlds();prepared=inputs([e.observe()for e in warm],next(model.parameters()).device)
    with torch.no_grad():
        for _ in range(3):model.step(**prepared,hidden=model.initial_state(args.worlds),
            reset=torch.ones(args.worlds,device=next(model.parameters()).device,dtype=torch.bool),deterministic=True)
    timings=[];last_chunk=None
    orders=(('serial','batched','compacted'),('compacted','batched','serial'))if args.finished_worlds else(('serial','batched'),('batched','serial'))
    compact_chunk=None
    for order in orders:
        states={};durations={}
        for mode in order:
            envs=worlds();ledgers=[GrowthLedger(e,development=True,historical_last_mark=True)for e in envs]
            sync();started=time.perf_counter()
            if mode=='serial':
                streams=[Stream(e,model,calibration=calibration,record_training=True,behavior_guard=args.guard)for e in envs]
                for stream,ledger in zip(streams,ledgers):
                    # Use the same deterministic neural actions as the batch.
                    for _ in range(args.steps):
                        if stream.env.now>=stream.env.end:break
                        stream.step(deterministic=True);ledger.transition()
                hidden=torch.cat([s.hidden for s in streams])
            else:
                stream=BatchStream(envs,model,ledgers,calibration=calibration,behavior_guard=args.guard,
                                   compact_finished=mode=='compacted')
                last_chunk=stream.collect(args.steps,deterministic=True);hidden=stream.hidden
                if mode=='compacted':compact_chunk=last_chunk
            sync();durations[mode]=time.perf_counter()-started
            states[mode]=([{'clock':e.now,'cash':e.cash,'lots':e.lots,'fills':e.api_fills,
                'api_id':e.api_id,'orders':repr(e.api_orders)}for e in envs],hidden.detach().clone())
        for mode in order:
            if states['serial'][0]!=states[mode][0]:raise ValueError('BATCHED_FINANCIAL_OR_CLOCK_DIFFERENCE')
            live=args.worlds-args.finished_worlds
            torch.testing.assert_close(states['serial'][1][:live],states[mode][1][:live],atol=1e-5,rtol=1e-4)
        timings.append(durations)
    if compact_chunk is not None:last_chunk=compact_chunk
    optimizer=torch.optim.Adam(model.parameters(),lr=1e-6);sync();started=time.perf_counter()
    result=update(model,optimizer,last_chunk,[f['economic_reward']for f in last_chunk.frames],role='train',epochs=1,
                  checkpoint_encoder=args.checkpoint_encoder)
    sync();elapsed=time.perf_counter()-started
    from .behavior_guard import identity
    guard_changed=identity(model,args.guard)!=last_chunk.behavior_sha256
    if args.guard=='tensor_versions_v1'and any(r['accepted']for r in result)and not guard_changed:
        raise ValueError('GPU_OPTIMIZER_MUTATION_NOT_DETECTED')
    serial=statistics.median(x['serial']for x in timings);batched=statistics.median(x['batched']for x in timings)
    out={'classification':('controlled_inspected_history_raw_api_gpu_batch_benchmark_not_roi'if next(model.parameters()).is_cuda
                           else'controlled_inspected_history_raw_api_cpu_batch_benchmark_not_roi'),
        'device':str(next(model.parameters()).device),
        'worlds':args.worlds,'steps_per_world':args.steps,'parameters':sum(p.numel()for p in model.parameters()),
        'finished_worlds_at_chunk_start':args.finished_worlds,'ppo_batch_worlds':len(last_chunk.initial_hidden),
        'contract_features':model.contract_features,'priming_book_reads_per_world':args.prime_books,
        'minimum_initial_delivered_books':min(len(e.actor_books)for e in warm),
        'timings':timings,'serial_median_seconds':serial,'batched_median_seconds':batched,
        'collection_speedup':serial/batched,'identical_wallets_orders_fills_and_clocks':True,
        'tf32_enabled':False,
        'behavior_guard':args.guard,'encoder_activation_checkpointing':args.checkpoint_encoder,
        'start_offset_ms':args.start_offset_ms,
        'process_memory_fraction_cap':args.memory_fraction,
        'on_policy_guard_changed_after_optimizer_step':guard_changed,
        'recurrent_states_match_tolerance':True,'batch_ppo_update_seconds':elapsed,'ppo':result,
        'cuda_peak_allocated_bytes':torch.cuda.max_memory_allocated()if next(model.parameters()).is_cuda else 0,
        'http_requests':0,'independent_test_used':False,'production_changed':False,
        'dataset_manifest_sha256':experiment.dataset_sha,'compute_calibration':calibration}
    if compact_chunk is not None:
        compacted=statistics.median(x['compacted']for x in timings)
        out.update(compacted_median_seconds=compacted,finished_world_collection_speedup=batched/compacted,
                   compaction_changes_legal_actions_or_virtual_clocks=False)
    frozen(args.output/'RESULT.json',out);print(json.dumps(out),flush=True)
    experiment.close()


if __name__=='__main__':main()
