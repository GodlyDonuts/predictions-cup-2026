"""Opt-in read endpoint factorization, without suppressing any legal target.

At equal target logits, each available (venue, endpoint) gets equal mass rather
than mass proportional to its catalog size. Learned logits remain unrestricted.
"""
import numpy as np
import torch

FLAT='flat_targets_v1'
BALANCED='endpoint_logmeanexp_v1'
VENUES=('SIG','Kalshi','Polymarket')
ENDPOINTS=('book','quote','trade_flow','account','positions','owned_orders','collateral',
           'realtime_token','bulk_prices','historical_candles','historical_trades',
           'historical_sample','reference','other')


def validate(mode):
    if type(mode)is not str or mode not in(FLAT,BALANCED):raise ValueError('EXPLICIT_READ_TARGET_SAMPLING_LAW')
    return mode


def groups(entities):
    out=np.zeros((len(entities),len(VENUES)*len(ENDPOINTS)),dtype=bool)
    for i,(kind,key)in enumerate(entities):
        if kind!='read':continue
        venue,target=key
        if venue not in VENUES:raise ValueError('EXPLICIT_READ_TARGET_VENUE')
        endpoint=target[0]if target[0]in ENDPOINTS else'other'
        out[i,VENUES.index(venue)*len(ENDPOINTS)+ENDPOINTS.index(endpoint)]=True
    return out


def logits(model,raw,family,eligible,masks):
    mode=validate(getattr(model,'read_target_sampling',FLAT))
    if mode==FLAT:return raw
    members=masks.get('read_groups')
    if members is None or members.dtype!=torch.bool or members.shape!=(*raw.shape,len(VENUES)*len(ENDPOINTS)):
        raise ValueError('EXPLICIT_ENDPOINT_GROUP_METADATA_REQUIRED')
    # Count only targets actually eligible under the original mechanics mask.
    counts=(members&eligible.unsqueeze(-1)).sum(1).to(raw.dtype)
    row_counts=(members.to(raw.dtype)*counts.unsqueeze(1)).sum(-1).clamp_min(1)
    return raw-torch.where((family==10).unsqueeze(1),row_counts.log(),torch.zeros_like(raw))
