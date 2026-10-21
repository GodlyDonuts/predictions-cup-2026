"""Spawned CPU-only simulated TRAIN worlds; the parent alone owns the model.

No account transport, automatic retry, world restart, altered virtual clock or
asynchronous PPO. Every batch waits for its original independent transitions.
Private pipes carry trusted local source observations, never execution rights.
"""
import atexit
from dataclasses import dataclass
import multiprocessing as mp
import os
import time
from pathlib import Path
from .selection import DataUse

PROFILES=('global_state_encoding','observation_protocol','read_target_sampling',
          'historical_memory_profile','managed_data_profile','price_parameterization')


class WaitingDataUse(DataUse):
    """Serialize spawned startup admissions through the original OS lock.

    Only its exact busy result is waited on. Protection, source, chronology and
    test-reservation failures propagate unchanged; no reservation is bypassed.
    """
    def record(self,*args,**kwargs):
        while True:
            try:return super().record(*args,**kwargs)
            except ValueError as error:
                if str(error)!='DATA_USE_REGISTRY_BUSY':raise
                time.sleep(.02)


def metadata(env):
    from .world_step import details
    return {'now':env.now,'end':env.end,'role':env.role,'details':details(env),
            'profiles':{key:getattr(env,key)for key in PROFILES if hasattr(env,key)}}


def _worker(pipe,factory,args):
    owner=None
    try:
        env,ledger,owner=factory(*args)
        if env.role!='train':raise ValueError('PROCESS_WORLD_MUST_BE_TRAIN_ROLE')
        import torch
        if torch.cuda.is_initialized():raise ValueError('SOURCE_WORKER_MUST_NOT_OWN_CUDA')
        pipe.send(('ok',{'pid':os.getpid(),'cudaInitialized':False,'state':metadata(env)}))
        prepared=None;epoch=0
        while True:
            command,payload=pipe.recv()
            if command=='close':break
            if command=='prepare':
                if prepared is not None:raise ValueError('UNCONSUMED_PROCESS_WORLD_ACTION')
                if payload!=(env.now<env.end):raise ValueError('PROCESS_WORLD_ACTIVE_STATE_CHANGED')
                if payload:env.advance(env.now)
                prepared=env.observe();epoch+=1
                result={'observation':prepared,'epoch':epoch,'state':metadata(env)}
            elif command=='transition':
                number,action,latency=payload
                if prepared is None or number!=epoch:raise ValueError('EXACT_PREPARED_PROCESS_WORLD_REQUIRED')
                if type(latency)is not int or latency<=0:raise ValueError('MEASURED_NONZERO_ACTOR_COST_REQUIRED')
                from .world_step import transition
                result=transition(env,ledger,action,prepared,latency)
                prepared=None;result['state']=metadata(env)
            elif command=='discard_padding':
                if prepared is None or env.now<env.end:raise ValueError('ONLY_FINISHED_WORLD_PADDING')
                prepared=None;result={'state':metadata(env)}
            elif command=='observe':
                if prepared is not None:raise ValueError('PROCESS_ACTION_PENDING')
                result={'observation':env.observe(),'state':metadata(env)}
            elif command=='audit':
                if prepared is not None:raise ValueError('PROCESS_ACTION_PENDING')
                from .episode_tape import financial_digest,tensor_digest
                from .stream import tensors
                result={'financialSha256':financial_digest(env),
                        'observationSha256':tensor_digest(tensors(env.observe(),'cpu')),
                        'feedRequests':list(env.feed_requests),'feedReceipts':list(env.feed_receipts),
                        'paidRequests':dict(env.feed_request_counts),'state':metadata(env)}
            else:raise ValueError('UNKNOWN_PROCESS_WORLD_COMMAND')
            pipe.send(('ok',result))
    except (EOFError,BrokenPipeError):pass
    except BaseException as error:
        try:pipe.send(('error',{'type':type(error).__name__,'reason':str(error)}))
        except (EOFError,BrokenPipeError,OSError):pass
    finally:
        try:
            if 'env'in locals():
                from .episode_training import release_worlds
                release_worlds([env])
            if owner is not None:owner.close()
        finally:pipe.close()


class WorldView:
    """Metadata and exact observation RPC; never a pretend local wallet."""
    def __init__(self,cohort,index,state):
        self.cohort=cohort;self.index=index;self.epoch=None;self.refresh(state)

    def refresh(self,state):
        self.now=state['now'];self.end=state['end'];self.role=state['role']
        self.details=state['details']
        for name,value in state['profiles'].items():setattr(self,name,value)

    def observe(self):
        value=self.cohort.request(self,'observe',None);return value['observation']


