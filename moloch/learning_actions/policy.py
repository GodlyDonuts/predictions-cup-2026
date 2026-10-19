"""Full price lattice and raw quantity/UTC time choices, dynamic entity count.

The actor can assemble 50-order batches or 10-leg atomic baskets using builder
tokens without advancing simulated time. Builder tokens are planning, not trades.
"""
import torch
from torch import nn
from torch.distributions import Beta
from moloch.learning_v2.fused_policy import FusedRecurrentPolicy
from moloch.learning_v2.policy import masked_categorical

class ApiPolicy(FusedRecurrentPolicy):
    architecture='tartarus-raw-api-family-first-dynamic-gru-v2'
    from .observation_protocol import CURRENT as observation_protocol

    def __init__(self,hidden_size=4096):
        super().__init__(40,16,hidden_size)
        sizes={'family':12,'side':2,'price':200,'patience':2}
        self.heads=nn.ModuleDict({k:nn.Linear(hidden_size+256,n)for k,n in sizes.items()})
        self.conditioners=nn.ModuleDict({k:nn.Embedding(n,hidden_size+256)for k,n in sizes.items()})

    def _act_from_encoding(self,encoded,h,present,masks,actions=None,deterministic=False):
        # Choose an operation before its target. Otherwise hundreds of read
        # tokens implicitly crowd out trading simply by outnumbering contracts.
        # This changes the factorization, not the available action set.
        pooled=(encoded*present.unsqueeze(-1)).sum(1)/present.sum(1,keepdim=True)
        family_mask=(masks['family']&present.unsqueeze(-1)).any(1)
        family_dist=masked_categorical(self.heads['family'](torch.cat((h,pooled),-1)),family_mask)
        family=(actions['family']if actions is not None else
                family_dist.probs.argmax(-1)if deterministic else family_dist.sample())
        logits=(encoded*self.market_query(h).unsqueeze(1)).sum(-1)/16
        batch=torch.arange(h.shape[0],device=h.device)
        eligible=masks['family'].gather(2,family[:,None,None].expand(-1,present.shape[1],1)).squeeze(-1)
        dist=masked_categorical(logits,masks['market']&present&eligible)
        market=actions['market']if actions is not None else dist.probs.argmax(-1)if deterministic else dist.sample()
        selected=torch.cat((h,encoded[batch,market]),-1)+self.conditioners['family'](family)
        out={'market':market,'family':family}
        query_mask=masks.get('trade_query')
        trade_query=(family==10)&(query_mask[batch,market,0]if query_mask is not None else torch.zeros_like(family,dtype=torch.bool))
        logprob=dist.log_prob(market).double()+family_dist.log_prob(family).double()
        entropy=dist.entropy().double()+family_dist.entropy().double()
        for name,head in self.heads.items():
            if name=='family':continue
            head_logits=head(selected)
            mask=masks[name][batch,market]if name=='family'else torch.ones_like(head_logits,dtype=torch.bool)
            d=masked_categorical(head_logits,mask)
            value=actions[name]if actions is not None else d.probs.argmax(-1)if deterministic else d.sample()
            out[name]=value
            meaningful=torch.ones_like(value,dtype=torch.bool)if name=='family'else ((out['family']==1)|(out['family']==2)|(out['family']==5)|(out['family']==6))
            if name=='side':meaningful=meaningful|trade_query
            if name=='patience':meaningful=meaningful&(out['price']!=0)
            # V5 exposes every sampled latent in next-step local memory. Keep
            # the original sampling/conditioning law, but score its full joint.
            scored=torch.ones_like(meaningful)if getattr(self,'local_memory_joint_density',False)else meaningful
            logprob+=torch.where(scored,d.log_prob(value).double(),0.)
            entropy+=torch.where(scored,d.entropy().double(),0.)
            selected=selected+self.conditioners[name](value)*meaningful.unsqueeze(-1)
        parameters=(torch.nn.functional.softplus(self.fractions(selected))+1).double()
        for name,pair in [('target_fraction',(0,1)),('child_fraction',(2,3))]:
            d=Beta(parameters[:,pair[0]],parameters[:,pair[1]])
            value=actions[name]if actions is not None else d.mean if deterministic else d.sample()
            out[name]=value
            order=(out['family']==1)|(out['family']==2)|(out['family']==5)|(out['family']==6)
            meaningful=(order|trade_query)if name=='target_fraction'else ((out['family']==0)|(order&(out['patience']==1)&(out['price']!=0)))
            scored=torch.ones_like(meaningful)if getattr(self,'local_memory_joint_density',False)else meaningful
            logprob+=torch.where(scored,d.log_prob(value),0.)
            entropy+=torch.where(scored,d.entropy(),0.)
        return {'actions':out,'logprob':logprob,'entropy':entropy,'hidden':h,
                'value':self.value(h).squeeze(-1),'monetary_quantiles':self.monetary_quantiles(h),
                'success_logit':self.target_success(h).squeeze(-1)}
