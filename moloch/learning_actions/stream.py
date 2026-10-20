"""Event-driven decisions with resumable compute chunks, no decision-count cap.

Model planning/builder tokens can occur at the same virtual timestamp. API
requests consume actual modeled latency and the shared API rate window. A
chunk yields control/memory to the trainer; it never resets time or inventory.
"""
import time
import math
import torch
from collections import deque,Counter


def tensors(observation,device):
    out={k:torch.as_tensor(observation[k],device='cpu').unsqueeze(0)
         for k in ('contracts','global_state','present')}
    out['masks']={k:torch.as_tensor(v,device='cpu').unsqueeze(0)for k,v in observation['masks'].items()}
    if torch.device(device).type!='cpu':
        from moloch.learning_v2.packed_io import transfer
        return transfer(out,device)
    return out


def calibrate(environment,model,rounds=12):
    """Freeze measured local actor compute cost; never sleep on virtual time."""
    from .state_encoding import matching
    matching(environment,model)
    device=next(model.parameters()).device;inputs=tensors(environment.observe(),device)
    hidden=model.initial_state(1);reset=torch.tensor([True],device=device);measurements=[]
    with torch.no_grad():
        for n in range(rounds+3):
            if device.type=='cuda':torch.cuda.synchronize()
            started=time.perf_counter()
            model.step(**inputs,hidden=hidden,reset=reset,deterministic=True)
            if device.type=='cuda':torch.cuda.synchronize()
            if n>=3:measurements.append((time.perf_counter()-started)*1000)
    samples=sorted(measurements)
    return {'decision_latency_ms':max(1,math.ceil(samples[math.ceil(.95*len(samples))-1])),
            'measured_ms':measurements,'device':str(device),'assumption':'frozen_local_p95_compute_cost'}


class Stream:
    def __init__(self,environment,model,*,calibration=None,record_training=False,behavior_guard='sha256',actions_only=False,rollout_only=False):
        if type(actions_only)is not bool or(actions_only and record_training):raise ValueError('ACTIONS_ONLY_IS_EVALUATION_ONLY')
        if type(rollout_only)is not bool or(rollout_only and(not record_training or actions_only)):
            raise ValueError('COMPACT_ROLLOUT_REQUIRES_TRAINING_CAPTURE')
        self.rollout_only=rollout_only
        self.actions_only=actions_only
        from .state_encoding import matching
        matching(environment,model)
        self.env=environment;self.model=model;self.device=next(model.parameters()).device
        self.hidden=model.initial_state(1);self.decisions=0;self.rows=deque(maxlen=4096)
        self.calibration=calibration or calibrate(environment,model)
        self.record_training=record_training;self.last_frame=None
        self.behavior_guard=behavior_guard
        self.action_counts=Counter();self.status_counts=Counter();self.entity_counts=Counter()
        self.latency=self.calibration['decision_latency_ms']
        if type(self.latency)is not int or self.latency<=0:raise ValueError('NONZERO_MEASURED_COMPUTE_COST')

    def step(self,forced_action=None,deterministic=False):
        if self.env.now>=self.env.end:return {'terminal':True}
        self.env.advance(self.env.now)
        before=self.env.now;observed=self.env.observe();inputs=tensors(observed,self.device)
        reset=torch.tensor([self.decisions==0],device=self.device)
        initial_hidden=self.hidden
        method=self.model.step_rollout if self.rollout_only else self.model.step_actions_only if self.actions_only else self.model.step
        with torch.no_grad():out=method(**inputs,hidden=self.hidden,reset=reset,deterministic=deterministic)
        self.hidden=out['hidden']
        if forced_action is not None:action=forced_action
        elif getattr(self.model,'acting_validation','strict')=='consolidated_v1':
            from .acting_validation import actions_to_python
            action=actions_to_python(out['actions'])[0]
        else:action={k:v.item()for k,v in out['actions'].items()}
        self.env.advance(min(self.env.end,before+self.latency))
        result=({'status':'HORIZON_REACHED_NO_DISPATCH'}if self.env.now>=self.env.end
                else self.env.act_raw(action,observed))
        # Builder tokens and independent venue requests can be issued without
        # a fake 1-second pause. Only WAIT suspends the policy. A busy SIG queue
        # or exhausted write budget cannot prevent independent external reads
        # or planning at the next measured inference tick.
        if result['status']=='ALARM':
            from .managed_data_service import advance_alarm
            advance_alarm(self.env,before,result)
        self.decisions+=1
        self.action_counts[int(action['family'])]+=1
        self.status_counts[result['status']]+=1
        self.entity_counts[observed['entities'][int(action['market'])][0]]+=1
        row={'at_ms':before,'next_ms':self.env.now,'action':action,'status':result['status'],
             'reset':self.decisions==1,'terminal':self.env.now>=self.env.end}
        self.rows.append(row)
        if self.record_training:
            if forced_action is not None:raise ValueError('FORCED_FIXTURE_ACTIONS_ARE_NOT_POLICY_TRAINING')
            self.last_frame={**inputs,'reset':reset,'initial_hidden':initial_hidden.detach(),
                'actions':out['actions'],'old_logprob':out['logprob'],'old_value':out['value'],
                'elapsed_ms':self.env.now-before,'known_at_ms':self.env.now,'terminal':row['terminal']}
        return row

    def run_chunk(self,compute_decisions=256,*,deterministic=False,action_provider=None):
        if type(compute_decisions)is not int or compute_decisions<=0:raise ValueError('POSITIVE_COMPUTE_CHUNK')
        before=self.decisions;started=time.perf_counter()
        for _ in range(compute_decisions):
            action=action_provider(self)if action_provider is not None else None
            if self.step(action,deterministic=deterministic).get('terminal'):break
        return {'chunk_decisions':self.decisions-before,'total_decisions':self.decisions,
                'virtual_ms':self.env.now,'wall_seconds':time.perf_counter()-started,
                'terminal':self.env.now>=self.env.end,'state_preserved':True}
