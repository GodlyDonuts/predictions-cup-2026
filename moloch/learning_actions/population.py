"""Raw-action population exploration; no exchange or evaluation-rule controls.

Mutation distance is the exact family/conditional-target categorical component
on a pinned TRAINING trajectory. Price/quantity/time distance is descriptive;
this is not full joint-action KL, executed behavior, or a financial risk limit.
Each policy recomputes its own recurrent history from the episode reset.
"""
from dataclasses import dataclass
from copy import deepcopy
import hashlib
import math
import random
import torch
from .policy import ApiPolicy
from moloch.learning_v2.policy import masked_categorical
from moloch.learning_v2.train_inner import fingerprint

OPERATORS=('local','medium','large','head_reset','fresh')
WEIGHTS=(.50,.25,.15,.05,.05)
KL_TARGETS={'local':.001,'medium':.01,'large':.1}


@dataclass(frozen=True)
class TrainingProbe:
    frames:tuple
    sha256:str
    dataset_sha256:str
    bounds:tuple


def freeze_probe(frames,*,role,dataset_sha256,bounds):
    if role!='train':raise ValueError('MUTATION_PROBE_MUST_BE_TRAINING_ONLY')
    if len(dataset_sha256)!=64 or any(c not in '0123456789abcdef'for c in dataset_sha256):
        raise ValueError('PINNED_PROBE_DATASET_REQUIRED')
    if not frames or len(bounds)!=2 or bounds[0]>=bounds[1]:raise ValueError('CAUSAL_PROBE_REQUIRED')
    digest=hashlib.sha256((dataset_sha256+repr(tuple(bounds))).encode());copied=[];previous=-1
    for index,frame in enumerate(frames):
        at=frame['at_ms']
        if not bounds[0]<=at<bounds[1]or at<previous:raise ValueError('PROBE_OUTSIDE_TRAINING_CALENDAR')
        previous=at;row={'at_ms':at,'masks':{}}
        for key in ('contracts','global_state','present','reset'):
            row[key]=frame[key].detach().cpu().clone()
        if row['contracts'].shape[0]!=1 or row['reset'].shape!=(1,):raise ValueError('ONE_CAUSAL_PROBE_HISTORY')
        if index==0 and not bool(row['reset'][0]):raise ValueError('PROBE_REQUIRES_EPISODE_RESET')
        for key in frame['masks']:row['masks'][key]=frame['masks'][key].detach().cpu().clone()
        digest.update(str(at).encode())
        for key,tensor in [(k,row[k])for k in ('contracts','global_state','present','reset')]+list(row['masks'].items()):
            digest.update((key+str(tensor.dtype)+repr(tuple(tensor.shape))).encode());digest.update(tensor.numpy().tobytes())
        copied.append(row)
    return TrainingProbe(tuple(copied),digest.hexdigest(),dataset_sha256,tuple(bounds))


@torch.no_grad()
def behavior(model,probe):
    if not isinstance(probe,TrainingProbe):raise ValueError('FROZEN_TRAINING_PROBE_REQUIRED')
    if freeze_probe(probe.frames,role='train',dataset_sha256=probe.dataset_sha256,bounds=probe.bounds).sha256!=probe.sha256:
        raise ValueError('FROZEN_TRAINING_PROBE_CHANGED')
    device=next(model.parameters()).device;hidden=model.initial_state(1);rows=[]
    for frame in probe.frames:
        x={k:frame[k].to(device)for k in ('contracts','global_state','present','reset')}
        masks={k:v.to(device)for k,v in frame['masks'].items()}
        encoded,hidden=model.encode(x['contracts'],x['global_state'],x['present'],hidden,x['reset'])
        present=x['present'];pooled=(encoded*present.unsqueeze(-1)).sum(1)/present.sum(1,keepdim=True)
        active=masks['family']&present.unsqueeze(-1)&masks['market'].unsqueeze(-1)
        enabled=active.any(1)
        family=masked_categorical(model.heads['family'](torch.cat((hidden,pooled),-1)),enabled).probs
        logits=(encoded*model.market_query(hidden).unsqueeze(1)).sum(-1)/16
        conditional=[]
        for f in range(12):
            target_mask=active[:,:,f].clone()
            # A disabled family has zero joint probability. Supply a harmless
            # target solely to define a normalized conditional distribution.
            if not bool(enabled[0,f]):target_mask[0,0]=True
            conditional.append(masked_categorical(logits,target_mask).probs)
        joint=family.unsqueeze(-1)*torch.stack(conditional,1)
        selected=model._act_from_encoding(encoded,hidden,present,masks,deterministic=True)['actions']
        raw=torch.stack((selected['target_fraction'],selected['child_fraction'],
                         selected['price'].double()/199,selected['patience'].double()),-1)
        if 'query_filter'in selected:
            raw=torch.cat((raw,torch.stack([selected[k].double()for k in
                ('query_filter','query_from_fraction','query_to_fraction')],-1)),-1)
        if not bool(torch.isfinite(joint).all()and torch.isfinite(raw).all()):raise ValueError('NONFINITE_MUTATION_PROBE')
        rows.append((joint.cpu().double(),raw.cpu()))
    return rows


