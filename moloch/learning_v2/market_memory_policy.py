"""Per-market temporal memory before the existing global recurrent controller.

Packed actor state includes each market's memory and actual presence bit. New
observations start from zero memory; absent slots never accumulate hidden state.
No new action, accounting or execution authority is introduced.
"""
import torch
from torch import nn
from .fused_policy import FusedRecurrentPolicy


class MarketMemoryPolicy(FusedRecurrentPolicy):
    architecture='tartarus-market-and-global-gru-v3'

    def __init__(self,market_slots,contract_features=24,global_features=14,hidden_size=1024,market_hidden_size=64):
        if min(market_slots,hidden_size,market_hidden_size)<=0:raise ValueError('POSITIVE_MEMORY_DIMENSIONS')
        super().__init__(contract_features,global_features,hidden_size)
        self.global_hidden_size=hidden_size;self.market_slots=market_slots;self.market_hidden_size=market_hidden_size
        self.market_recurrent=nn.GRU(256,market_hidden_size)
        self.market_context=nn.Linear(market_hidden_size,256,bias=False)
        self.hidden_size=hidden_size+market_slots*market_hidden_size+market_slots

    def _parts(self,hidden):
        g=self.global_hidden_size;n=self.market_slots;h=self.market_hidden_size
        if hidden.ndim!=2 or hidden.shape[1]!=self.hidden_size:raise ValueError('PACKED_MARKET_MEMORY_STATE')
        return hidden[:,:g],hidden[:,g:g+n*h].reshape(-1,n,h),hidden[:,g+n*h:]>0.5

    def step(self,contracts,global_state,present,masks,hidden,reset,actions=None,deterministic=False):
        B,N,F=contracts.shape
        if N!=self.market_slots or F!=self.contract_features or global_state.shape!=(B,self.global_features)or present.shape!=(B,N)or present.dtype!=torch.bool or not bool(present.any(-1).all()):raise ValueError('MARKET_MEMORY_FEATURE_SCOPE')
        gh,mh,was_present=self._parts(hidden);encoded=self.encoder(contracts)
        keep=present&was_present&~reset.unsqueeze(-1)
        _,mh=self.market_recurrent(encoded.reshape(1,B*N,256),(mh*keep.unsqueeze(-1)).reshape(1,B*N,self.market_hidden_size))
        mh=mh.reshape(B,N,self.market_hidden_size)*present.unsqueeze(-1)
        context=encoded+self.market_context(mh)
        pooled=(context*present.unsqueeze(-1)).sum(1)/present.sum(1,keepdim=True)
        _,gh=self.recurrent(torch.cat([pooled,self.global_encoder(global_state)],-1).unsqueeze(0),(gh*~reset.unsqueeze(-1)).unsqueeze(0));gh=gh.squeeze(0)
        out=self._act_from_encoding(context,gh,present,masks,actions,deterministic)
        out['hidden']=torch.cat([gh,mh.flatten(1),present.to(gh.dtype)],-1);return out

    def sequence(self,contracts,global_state,present,masks,reset,actions=None):
        T,B,N,F=contracts.shape
        if N!=self.market_slots or F!=self.contract_features or global_state.shape!=(T,B,self.global_features)or present.shape!=(T,B,N)or reset.shape!=(T,B)or present.dtype!=torch.bool or not bool(present.any(-1).all()):raise ValueError('MARKET_MEMORY_SEQUENCE_SCOPE')
        encoded=self.encoder(contracts)
        changes=present[1:].ne(present[:-1]).flatten(1).any(-1)
        cuts={0,T,*[int(t)for t in reset.any(-1).nonzero().flatten().tolist()if t>0],*[int(t)+1 for t in changes.nonzero().flatten().tolist()]}
        mh=torch.zeros(1,B*N,self.market_hidden_size,device=contracts.device,dtype=encoded.dtype);segments=[]
        for lo,hi in zip(sorted(cuts),sorted(cuts)[1:]):
            was=present[lo-1]if lo else torch.zeros_like(present[lo])
            keep=present[lo]&was&~reset[lo].unsqueeze(-1)
            values,mh=self.market_recurrent(encoded[lo:hi].reshape(hi-lo,B*N,256),mh*keep.reshape(1,B*N,1))
            values=values.reshape(hi-lo,B,N,self.market_hidden_size)*present[lo:hi].unsqueeze(-1)
            mh=mh*present[hi-1].reshape(1,B*N,1);segments.append(values)
        memories=torch.cat(segments);context=encoded+self.market_context(memories)
        pooled=(context*present.unsqueeze(-1)).sum(2)/present.sum(2,keepdim=True)
        inputs=torch.cat([pooled,self.global_encoder(global_state)],-1)
        cuts=[0]+[int(t)for t in reset.any(-1).nonzero().flatten().tolist()if t>0]+[T]
        gh=torch.zeros(1,B,self.global_hidden_size,device=contracts.device,dtype=encoded.dtype);segments=[]
        for lo,hi in zip(cuts,cuts[1:]):
            values,gh=self.recurrent(inputs[lo:hi],gh*~reset[lo].reshape(1,B,1));segments.append(values)
        globals_=torch.cat(segments)
        fm={k:v.reshape(T*B,*v.shape[2:])for k,v in masks.items()};fa=None if actions is None else{k:v.reshape(T*B,*v.shape[2:])for k,v in actions.items()}
        out=self._act_from_encoding(context.reshape(T*B,N,256),globals_.reshape(T*B,self.global_hidden_size),present.reshape(T*B,N),fm,fa)
        out['hidden']=torch.cat([globals_,memories.flatten(2),present.to(globals_.dtype)],-1).reshape(T*B,self.hidden_size)
        values={k:v.reshape(T,B,*v.shape[1:])for k,v in out.items()if k!='actions'};chosen={k:v.reshape(T,B,*v.shape[1:])for k,v in out['actions'].items()}
        return [{**{k:v[t]for k,v in values.items()},'actions':{k:v[t]for k,v in chosen.items()}}for t in range(T)]
