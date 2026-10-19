"""Measured scalar/batched actor comparison on inspected data; no live orders."""
import argparse
import hashlib
import json
from pathlib import Path
from dataclasses import replace
import statistics
import time
import subprocess
import torch
from moloch.learning_flow.episodes import FlowTape
from moloch.learning_flow.policy import FlowPolicy
from moloch.learning_v2.run_experiment import load_mechanics
from moloch.learning_v2.train_inner import PPOTrainer,PPOConfig
from .fast_replay import PreparedTape
from .protocol import Split,DAY
from .vector_rollout import collect


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--dataset',type=Path,required=True);p.add_argument('--mechanics',type=Path,required=True)
    p.add_argument('--output',type=Path,required=True);p.add_argument('--worlds',type=int,default=8)
    p.add_argument('--steps',type=int,default=60);p.add_argument('--hidden-size',type=int,default=1024)
    p.add_argument('--market-hidden-size',type=int,default=64);p.add_argument('--repeats',type=int,default=3)
    p.add_argument('--device',choices=['cpu','cuda'],default='cuda');p.add_argument('--ppo-probe',action='store_true')
    args=p.parse_args()
    if args.output.exists() or not 1<=args.repeats<=5: raise ValueError('NEW_FINITE_BENCHMARK_OUTPUT_REQUIRED')
    torch.manual_seed(1901);torch.set_num_threads(2)
    background_cuda_pids=[]
    if args.device=='cuda':
        state=subprocess.run(['nvidia-smi','--query-compute-apps=pid','--format=csv,noheader'],capture_output=True,text=True)
        background_cuda_pids=[int(s)for s in state.stdout.splitlines()if s.strip().isdigit()]
    begun=time.perf_counter();original=FlowTape(args.dataset);load_seconds=time.perf_counter()-begun
    # Previously inspected engineering data; synthetic split is not held-out.
    split=Split(original.times[0],original.times[-1]+9*DAY)
    begun=time.perf_counter();tape=PreparedTape.from_loaded(original,split=split,role='train')
    prepare_seconds=time.perf_counter()-begun
    mechanics=load_mechanics(args.mechanics)
    model=FlowPolicy(len(tape.exchanges),args.hidden_size,args.market_hidden_size).to(args.device)
    starts=[original.times[0]+10000+i*1000 for i in range(args.worlds)]
    common={'steps':args.steps,'burnin_steps':5,'drain_ms':10000,'deterministic':True,'capture_trajectory':False}
    # Warm CUDA/cuDNN before measuring either mode.
    collect(tape,mechanics,model,starts,**{**common,'steps':8,'burnin_steps':1})
    results={};reference=None;maximum_fraction_difference=0.
    for mode in ('scalar','batched'):
        reports=[]
        for _ in range(args.repeats):
            _,report=collect(tape,mechanics,model,starts,inference_mode=mode,**common)
            comparable=(report['deterministic_action_trace'],report['worlds_summary'],report['unresolved_worlds'])
            if reference is None:reference=comparable
            else:
                if comparable[1:]!=reference[1:]:raise ValueError('SCALAR_BATCHED_ACCOUNTING_DIFFERENTIAL')
                for actual,expected in zip(comparable[0],reference[0]):
                    for key in actual:
                        if key.endswith('fraction'):
                            delta=max(abs(a-b)for a,b in zip(actual[key],expected[key]))
                            maximum_fraction_difference=max(maximum_fraction_difference,delta)
                            if delta>0.0001:raise ValueError(f'SCALAR_BATCHED_FRACTION_DIFFERENTIAL: {key} delta={delta}')
                        elif actual[key]!=expected[key]:raise ValueError('SCALAR_BATCHED_DISCRETE_ACTION_DIFFERENTIAL')
            report.pop('deterministic_action_hashes');report.pop('deterministic_action_trace');report.pop('worlds_summary')
            reports.append(report)
        results[mode]={'runs':reports,'median_seconds':statistics.median(r['wall_seconds']for r in reports)}
    probe=None
    if args.ppo_probe:
        if args.device=='cuda':torch.cuda.reset_peak_memory_stats()
        begun=time.perf_counter()
        trajectory,report=collect(tape,mechanics,model,starts,steps=args.steps,burnin_steps=5,
            drain_ms=10000,capture_trajectory=True,split_manifest_sha256='a'*64)
        eligible=torch.tensor([w['qualified_relative_interval_accounting']for w in report['worlds_summary']],device=args.device)
        trajectory=replace(trajectory,learn=trajectory.learn&eligible.unsqueeze(0))
        rollout_seconds=time.perf_counter()-begun
        trainer=PPOTrainer(model,PPOConfig(epochs=1),trajectory.split_manifest_sha256)
        if args.device=='cuda':torch.cuda.synchronize()
        begun=time.perf_counter();updates=trainer.update(trajectory)
        if args.device=='cuda':torch.cuda.synchronize()
        probe={'rollout_seconds':rollout_seconds,'optimizer_seconds':time.perf_counter()-begun,
            'accepted_optimizer_updates':trainer.accepted_optimizer_updates,
            'learning_decisions':trainer.training_decisions,'updates':updates,
            'qualified_worlds':int(eligible.sum()),
            'peak_cuda_allocated_bytes':torch.cuda.max_memory_allocated()if args.device=='cuda'else None,
            'profitability_established':False,'new_daily_reward_used':False}
    out={'classification':'inspected_history_software_throughput_not_trading_performance',
        'dataset_manifest_sha256':tape.manifest_sha256,'device':args.device,
        'gpu':torch.cuda.get_device_name()if args.device=='cuda'else None,
        'parameter_count':sum(p.numel()for p in model.parameters()),'worlds':args.worlds,'steps':args.steps,
        'repeats':args.repeats,'warmup_runs':1,'decode_seconds_once':load_seconds,'prepare_seconds_once':prepare_seconds,
        'results':results,'speedup':results['scalar']['median_seconds']/results['batched']['median_seconds'],
        'deterministic_discrete_actions_and_accounting_match':True,
        'maximum_fraction_difference':maximum_fraction_difference,'ppo_probe':probe,
        'new_information_created_by_repetition':False,'production_changed':False,'independent_test_used':False}
    out['background_cuda_pids_before_benchmark']=background_cuda_pids
    out['isolated_gpu_benchmark']=not background_cuda_pids
    out['source_sha256']={p.name:hashlib.sha256(p.read_bytes()).hexdigest()
        for p in Path(__file__).parent.glob('*.py')if not p.name.startswith('test_')}
    args.output.parent.mkdir(parents=True,exist_ok=True)
    with args.output.open('x',encoding='utf8')as f:json.dump(out,f,indent=2,allow_nan=False)
    print(json.dumps({k:v for k,v in out.items()if k!='results'}))


if __name__=='__main__':main()