class ProcessCohort:
    def __init__(self,factories):
        if not factories:raise ValueError('NONEMPTY_PROCESS_WORLD_COHORT')
        self.handles=[];self.closed=False;self.failed=False;self.worlds=[];self.identities=[]
        context=mp.get_context('spawn')
        try:
            # Start every independent worker before receiving source setup.
            for factory,args in factories:
                parent,child=context.Pipe()
                process=context.Process(target=_worker,args=(child,factory,args),daemon=True)
                process.start();child.close();self.handles.append((process,parent))
            for index in range(len(self.handles)):
                ready=self.receive(index)
                if ready['pid']!=self.handles[index][0].pid or ready['cudaInitialized']is not False:
                    raise ValueError('EXACT_CPU_ONLY_SOURCE_PROCESS_REQUIRED')
                self.identities.append({k:ready[k]for k in('pid','cudaInitialized')})
                self.worlds.append(WorldView(self,index,ready['state']))
            if len({x['pid']for x in self.identities})!=len(factories):raise ValueError('INDEPENDENT_SOURCE_PROCESSES_REQUIRED')
            atexit.register(self.close)
        except BaseException:
            self.close();raise

    def receive(self,index):
        process,pipe=self.handles[index]
        # A long simulated wait is not a dead process. There is no observation
        # timeout or restart. A missing worker makes the cohort unusable.
        while not pipe.poll(.2):
            if not process.is_alive():raise RuntimeError('SOURCE_WORLD_PROCESS_EXITED')
        try:status,value=pipe.recv()
        except (EOFError,OSError)as error:raise RuntimeError('SOURCE_WORLD_PIPE_CLOSED')from error
        if status!='ok':raise RuntimeError('SOURCE_WORLD_FAILED: '+value['type']+': '+value['reason'])
        return value

    def exchange(self,worlds,commands):
        if self.closed:raise ValueError('CLOSED_PROCESS_WORLD_COHORT')
        if self.failed:raise ValueError('FAILED_PROCESS_COHORT_CANNOT_RETRY_TRANSITIONS')
        if len(worlds)!=len(commands)or len({v.index for v in worlds})!=len(worlds):
            raise ValueError('ONE_COMMAND_PER_DISTINCT_WORLD')
        if any(view.cohort is not self for view in worlds):raise ValueError('FOREIGN_PROCESS_WORLD')
        try:
            for view,command in zip(worlds,commands):self.handles[view.index][1].send(command)
            results=[]
            for view in worlds:
                value=self.receive(view.index)
                if 'state'in value:view.refresh(value['state'])
                results.append(value)
            return results
        except BaseException:
            # Some other world may have completed its transition already.
            # Never replay the batch, fabricate a replacement or keep training
            # from a mixture of acknowledged and unknown simulation states.
            self.failed=True;raise

    def request(self,view,command,payload):return self.exchange([view],[(command,payload)])[0]

    def prepare(self,worlds,active):
        rows=self.exchange(worlds,[('prepare',live)for live in active])
        for view,row in zip(worlds,rows):view.epoch=row['epoch']
        return [row['observation']for row in rows]

    def transitions(self,worlds,active,actions,latency):
        commands=[('transition',(w.epoch,a,latency))if live else('discard_padding',None)
                  for w,live,a in zip(worlds,active,actions)]
        rows=self.exchange(worlds,commands)
        return [row if live else {'reward':0.,'terminal':True,'elapsed_ms':0,'final':None}
                for row,live in zip(rows,active)]

    def close(self):
        if self.closed:return
        self.closed=True
        for process,pipe in self.handles:
            try:pipe.send(('close',None))
            except (OSError,BrokenPipeError):pass
        for process,pipe in self.handles:
            process.join(5)
            if process.is_alive():
                # Explicit abandonment after parent failure/close, never a
                # timeout-based restart or a resumable financial checkpoint.
                process.terminate();process.join()
            pipe.close()
        atexit.unregister(self.close)


@dataclass(frozen=True)
class ExperimentFactory:
    config:dict
    directory:str
    start:int
    end:int

    def __call__(self):
        from .experiment import Experiment
        from .training_reward import ledger
        # Device/thread changes describe offline source compute only. Virtual
        # mechanics, role calendars and all economic configuration stay intact.
        config=dict(self.config,device='cpu',cpu_threads=1,process_source_worlds=False)
        experiment=Experiment(config,Path(self.directory),data_use=WaitingDataUse())
        try:
            if not experiment.train_bounds[0]<=self.start<self.end<=experiment.train_bounds[1]:
                raise ValueError('PROCESS_WORLD_OUTSIDE_AUTHORIZED_TRAIN_CALENDAR')
            env=experiment.world('train',self.start,self.end)
            return env,ledger(env,config,development=experiment.development),experiment
        except BaseException:experiment.close();raise


def prime_books(env,count):
    """Charged reads for an explicit capacity diagnostic, never free prices."""
    if type(count)is not int or count<0:raise ValueError('NONNEGATIVE_CAPACITY_PRIMING_READS')
    targets=[key for key in env.keys['SIG']if key[0]=='book'][:count]
    if len(targets)!=count:raise ValueError('CAPACITY_PRIMING_TARGETS_UNAVAILABLE')
    for key in targets:
        while True:
            result=env.request_observation('SIG',key)
            if result['status']=='IN_FLIGHT':env.advance(env.busy_until);break
            if result['status']not in('BUSY','RATE_WAIT'):raise ValueError('CAPACITY_PRIMING_READ_FAILED')
            at=result['ready_ms']
            if not env.now<at<env.end:raise ValueError('CAPACITY_PRIMING_EXCEEDS_DIAGNOSTIC_HORIZON')
            env.advance(at)


@dataclass(frozen=True)
class CapacityFactory:
    """Short allocation/optimizer probe. Cannot stand in for full-day training."""
    config:dict
    directory:str
    start:int
    end:int
    priming_reads:int=0

    def __call__(self):
        from .experiment import Experiment
        from .economics import GrowthLedger
        config=dict(self.config,device='cpu',cpu_threads=1,process_source_worlds=False)
        experiment=Experiment(config,Path(self.directory),data_use=WaitingDataUse())
        try:
            if not experiment.train_bounds[0]<=self.start<self.end<=experiment.train_bounds[1]:
                raise ValueError('CAPACITY_WORLD_OUTSIDE_AUTHORIZED_TRAIN_CALENDAR')
            env=experiment.world('train',self.start,self.end);prime_books(env,self.priming_reads)
            # This matches the existing allocation probe's explicit short
            # development ledger. It never creates complete-day fitness.
            ledger=GrowthLedger(env,development=True,historical_last_mark=True)
            return env,ledger,experiment
        except BaseException:experiment.close();raise
