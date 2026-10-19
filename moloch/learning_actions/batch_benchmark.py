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
    p.add_argument('--steps',type=int,default=32);args=p.parse_args()
    config=json.loads(args.config.read_text(encoding='utf-8-sig'))
    # Compare mathematical batching without TF32's shape-dependent rounding.
    torch.backends.cuda.matmul.allow_tf32=False;torch.backends.cudnn.allow_tf32=False
    experiment=Experiment(config,args.output);torch.manual_seed(191104)
    model=ApiPolicy(config.get('hidden_size',4096)).to(config.get('device','cuda'))
    with torch.no_grad():model.fractions.bias[2]=-3.;model.fractions.bias[3]=9.
    def worlds():
        a,b=experiment.train_bounds;start=a+120000
        return [experiment.world('train',start+i*500,start+i*500+60000)for i in range(args.worlds)]
    calibration=calibrate(worlds()[0],model)
    def sync():
        if next(model.parameters()).is_cuda:torch.cuda.synchronize()
    warm=worlds();prepared=inputs([e.observe()for e in warm],next(model.parameters()).device)
    with torch.no_grad():
        for _ in range(3):model.step(**prepared,hidden=model.initial_state(args.worlds),
            reset=torch.ones(args.worlds,device=next(model.parameters()).device,dtype=torch.bool),deterministic=True)
    timings=[];last_chunk=None
    for order in (('serial','batched'),('batched','serial')):
        states={};durations={}
        for mode in order:
            envs=worlds();ledgers=[GrowthLedger(e,development=True,historical_last_mark=True)for e in envs]
            sync();started=time.perf_counter()
            if mode=='serial':
                streams=[Stream(e,model,calibration=calibration,record_training=True)for e in envs]
                for stream,ledger in zip(streams,ledgers):
                    # Use the same deterministic neural actions as the batch.
                    for _ in range(args.steps):stream.step(deterministic=True);ledger.transition()
                hidden=torch.cat([s.hidden for s in streams])
            else:
                stream=BatchStream(envs,model,ledgers,calibration=calibration)
                last_chunk=stream.collect(args.steps,deterministic=True);hidden=stream.hidden
            sync();durations[mode]=time.perf_counter()-started
            states[mode]=([{'clock':e.now,'cash':e.cash,'lots':e.lots,'fills':e.api_fills,
                'api_id':e.api_id,'orders':repr(e.api_orders)}for e in envs],hidden.detach().clone())
        if states['serial'][0]!=states['batched'][0]:raise ValueError('BATCHED_FINANCIAL_OR_CLOCK_DIFFERENCE')
        torch.testing.assert_close(states['serial'][1],states['batched'][1],atol=1e-5,rtol=1e-4)
        timings.append(durations)
    optimizer=torch.optim.Adam(model.parameters(),lr=1e-6);sync();started=time.perf_counter()
    result=update(model,optimizer,last_chunk,[f['economic_reward']for f in last_chunk.frames],role='train',epochs=1)
    sync();elapsed=time.perf_counter()-started
    serial=statistics.median(x['serial']for x in timings);batched=statistics.median(x['batched']for x in timings)
    out={'classification':'controlled_inspected_history_raw_api_gpu_batch_benchmark_not_roi',
        'worlds':args.worlds,'steps_per_world':args.steps,'parameters':sum(p.numel()for p in model.parameters()),
        'timings':timings,'serial_median_seconds':serial,'batched_median_seconds':batched,
        'collection_speedup':serial/batched,'identical_wallets_orders_fills_and_clocks':True,
        'tf32_enabled':False,
        'recurrent_states_match_tolerance':True,'batch_ppo_update_seconds':elapsed,'ppo':result,
        'cuda_peak_allocated_bytes':torch.cuda.max_memory_allocated()if next(model.parameters()).is_cuda else 0,
        'http_requests':0,'independent_test_used':False,'production_changed':False,
        'dataset_manifest_sha256':experiment.dataset_sha,'compute_calibration':calibration}
    frozen(args.output/'RESULT.json',out);print(json.dumps(out),flush=True)


if __name__=='__main__':main()
