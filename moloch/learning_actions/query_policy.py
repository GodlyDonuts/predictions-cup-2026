"""Full-depth actor with optional, unrestricted millisecond trade query bounds."""
import torch
from torch import nn
from torch.distributions import Beta
from .full_depth import FullDepthPolicy
from .observation_protocol import QUERY_WINDOW
from moloch.learning_v2.policy import masked_categorical


class QueryWindowPolicy(FullDepthPolicy):
    architecture='tartarus-raw-api-full-depth-query-window-gru-v4'
    observation_protocol=QUERY_WINDOW

    def __init__(self,hidden_size=4096):
        super().__init__(hidden_size)
        self.query_filter=nn.Linear(hidden_size+256,2)
        self.query_times=nn.Linear(hidden_size+256,4)

    def _act_from_encoding(self,encoded,h,present,masks,actions=None,deterministic=False):
        out=super()._act_from_encoding(encoded,h,present,masks,actions,deterministic)
        batch=torch.arange(h.shape[0],device=h.device);a=out['actions']
        selected=torch.cat((h,encoded[batch,a['market']]),-1)
        query=masks['trade_query'][batch,a['market'],0]&(a['family']==10)&(a['side']==0)
        logits=self.query_filter(selected);d=masked_categorical(logits,torch.ones_like(logits,dtype=torch.bool))
        value=actions['query_filter']if actions is not None else d.probs.argmax(-1)if deterministic else d.sample()
        a['query_filter']=value
        out['logprob']=out['logprob']+torch.where(query,d.log_prob(value).double(),0.)
        out['entropy']=out['entropy']+torch.where(query,d.entropy().double(),0.)
        active=query&(value==1)
        params=(torch.nn.functional.softplus(self.query_times(selected))+1).double()
        for name,pair in [('query_from_fraction',(0,1)),('query_to_fraction',(2,3))]:
            dist=Beta(params[:,pair[0]],params[:,pair[1]])
            value=actions[name]if actions is not None else dist.mean if deterministic else dist.sample()
            a[name]=value
            out['logprob']=out['logprob']+torch.where(active,dist.log_prob(value),0.)
            out['entropy']=out['entropy']+torch.where(active,dist.entropy(),0.)
        return out


def migrate_full_depth(parent):
    if type(parent)is not FullDepthPolicy:raise ValueError('EXACT_FULL_DEPTH_PARENT_REQUIRED_FOR_QUERY_MIGRATION')
    child=QueryWindowPolicy(parent.hidden_size).to(next(parent.parameters()).device)
    child.global_state_encoding=getattr(parent,'global_state_encoding','raw')
    values=child.state_dict();old=parent.state_dict()
    for key,value in old.items():values[key].copy_(value)
    child.load_state_dict(values)
    if any(not torch.equal(child.state_dict()[k],v)for k,v in old.items()):raise ValueError('QUERY_MIGRATION_CHANGED_EXISTING_PARAMETERS')
    return child,{'operator':'explicit_trade_query_window_action_extension','old_architecture':parent.architecture,
        'new_architecture':child.architecture,'all_existing_weights_preserved_exactly':True,
        'new_query_heads_initialized':True,'optimizer_reset_required':True,
        'old_trajectories_must_not_be_reused':True,'old_scores_must_not_be_reused':True}
