"""One fixed policy's batched inference across independent exact replay worlds.

CPU simulation threads retain separate wallets/liquidity/budgets. Only the
coordinator uses CUDA. No account client or production code is imported.
"""
from concurrent.futures import ThreadPoolExecutor
from dataclasses import fields
import queue
import threading
import time
import torch
from .rollout import rollout
from moloch.learning_v2.train_inner import Trajectory,fingerprint
from moloch.learning_v2.packed_io import transfer


class Proxy:
    def __init__(self,model,index,requests,cancel):
        self.model=model;self.index=index;self.requests=requests;self.cancel=cancel
        self.contract_features=model.contract_features;self.global_features=model.global_features
        self.anchor=torch.empty(0);self.cached_state={k:v.detach().cpu().clone()for k,v in model.state_dict().items()}
    def parameters(self):return iter([self.anchor])
    def state_dict(self):return self.cached_state
    def initial_state(self,batch):return torch.zeros(batch,self.model.hidden_size)
    def step(self,**kwargs):
        done=threading.Event();reply={};self.requests.put((self.index,kwargs,done,reply))
        while not done.wait(.2):
            if self.cancel.is_set():raise RuntimeError('BATCH_COORDINATOR_ABORTED')
        return reply['value']


def combine(trajectories,device):
    if len({t.behavior_sha256 for t in trajectories})!=1 or len({t.split_manifest_sha256 for t in trajectories})!=1:raise ValueError('BATCH_MIXED_BEHAVIOR_OR_ROLES')
    values={}
    for f in fields(Trajectory):
        x=[getattr(t,f.name)for t in trajectories]
        if isinstance(x[0],torch.Tensor):values[f.name]=torch.cat(x,dim=1).to(device)
        elif isinstance(x[0],dict):values[f.name]={k:torch.cat([v[k]for v in x],dim=1).to(device)for k in x[0]}
        else:
            if any(v!=x[0]for v in x):raise ValueError('BATCH_PROVENANCE_DIFFERS')
            values[f.name]=x[0]
    return Trajectory(**values)


def rollout_batch(tape,mechanics,protocol,model,starts,*,baseline=False,split_role='fit_training'):
    if not starts:raise ValueError('EMPTY_BATCH')
    if protocol['decision_interval_ms']<=0 or (protocol['recurrent_burnin_ms']+protocol['episode_length_ms'])%protocol['decision_interval_ms']:
        raise ValueError('COMMON_COMPLETE_ACTOR_BOUNDARIES_REQUIRED')
    device=next(model.parameters()).device;before=fingerprint(model)
    requests=queue.Queue();cancel=threading.Event();count=len(starts)
    proxies=[Proxy(model,i,requests,cancel)for i in range(count)]
    actor_steps=(protocol['recurrent_burnin_ms']+protocol['episode_length_ms'])//protocol['decision_interval_ms']
    begun=time.perf_counter();gpu_ms=0.;cpu_wait_seconds=0.
    with ThreadPoolExecutor(max_workers=count)as pool:
        futures=[pool.submit(rollout,tape,mechanics,protocol,proxies[i],start,baseline=baseline,split_role=split_role)for i,start in enumerate(starts)]
        try:
            for _ in range(actor_steps):
                waiting=time.perf_counter();batch=[]
                while len(batch)<count:
                    try:batch.append(requests.get(timeout=.2))
                    except queue.Empty:
                        for future in futures:
                            if future.done():future.result();raise RuntimeError('ACTOR_ENDED_BEFORE_COMMON_BOUNDARY')
                batch.sort(key=lambda x:x[0]);cpu_wait_seconds+=time.perf_counter()-waiting
                if [x[0]for x in batch]!=list(range(count)):raise ValueError('DUPLICATE_OR_MISSING_ACTOR')
                names=['contracts','global_state','present','hidden','reset']
                kwargs={k:torch.cat([x[1][k]for x in batch])for k in names}
                kwargs['masks']={k:torch.cat([x[1]['masks'][k]for x in batch])for k in batch[0][1]['masks']};kwargs=transfer(kwargs,device)
                if device.type=='cuda':a=torch.cuda.Event(enable_timing=True);b=torch.cuda.Event(enable_timing=True);a.record()
                with torch.no_grad():output=model.step(**kwargs)
                if device.type=='cuda':b.record()
                host=transfer(output,'cpu')
                if device.type=='cuda':gpu_ms+=a.elapsed_time(b)
                for i,(_,_,done,reply)in enumerate(batch):
                    reply['value']={k:({n:v[i:i+1]for n,v in value.items()}if isinstance(value,dict)else value[i:i+1])for k,value in host.items()};done.set()
            results=[f.result()for f in futures]
        finally:cancel.set()
    if fingerprint(model)!=before:raise ValueError('BATCH_CHANGED_BEHAVIOR_POLICY')
    trajectory=combine([r[0]for r in results],device)
    return trajectory,[r[1]for r in results],{'batch_worlds':count,'wall_seconds':time.perf_counter()-begun,
        'coordinator_cuda_actor_ms':gpu_ms,'cpu_actor_barrier_seconds':cpu_wait_seconds,'actor_steps_per_world':actor_steps}
