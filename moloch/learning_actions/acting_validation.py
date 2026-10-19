"""Opt-in inference validation with unchanged Torch distribution arithmetic.

Validate masks and normalized categorical parameters together before sampling;
validate both Beta concentrations together. Teacher-forced/gradient paths keep
the original distribution checks. Trusted Torch samples need no repeated
log_prob support check. Invalid parameters still fail before API dispatch.
"""
import torch
from torch.distributions import Beta,Categorical
from moloch.learning_v2.policy import masked_categorical

STRICT='strict'
CONSOLIDATED='consolidated_v1'


def validate(mode):
    if type(mode)is not str or mode not in(STRICT,CONSOLIDATED):raise ValueError('EXPLICIT_ACTING_VALIDATION_MODE')
    return mode


def enabled(model,actions,sampler=None):
    fast=validate(getattr(model,'acting_validation',STRICT))==CONSOLIDATED and actions is None and not torch.is_grad_enabled()
    if fast and sampler is not None:
        from .independent_sampling import IndependentSampling
        # An arbitrary callback need not return valid Torch-distribution samples.
        fast=type(sampler)is IndependentSampling
    return fast


def categorical(logits,mask,fast):
    if not fast:return masked_categorical(logits,mask)
    if mask.dtype!=torch.bool or mask.shape!=logits.shape:raise ValueError('MECHANICS_MASK_SHAPE')
    d=Categorical(logits=logits.masked_fill(~mask,torch.finfo(logits.dtype).min),validate_args=False)
    # Real-valued logits may contain -inf for zero probability, exactly as the
    # original distribution permits; normalized NaNs remain invalid.
    feasible,valid=torch.stack((mask.any(-1).all(),(~torch.isnan(d.logits)).all())).tolist()
    if not feasible:raise ValueError('MASK_HAS_NO_FEASIBLE_ACTION')
    if not valid:raise ValueError('INVALID_CATEGORICAL_PARAMETERS')
    return d


def beta(a,b,fast):
    if not fast:return Beta(a,b)
    if not bool(((a>0)&(b>0)).all()):raise ValueError('INVALID_BETA_CONCENTRATIONS')
    return Beta(a,b,validate_args=False)


def validate_samples(actions):
    values=torch.stack([v for k,v in actions.items()if k.endswith('fraction')],-1)
    if not bool(((values>=0)&(values<=1)).all()):raise ValueError('INVALID_BETA_SAMPLES')


def actions_to_python(actions):
    """One device transfer, preserving integer types and float64 latents."""
    names=list(actions)
    if not names or any(v.ndim!=1 for v in actions.values()):raise ValueError('VECTOR_ACTING_FIELDS_REQUIRED')
    # Integer actions are categorical indices bounded by actual tensor sizes;
    # float64 represents these exactly. Raw quantity is decoded later by the API.
    matrix=torch.stack([actions[k].double()for k in names],-1).tolist()
    integers={k for k in names if not actions[k].is_floating_point()}
    return [{k:int(v)if k in integers else v for k,v in zip(names,row)}for row in matrix]
