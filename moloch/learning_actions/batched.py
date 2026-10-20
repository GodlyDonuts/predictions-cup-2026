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
    values=[tensors(o,'cpu')for o in observations]
    n=max(v['contracts'].shape[1]for v in values)
    out={'contracts':torch.cat([F.pad(v['contracts'],(0,0,0,n-v['contracts'].shape[1]))for v in values]),
        'global_state':torch.cat([v['global_state']for v in values]),
        'present':torch.cat([F.pad(v['present'],(0,n-v['contracts'].shape[1]))for v in values]),
        'masks':{k:torch.cat([F.pad(v['masks'][k],(0,n-v['contracts'].shape[1])if k=='market'
                    else(0,0,0,n-v['contracts'].shape[1]))for v in values])for k in values[0]['masks']}}
    if torch.device(device).type!='cpu':
        from moloch.learning_v2.packed_io import transfer
        return transfer(out,device)
    return out


class BatchStream:
    def __init__(self,envs,model,ledgers,*,calibration=None,behavior_guard='sha256',compact_finished=False,rollout_only=False):
        if type(rollout_only)is not bool:raise ValueError('EXPLICIT_ROLLOUT_STATISTICS_PROFILE')
        self.rollout_only=rollout_only
        if not envs or len(envs)!=len(ledgers)or any(e.role!='train'for e in envs):
            raise ValueError('ONE_TRAIN_ROLE_LEDGER_PER_INDEPENDENT_WORLD_REQUIRED')
        if len({id(e)for e in envs})!=len(envs):raise ValueError('WORLDS_MUST_NOT_SHARE_MUTABLE_ACCOUNTS')
        from .state_encoding import matching
        for env in envs:matching(env,model)
        self.envs=envs;self.model=model;self.ledgers=ledgers
        self.device=next(model.parameters()).device
        self.hidden=model.initial_state(len(envs));self.decisions=[0]*len(envs)
        self.calibration=calibration or calibrate(envs[0],model)
        self.behavior_guard=behavior_guard
        if type(compact_finished)is not bool:raise ValueError('EXPLICIT_FINISHED_WORLD_COMPACTION')
        self.compact_finished=compact_finished
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
        method=self.model.step_rollout if self.rollout_only else self.model.step
        with torch.no_grad():out=method(**prepared,hidden=self.hidden,reset=reset,deterministic=deterministic)
        self.hidden=out['hidden']
        packed=None
        if getattr(self.model,'acting_validation','strict')=='consolidated_v1':
            from .acting_validation import actions_to_python
            packed=actions_to_python(out['actions'])
        else:actions={k:v.detach().cpu().tolist()for k,v in out['actions'].items()}
        rewards=[];terminals=[];elapsed=[]
        for i,(env,live,observation)in enumerate(zip(self.envs,active,observations)):
            if not live:rewards.append(0.);terminals.append(True);elapsed.append(0);continue
            before=env.now;action=packed[i]if packed is not None else{k:v[i]for k,v in actions.items()}
            env.advance(min(env.end,before+self.latency))
            if env.now<env.end:
                result=env.act_raw(action,observation)
                if result['status']=='ALARM':
                    from .managed_data_service import advance_alarm
                    advance_alarm(env,before,result)
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

    def collect(self,compute_steps=32,*,deterministic=False,bootstrap=True):
        if type(compute_steps)is not int or compute_steps<=0:raise ValueError('POSITIVE_COMPUTE_CHUNK')
        if type(bootstrap)is not bool:raise ValueError('EXPLICIT_CHUNK_BOOTSTRAP_PROFILE')
        live=[i for i,e in enumerate(self.envs)if e.now<e.end]
        if self.compact_finished and 0<len(live)<len(self.envs):
            # Freeze the batch membership for this entire PPO chunk. Worlds
            # finishing inside it retain terminal padding until the next chunk.
            # The view sends accepted optimizer hidden states back to the same
            # original world IDs; no world/account or clock is restarted.
            view=_ActiveWorldView(self,live)
            chunk=view.collect(compute_steps,deterministic=deterministic,bootstrap=bootstrap)
            for j,i in enumerate(live):
                self.decisions[i]=view.decisions[j]
                if j in view.completed:self.completed[i]=view.completed[j]
            return chunk
        from .behavior_guard import identity,contract
        before=identity(self.model,self.behavior_guard);frames=[]
        for _ in range(compute_steps):
            frame=self.step(deterministic=deterministic)
            if frame is None:break
            frames.append(frame)
        if not frames:raise ValueError('NO_ACTIVE_WORLD_FRAMES')
        value=None
        if bootstrap:
            with torch.no_grad():
                prepared=inputs([e.observe()for e in self.envs],self.device)
                kwargs={**prepared,'hidden':self.hidden,'reset':torch.zeros(len(self.envs),device=self.device,dtype=torch.bool)}
                value=(self.model.bootstrap_value(**kwargs)if self.rollout_only else self.model.step(**kwargs,deterministic=True)['value'])
                value=value*torch.tensor([e.now<e.end for e in self.envs],device=self.device)
        return Chunk(frames,before,'train',frames[0]['initial_hidden'],None if value is None else value.detach(),
                     self,self.behavior_guard,contract(self.model))


class _ActiveWorldView(BatchStream):
    """One chunk's gathered worlds with write-back for carried recurrent state."""
    def __init__(self,parent,indices):
        self.parent=parent;self.indices=torch.tensor(indices,device=parent.device,dtype=torch.long)
        self.envs=[parent.envs[i]for i in indices];self.ledgers=[parent.ledgers[i]for i in indices]
        self.model=parent.model;self.device=parent.device;self.calibration=parent.calibration
        self.behavior_guard=parent.behavior_guard;self.compact_finished=False;self.latency=parent.latency
        self.rollout_only=parent.rollout_only
        self.decisions=[parent.decisions[i]for i in indices]
        self.action_counts=[parent.action_counts[i]for i in indices];self.completed={}
        self._hidden=parent.hidden.index_select(0,self.indices)

    @property
    def hidden(self):return self._hidden

    @hidden.setter
    def hidden(self,value):
        self._hidden=value
        # Out-of-place write-back preserves hidden tensors already held by old
        # frames. In particular, an accepted PPO update must reach the parent.
        self.parent.hidden=self.parent.hidden.index_copy(0,self.indices,value)
