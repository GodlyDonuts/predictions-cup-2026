"""Interleaved serial/batched frozen-model probe on inspected development data."""
import argparse,json,statistics,time
from pathlib import Path
import torch
from .experiment import Experiment
from .evaluation_batch import EvaluationBatch
from .stream import Stream,calibrate
from .training import load_checkpoint
from .selection import frozen


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for key in('config','output','checkpoint'):p.add_argument('--'+key,type=Path,required=True)
    p.add_argument('--checkpoint-sha256',required=True);p.add_argument('--steps',type=int,default=64)
    p.add_argument('--memory-fraction',type=float,default=.16);p.add_argument('--stochastic',action='store_true')
    args=p.parse_args();config=json.loads(args.config.read_text(encoding='utf-8-sig'))
    if config['mode']!='inspected_development':raise ValueError('BENCHMARK_CANNOT_OPEN_INDEPENDENT_TEST')
    if args.steps<=0:raise ValueError('POSITIVE_BENCHMARK_CHUNK')
    config['cuda_memory_fraction']=args.memory_fraction
    experiment=Experiment(config,args.output)
    model,bundle=load_checkpoint(args.checkpoint,args.checkpoint_sha256,experiment.device);del bundle
    if type(model)is not experiment.policy_type():
        if config.get('migrate_full_depth_encoder')is not True:raise ValueError('EXPLICIT_BENCHMARK_DEPTH_MIGRATION')
        from .full_depth import migrate_legacy_policy
        model,migration=migrate_legacy_policy(model)
    start,end=experiment.evolution_bounds;mid=(start+end)//2
    cases=[(a,b,s)for a,b in((start,mid),(mid,end))for s in(False,True)]
    seeds=[experiment.seed+90000+i for i in range(len(cases))]
    def worlds():return [experiment.world('evolution',a,b,None,s)for a,b,s in cases]
    calibration=calibrate(worlds()[0],model)
    def sync():
        if experiment.device.startswith('cuda'):torch.cuda.synchronize()
    def finance(e):return {'now':e.now,'cash':e.cash,'lots':e.lots,'fills':e.api_fills,
        'orders':repr(e.api_orders),'budget_reads':e.budget.reads,'budget_writes':e.budget.writes}
    timings=[];max_hidden_error=0.;max_action_fraction_error=0.;same_rng=True
    for order in(('serial','batched'),('batched','serial')):
        results={};durations={}
        for mode in order:
            envs=worlds();sync();began=time.perf_counter();traces=[];rng=[]
            if mode=='serial':
                hidden=[]
                for seed,env in zip(seeds,envs):
                    torch.manual_seed(seed);stream=Stream(env,model,calibration=calibration)
                    stream.run_chunk(args.steps,deterministic=not args.stochastic)
                    hidden.append(stream.hidden);traces.append(list(stream.rows))
                    rng.append(torch.cuda.get_rng_state(next(model.parameters()).device)if experiment.device.startswith('cuda')else torch.get_rng_state())
                hidden=torch.cat(hidden)
            else:
                batch=EvaluationBatch(envs,model,seeds,calibration=calibration);traces=[[]for _ in envs]
                for _ in range(args.steps):
                    if not batch.step(deterministic=not args.stochastic):break
                    for row in batch.last_rows:traces[row['case']].append(row)
                hidden=batch.hidden;rng=batch.rng
            sync();durations[mode]=time.perf_counter()-began
            results[mode]=([finance(e)for e in envs],hidden.detach().clone(),traces,rng)
        a,b=results['serial'],results['batched']
        if a[0]!=b[0]:raise ValueError('BATCHED_EVALUATION_FINANCIAL_OR_CLOCK_DIFFERENCE')
        torch.testing.assert_close(a[1],b[1],atol=1e-5,rtol=1e-4)
        max_hidden_error=max(max_hidden_error,float((a[1]-b[1]).abs().max()))
        for x,y in zip(a[2],b[2]):
            if len(x)!=len(y):raise ValueError('BATCHED_EVALUATION_TRACE_LENGTH')
            for u,v in zip(x,y):
                for key in('at_ms','next_ms','status','terminal'):
                    if u[key]!=v[key]:raise ValueError('BATCHED_EVALUATION_TIMELINE_DIFFERENCE')
                for key in u['action']:
                    if key.endswith('fraction'):
                        error=abs(u['action'][key]-v['action'][key]);max_action_fraction_error=max(max_action_fraction_error,error)
                        if error>1e-5:raise ValueError('BATCHED_EVALUATION_LATENT_DIFFERENCE')
                    elif u['action'][key]!=v['action'][key]:raise ValueError('BATCHED_EVALUATION_ACTION_DIFFERENCE')
        same_rng=same_rng and all(torch.equal(x,y)for x,y in zip(a[3],b[3]))
        timings.append(durations)
    serial=statistics.median(t['serial']for t in timings);batched=statistics.median(t['batched']for t in timings)
    result={'classification':'frozen_development_model_evaluation_compute_probe_not_roi',
        'cases':len(cases),'steps_per_case':args.steps,'stochastic':args.stochastic,
        'parameters':sum(p.numel()for p in model.parameters()),'contract_features':model.contract_features,
        'source_checkpoint_sha256':args.checkpoint_sha256,'timings':timings,'serial_median_seconds':serial,
        'batched_median_seconds':batched,'collection_speedup':serial/batched,'per_case_rng_states_match':same_rng,
        'max_hidden_absolute_error':max_hidden_error,'max_action_fraction_absolute_error':max_action_fraction_error,
        'identical_financial_states_orders_fills_clocks_and_discrete_actions':True,
        'valuation_not_replaced_or_imputed':True,'full_horizon_roi_measured':False,
        'cuda_peak_allocated_bytes':torch.cuda.max_memory_allocated()if experiment.device.startswith('cuda')else 0,
        'compute_calibration':calibration,'dataset_manifest_sha256':experiment.dataset_sha,
        'benchmark_experiment_sha256':experiment.spec_sha,'process_memory_fraction_cap':args.memory_fraction,
        'http_requests':0,'independent_test_used':False,'production_changed':False}
    if not same_rng:raise ValueError('PER_CASE_RNG_CONSUMPTION_DIFFERENCE')
    frozen(args.output/'RESULT.json',result);print(json.dumps(result),flush=True)


if __name__=='__main__':main()
