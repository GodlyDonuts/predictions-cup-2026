"""Frozen-policy evaluation with independent case RNGs and batched encoding.

No optimizer, training frames, intermediate wealth imputation or account client.
Model compute is batched; case clocks and API budgets remain independent.
"""
from collections import Counter
import torch
from .batched import inputs
from .behavior_guard import identity
from .neural_sessions import neural_section


class EvaluationBatch:
    def __init__(self,envs,model,seeds,*,calibration,batch_action_heads=False,actions_only=False):
        if type(actions_only)is not bool:raise ValueError('EXPLICIT_EVALUATION_STATISTICS_PROFILE')
        self.actions_only=actions_only
        if type(batch_action_heads)is not bool:raise ValueError('EXPLICIT_BATCH_ACTION_HEADS_PROFILE')
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
        self.generators=[torch.Generator(device=self.device).manual_seed(seed)for seed in seeds]
        self.rng=[g.get_state()for g in self.generators];self.batch_action_heads=batch_action_heads

    def get_rng(self):
        return torch.cuda.get_rng_state(self.device)if self.device.type=='cuda'else torch.get_rng_state()

    def set_rng(self,state):
        if self.device.type=='cuda':torch.cuda.set_rng_state(state,self.device)
        else:torch.set_rng_state(state)

    def step(self,*,deterministic=False):
        live=[i for i,e in enumerate(self.envs)if e.now<e.end]
        if not live:self.last_rows=[];return 0
        for i in live:self.envs[i].advance(self.envs[i].now)
        observed=[self.envs[i].observe()for i in live]
        with neural_section():
            prepared=inputs(observed,self.device)
            indices=torch.tensor(live,device=self.device)
            reset=torch.tensor([self.decisions[i]==0 for i in live],device=self.device)
            actions=[];global_rng=self.get_rng()
            try:
                with torch.no_grad():
                    encoded,h=self.model.encode(prepared['contracts'],prepared['global_state'],prepared['present'],
                                                self.hidden.index_select(0,indices),reset)
                    if self.batch_action_heads:
                        from .independent_sampling import IndependentSampling
                        sampler=IndependentSampling([self.generators[i]for i in live],[len(o['entities'])for o in observed])
                        out=self.model._act_from_encoding(encoded,h,prepared['present'],prepared['masks'],
                            deterministic=deterministic,sampler=sampler,statistics=not self.actions_only)
                        # One transfer per action head rather than a synchronization
                        # for every scalar of every case.
                        if getattr(self.model,'acting_validation','strict')=='consolidated_v1':
                            from .acting_validation import actions_to_python
                            actions=actions_to_python(out['actions'])
                        else:
                            values={k:v.tolist()for k,v in out['actions'].items()}
                            actions=[{k:v[j]for k,v in values.items()}for j in range(len(live))]
                        for i in live:self.rng[i]=self.generators[i].get_state()
                    for j,i in enumerate(live if not self.batch_action_heads else[]):
                        # Trim padding before sampling. Otherwise categorical RNG
                        # consumption could depend on another case's entity count.
                        n=len(observed[j]['entities']);self.set_rng(self.rng[i])
                        out=self.model._act_from_encoding(encoded[j:j+1,:n],h[j:j+1],prepared['present'][j:j+1,:n],
                            {k:v[j:j+1,:n]for k,v in prepared['masks'].items()},deterministic=deterministic,statistics=not self.actions_only)
                        self.rng[i]=self.get_rng()
                        if getattr(self.model,'acting_validation','strict')=='consolidated_v1':
                            from .acting_validation import actions_to_python
                            actions.append(actions_to_python(out['actions'])[0])
                        else:actions.append({k:v.item()for k,v in out['actions'].items()})
                    self.hidden=self.hidden.index_copy(0,indices,h)
            finally:self.set_rng(global_rng)
        self.last_rows=[]
        for j,i in enumerate(live):
            env=self.envs[i];before=env.now;action=actions[j];obs=observed[j]
            env.advance(min(env.end,before+self.latency))
            result=({'status':'HORIZON_REACHED_NO_DISPATCH'}if env.now>=env.end else env.act_raw(action,obs))
            if result['status']=='ALARM':
                from .managed_data_service import advance_alarm
                advance_alarm(env,before,result)
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
