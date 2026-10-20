"""Explicit research initialization of trainable operation preferences."""
import math
import torch


def mutate_family_logits(model,shifts):
    bias=model.heads['family'].bias
    if (not isinstance(shifts,list)or len(shifts)!=bias.numel()
            or any(type(x)not in(int,float)or not math.isfinite(x)for x in shifts)):
        raise ValueError('FINITE_FULL_FAMILY_LOGIT_SHIFT_VECTOR_REQUIRED')
    delta=torch.tensor(shifts,device=bias.device,dtype=bias.dtype)
    target=bias.detach()+delta
    if not bool(torch.isfinite(target).all()):raise ValueError('FINITE_FAMILY_PRIOR_PARAMETERS_REQUIRED')
    if any(shifts):
        with torch.no_grad():bias.copy_(target)
    return {'operator':'explicit_trainable_family_logit_mutation','logit_shifts':shifts,
            'action_masks_changed':False,'conditional_order_parameters_changed':False,
            'decision_cap_added':False,'optimizer_moments_must_be_reset':True,
            'fresh_trajectories_and_scores_required':True}


def mutate_price_logits(model,shifts):
    bias=model.heads['price'].bias
    if(not isinstance(shifts,list)or len(shifts)!=200 or bias.numel()!=200
            or any(type(x)not in(int,float)or not math.isfinite(x)for x in shifts)):
        raise ValueError('FINITE_FULL_PRICE_LOGIT_SHIFT_VECTOR_REQUIRED')
    target=bias.detach()+torch.tensor(shifts,device=bias.device,dtype=bias.dtype)
    if not bool(torch.isfinite(target).all()):raise ValueError('FINITE_PRICE_PRIOR_PARAMETERS_REQUIRED')
    with torch.no_grad():bias.copy_(target)
    return {'operator':'explicit_trainable_price_logit_mutation','logit_shifts':shifts,
            'legal_price_support_changed':False,'quantity_parameters_changed':False,
            'optimizer_moments_must_be_reset':True,'fresh_trajectories_and_scores_required':True}
