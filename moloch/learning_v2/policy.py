"""Compact recurrent actor with shared contract encoder and set aggregation.

The environment supplies mechanics-only feasibility masks. No old strategy
proposal, alpha prediction, future packet identity or simulator control is an
input. The recurrent state must be recomputed for each changed policy.
"""
import torch
from torch import nn
from torch.distributions import Beta, Categorical

ARCHITECTURE = 'tartarus-set-gru-512-v1'
FAMILIES = ('WAIT', 'BUY', 'SELL', 'CANCEL', 'EXIT')
HEAD_SIZES = {'family': len(FAMILIES), 'side': 2, 'price': 5, 'patience': 3}


def masked_categorical(logits, mask):
    if mask.dtype != torch.bool or mask.shape != logits.shape:
        raise ValueError('MECHANICS_MASK_SHAPE')
    if not bool(mask.any(dim=-1).all()):
        raise ValueError('MASK_HAS_NO_FEASIBLE_ACTION')
    return Categorical(logits=logits.masked_fill(~mask, torch.finfo(logits.dtype).min))


class RecurrentPolicy(nn.Module):
    def __init__(self, contract_features=16, global_features=12, hidden_size=512):
        super().__init__()
        self.contract_features = contract_features
        self.global_features = global_features
        self.hidden_size = hidden_size
        self.encoder = nn.Sequential(nn.Linear(contract_features, 128), nn.SiLU(), nn.Linear(128, 256), nn.SiLU())
        self.global_encoder = nn.Sequential(nn.Linear(global_features, 128), nn.SiLU())
        self.recurrent = nn.GRUCell(384, hidden_size)
        self.market_query = nn.Linear(hidden_size, 256)
        self.heads = nn.ModuleDict({name: nn.Linear(hidden_size+256, n) for name, n in HEAD_SIZES.items()})
        self.conditioners = nn.ModuleDict({name: nn.Embedding(n, hidden_size+256) for name, n in HEAD_SIZES.items()})
        self.fractions = nn.Linear(hidden_size+256, 4)
        self.value = nn.Linear(hidden_size, 1)
        self.monetary_quantiles = nn.Linear(hidden_size, 9)
        self.target_success = nn.Linear(hidden_size, 1)

    def initial_state(self, batch, *, device=None):
        return torch.zeros(batch, self.hidden_size, device=device or next(self.parameters()).device)

    def encode(self, contracts, global_state, present, hidden, reset):
        if contracts.ndim != 3 or contracts.shape[-1] != self.contract_features:
            raise ValueError('CONTRACT_FEATURE_SHAPE')
        if global_state.shape != (contracts.shape[0], self.global_features) or present.shape != contracts.shape[:2]:
            raise ValueError('GLOBAL_OR_PRESENT_SHAPE')
        if present.dtype != torch.bool or not bool(present.any(-1).all()):
            raise ValueError('NO_OBSERVED_CONTRACT')
        encoded = self.encoder(contracts)
        pooled = (encoded * present.unsqueeze(-1)).sum(1) / present.sum(1, keepdim=True)
        h = self.recurrent(torch.cat([pooled, self.global_encoder(global_state)], -1), hidden * (~reset).unsqueeze(-1))
        return encoded, h

    def step(self, contracts, global_state, present, masks, hidden, reset, actions=None, deterministic=False):
        """Autoregressive joint action and exact behavior-policy log likelihood.

        Conditional masks are indexed by the preceding market/family/side.
        family: BxNxF; side: BxNxFx2; price/patience: BxNxFx2xchoices.
        Every market/head needs a physical fallback (WAIT), supplied externally.
        Fractions are raw policy requests; exact integer projection stays outside.
        """
        encoded, h = self.encode(contracts, global_state, present, hidden, reset)
        return self._act_from_encoding(encoded,h,present,masks,actions,deterministic)

    def _act_from_encoding(self,encoded,h,present,masks,actions=None,deterministic=False):
        market_logits = (encoded * self.market_query(h).unsqueeze(1)).sum(-1) / 16
        market_dist = masked_categorical(market_logits, masks['market'] & present)
        market = actions['market'] if actions is not None else (market_logits.masked_fill(~(masks['market'] & present), -torch.inf).argmax(-1) if deterministic else market_dist.sample())
        batch = torch.arange(h.shape[0], device=h.device)
        selected = torch.cat([h, encoded[batch, market]], -1)
        out = {'market': market}
        logprob = market_dist.log_prob(market)
        entropy = market_dist.entropy()
        for name, head in self.heads.items():
            logits = head(selected)
            if name == 'family':
                mask = masks[name][batch, market]
            elif name == 'side':
                mask = masks[name][batch, market, out['family']]
            else:
                mask = masks[name][batch, market, out['family'], out['side']]
            dist = masked_categorical(logits, mask)
            value = actions[name] if actions is not None else (dist.probs.argmax(-1) if deterministic else dist.sample())
            out[name] = value
            meaningful = torch.ones_like(value,dtype=torch.bool) if name=='family' else ((out['family']==1)|(out['family']==2)|(out['family']==4))
            logprob = logprob + torch.where(meaningful,dist.log_prob(value),0.)
            entropy = entropy + torch.where(meaningful,dist.entropy(),0.)
            selected = selected + self.conditioners[name](value)
        parameters = torch.nn.functional.softplus(self.fractions(selected)) + 1
        for name, pair in [('target_fraction', (0, 1)), ('child_fraction', (2, 3))]:
            dist = Beta(parameters[:, pair[0]], parameters[:, pair[1]])
            scale=(masks['target_scale'][batch,market,out['family'],out['side']]
                   if name=='target_fraction'and'target_scale'in masks else torch.ones_like(parameters[:,0]))
            # SELL targets span the complete current reducible exposure rather
            # than wasting almost every sample above the actual holding. This
            # is action semantics, not an alpha/return filter. Keep the exact
            # density Jacobian for the bounded absolute portfolio fraction.
            value = actions[name] if actions is not None else (dist.mean if deterministic else dist.sample())*scale
            out[name] = value
            meaningful = ((out['family']==1)|(out['family']==2)) if name=='target_fraction' else ((out['family']==1)|(out['family']==2)|(out['family']==4))
            logprob = logprob + torch.where(meaningful,dist.log_prob(value/scale)-scale.log(),0.)
            entropy = entropy + torch.where(meaningful,dist.entropy()+scale.log(),0.)
        return {'actions': out, 'logprob': logprob, 'entropy': entropy,
                'value': self.value(h).squeeze(-1), 'monetary_quantiles': self.monetary_quantiles(h),
                'success_logit': self.target_success(h).squeeze(-1), 'hidden': h}

    def sequence(self, contracts, global_state, present, masks, reset, actions=None):
        """Recompute each policy's entire sequence; no inherited parent hidden state."""
        if contracts.ndim!=4 or contracts.shape[-1]!=self.contract_features:
            raise ValueError('CONTRACT_SEQUENCE_SHAPE')
        T,B,N,_=contracts.shape
        if present.shape!=(T,B,N)or global_state.shape!=(T,B,self.global_features)or reset.shape!=(T,B):
            raise ValueError('SEQUENCE_FEATURE_SHAPE')
        if present.dtype!=torch.bool or not bool(present.any(-1).all()):raise ValueError('NO_OBSERVED_CONTRACT')
        # Stateless encoders and action likelihoods batch across time. Only
        # the GRU carries time dependence; future rows cannot affect a prefix.
        # Parameter names/architecture and exact recurrent resets are retained.
        encoded=self.encoder(contracts)
        pooled=(encoded*present.unsqueeze(-1)).sum(2)/present.sum(2,keepdim=True)
        globals_encoded=self.global_encoder(global_state)
        hidden=self.initial_state(B,device=contracts.device);states=[]
        for t in range(T):
            hidden=self.recurrent(torch.cat([pooled[t],globals_encoded[t]],-1),hidden*(~reset[t]).unsqueeze(-1))
            states.append(hidden)
        h=torch.stack(states)
        flat_masks={k:v.reshape(T*B,*v.shape[2:])for k,v in masks.items()}
        flat_actions=None if actions is None else{k:v.reshape(T*B,*v.shape[2:])for k,v in actions.items()}
        out=self._act_from_encoding(encoded.reshape(T*B,N,256),h.reshape(T*B,self.hidden_size),present.reshape(T*B,N),flat_masks,flat_actions)
        values={k:v.reshape(T,B,*v.shape[1:])for k,v in out.items()if k!='actions'}
        chosen={k:v.reshape(T,B,*v.shape[1:])for k,v in out['actions'].items()}
        return [{**{k:v[t]for k,v in values.items()},'actions':{k:v[t]for k,v in chosen.items()}}for t in range(T)]
