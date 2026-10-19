"""Persistent CPU replay processes feeding one CUDA inference coordinator."""
import queue
import time
import traceback
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
import torch
import torch.multiprocessing as mp
from .episodes import Tape
from .run_experiment import rollout
from .batched_rollout import combine
from .train_inner import fingerprint
from .packed_io import transfer,ipc_pack,ipc_unpack


class ProcessProxy:
    def __init__(self,index,state,shape,requests,replies,stop):
        self.index=index;self.state=state;self.contract_features,self.global_features,self.hidden_size=shape
        self.requests=requests;self.replies=replies;self.stop=stop;self.anchor=torch.empty(0)
    def parameters(self):return iter([self.anchor])
    def state_dict(self):return self.state
    def initial_state(self,batch):return torch.zeros(batch,self.hidden_size)
    def step(self,**kwargs):
        self.requests.put(('step',self.index,ipc_pack(kwargs)))
        while not self.stop.is_set():
            try:return ipc_unpack(self.replies.get(timeout=.2))
            except queue.Empty:pass
        raise RuntimeError('CPU_REPLAY_POOL_STOPPED')


def worker(index,directory,expected_sha,jobs,requests,replies,stop):
    torch.set_num_threads(1)
    try:
        tape=Tape(Path(directory))
        if tape.manifest_sha256!=expected_sha:raise ValueError('ACTOR_DATASET_CHANGED')
        requests.put(('ready',index,None))
        while not stop.is_set():
            try:job=jobs.get(timeout=.2)
            except queue.Empty:continue
            if job is None:return
            state,shape,mechanics,protocol,assignments,baseline,role=job
            def run_world(world,start):
                try:
                    proxy=ProcessProxy(world,state,shape,requests,replies[world],stop)
                    result=rollout(tape,mechanics,protocol,proxy,start,baseline=baseline,split_role=role)
                    requests.put(('done',world,result))
                except BaseException:
                    requests.put(('failure',world,traceback.format_exc(limit=6)));stop.set();raise
            with ThreadPoolExecutor(max_workers=len(assignments))as pool:
                futures=[pool.submit(run_world,world,start)for world,start in assignments]
                for future in futures:future.result()
    except BaseException:
        requests.put(('failure',index,traceback.format_exc(limit=6)))


class ProcessRollouts:
    def __init__(self,directory,expected_sha,worlds,actor_processes=4):
        if not 1<=worlds<=64 or not 1<=actor_processes<=8:raise ValueError('BOUNDED_CPU_ACTOR_COUNT')
        self.worlds=worlds;self.actors=min(worlds,actor_processes);ctx=mp.get_context('spawn');self.stop=ctx.Event();self.requests=ctx.Queue()
        self.jobs=[ctx.Queue()for _ in range(self.actors)];self.replies=[ctx.Queue()for _ in range(worlds)]
        self.processes=[ctx.Process(target=worker,args=(i,str(directory),expected_sha,self.jobs[i],self.requests,self.replies,self.stop))for i in range(self.actors)]
        for p in self.processes:p.start()
        try:
            ready=set()
            while len(ready)<self.actors:
                kind,i,value=self.receive()
                if kind!='ready'or i in ready:raise ValueError('ACTOR_STARTUP_PROTOCOL')
                ready.add(i)
        except BaseException:self.close();raise

    def receive(self):
        while True:
            try:
                result=self.requests.get(timeout=.5)
                if result[0]=='failure':raise RuntimeError(result[2])
                return result
            except queue.Empty:
                dead=[p.pid for p in self.processes if not p.is_alive()]
                if dead:raise RuntimeError('CPU_REPLAY_ACTOR_EXITED:'+str(dead))

    def rollout(self,mechanics,protocol,model,starts,*,baseline=False,split_role='fit_training'):
        if len(starts)!=self.worlds:raise ValueError('EXACT_ACTOR_WORLD_COUNT')
        interval=protocol['decision_interval_ms'];span=protocol['recurrent_burnin_ms']+protocol['episode_length_ms']
        if interval<=0 or span%interval:raise ValueError('COMMON_ACTOR_BOUNDARIES')
        device=next(model.parameters()).device;before=fingerprint(model)
        state={k:v.detach().cpu().clone()for k,v in model.state_dict().items()};shape=(model.contract_features,model.global_features,model.hidden_size)
        begun=time.perf_counter();wait=0.;gpu_ms=0.
        for i in range(self.actors):
            assignments=[(world,start)for world,start in enumerate(starts)if world%self.actors==i]
            self.jobs[i].put((state,shape,mechanics,protocol,assignments,baseline,split_role))
        for _ in range(span//interval):
            at=time.perf_counter();batch=[]
            while len(batch)<self.worlds:
                kind,i,value=self.receive()
                if kind!='step':raise RuntimeError('ACTOR_ENDED_BEFORE_BATCH_BOUNDARY')
                batch.append((i,ipc_unpack(value)))
            batch.sort();wait+=time.perf_counter()-at
            if [i for i,_ in batch]!=list(range(self.worlds)):raise ValueError('ACTOR_IDENTITY')
            kwargs={k:torch.cat([v[k]for _,v in batch])for k in ['contracts','global_state','present','hidden','reset']}
            kwargs['masks']={k:torch.cat([v['masks'][k]for _,v in batch])for k in batch[0][1]['masks']};kwargs=transfer(kwargs,device)
            if device.type=='cuda':a=torch.cuda.Event(enable_timing=True);b=torch.cuda.Event(enable_timing=True);a.record()
            with torch.no_grad():out=model.step(**kwargs)
            if device.type=='cuda':b.record()
            host=transfer(out,'cpu')
            if device.type=='cuda':gpu_ms+=a.elapsed_time(b)
            for i in range(self.worlds):self.replies[i].put(ipc_pack({k:({n:v[i:i+1]for n,v in value.items()}if isinstance(value,dict)else value[i:i+1])for k,value in host.items()}))
        results={}
        while len(results)<self.worlds:
            kind,i,value=self.receive()
            if kind!='done'or i in results:raise ValueError('ACTOR_COMPLETION_PROTOCOL')
            results[i]=value
        if fingerprint(model)!=before:raise ValueError('ACTOR_POLICY_CHANGED')
        return combine([results[i][0]for i in range(self.worlds)],device),[results[i][1]for i in range(self.worlds)],{
            'batch_worlds':self.worlds,'cpu_actor_processes':self.actors,'wall_seconds':time.perf_counter()-begun,
            'coordinator_cuda_actor_ms':gpu_ms,'cpu_actor_barrier_seconds':wait,'actor_steps_per_world':span//interval}

    def close(self):
        self.stop.set()
        for q in self.jobs:q.put(None)
        for p in self.processes:p.join(timeout=2)
        for p in self.processes:
            if p.is_alive():p.terminate();p.join(timeout=2)
        for q in [self.requests,*self.jobs,*self.replies]:q.cancel_join_thread();q.close()
