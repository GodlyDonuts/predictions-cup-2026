"""Measure real-history PPO update throughput against identical step semantics."""
import argparse
import json
from pathlib import Path
import statistics
import time
import types
import torch
from .episodes import CONTRACT_FEATURES,GLOBAL_FEATURES
from .policy import RecurrentPolicy
from .run_experiment import sha,write_json
from .train_inner import Trajectory,PPOConfig,PPOTrainer,fingerprint


def sequential(self,contracts,global_state,present,masks,reset,actions=None):
    hidden=self.initial_state(contracts.shape[1],device=contracts.device);rows=[]
    for i in range(contracts.shape[0]):
        row=self.step(contracts[i],global_state[i],present[i],{k:v[i]for k,v in masks.items()},hidden,reset[i],None if actions is None else{k:v[i]for k,v in actions.items()})
        hidden=row['hidden'];rows.append(row)
    return rows


def main():
    p=argparse.ArgumentParser();p.add_argument('--anchors',type=Path,required=True);p.add_argument('--output',type=Path,required=True)
    p.add_argument('--seed',type=int,default=7001);args=p.parse_args();torch.set_num_threads(4)
    t=Trajectory(**torch.load(args.anchors,map_location='cuda',weights_only=True))
    torch.manual_seed(args.seed);original=RecurrentPolicy(CONTRACT_FEATURES,GLOBAL_FEATURES).cuda()
    if fingerprint(original)!=t.behavior_sha256:raise ValueError('BENCHMARK_REQUIRES_EXACT_ORIGINAL_BEHAVIOR_WEIGHTS')
    initial={k:v.detach().clone()for k,v in original.state_dict().items()}
    with torch.no_grad():
        a=sequential(original,t.contracts,t.global_state,t.present,t.masks,t.reset,t.actions)
        b=original.sequence(t.contracts,t.global_state,t.present,t.masks,t.reset,t.actions)
        delta=float((torch.stack([r['logprob']for r in a])-torch.stack([r['logprob']for r in b])).abs().max())
    if delta>0.0001:raise ValueError('BATCHED_LIKELIHOOD_DIFFERENTIAL')
    del a,b;results={}
    for name in ['sequential_step','batched_encoder_heads']:
        timings=[];epochs=[]
        for repeat in range(4):
            model=RecurrentPolicy(CONTRACT_FEATURES,GLOBAL_FEATURES).cuda();model.load_state_dict(initial)
            if name=='sequential_step':model.sequence=types.MethodType(sequential,model)
            trainer=PPOTrainer(model,PPOConfig(epochs=2),t.split_manifest_sha256)
            torch.cuda.synchronize();at=time.perf_counter();out=trainer.update(t);torch.cuda.synchronize();elapsed=time.perf_counter()-at
            if repeat:timings.append(elapsed);epochs.append(out)
            del model,trainer
        results[name]={'warmup_rounds':1,'measured_rounds':3,'seconds':timings,'median_seconds':statistics.median(timings),'updates':epochs}
    report={'classification':'real_history_software_throughput_not_trading_performance','anchor_sha256':sha(args.anchors),
            'original_behavior_sha256':t.behavior_sha256,'frames':t.contracts.shape[0],'markets':t.contracts.shape[2],
            'maximum_initial_logprob_difference':delta,'results':results,
            'measured_update_speedup':results['sequential_step']['median_seconds']/results['batched_encoder_heads']['median_seconds'],
            'source_sha256':{n:sha(Path(__file__).with_name(n))for n in ['policy.py','train_inner.py','benchmark_sequence.py']},
            'device':torch.cuda.get_device_name(),'production_changed':False,'end_to_end_speedup_measured':False}
    write_json(args.output,report);print(json.dumps(report),flush=True)


if __name__=='__main__':main()
