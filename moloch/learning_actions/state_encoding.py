"""Explicit numerical conditioning, not market/risk/action restrictions."""
import numpy as np

RAW='raw'
SIGNED_LOG='signed_log_time_counts_v1'
# Elapsed/remaining seconds, known order/builder counts, budget wait seconds,
# string-valued batch-result count, and observable finance-publication count.
# Legacy checkpoints used internal finance revision at15; protocol migration
# is distinct from this numerical transform. Prices, money,
# flags and rolling-budget fractions are not transformed.
INDICES=(2,3,4,5,7,12,15)


def validate(value):
    if type(value)is not str or value not in(RAW,SIGNED_LOG):raise ValueError('EXPLICIT_GLOBAL_STATE_ENCODING')
    return value


def encode(state,mode):
    validate(mode)
    if mode==RAW:return state
    out=state.copy();values=out[list(INDICES)]
    out[list(INDICES)]=np.sign(values)*np.log1p(np.abs(values))
    if not np.isfinite(out).all():raise ValueError('FINITE_ENCODED_GLOBAL_STATE_REQUIRED')
    return out


def matching(environment,model):
    from .historical_memory import bound_profile
    if bound_profile(environment)!=bound_profile(model):
        raise ValueError('MODEL_AND_ENVIRONMENT_HISTORICAL_MEMORY_MISMATCH')
    if validate(getattr(environment,'global_state_encoding',RAW))!=validate(getattr(model,'global_state_encoding',RAW)):
        raise ValueError('MODEL_AND_OBSERVATION_STATE_ENCODING_MISMATCH')
    from .observation_protocol import validate as protocol,CURRENT
    if protocol(getattr(environment,'observation_protocol',CURRENT))!=protocol(getattr(model,'observation_protocol',CURRENT)):
        raise ValueError('MODEL_AND_OBSERVATION_PROTOCOL_MISMATCH')
    from .read_target_sampling import validate as sampling_mode,FLAT
    if sampling_mode(getattr(environment,'read_target_sampling',FLAT))!=sampling_mode(getattr(model,'read_target_sampling',FLAT)):
        raise ValueError('MODEL_AND_ENVIRONMENT_READ_SAMPLING_MISMATCH')
    from .managed_data_service import profile
    if profile(getattr(environment,'managed_data_profile',None))!=profile(getattr(model,'managed_data_profile',None)):
        raise ValueError('MODEL_AND_ENVIRONMENT_MANAGED_DATA_MISMATCH')
    from .price_parameterization import validate as price_mode,ABSOLUTE
    if price_mode(getattr(environment,'price_parameterization',ABSOLUTE))!=price_mode(getattr(model,'price_parameterization',ABSOLUTE)):
        raise ValueError('MODEL_AND_ENVIRONMENT_PRICE_PARAMETERIZATION_MISMATCH')