def distance(parent,child):
    if len(parent)!=len(child)or not parent:raise ValueError('MATCHED_PROBE_HISTORIES_REQUIRED')
    categorical=[];raw=[]
    for (p,a),(q,b)in zip(parent,child):
        if p.shape!=q.shape:raise ValueError('MATCHED_PROBE_ACTION_DOMAIN_REQUIRED')
        categorical.append((p*(p.clamp_min(1e-300).log()-q.clamp_min(1e-300).log())).sum())
        raw.append((a-b).abs().mean())
    return {'family_target_kl':max(0.,float(torch.stack(categorical).mean())),
            'raw_normalized_action_distance':float(torch.stack(raw).mean()),
            'full_joint_action_kl_measured':False,'projected_execution_distance':None}


def _actor_parameter(name,operator):
    if name.startswith(('value.','monetary_quantiles.','target_success.')):return False
    if operator=='local':return name.startswith(('heads.','conditioners.','fractions.','market_query.','query_filter.','query_times.'))
    return True


def mutate(model,probe,operator,seed,*,maximum_trials=8,fresh_initializer=None):
    if operator not in OPERATORS:raise ValueError('KNOWN_POPULATION_OPERATOR_REQUIRED')
    if type(maximum_trials)is not int or maximum_trials<1:raise ValueError('POSITIVE_MUTATION_TRIALS')
    original={k:v.detach().clone()for k,v in model.state_dict().items()};parent_sha=fingerprint(model)
    parent=behavior(model,probe);device=next(model.parameters()).device
    report={'operator':operator,'seed':seed,'parent_parameter_sha256':parent_sha,
            'probe_sha256':probe.sha256,'probe_dataset_sha256':probe.dataset_sha256,
            'training_only_probe':True,'optimizer_moments_must_be_reset':True,
            'old_on_policy_trajectories_must_be_discarded':True}
    try:
        with torch.random.fork_rng(devices=[device]if device.type=='cuda'else[]):
            torch.manual_seed(seed)
            if operator=='fresh':
                model.load_state_dict(type(model)(model.hidden_size).to(device).state_dict())
                if fresh_initializer is not None:report['fresh_initialization']=fresh_initializer(model)
            elif operator=='head_reset':
                modules=[model.heads,model.conditioners,model.fractions,model.market_query,
                         model.value,model.monetary_quantiles,model.target_success]
                if hasattr(model,'query_filter'):modules.extend((model.query_filter,model.query_times))
                for module in modules:
                    for part in module.modules():
                        if hasattr(part,'reset_parameters'):part.reset_parameters()
            else:
                noise={name:torch.randn_like(p)*max(float(p.detach().square().mean().sqrt()),.01)
                       for name,p in model.named_parameters()if _actor_parameter(name,operator)}
                target=KL_TARGETS[operator];scale=.01;trials=[];best=None
                for _ in range(maximum_trials):
                    model.load_state_dict(original)
                    with torch.no_grad():
                        for name,p in model.named_parameters():
                            if name in noise:p.add_(noise[name]*scale)
                    measured=distance(parent,behavior(model,probe));kl=measured['family_target_kl']
                    if not math.isfinite(kl+measured['raw_normalized_action_distance']):raise ValueError('NONFINITE_MUTATION_DISTANCE')
                    trials.append({'scale':scale,**measured});error=abs(math.log(max(kl,1e-12)/target))
                    if best is None or error<best[0]:best=(error,scale,measured)
                    if .5*target<=kl<=2*target:break
                    scale=min(10.,max(1e-6,scale*min(3.,max(.25,math.sqrt(target/max(kl,1e-12))))))
                model.load_state_dict(original)
                with torch.no_grad():
                    for name,p in model.named_parameters():
                        if name in noise:p.add_(noise[name]*best[1])
                report.update(target_component_kl=target,chosen_scale=best[1],trials=trials,
                              target_band_reached=.5*target<=best[2]['family_target_kl']<=2*target)
        report.update(distance(parent,behavior(model,probe)),child_parameter_sha256=fingerprint(model))
        return report
    except BaseException:
        model.load_state_dict(original);raise


