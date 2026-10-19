"""Frozen-policy evaluation with independent case RNGs and batched encoding.

No optimizer, training frames, intermediate wealth imputation or account client.
Model compute is batched; case clocks and API budgets remain independent.
"""
from collections import Counter
import torch
from .batched import inputs
from .behavior_guard import identity


class EvaluationBatch:
    def __init__(self,envs,model,seeds,*,calibration):
        if not envs or len(envs)!=len(seeds)or len({id(e)for e in envs})!=len(envs):
            raise ValueError('ONE_INDEPENDENT_WORLD_AND_SEED_PER_EVALUATION_CASE')
        if any(type(seed)is not int for seed in seeds):raise ValueError('INTEGER_EVALUATION_SEEDS')
        from .state_encoding import matching
        for env in envs:matching(env,model)
        self.envs=envs;self.model=model;self.device=next(model.parameters()).device
        if self.device.type not in('cpu','cuda'):raise ValueError('SUPPORTED_EVALUATION_RNG_DEVICE')
        self.latency=calibration['decision_latency_ms']
        if type(self.latency)is not int or self.latency<=0:raise ValueError('NONZERO_MEASURED_COMPUTE_COST')
        self.before=identity(model,'tensor_versions_v1');self.hidden=model.initial_state(len(envs))
        self.decisions=[0]*len(envs)
        self.action_counts=[Counter()for _ in envs];self.status_counts=[Counter()for _ in envs]
        self.entity_counts=[Counter()for _ in envs];self.last_rows=[]
        self.rng=[torch.Generator(device=self.device).manual_seed(seed).get_state()for seed in seeds]

    def get_rng(self):
        return torch.cuda.get_rng_state(self.device)if self.device.type=='cuda'else torch.get_rng_state()

    def set_rng(self,state):
        if self.device.type=='cuda':torch.cuda.set_rng_state(state,self.device)
        else:torch.set_rng_state(state)

    def step(self,*,deterministic=False):
        live=[i for i,e in enumerate(self.envs)if e.now<e.end]
        if not live:self.last_rows=[];return 0
        for i in live:self.envs[i].advance(self.envs[i].now)
        observed=[self.envs[i].observe()for i in live];prepared=inputs(observed,self.device)
        indices=torch.tensor(live,device=self.device)
        reset=torch.tensor([self.decisions[i]==0 for i in live],device=self.device)
        actions=[];global_rng=self.get_rng()
        try:
            with torch.no_grad():
                encoded,h=self.model.encode(prepared['contracts'],prepared['global_state'],prepared['present'],
                                            self.hidden.index_select(0,indices),reset)
                for j,i in enumerate(live):
                    # Trim padding before sampling. Otherwise categorical RNG
                    # consumption could depend on another case's entity count.
                    n=len(observed[j]['entities']);self.set_rng(self.rng[i])
                    out=self.model._act_from_encoding(encoded[j:j+1,:n],h[j:j+1],prepared['present'][j:j+1,:n],
                        {k:v[j:j+1,:n]for k,v in prepared['masks'].items()},deterministic=deterministic)
                    self.rng[i]=self.get_rng()
                    actions.append({k:v.item()for k,v in out['actions'].items()})
                self.hidden=self.hidden.index_copy(0,indices,h)
        finally:self.set_rng(global_rng)
        self.last_rows=[]
        for j,i in enumerate(live):
            env=self.envs[i];before=env.now;action=actions[j];obs=observed[j]
            env.advance(min(env.end,before+self.latency))
            result=({'status':'HORIZON_REACHED_NO_DISPATCH'}if env.now>=env.end else env.act_raw(action,obs))
            if result['status']=='ALARM':
                target=env.decision_times(result.get('ready_ms'))
                env.advance_until_observable(min(env.end,max(before+1,target)))
            self.decisions[i]+=1;self.action_counts[i][int(action['family'])]+=1
            self.status_counts[i][result['status']]+=1;self.entity_counts[i][obs['entities'][int(action['market'])][0]]+=1
            self.last_rows.append({'case':i,'at_ms':before,'next_ms':env.now,'action':action,
                                   'status':result['status'],'terminal':env.now>=env.end})
        return len(live)

    def run_chunk(self,decisions=128,*,deterministic=False):
        if type(decisions)is not int or decisions<=0:raise ValueError('POSITIVE_EVALUATION_COMPUTE_CHUNK')
        if identity(self.model,'tensor_versions_v1')!=self.before:raise ValueError('FROZEN_EVALUATION_POLICY_CHANGED')
        total=0
        for _ in range(decisions):
            n=self.step(deterministic=deterministic);total+=n
            if not n:break
        return total
