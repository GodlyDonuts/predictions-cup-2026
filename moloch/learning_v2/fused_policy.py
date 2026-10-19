"""Fused recurrent sequences with an explicit checkpoint architecture."""
import torch
from torch import nn
from .policy import RecurrentPolicy


class FusedRecurrentPolicy(RecurrentPolicy):
    architecture='tartarus-set-fused-gru-v2'

    def __init__(self,contract_features=24,global_features=14,hidden_size=1024):
        super().__init__(contract_features,global_features,hidden_size)
        self.recurrent=nn.GRU(384,hidden_size)

    def encode(self,contracts,global_state,present,hidden,reset):
        if contracts.ndim!=3 or contracts.shape[-1]!=self.contract_features or global_state.shape!=(contracts.shape[0],self.global_features):raise ValueError('FUSED_FEATURE_SHAPE')
        if present.shape!=contracts.shape[:2] or present.dtype!=torch.bool or not bool(present.any(-1).all()):raise ValueError('FUSED_PRESENT_MASK')
        encoded=self.encoder(contracts);pooled=(encoded*present.unsqueeze(-1)).sum(1)/present.sum(1,keepdim=True)
        inputs=torch.cat([pooled,self.global_encoder(global_state)],-1).unsqueeze(0)
        _,h=self.recurrent(inputs,(hidden*(~reset).unsqueeze(-1)).unsqueeze(0))
        return encoded,h.squeeze(0)

    def sequence(self,contracts,global_state,present,masks,reset,actions=None):
        if contracts.ndim!=4 or contracts.shape[-1]!=self.contract_features:raise ValueError('FUSED_SEQUENCE_SHAPE')
        T,B,N,_=contracts.shape
        if global_state.shape!=(T,B,self.global_features)or present.shape!=(T,B,N)or reset.shape!=(T,B):raise ValueError('FUSED_SEQUENCE_SCOPE')
        if present.dtype!=torch.bool or not bool(present.any(-1).all()):raise ValueError('FUSED_PRESENT_MASK')
        encoded=self.encoder(contracts);pooled=(encoded*present.unsqueeze(-1)).sum(2)/present.sum(2,keepdim=True)
        inputs=torch.cat([pooled,self.global_encoder(global_state)],-1)
        boundaries=[0]+[int(t)for t in reset.any(-1).nonzero().flatten().tolist()if t>0]+[T]
        hidden=self.initial_state(B,device=contracts.device).unsqueeze(0);segments=[]
        for lo,hi in zip(boundaries,boundaries[1:]):
            hidden=hidden*(~reset[lo]).unsqueeze(0).unsqueeze(-1)
            out,hidden=self.recurrent(inputs[lo:hi],hidden);segments.append(out)
        h=torch.cat(segments)
        fm={k:v.reshape(T*B,*v.shape[2:])for k,v in masks.items()}
        fa=None if actions is None else{k:v.reshape(T*B,*v.shape[2:])for k,v in actions.items()}
        out=self._act_from_encoding(encoded.reshape(T*B,N,256),h.reshape(T*B,self.hidden_size),present.reshape(T*B,N),fm,fa)
        values={k:v.reshape(T,B,*v.shape[1:])for k,v in out.items()if k!='actions'}
        chosen={k:v.reshape(T,B,*v.shape[1:])for k,v in out['actions'].items()}
        return [{**{k:v[t]for k,v in values.items()},'actions':{k:v[t]for k,v in chosen.items()}}for t in range(T)]


def from_cell_policy(parent):
    child=FusedRecurrentPolicy(parent.contract_features,parent.global_features,parent.hidden_size).to(next(parent.parameters()).device)
    values={('recurrent.'+k.split('.',1)[1]+'_l0'if k.startswith('recurrent.')else k):v for k,v in parent.state_dict().items()}
    child.load_state_dict(values);return child
