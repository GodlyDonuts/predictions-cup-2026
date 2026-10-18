"""Checkpoint-safe population operators and a provisional economic archive.

No mechanics setting belongs to a genome. Behavioral mutation control measures
the exact market/family categorical KL component on pinned training histories;
it also reports raw normalized action changes. This is not full joint-action
KL or calibrated projected/executed behavior distance.
"""
from collections import Counter
from dataclasses import replace
import math
import torch
from .policy import masked_categorical
from .train_inner import PPOTrainer, fingerprint

OPERATORS=('local','medium','large','head_reset','fresh')
OPERATOR_WEIGHTS=(.50,.25,.15,.05,.05)
KL_TARGETS={'local':.001,'medium':.01,'large':.1}


@torch.no_grad()
def probe(model,t,maximum_points=16):
    if t.split_role!='fit_training': raise ValueError('MUTATION_PROBES_ARE_TRAINING_ONLY')
    eligible=t.learn[:,0].nonzero().flatten().tolist()
    if not eligible: raise ValueError('NO_CAUSAL_TRAINING_PROBE')
    chosen=set(eligible[::max(1,math.ceil(len(eligible)/maximum_points))]);hidden=model.initial_state(t.contracts.shape[1])
    market=[];family=[];raw=[]
    for i in range(t.contracts.shape[0]):
        before=hidden;encoded,hidden=model.encode(t.contracts[i],t.global_state[i],t.present[i],hidden,t.reset[i])
        if i not in chosen: continue
        masks={k:v[i]for k,v in t.masks.items()}
        logits=(encoded*model.market_query(hidden).unsqueeze(1)).sum(-1)/16
        pm=masked_categorical(logits,masks['market']&t.present[i]).probs
        conditioned=torch.cat([hidden.unsqueeze(1).expand(-1,encoded.shape[1],-1),encoded],-1)
        pf=masked_categorical(model.heads['family'](conditioned),masks['family']).probs
        a=model.step(t.contracts[i],t.global_state[i],t.present[i],masks,before,t.reset[i],deterministic=True)['actions']
        market.append(pm.cpu());family.append(pf.cpu())
        raw.append(torch.stack([a['target_fraction'],a['child_fraction'],a['price']/4,a['patience']/2],-1).cpu())
    return {'market':torch.stack(market),'family':torch.stack(family),'raw':torch.stack(raw)}


def distance(parent,child):
    def kl(p,q): return (p*(p.clamp_min(1e-20).log()-q.clamp_min(1e-20).log())).sum(-1)
    component=kl(parent['market'],child['market'])+(parent['market']*kl(parent['family'],child['family'])).sum(-1)
    return {'market_family_kl':max(0.,float(component.mean())),
            'raw_normalized_action_distance':float((parent['raw']-child['raw']).abs().mean()),
            'projected_execution_distance':None,'full_joint_action_kl_measured':False}