def island_replacements(ranking,*,members,islands,cycle,rng,migration_every=4):
    if type(islands)is not int or islands<1 or members%islands or members//islands<2:
        raise ValueError('AT_LEAST_TWO_MEMBERS_PER_EQUAL_ISLAND')
    if len(ranking)!=members or {r['member']for r in ranking}!=set(range(members)):
        raise ValueError('COMPLETE_CURRENT_POPULATION_REQUIRED')
    if any(not math.isfinite(r['selection']['reward'])for r in ranking):raise ValueError('FINITE_SELECTION_REWARD_REQUIRED')
    groups=[sorted((r for r in ranking if r['member']%islands==i),
                   key=lambda r:r['selection']['reward'],reverse=True)for i in range(islands)]
    replacements=[]
    for island,group in enumerate(groups):
        donor=groups[(island-1)%islands][0]if islands>1 and cycle>0 and migration_every>0 and cycle%migration_every==0 else group[0]
        replacements.append({'island':island,'loser':group[-1]['member'],'donor':donor['member'],
                             'operator':rng.choices(OPERATORS,weights=WEIGHTS,k=1)[0],
                             'migration':donor['member']%islands!=island})
    return replacements


def retain_island_champions(ranking,incumbents,*,members,islands):
    """Elitist search on fixed search cases, including strictly negative scores.

    Historical scores are reused only within one experiment's fixed search
    context. The caller owns that experiment boundary; independent test scores
    are forbidden. Returned rows retain their actual evaluation cycle.
    """
    island_replacements(ranking,members=members,islands=islands,cycle=0,
                        rng=random.Random(0))
    def context(row):
        s=row['selection']
        if s['role']not in('development_selection','evolution'):
            raise ValueError('ELITES_REQUIRE_SEARCH_NOT_INDEPENDENT_TEST')
        if s['checkpoint_sha256']!=row['checkpoint']['sha256']:
            raise ValueError('ELITE_CHECKPOINT_SCORE_IDENTITY')
        if not s['cases']or not all(math.isfinite(c['reward'])for c in s['cases']):
            raise ValueError('FINITE_ELITE_CASES_REQUIRED')
        if not math.isclose(s['reward'],min(c['reward']for c in s['cases']),rel_tol=1e-12,abs_tol=1e-12):
            raise ValueError('ELITE_FIXED_WORST_CASE_SCORE_REQUIRED')
        fields=('start_ms','end_ms','stress','selection_seed','evaluation_compute_mode','classification','valuation')
        return s['role'],tuple(tuple(c.get(k)for k in fields)for c in s['cases'])
    expected=context(ranking[0])
    if any(context(row)!=expected for row in ranking):raise ValueError('ELITE_SEARCH_CONTEXT_CHANGED')
    selected=list(ranking);champions={};events=[]
    if set(incumbents)-set(range(islands)):raise ValueError('ELITE_ISLAND_LAYOUT_CHANGED')
    for island in range(islands):
        group=sorted((r for r in selected if r['member']%islands==island),
                     key=lambda r:r['selection']['reward'],reverse=True)
        current=group[0];old=incumbents.get(island)
        if old is not None:
            if context(old)!=expected:raise ValueError('ELITE_SEARCH_CONTEXT_CHANGED')
            if old['selection']['reward']>current['selection']['reward']:
                loser=group[-1];restored=deepcopy(old);restored['member']=loser['member']
                selected=[restored if r['member']==loser['member']else r for r in selected]
                events.append({'island':island,'slot':loser['member'],
                    'retained_checkpoint':restored['checkpoint'],'retained_evaluation_cycle':old['cycle'],
                    'retained_score':old['selection']['reward'],'current_best_score':current['selection']['reward'],
                    'rejected_checkpoint':loser['checkpoint'],'fixed_search_score_reused':True,
                    'independent_test_used':False})
                current=restored
        champions[island]=deepcopy(current)
    return sorted(selected,key=lambda r:r['selection']['reward'],reverse=True),champions,events


