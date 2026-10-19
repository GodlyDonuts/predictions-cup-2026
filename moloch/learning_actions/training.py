"""PPO chunks with retained recurrent state and caller-supplied economic rewards.

There is no manufactured profit label or fallback to interval-HOLD. Missing
complete valuation/reward means a chunk cannot train. A compute chunk never
claims the evaluation day or market episode ended.
"""
from dataclasses import dataclass
import math
import torch
from pathlib import Path
import hashlib
from moloch.learning_v2.train_inner import fingerprint


@dataclass
class Chunk:
    frames:list
    behavior_sha256:str
    role:str
    initial_hidden:torch.Tensor
    bootstrap_value:torch.Tensor
    stream:object=None
    behavior_guard:str='sha256'
    behavior_contract:tuple=None


def sequence_inputs(frames):
    """Pad variable entity counts, without truncating any entity or action."""
    import torch.nn.functional as F
    n=max(f['contracts'].shape[1]for f in frames)
    contracts=torch.stack([F.pad(f['contracts'],(0,0,0,n-f['contracts'].shape[1]))for f in frames])
    present=torch.stack([F.pad(f['present'],(0,n-f['present'].shape[1]))for f in frames])
    masks={key:torch.stack([F.pad(f['masks'][key],
        (0,n-f['contracts'].shape[1])if key=='market'else(0,0,0,n-f['contracts'].shape[1]))for f in frames])
        for key in frames[0]['masks']}
    return contracts,torch.stack([f['global_state']for f in frames]),present,masks


def sequence_forward(model,frames,initial_hidden,prepared=None,*,checkpoint_encoder=False):
    """One fused GRU sequence, equivalent to per-decision teacher forcing."""
    contracts,global_state,present,masks=prepared or sequence_inputs(frames)
    t,b,n,_=contracts.shape
    if checkpoint_encoder and torch.is_grad_enabled():
        from torch.utils.checkpoint import checkpoint
        encoded=checkpoint(model.encoder,contracts,use_reentrant=False)
    else:encoded=model.encoder(contracts)
    pooled=(encoded*present.unsqueeze(-1)).sum(2)/present.sum(2,keepdim=True)
    inputs=torch.cat((pooled,model.global_encoder(global_state)),-1)
    resets=torch.stack([f['reset']for f in frames])
    boundaries=[0]+[int(i)for i in resets.any(-1).nonzero().flatten().tolist()if i>0]+[t]
    hidden=initial_hidden.unsqueeze(0);segments=[]
    for lo,hi in zip(boundaries,boundaries[1:]):
        hidden=hidden*(~resets[lo]).unsqueeze(0).unsqueeze(-1)
        values,hidden=model.recurrent(inputs[lo:hi],hidden);segments.append(values)
    states=torch.cat(segments)
    actions={key:torch.stack([f['actions'][key]for f in frames]).reshape(t*b)
             for key in frames[0]['actions']}
    output=model._act_from_encoding(encoded.reshape(t*b,n,256),states.reshape(t*b,model.hidden_size),
        present.reshape(t*b,n),{k:v.reshape(t*b,*v.shape[2:])for k,v in masks.items()},actions)
    return {k:output[k].reshape(t,b)for k in ('logprob','value','entropy')},states[-1]


def collect_chunk(stream,decisions=128,*,reward_ledger=None):
    if getattr(stream.env,'role',None)!='train':raise ValueError('TRAINING_CAPTURE_ONLY_FROM_TRAIN_ROLE')
    if not stream.record_training:raise ValueError('TRAINING_CAPTURE_MUST_BE_EXPLICIT')
    if type(decisions)is not int or decisions<=0:raise ValueError('POSITIVE_MEMORY_CHUNK')
    from .behavior_guard import identity,contract
    guard=getattr(stream,'behavior_guard','sha256');before=identity(stream.model,guard);frames=[]
    for _ in range(decisions):
        stream.last_frame=None;row=stream.step()
        if stream.last_frame is not None:
            if reward_ledger is not None:stream.last_frame['economic_reward']=reward_ledger.transition()
            frames.append(stream.last_frame)
        if row.get('terminal'):break
    if not frames:raise ValueError('NO_POLICY_FRAMES')
    if stream.env.now>=stream.env.end:
        frames[-1]['elapsed_ms']+=max(0,stream.env.now-frames[-1]['known_at_ms'])
        frames[-1]['terminal']=True
        if reward_ledger is not None:
            # Account for a final compute interval that reached the horizon
            # without dispatching another action. It belongs to the last frame.
            frames[-1]['economic_reward']+=reward_ledger.transition()
            final=reward_ledger.finish()
            frames[-1]['economic_reward']+=final['terminal_reward_correction']
            frames[-1]['economic_result']=final
    from .stream import tensors
    with torch.no_grad():
        # Critic bootstrap is training bookkeeping; it sends no API request and
        # does not mutate the actor's carried hidden state or virtual clock.
        if frames[-1]['terminal']:value=torch.zeros_like(frames[-1]['old_value'])
        else:
            view=tensors(stream.env.observe(),stream.device)
            value=stream.model.step(**view,hidden=stream.hidden,
                reset=torch.tensor([False],device=stream.device),deterministic=True)['value']
    return Chunk(frames,before,stream.env.role if hasattr(stream.env,'role')else'train',
                 frames[0]['initial_hidden'],value.detach(),stream,guard,contract(stream.model))


