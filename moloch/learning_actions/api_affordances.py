"""Public endpoint semantics/identifier links, never undisclosed API payloads.

All legacy columns survive. Extra features describe registered request targets
and local packet planning only; quotes and finance still require paid receipts.
"""
import numpy as np
import torch
from torch import nn
from .bulk_quotes import BulkQuoteEpisode,BulkQuoteTape,BulkQuotePolicy
from .feature_cache import identity
from .full_depth import FEATURES as LEGACY_FEATURES
from .observation_protocol import API_TARGETS

ENDPOINTS=('book','price','trade_flow','account','positions','owned_orders','collateral','realtime_token')
TYPE=slice(438,446);CONTRACT=slice(446,453);MARKET=slice(453,460);SIGNATURE=slice(460,467)
PACKET_INDEX=467;PACKET_SIZE=468;FEATURES=469


def canonical_packets(env):
    ids=env.exchanges[1:]
    return [('bulk_prices','group',tuple(ids[i:i+100]))for i in range(0,len(ids),100)]


def scope(env,venue,target):
    if venue!='SIG':return 'price',None,target[2]if len(target)>=4 else None,None
    kind=target[0];ex=market=None;packet=None
    if kind in('book','quote','trade_flow'):ex=target[1]
    elif kind=='owned_orders'and len(target)==3:
        if target[1]=='exchange':ex=target[2]
        elif target[1]=='market':market=target[2]
        elif target[1]=='cursor':
            _,ex,market=env.order_page_issued_cursors[target[2]]
    elif kind=='bulk_prices':packet=target[2]if len(target)==3 else tuple(env.quote_selection)
    if ex is not None:market=env.mapping.get(ex)
    return('price'if kind in('quote','bulk_prices')else kind),ex,market,packet


class ApiAffordanceEpisode(BulkQuoteEpisode):
    observation_protocol=API_TARGETS

    def __init__(self,*args,**kwargs):
        self.named_api_targets=True;super().__init__(*args,**kwargs)

    def observe(self):
        base=super().observe();old=base['contracts']
        values=np.zeros((len(old),FEATURES),np.float32);values[:,:LEGACY_FEATURES]=old
        # Only public request/catalog metadata is cached. Every dynamic prefix,
        # mask and age still comes from the fresh parent observation above.
        context=(tuple(base['entities']),tuple(self.exchanges),tuple(self.mapping.items()),
                 tuple(self.quote_selection),tuple(self.order_page_issued_cursors.items()))
        cached=getattr(self,'_api_metadata_cache',None)
        if cached is not None and cached[0]==context:
            values[:,LEGACY_FEATURES:]=cached[1]
            return {**base,'contracts':values,'observation_feature_schema':'raw_api_named_targets_469_v4'}
        membership={ex:(key,j,len(key[2]))for key in canonical_packets(self)for j,ex in enumerate(key[2])}
        for i,(kind,key)in enumerate(base['entities']):
            ex=market=None
            if kind in('contract','quote_add'):ex=key
            elif kind=='quote_remove':ex=self.quote_selection[key]
            elif kind=='market':market=key
            elif kind=='read':
                venue,target=key;category,ex,market,packet=scope(self,venue,target)
                if category in ENDPOINTS:values[i,TYPE.start+ENDPOINTS.index(category)]=1
                values[i,SIGNATURE]=identity('read',key)
                if packet is not None:values[i,PACKET_SIZE]=len(packet)/100.
            if ex is not None:
                values[i,CONTRACT]=identity('contract',ex);market=self.mapping.get(ex)
            if market is not None:values[i,MARKET]=identity('market',market)
            # Existing planner rows also expose the ordered members of the
            # stable public-catalog presets, without quadratic extra tokens.
            if kind=='quote_add'and ex in membership:
                packet,j,n=membership[ex];values[i,SIGNATURE]=identity('read',('SIG',packet))
                values[i,PACKET_INDEX]=j/100.;values[i,PACKET_SIZE]=n/100.
            elif kind=='quote_remove':
                values[i,SIGNATURE]=identity('read',('SIG',('bulk_prices',)))
                values[i,PACKET_INDEX]=key/100.;values[i,PACKET_SIZE]=len(self.quote_selection)/100.
        metadata=values[:,LEGACY_FEATURES:].copy();metadata.setflags(write=False)
        self._api_metadata_cache=(context,metadata)
        return {**base,'contracts':values,'observation_feature_schema':'raw_api_named_targets_469_v4'}


class ApiAffordanceTape(BulkQuoteTape):episode_type=ApiAffordanceEpisode


class ApiAffordancePolicy(BulkQuotePolicy):
    architecture='tartarus-raw-api-named-targets-gru-v9'
    observation_protocol=API_TARGETS

    def __init__(self,hidden_size=4096):
        super().__init__(hidden_size);self.contract_features=FEATURES;self.encoder[0]=nn.Linear(FEATURES,128)


def migrate_api_affordances(parent):
    if type(parent)is not BulkQuotePolicy:raise ValueError('EXACT_BULK_QUOTE_PARENT_REQUIRED_FOR_API_LABELS')
    child=ApiAffordancePolicy(parent.hidden_size).to(next(parent.parameters()).device)
    values=child.state_dict();old=parent.state_dict()
    for key,value in values.items():
        if key=='encoder.0.weight':value.zero_();value[:,:LEGACY_FEATURES].copy_(old[key])
        else:
            if value.shape!=old[key].shape:raise ValueError('UNDECLARED_API_AFFORDANCE_PARAMETER_CHANGE')
            value.copy_(old[key])
    child.load_state_dict(values);child.global_state_encoding=getattr(parent,'global_state_encoding','raw')
    child.acting_validation=getattr(parent,'acting_validation','strict');child.train(parent.training)
    return child,{'operator':'explicit_public_api_type_and_identifier_features','old_features':LEGACY_FEATURES,'new_features':FEATURES,
        'all_old_parameter_values_preserved':True,'new_columns_zero_initialized':True,
        'optimizer_reset_required':True,'old_trajectories_must_not_be_reused':True,'old_scores_must_not_be_reused':True,
        'observations_and_preset_grouping_changed':True,'independent_test_qualification':False}