class ResearchArchive:
    """Protected development stepping stones; never a deployment qualification.

    Negative and flat alternatives can be retained as research, but are clearly
    separate from positive, repeated-calendar evidence. No archive entry can
    unlock the independent test or submit a production action.
    """
    def __init__(self,dataset_sha256,objective_sha256,maximum=32):
        if type(maximum)is not int or maximum<1:raise ValueError('POSITIVE_ARCHIVE_SIZE')
        self.dataset_sha256=dataset_sha256;self.objective_sha256=objective_sha256
        self.maximum=maximum;self.entries={}

    def consider(self,row):
        selection=row['selection'];checkpoint=row['checkpoint'];cases=selection['cases']
        if selection['role']not in ('development_selection','evolution')or not cases:
            raise ValueError('ARCHIVE_USES_SEARCH_NOT_INDEPENDENT_TEST')
        if selection['checkpoint_sha256']!=checkpoint['sha256']:raise ValueError('ARCHIVE_POLICY_IDENTITY_CHANGED')
        if any(not math.isfinite(c['reward'])for c in cases):raise ValueError('FINITE_ARCHIVE_ECONOMICS_REQUIRED')
        if not math.isclose(selection['reward'],min(c['reward']for c in cases),rel_tol=1e-12,abs_tol=1e-12):
            raise ValueError('ARCHIVE_FIXED_WORST_CASE_SCORE_REQUIRED')
        counts=[sum(int(c.get('family_counts',{}).get(str(f),c.get('family_counts',{}).get(f,0)))for c in cases)for f in range(12)]
        total=sum(counts)
        if not total:return False
        descriptor=tuple(round(8*n/total)for n in counts)
        profiles={};positive=True
        for c in cases:
            lo,hi=c['start_ms'],c['end_ms']
            if type(lo)is not int or type(hi)is not int or hi<=lo:raise ValueError('ARCHIVE_OBSERVED_CALENDAR_REQUIRED')
            profiles.setdefault(bool(c['stress']),[]).append((lo,hi))
            positive=positive and c['reward']>0 and c.get('stale_mark_observations',0)==0
        spans=[sorted(v)for v in profiles.values()]
        repeated=len(spans)==2 and len(spans[0])>=2 and all(v==spans[0]for v in spans)
        repeated=repeated and all(a[1]<=b[0]for a,b in zip(spans[0],spans[0][1:]))
        entry={'checkpoint':checkpoint,'member':row['member'],'cycle':row['cycle'],
               'descriptor':list(descriptor),'score':selection['reward'],
               'dataset_sha256':self.dataset_sha256,'objective_sha256':self.objective_sha256,
               'classification':'protected_development_stepping_stone_not_validated_profit',
               'positive_repeated_development_evidence':bool(positive and repeated),
               'case_calendars':{str(k):v for k,v in profiles.items()},
               'production_eligible':False,'independent_test_qualification':False}
        old=self.entries.get(descriptor)
        if old is not None and old['score']>=entry['score']:return False
        if old is None and len(self.entries)>=self.maximum:
            weakest=min(self.entries,key=lambda key:self.entries[key]['score'])
            if self.entries[weakest]['score']>=entry['score']:return False
            del self.entries[weakest]
        self.entries[descriptor]=entry;return True

    def json(self):
        return {'classification':'development_research_archive_not_live_or_locked_test',
                'dataset_sha256':self.dataset_sha256,'objective_sha256':self.objective_sha256,
                'protected_from_weak_slot_replacement':True,'entries':list(self.entries.values()),
                'production_authority':False,'independent_test_used':False}
