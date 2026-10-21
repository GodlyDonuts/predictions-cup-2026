"""Optional learned operation-specific target attention; all targets stay legal.

Zero extension preserves the legacy distribution and RNG state. Checkpoints
and behavior contracts pin the extension; inherited fitness is never reused.
"""
import torch
from torch import nn

LEGACY = 'shared_target_query_v1'
FAMILY = 'family_target_query_v1'


def validate(value):
    if value not in (LEGACY, FAMILY):
        raise ValueError('EXPLICIT_FAMILY_TARGET_QUERY_PROFILE')
    return value


def mode(model):
    value = validate(getattr(model, 'target_query_profile', LEGACY))
    embedding = getattr(model, 'target_family_embedding', None)
    if value == LEGACY:
        if embedding is not None: raise ValueError('UNDECLARED_FAMILY_TARGET_QUERY')
    elif (not isinstance(embedding, nn.Embedding)
          or tuple(embedding.weight.shape) != (12, model.market_query.out_features)):
        raise ValueError('FAMILY_TARGET_QUERY_PARAMETERS_REQUIRED')
    return value


def install(model, requested):
    requested = validate(requested); old = mode(model)
    if old == requested: return None
    if requested == LEGACY: raise ValueError('CANNOT_DISCARD_LEARNED_FAMILY_TARGET_QUERY')
    reference = model.market_query.weight
    # from_pretrained avoids an initialization draw, preserving the RNG stream.
    model.target_family_embedding = nn.Embedding.from_pretrained(
        torch.zeros(12, model.market_query.out_features, device=reference.device,
                    dtype=reference.dtype), freeze=False)
    model.target_query_profile = requested
    return dict(old=old, new=requested, oldParametersPreserved=True,
                newParameters=int(model.target_family_embedding.weight.numel()),
                zeroInitialized=True, initialActionDistributionPreserved=True,
                optimizerMomentsResetRequired=True, freshTrajectoriesAndScoresRequired=True,
                availableActionsChanged=False)


def query(model, hidden, family, *, shared=None):
    if shared is None: shared = model.market_query(hidden)
    if mode(model) == LEGACY: return shared
    return shared + model.target_family_embedding(family)