def update(model,optimizer,chunk,rewards,*,role,bootstrap_value=None,clip=.2,epochs=2,
           checkpoint_encoder=False,entropy_coefficient=.01,episode_session=None):
    """Economic rewards must be supplied for every decision, never imputed.

    Default no time discount matches the requested compounded-growth objective.
    Complete evaluation scoring/event-independence belongs to the frozen reward
    coordinator. This function neither opens held-out data nor selects finalists.
    """
    if role!='train'or chunk.role!='train':raise ValueError('INDEPENDENT_TEST_OR_EVOLUTION_CANNOT_OPTIMIZE_WEIGHTS')
    from .behavior_guard import identity,require_contract
    require_contract(model,chunk.behavior_contract)
    prescribed=None;global_advantage=None
    if episode_session is None:
        if identity(model,chunk.behavior_guard)!=chunk.behavior_sha256:raise ValueError('FRESH_ON_POLICY_CHUNK_REQUIRED')
    else:
        from .episode_training import EpisodeSession
        if type(episode_session)is not EpisodeSession:raise ValueError('SEALED_TRAIN_EPISODE_SESSION_REQUIRED')
        prescribed,global_advantage=episode_session.authorize(model,chunk)
    if type(entropy_coefficient)not in(int,float)or not math.isfinite(entropy_coefficient)or entropy_coefficient<0:
        raise ValueError('NONNEGATIVE_FINITE_ENTROPY_COEFFICIENT')
    flattened=[x for row in rewards for x in (row if isinstance(row,(list,tuple))else[row])]
    if len(rewards)!=len(chunk.frames)or any(type(x)not in(int,float)or not math.isfinite(x)for x in flattened):
        raise ValueError('COMPLETE_VERIFIED_ECONOMIC_REWARDS_REQUIRED')
    device=next(model.parameters()).device
    old_values=torch.stack([f['old_value']for f in chunk.frames]).double()
    rewards=torch.tensor(rewards,dtype=torch.float64,device=device).reshape(old_values.shape)
    old_logprob=torch.stack([f['old_logprob']for f in chunk.frames]).detach()
    learn=torch.stack([f.get('learn',torch.ones_like(f['old_value'],dtype=torch.bool))for f in chunk.frames])
    count=learn.sum().clamp_min(1)
    if not bool(learn.any()):raise ValueError('NO_ACTIVE_ECONOMIC_TRAINING_TRANSITIONS')
    value=chunk.bootstrap_value.double()if bootstrap_value is None else bootstrap_value.double()
    returns=[]
    for i in reversed(range(len(chunk.frames))):
        terminal=torch.as_tensor(chunk.frames[i]['terminal'],device=device,dtype=torch.bool)
        value=rewards[i]+(~terminal)*value
        returns.append(value)
    returns=torch.stack(list(reversed(returns))).detach()
    if prescribed is not None:
        returns=torch.tensor(prescribed,dtype=torch.float64,device=device)
        if returns.shape!=old_values.shape or not bool(torch.isfinite(returns).all()):raise ValueError('COMPLETE_CLOSED_EPISODE_TARGETS_REQUIRED')
    advantage=returns-old_values
    if global_advantage is None:
        advantage=(advantage-advantage[learn].mean())/advantage[learn].std(unbiased=False).clamp_min(1e-8)
    else:advantage=(advantage-global_advantage[0])/global_advantage[1]
    reports=[]
    prepared=sequence_inputs(chunk.frames)
    for epoch in range(epochs):
        out,hidden=sequence_forward(model,chunk.frames,chunk.initial_hidden,prepared,checkpoint_encoder=checkpoint_encoder)
        delta=out['logprob']-old_logprob;ratio=delta.exp()
        policy=-(torch.minimum(ratio*advantage,ratio.clamp(1-clip,1+clip)*advantage)*learn).sum()/count
        value_loss=((out['value'].double()-returns).square()*learn).sum()/count
        entropy=(out['entropy']*learn).sum()/count
        loss=policy+.5*value_loss-entropy_coefficient*entropy
        if not bool(torch.isfinite(loss)):raise ValueError('NONFINITE_API_POLICY_UPDATE')
        before={k:v.detach().clone()for k,v in model.state_dict().items()}
        import copy
        optimizer_before=copy.deepcopy(optimizer.state_dict())
        optimizer.zero_grad(set_to_none=True);loss.backward()
        torch.nn.utils.clip_grad_norm_(model.parameters(),.5);optimizer.step()
        with torch.no_grad():
            checked,hidden=sequence_forward(model,chunk.frames,chunk.initial_hidden,prepared)
            difference=checked['logprob']-old_logprob
            kl=float(((difference.exp()-1-difference)*learn).sum()/count)
        accepted=math.isfinite(kl)and kl<=.03
        if not accepted:model.load_state_dict(before);optimizer.load_state_dict(optimizer_before)
        elif chunk.stream is not None:chunk.stream.hidden=hidden.detach()
        reports.append({'epoch':epoch,'loss':float(loss.detach()),'post_update_kl':kl,
                        'policy_loss':float(policy.detach()),'value_loss':float(value_loss.detach()),
                        'mean_entropy':float(entropy.detach()),'entropy_coefficient':entropy_coefficient,
                        'encoder_activation_checkpointing':bool(checkpoint_encoder),'behavior_guard':chunk.behavior_guard,
                        'accepted':accepted,'recurrent_state_carried':True})
        if not accepted:break
    if episode_session is not None:
        episode_session.finished(model)
        for report in reports:report['return_source']='sealed_full_training_episode';report['recurrent_context']='stored_behavior_state_at_chunk_boundary'
    return reports