def mutate(trainer,t,operator,seed):
    if operator not in KL_TARGETS: raise ValueError('OUTPUT_CONTROLLED_MUTATION_OPERATOR')
    parent_sha=fingerprint(trainer.model)
    original={k:v.detach().clone()for k,v in trainer.model.state_dict().items()};parent=probe(trainer.model,t)
    g=torch.Generator(device=next(trainer.model.parameters()).device);g.manual_seed(seed)
    noise={}
    for name,p in trainer.model.named_parameters():
        if name.startswith(('value.','monetary_quantiles.','target_success.')): continue
        noise[name]=torch.randn(p.shape,device=p.device,generator=g)*max(float(p.detach().square().mean().sqrt()),.01)
    scale=.01;trials=[];best=None
    for _ in range(8):
        trainer.model.load_state_dict(original)
        with torch.no_grad():
            for name,p in trainer.model.named_parameters():
                if name in noise: p.add_(noise[name]*scale)
        measured=distance(parent,probe(trainer.model,t));kl=measured['market_family_kl']
        if not math.isfinite(kl+measured['raw_normalized_action_distance']):
            trainer.model.load_state_dict(original);raise ValueError('NONFINITE_MUTATION_BEHAVIOR')
        trials.append({'scale':scale,**measured})
        error=abs(math.log(max(kl,1e-12)/KL_TARGETS[operator]))
        if best is None or error<best[0]:best=(error,scale,measured)
        if .5*KL_TARGETS[operator]<=kl<=2*KL_TARGETS[operator]:break
        scale=min(10.,max(1e-6,scale*min(3.,max(.25,math.sqrt(KL_TARGETS[operator]/max(kl,1e-12))))))
    trainer.model.load_state_dict(original)
    with torch.no_grad():
        for name,p in trainer.model.named_parameters():
            if name in noise:p.add_(noise[name]*best[1])
    # Reset all optimizer moments after any actor change. Old trajectories are
    # rejected by PPOTrainer's exact full-parameter fingerprint.
    trainer.optimizer=torch.optim.Adam(trainer.model.parameters(),lr=trainer.config.learning_rate)
    return {'operator':operator,'seed':seed,'target_component_kl':KL_TARGETS[operator],
            'chosen_scale':best[1],**best[2],'trials':trials,'optimizer_moments_reset':True,
            'parent_parameter_sha256':parent_sha,'probe_trajectory_behavior_sha256':t.behavior_sha256,'child_parameter_sha256':fingerprint(trainer.model),
            'target_band_reached':.5*KL_TARGETS[operator]<=best[2]['market_family_kl']<=2*KL_TARGETS[operator]}


def change_hyperparameters(config,rng):
    # These are learning settings only, never cash caps, exchange time or fees.
    return replace(config,learning_rate=min(.001,max(.00001,config.learning_rate*rng.choice([.8,1.2]))),
                   entropy_coefficient=min(.1,max(.0001,config.entropy_coefficient*rng.choice([.8,1.2]))))


class BehaviorArchive:
    """Protected, reevaluated DEVELOPMENT alternatives, not validated profits."""
    def __init__(self,maximum=32):self.maximum=maximum;self.entries={}

    def consider(self,identity,parameter_sha,episodes,descriptor,checkpoint):
        if len(episodes)<2 or any(e.get('split_role')!='search_development'for e in episodes):
            raise ValueError('ARCHIVE_REQUIRES_REPEATED_SEARCH_EVALUATION')
        spans=sorted((e['start_ms'],e['terminal_ms'])for e in episodes)
        if any(a[1]>b[0]for a,b in zip(spans,spans[1:])):raise ValueError('ARCHIVE_EVALUATION_BLOCKS_OVERLAP')
        if any(e.get('parameter_sha256')!=parameter_sha for e in episodes):raise ValueError('ARCHIVE_POLICY_CHANGED_BETWEEN_EVALUATIONS')
        if any(not e['qualified_accounting'] or e['reference_return'] is None or e['reference_return']<0 or e.get('observable_market_frames',e.get('tradable_decision_frames',0))<=0 for e in episodes):
            return False
        # Fixed bins preserve different activity/capital use. Selection remains
        # development optimization and must not be reported as held-out alpha.
        key=tuple(descriptor);score=min(e['reference_return']for e in episodes)
        old=self.entries.get(key)
        if old is not None and old['worst_block_reference_return']>=score:return False
        entry={'identity':identity,'parameter_sha256':parameter_sha,'checkpoint':checkpoint,
               'descriptor':list(key),'worst_block_reference_return':score,'evaluations':episodes,
               'classification':'provisional_conditional_development_archive_not_locked_test'}
        if old is None and len(self.entries)>=self.maximum:return False
        self.entries[key]=entry;return True

    def json(self):return {'entries':list(self.entries.values()),'protected_from_weak_slot_replacement':True,
                           'locked_test_evaluated':False,'execution_authority':False}


def founder_counts(members):return dict(Counter(m['founder']for m in members))
