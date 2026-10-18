"""Explicit append-only capacity-feature branch; no optimizer/trajectory reuse."""
import torch
from .policy import RecurrentPolicy
from .train_inner import fingerprint


def expand_capacity_features(parent):
    if (parent.contract_features,parent.global_features)!=(20,12):raise ValueError('EXACT_LEGACY_FEATURE_LAYOUT_REQUIRED')
    child=RecurrentPolicy(24,14,parent.hidden_size).to(next(parent.parameters()).device)
    old=parent.state_dict();new=child.state_dict()
    for name,value in old.items():
        if name in ['encoder.0.weight','global_encoder.0.weight']:
            new[name].zero_();new[name][:,:value.shape[1]]=value
        else:
            if new[name].shape!=value.shape:raise ValueError('INCOMPATIBLE_NONFEATURE_ARCHITECTURE')
            new[name].copy_(value)
    child.load_state_dict(new)
    return child,{'operator':'append_capacity_features_zero_initialized','parent_parameter_sha256':fingerprint(parent),
                  'child_parameter_sha256':fingerprint(child),'optimizer_transfer':False,
                  'requires_fresh_trajectories':True,'legacy_prefix_preserved':True,
                  'new_sell_support_is_an_explicit_behavior_change':True}
