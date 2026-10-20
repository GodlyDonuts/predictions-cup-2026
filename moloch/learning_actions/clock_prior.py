"""Initialize short WAIT alarms without biasing order expiration toward 20ms.

Uses the existing trainable family embedding and linear fraction head. No new
architecture, order mask, lifetime floor or fixed action is introduced.
"""
import torch


def _shift_wait(model,alpha,beta):
    weight=model.fractions.weight.detach().cpu().double()
    target=torch.tensor([0.,0.,alpha,beta],dtype=torch.float64)
    delta=weight.T@torch.linalg.solve(weight@weight.T,target)
    if not bool(torch.isfinite(delta).all()):raise ValueError('FINITE_WAIT_PRIOR_REPARAMETERIZATION_REQUIRED')
    before=model.conditioners['family'].weight[0].detach().clone()
    try:
        with torch.no_grad():model.conditioners['family'].weight[0].add_(delta.to(device=before.device,dtype=before.dtype))
        actual=(model.conditioners['family'].weight[0].detach()-before).cpu().double()
        residual=float((weight@actual-target).abs().max())
        if residual>1e-4:raise ValueError('WAIT_PRIOR_REPARAMETERIZATION_RESIDUAL')
    except BaseException:
        with torch.no_grad():model.conditioners['family'].weight[0].copy_(before)
        raise
    return {'maximum_linear_residual':residual,'embedding_shift_norm':float(actual.norm()),
            'legal_deadline_domain_changed':False,'quantity_head_shift_target':0.}


def initialize_wait_only(model, alpha_logit=-3., beta_logit=9.):
    import math
    if any(type(v) not in (int, float) or not math.isfinite(v)
           for v in (alpha_logit, beta_logit)):
        raise ValueError('FINITE_WAIT_PRIOR_LOGITS_REQUIRED')
    bias=model.fractions.bias.detach().cpu().double()
    return {'operator':'initialize_wait_only_short_alarm_prior',
            'alpha_logit':alpha_logit,'beta_logit':beta_logit,
            **_shift_wait(model,alpha_logit-float(bias[2]),beta_logit-float(bias[3]))}


def migrate_shared_prior(model):
    before=model.fractions.bias.detach().clone()
    try:
        with torch.no_grad():model.fractions.bias[2]+=3.;model.fractions.bias[3]-=9.
        report=_shift_wait(model,-3.,9.)
    except BaseException:
        with torch.no_grad():model.fractions.bias.copy_(before)
        raise
    return {'operator':'migrate_global_short_alarm_to_wait_only',**report,
            'WAIT_distribution_preserved_up_to_floating_point':True,
            'order_expiration_distribution_intentionally_changed':True,'optimizer_moments_must_be_reset':True}
