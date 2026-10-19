"""GPU-batched counterfactual worlds, independent clocks/wallets/API budgets.

Batch size is compute organization, never decision frequency or capital limits.
Finished worlds are masked out of learning; their zeros are padding, not invented
wealth observations. Continuing worlds retain hidden state across compute chunks.
"""
import torch
import torch.nn.functional as F
from collections import Counter
from .stream import tensors,calibrate
from .training import Chunk
from moloch.learning_v2.train_inner import fingerprint


def inputs(observations,device):
    values=[tensors(o,device)for o in observations]
    n=max(v['contracts'].shape[1]for v in values)
    return {'contracts':torch.cat([F.pad(v['contracts'],(0,0,0,n-v['contracts'].shape[1]))for v in values]),
        'global_state':torch.cat([v['global_state']for v in values]),
        'present':torch.cat([F.pad(v['present'],(0,n-v['contracts'].shape[1]))for v in values]),
        'masks':{k:torch.cat([F.pad(v['masks'][k],(0,n-v['contracts'].shape[1])if k=='market'
                    else(0,0,0,n-v['contracts'].shape[1]))for v in values])for k in ('market','family')}}


class BatchStream:
    def __init__(self,envs,model,ledgers,*,calibration=None,behavior_guard='sha256'):
        if not envs or len(envs)!=len(ledgers)or any(e.role!='train'for e in envs):
            raise ValueError('ONE_TRAIN_ROLE_LEDGER_PER_INDEPENDENT_WORLD_REQUIRED')
        if len({id(e)for e in envs})!=len(envs):raise ValueError('WORLDS_MUST_NOT_SHARE_MUTABLE_ACCOUNTS')
        self.envs=envs;self.model=model;self.ledgers=ledgers
        self.device=next(model.parameters()).device
        self.hidden=model.initial_state(len(envs));self.decisions=[0]*len(envs)
        self.calibration=calibration or calibrate(envs[0],model)
        self.behavior_guard=behavior_guard
        self.latency=self.calibration['decision_latency_ms']
        self.completed={};self.action_counts=[Counter()for e in envs]

    def step(self,*,deterministic=False):
        active=[e.now<e.end for e in self.envs]
        if not any(active):return None
        for e,live in zip(self.envs,active):
            if live:e.advance(e.now)
        observations=[e.observe()for e in self.envs]
        prepared=inputs(observations,self.device)
        reset=torch.tensor([n==0 or not live for n,live in zip(self.decisions,active)],device=self.device)
        initial=self.hidden
        with torch.no_grad():out=self.model.step(**prepared,hidden=self.hidden,reset=reset,deterministic=deterministic)
        self.hidden=out['hidden']
        actions={k:v.detach().cpu().tolist()for k,v in out['actions'].items()}
        rewards=[];terminals=[];elapsed=[]
        for i,(env,live,observation)in enumerate(zip(self.envs,active,observations)):
            if not live:rewards.append(0.);terminals.append(True);elapsed.append(0);continue
            before=env.now;action={k:v[i]for k,v in actions.items()}
            env.advance(min(env.end,before+self.latency))
            if env.now<env.end:
                result=env.act_raw(action,observation)
                if result['status']=='ALARM':
                    target=env.decision_times(result.get('ready_ms'))
                    env.advance_until_observable(min(env.end,max(before+1,target)))
            reward=self.ledgers[i].transition()
            if env.now>=env.end:
                final=self.ledgers[i].finish();reward+=final['terminal_reward_correction']
                self.completed[i]=final
            self.decisions[i]+=1;self.action_counts[i][int(action['family'])]+=1
            rewards.append(reward);terminals.append(env.now>=env.end);elapsed.append(env.now-before)
        return {**prepared,'reset':reset,'initial_hidden':initial.detach(),'actions':out['actions'],
            'old_logprob':out['logprob'],'old_value':out['value'],
            'terminal':torch.tensor(terminals,device=self.device),
            'learn':torch.tensor(active,device=self.device),'economic_reward':rewards,'elapsed_ms':elapsed}

    def collect(self,compute_steps=32,*,deterministic=False):
        if type(compute_steps)is not int or compute_steps<=0:raise ValueError('POSITIVE_COMPUTE_CHUNK')
        from .behavior_guard import identity
        before=identity(self.model,self.behavior_guard);frames=[]
        for _ in range(compute_steps):
            frame=self.step(deterministic=deterministic)
            if frame is None:break
            frames.append(frame)
        if not frames:raise ValueError('NO_ACTIVE_WORLD_FRAMES')
        with torch.no_grad():
            prepared=inputs([e.observe()for e in self.envs],self.device)
            value=self.model.step(**prepared,hidden=self.hidden,
                reset=torch.zeros(len(self.envs),device=self.device,dtype=torch.bool),deterministic=True)['value']
            value=value*torch.tensor([e.now<e.end for e in self.envs],device=self.device)
        return Chunk(frames,before,'train',frames[0]['initial_hidden'],value.detach(),self,self.behavior_guard)
