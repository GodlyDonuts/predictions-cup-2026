"""All API price levels from delivered book snapshots, not hidden world books.

The 199 prices per side are the complete documented .005 lattice, not a
top-k/quantity cap. Original integer depth still governs execution. Log depth
is a floating-point model input, not an accounting or liquidity substitute.
"""
import numpy as np
import torch
from torch import nn
from .contract import TICK,UNIT
from .engine import ApiEpisode,ApiTape
from .policy import ApiPolicy
from .feature_cache import identity as _identity,log_quantity

LEVELS=UNIT//TICK-1
FEATURES=40+2*LEVELS
SCHEMA='raw_api_full_delivered_depth_438_v3'


def planning_observation(env,base):
    """Own draft orders and issued action are local memory, not API evidence."""
    rows=[];entities=[]
    for index,spec in enumerate(env.builder):
        row=np.zeros(40,np.float32);row[:9]=[(spec.price or 0)/UNIT,log_quantity(spec.quantity)/16.,
            float(spec.side=='YES'),float(spec.action=='BUY'),float(spec.market),
            np.log1p(max(0,(spec.expiration_ms or env.now)-env.now))/32.,
            float(spec.expiration_ms is not None),float(spec.expiration_ms is not None and spec.expiration_ms<=env.now),index/50.]
        row[24]=5.;row[33:40]=_identity('contract',spec.exchange)
        rows.append(row);entities.append(('builder_order',index))
    action=getattr(env,'_last_local_action',None)
    if action is not None:
        row=np.zeros(40,np.float32);a=action['action']
        row[:7]=[a['family']/11.,a['price']/199.,a['side'],a['patience'],a['target_fraction'],a['child_fraction'],
            (env.now-action['at_ms'])/1000.]
        row[8:15]=_identity('local_result_status',action['result_status'])
        row[24]=6.;row[33:40]=action['target_identity']
        if 'query_filter'in a:
            row[15:18]=[a['query_filter'],a['query_from_fraction'],a['query_to_fraction']]
        rows.append(row);entities.append(('local_action',0))
    if not rows:return base
    extra=len(rows)
    return {**base,'contracts':np.concatenate((base['contracts'],np.stack(rows))),
        'present':np.concatenate((base['present'],np.ones(extra,bool))),
        'masks':{k:np.concatenate((v,np.zeros((extra,*v.shape[1:]),dtype=v.dtype)))for k,v in base['masks'].items()},
        'entities':[*base['entities'],*entities]}


def extend_observation(env,base):
    base=planning_observation(env,base)
    old=base['contracts'];features=np.zeros((len(old),FEATURES),np.float32);features[:,:40]=old
    cache=getattr(env,'_delivered_depth_cache',None)
    if cache is None:cache=env._delivered_depth_cache={}
    for i,(kind,key)in enumerate(base['entities']):
        if kind!='contract':continue
        observed=env.actor_books.get(key)
        if observed is None:continue
        cached=cache.get(key)
        if cached is not None and cached[0]is observed.book:
            features[i,40:]=cached[1];continue
        for side,name in enumerate(('bids','asks')):
            for price,quantity in getattr(observed.book,name):
                if type(price)is not int or not TICK<=price<=UNIT-TICK or price%TICK:
                    raise ValueError('DELIVERED_DEPTH_OUTSIDE_OFFICIAL_PRICE_LATTICE')
                if type(quantity)is not int or quantity<=0:raise ValueError('DELIVERED_DEPTH_POSITIVE_INTEGER_QUANTITY')
                index=40+side*LEVELS+price//TICK-1
                if features[i,index]!=0:raise ValueError('DUPLICATE_DELIVERED_PRICE_LEVEL')
                features[i,index]=log_quantity(quantity)/16.
        encoded=features[i,40:].copy();encoded.setflags(write=False);cache[key]=(observed.book,encoded)
    if not np.isfinite(features).all():raise ValueError('FINITE_FULL_DEPTH_OBSERVATION_REQUIRED')
    return {**base,'contracts':features,'observation_feature_schema':SCHEMA}


class FullDepthEpisode(ApiEpisode):
    def observe(self):return extend_observation(self,super().observe())

    def act_raw(self,action,observation):
        result=super().act_raw(action,observation)
        kind,key=observation['entities'][int(action['market'])]
        self._last_local_action={'action':dict(action),'at_ms':self.now,'target_identity':_identity(kind,key),
            # Store the immediate string, not the mutable dispatch record that
            # can change on server arrival before the actor receives its ACK.
            'result_status':str(result['status'])}
        return result


class FullDepthTape(ApiTape):
    episode_type=FullDepthEpisode


class FullDepthPolicy(ApiPolicy):
    architecture='tartarus-raw-api-full-depth-family-first-gru-v3'

    def __init__(self,hidden_size=4096):
        super().__init__(hidden_size)
        self.contract_features=FEATURES;self.encoder[0]=nn.Linear(FEATURES,128)


def migrate_legacy_policy(parent):
    if type(parent)is not ApiPolicy:raise ValueError('EXACT_LEGACY_RAW_POLICY_REQUIRED_FOR_DEPTH_MIGRATION')
    child=FullDepthPolicy(parent.hidden_size).to(next(parent.parameters()).device)
    child.global_state_encoding=getattr(parent,'global_state_encoding','raw')
    child.observation_protocol=parent.observation_protocol
    old=parent.state_dict();values=child.state_dict()
    for name,target in values.items():
        if name=='encoder.0.weight':
            target.zero_();target[:,:40].copy_(old[name])
        else:
            if target.shape!=old[name].shape:raise ValueError('UNDECLARED_DEPTH_MIGRATION_SHAPE_CHANGE')
            target.copy_(old[name])
    child.load_state_dict(values)
    if any(not torch.equal(value[:,:40]if name=='encoder.0.weight'else value,old[name])for name,value in child.state_dict().items()):
        raise ValueError('DEPTH_MIGRATION_CHANGED_EXISTING_PARAMETERS')
    return child,{'operator':'explicit_full_depth_input_extension','old_features':40,'new_features':FEATURES,
        'all_existing_weights_preserved_exactly':True,'new_columns_initialized_zero':True,
        'optimizer_reset_required':True,'old_trajectories_must_not_be_reused':True,'legal_action_support_changed':False}
