"""An optional bijection of the complete legal limit-price lattice.

Rank zero remains a market order. Other ranks enumerate every official tick,
starting around the selected outcome's delivered quotes when available. The
mapping never reads the later hidden exchange book or removes a price.
"""
import numpy as np

ABSOLUTE='absolute_ticks_v1'
QUOTE_RANK='observed_quote_rank_v1'


def validate(mode):
    if type(mode)is not str or mode not in(ABSOLUTE,QUOTE_RANK):raise ValueError('EXPLICIT_PRICE_PARAMETERIZATION')
    return mode


def policy_mode(model):
    mode=validate(getattr(model,'price_parameterization',ABSOLUTE))
    if mode==QUOTE_RANK and model.contract_features<438:raise ValueError('DELIVERED_FULL_DEPTH_POLICY_REQUIRED')
    return mode


def permutation(features,side,family):
    if side not in(0,1)or family not in(1,2,5,6):raise ValueError('EXPLICIT_OUTCOME_AND_ORDER_FAMILY')
    if len(features)<438:raise ValueError('DELIVERED_FULL_DEPTH_REQUIRED')
    bids=(np.flatnonzero(np.asarray(features[40:239])>0)+1).tolist()
    asks=(np.flatnonzero(np.asarray(features[239:438])>0)+1).tolist()
    if side:bids,asks=[200-v for v in asks],[200-v for v in bids]
    bid=max(bids)if bids else None;ask=min(asks)if asks else None
    maker,other=(bid,ask)if family in(1,5)else(ask,bid)
    anchors=list(dict.fromkeys(v for v in(maker,other)if v is not None))
    if not anchors:return list(range(200))
    remaining=sorted(set(range(1,200))-set(anchors),key=lambda v:(abs(v-anchors[0]),v))
    return [0,*anchors,*remaining]


def tick(mode,rank,features,side,family):
    validate(mode)
    if type(rank)is not int or not 0<=rank<=199:raise ValueError('FULL_PRICE_RANK_DOMAIN_REQUIRED')
    if mode==ABSOLUTE or rank==0:return rank
    return permutation(features,side,family)[rank]
