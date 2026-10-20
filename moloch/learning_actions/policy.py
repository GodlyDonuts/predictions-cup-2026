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

    def step_actions_only(self,contracts,global_state,present,masks,hidden,reset,deterministic=False):
        if torch.is_grad_enabled():raise ValueError('ACTIONS_ONLY_IS_EVALUATION_ONLY')
        encoded,h=self.encode(contracts,global_state,present,hidden,reset)
        return self._act_from_encoding(encoded,h,present,masks,deterministic=deterministic,statistics=False)

    def step_rollout(self,contracts,global_state,present,masks,hidden,reset,deterministic=False):
        if torch.is_grad_enabled():raise ValueError('ROLLOUT_STATISTICS_IS_NO_GRAD_ONLY')
        encoded,h=self.encode(contracts,global_state,present,hidden,reset)
        return self._act_from_encoding(encoded,h,present,masks,deterministic=deterministic,rollout_only=True)

    def bootstrap_value(self,contracts,global_state,present,masks,hidden,reset):
        if torch.is_grad_enabled():raise ValueError('BOOTSTRAP_VALUE_IS_NO_GRAD_ONLY')
        _,h=self.encode(contracts,global_state,present,hidden,reset)
        return self.value(h).squeeze(-1)

    def _act_from_encoding(self,encoded,h,present,masks,actions=None,deterministic=False,*,sampler=None,statistics=True,rollout_only=False):
        if type(rollout_only)is not bool or(rollout_only and(not statistics or actions is not None or torch.is_grad_enabled())):
            raise ValueError('ROLLOUT_STATISTICS_IS_NO_GRAD_ONLY')
        if type(statistics)is not bool or(not statistics and(actions is not None or torch.is_grad_enabled())):
            raise ValueError('ACTIONS_ONLY_IS_EVALUATION_ONLY')
        if not statistics and sampler is not None:
            from .independent_sampling import IndependentSampling
            if type(sampler)is not IndependentSampling:raise ValueError('ACTIONS_ONLY_REQUIRES_TRUSTED_TORCH_SAMPLING')
        from .acting_validation import enabled,categorical,beta
        fast=enabled(self,actions,sampler)
        # Choose an operation before its target. Otherwise hundreds of read
        # tokens implicitly crowd out trading simply by outnumbering contracts.
        # This changes the factorization, not the available action set.
        pooled=(encoded*present.unsqueeze(-1)).sum(1)/present.sum(1,keepdim=True)
        family_mask=(masks['family']&present.unsqueeze(-1)).any(1)
        family_dist=categorical(self.heads['family'](torch.cat((h,pooled),-1)),family_mask,fast)
        family=(actions['family']if actions is not None else
                family_dist.probs.argmax(-1)if deterministic else family_dist.sample()if sampler is None else sampler.categorical(family_dist))
        logits=(encoded*self.market_query(h).unsqueeze(1)).sum(-1)/16
        batch=torch.arange(h.shape[0],device=h.device)
        eligible=masks['family'].gather(2,family[:,None,None].expand(-1,present.shape[1],1)).squeeze(-1)
        from .read_target_sampling import logits as target_logits
        logits=target_logits(self,logits,family,eligible&present&masks['market'],masks)
        dist=categorical(logits,masks['market']&present&eligible,fast)
        market=actions['market']if actions is not None else dist.probs.argmax(-1)if deterministic else dist.sample()if sampler is None else sampler.categorical(dist,market=True)
        selected=torch.cat((h,encoded[batch,market]),-1)+self.conditioners['family'](family)
        out={'market':market,'family':family}
        query_mask=masks.get('trade_query')
        trade_query=(family==10)&(query_mask[batch,market,0]if query_mask is not None else torch.zeros_like(family,dtype=torch.bool))
        if statistics:
            logprob=dist.log_prob(market).double()+family_dist.log_prob(family).double()
            if not rollout_only:entropy=dist.entropy().double()+family_dist.entropy().double()
        for name,head in self.heads.items():
            if name=='family':continue
            head_logits=head(selected)
            mask=masks[name][batch,market]if name=='family'else torch.ones_like(head_logits,dtype=torch.bool)
            d=categorical(head_logits,mask,fast)
            value=actions[name]if actions is not None else d.probs.argmax(-1)if deterministic else d.sample()if sampler is None else sampler.categorical(d)
            out[name]=value
            meaningful=torch.ones_like(value,dtype=torch.bool)if name=='family'else ((out['family']==1)|(out['family']==2)|(out['family']==5)|(out['family']==6))
            if name=='side':meaningful=meaningful|trade_query
            if name=='patience':
                meaningful=meaningful&(out['price']!=0)
                if getattr(self,'managed_data_profile',None)is not None:meaningful=meaningful|(family==0)
            # V5 exposes every sampled latent in next-step local memory. Keep
            # the original sampling/conditioning law, but score its full joint.
            if statistics:
                scored=torch.ones_like(meaningful)if getattr(self,'local_memory_joint_density',False)else meaningful
                logprob+=torch.where(scored,d.log_prob(value).double(),0.)
                if not rollout_only:entropy+=torch.where(scored,d.entropy().double(),0.)
            selected=selected+self.conditioners[name](value)*meaningful.unsqueeze(-1)
        parameters=(torch.nn.functional.softplus(self.fractions(selected))+1).double()
        for name,pair in [('target_fraction',(0,1)),('child_fraction',(2,3))]:
            d=beta(parameters[:,pair[0]],parameters[:,pair[1]],fast,finite=not statistics or rollout_only)
            value=actions[name]if actions is not None else d.mean if deterministic else d.sample()if sampler is None else sampler.beta(d)
            out[name]=value
            order=(out['family']==1)|(out['family']==2)|(out['family']==5)|(out['family']==6)
            meaningful=(order|trade_query)if name=='target_fraction'else ((out['family']==0)|(order&(out['patience']==1)&(out['price']!=0)))
            if statistics:
                scored=torch.ones_like(meaningful)if getattr(self,'local_memory_joint_density',False)else meaningful
                logprob+=torch.where(scored,d.log_prob(value),0.)
                if not rollout_only:entropy+=torch.where(scored,d.entropy(),0.)
        if(fast or not statistics)and not hasattr(self,'query_times'):
            from .acting_validation import validate_samples
            validate_samples(out)
        if not statistics:return {'actions':out,'hidden':h}
        if rollout_only:return {'actions':out,'logprob':logprob,'hidden':h,'value':self.value(h).squeeze(-1)}
        return {'actions':out,'logprob':logprob,'entropy':entropy,'hidden':h,
                'value':self.value(h).squeeze(-1),'monetary_quantiles':self.monetary_quantiles(h),
                'success_logit':self.target_success(h).squeeze(-1)}