def save_checkpoint(path,model,optimizer,*,dataset_manifest_sha256,split_specification_sha256):
    from dataclasses import asdict
    if any(len(h)!=64 for h in (dataset_manifest_sha256,split_specification_sha256)):
        raise ValueError('PINNED_DATA_AND_SPLIT_REQUIRED')
    from .state_encoding import validate
    encoding=validate(getattr(model,'global_state_encoding','raw'))
    from .observation_protocol import validate as protocol,LEGACY
    observation_protocol=protocol(getattr(model,'observation_protocol',LEGACY))
    from .joint_memory_policy import JointMemoryPolicy
    from .observation_protocol import MEMORY_JOINT
    if (type(model)is JointMemoryPolicy)!=(observation_protocol==MEMORY_JOINT):
        raise ValueError('JOINT_MEMORY_ARCHITECTURE_PROTOCOL_MISMATCH')
    bundle={'architecture':model.architecture,'hidden_size':model.hidden_size,'global_state_encoding':encoding,
        'observation_protocol':observation_protocol,
        'model':model.state_dict(),'optimizer':optimizer.state_dict(),
        'parameter_sha256':fingerprint(model),'dataset_manifest_sha256':dataset_manifest_sha256,
        'split_specification_sha256':split_specification_sha256,'production_authority':False}
    path=Path(path)
    with path.open('xb')as f:torch.save(bundle,f)
    return hashlib.sha256(path.read_bytes()).hexdigest()


def load_checkpoint(path,expected_sha256,device='cpu'):
    from .policy import ApiPolicy
    from .full_depth import FullDepthPolicy
    from .query_policy import QueryWindowPolicy
    from .joint_memory_policy import JointMemoryPolicy
    path=Path(path)
    if hashlib.sha256(path.read_bytes()).hexdigest()!=expected_sha256:raise ValueError('FROZEN_RAW_API_CHECKPOINT_CHANGED')
    bundle=torch.load(path,map_location=device,weights_only=True)
    architectures={ApiPolicy.architecture:ApiPolicy,FullDepthPolicy.architecture:FullDepthPolicy,
                   QueryWindowPolicy.architecture:QueryWindowPolicy,JointMemoryPolicy.architecture:JointMemoryPolicy}
    if bundle['architecture']not in architectures or bundle['production_authority']is not False:
        raise ValueError('RAW_API_ARCHITECTURE_REQUIRED_NO_LEGACY_CHECKPOINT_RELABELLING')
    model=architectures[bundle['architecture']](bundle['hidden_size']).to(device);model.load_state_dict(bundle['model'])
    from .state_encoding import validate
    model.global_state_encoding=validate(bundle.get('global_state_encoding','raw'))
    from .observation_protocol import validate as protocol,LEGACY
    model.observation_protocol=protocol(bundle.get('observation_protocol',LEGACY))
    from .observation_protocol import MEMORY_JOINT
    if (type(model)is JointMemoryPolicy)!=(model.observation_protocol==MEMORY_JOINT):
        raise ValueError('JOINT_MEMORY_ARCHITECTURE_PROTOCOL_MISMATCH')
    if fingerprint(model)!=bundle['parameter_sha256']:raise ValueError('RAW_API_PARAMETERS_CHANGED')
    return model,bundle
