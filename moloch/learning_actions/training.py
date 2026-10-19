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


def collect_chunk(stream,decisions=128):
    if getattr(stream.env,'role',None)!='train':raise ValueError('TRAINING_CAPTURE_ONLY_FROM_TRAIN_ROLE')
    if not stream.record_training:raise ValueError('TRAINING_CAPTURE_MUST_BE_EXPLICIT')
    if type(decisions)is not int or decisions<=0:raise ValueError('POSITIVE_MEMORY_CHUNK')
    before=fingerprint(stream.model);frames=[]
    for _ in range(decisions):
        stream.last_frame=None;row=stream.step()
        if stream.last_frame is not None:frames.append(stream.last_frame)
        if row.get('terminal'):break
    if not frames:raise ValueError('NO_POLICY_FRAMES')
    if stream.env.now>=stream.env.end:
        frames[-1]['elapsed_ms']+=max(0,stream.env.now-frames[-1]['known_at_ms'])
        frames[-1]['terminal']=True
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
                 frames[0]['initial_hidden'],value.detach(),stream)


def update(model,optimizer,chunk,rewards,*,role,bootstrap_value=None,clip=.2,epochs=2):
    """Economic rewards must be supplied for every decision, never imputed.

    Default no time discount matches the requested compounded-growth objective.
    Complete evaluation scoring/event-independence belongs to the frozen reward
    coordinator. This function neither opens held-out data nor selects finalists.
    """
    if role!='train'or chunk.role!='train':raise ValueError('INDEPENDENT_TEST_OR_EVOLUTION_CANNOT_OPTIMIZE_WEIGHTS')
    if fingerprint(model)!=chunk.behavior_sha256:raise ValueError('FRESH_ON_POLICY_CHUNK_REQUIRED')
    if len(rewards)!=len(chunk.frames)or any(type(x)not in(int,float)or not math.isfinite(x)for x in rewards):
        raise ValueError('COMPLETE_VERIFIED_ECONOMIC_REWARDS_REQUIRED')
    device=next(model.parameters()).device
    rewards=torch.tensor(rewards,dtype=torch.float64,device=device).reshape(-1,1)
    old_values=torch.stack([f['old_value']for f in chunk.frames]).double()
    old_logprob=torch.stack([f['old_logprob']for f in chunk.frames]).detach()
    value=chunk.bootstrap_value.double()if bootstrap_value is None else bootstrap_value.double()
    returns=[]
    for i in reversed(range(len(chunk.frames))):
        value=rewards[i]+(0 if chunk.frames[i]['terminal']else value)
        returns.append(value)
    returns=torch.stack(list(reversed(returns))).detach()
    advantage=returns-old_values;advantage=(advantage-advantage.mean())/advantage.std(unbiased=False).clamp_min(1e-8)
    reports=[]
    for epoch in range(epochs):
        hidden=chunk.initial_hidden;logprob=[];values=[];entropy=[]
        for frame in chunk.frames:
            out=model.step(**{k:frame[k]for k in ('contracts','global_state','present','masks','reset')},
                           hidden=hidden,actions=frame['actions'])
            hidden=out['hidden'];logprob.append(out['logprob']);values.append(out['value']);entropy.append(out['entropy'])
        delta=torch.stack(logprob)-old_logprob;ratio=delta.exp()
        policy=-torch.minimum(ratio*advantage,ratio.clamp(1-clip,1+clip)*advantage).mean()
        value_loss=(torch.stack(values).double()-returns).square().mean()
        loss=policy+.5*value_loss-.01*torch.stack(entropy).mean()
        if not bool(torch.isfinite(loss)):raise ValueError('NONFINITE_API_POLICY_UPDATE')
        before={k:v.detach().clone()for k,v in model.state_dict().items()}
        import copy
        optimizer_before=copy.deepcopy(optimizer.state_dict())
        optimizer.zero_grad(set_to_none=True);loss.backward()
        torch.nn.utils.clip_grad_norm_(model.parameters(),.5);optimizer.step()
        with torch.no_grad():
            hidden=chunk.initial_hidden;checked=[]
            for frame in chunk.frames:
                out=model.step(**{k:frame[k]for k in ('contracts','global_state','present','masks','reset')},
                               hidden=hidden,actions=frame['actions'])
                hidden=out['hidden'];checked.append(out['logprob'])
            difference=torch.stack(checked)-old_logprob
            kl=float((difference.exp()-1-difference).mean())
        accepted=math.isfinite(kl)and kl<=.03
        if not accepted:model.load_state_dict(before);optimizer.load_state_dict(optimizer_before)
        elif chunk.stream is not None:chunk.stream.hidden=hidden.detach()
        reports.append({'epoch':epoch,'loss':float(loss.detach()),'post_update_kl':kl,
                        'accepted':accepted,'recurrent_state_carried':True})
        if not accepted:break
    return reports


def save_checkpoint(path,model,optimizer,*,dataset_manifest_sha256,split_specification_sha256):
    from dataclasses import asdict
    if any(len(h)!=64 for h in (dataset_manifest_sha256,split_specification_sha256)):
        raise ValueError('PINNED_DATA_AND_SPLIT_REQUIRED')
    bundle={'architecture':model.architecture,'hidden_size':model.hidden_size,
        'model':model.state_dict(),'optimizer':optimizer.state_dict(),
        'parameter_sha256':fingerprint(model),'dataset_manifest_sha256':dataset_manifest_sha256,
        'split_specification_sha256':split_specification_sha256,'production_authority':False}
    path=Path(path)
    with path.open('xb')as f:torch.save(bundle,f)
    return hashlib.sha256(path.read_bytes()).hexdigest()


def load_checkpoint(path,expected_sha256,device='cpu'):
    from .policy import ApiPolicy
    path=Path(path)
    if hashlib.sha256(path.read_bytes()).hexdigest()!=expected_sha256:raise ValueError('FROZEN_RAW_API_CHECKPOINT_CHANGED')
    bundle=torch.load(path,map_location=device,weights_only=True)
    if bundle['architecture']!=ApiPolicy.architecture or bundle['production_authority']is not False:
        raise ValueError('RAW_API_ARCHITECTURE_REQUIRED_NO_LEGACY_CHECKPOINT_RELABELLING')
    model=ApiPolicy(bundle['hidden_size']).to(device);model.load_state_dict(bundle['model'])
    if fingerprint(model)!=bundle['parameter_sha256']:raise ValueError('RAW_API_PARAMETERS_CHANGED')
    return model,bundle
