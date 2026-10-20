"""Process-local guards for on-policy chunks; checkpoints retain full hashes.

The opt-in fast guard supports the fixed, buffer-free ApiPolicy and ordinary
PyTorch in-place updates/load_state_dict/parameter replacement. Tensor.data or
external storage writes that bypass PyTorch version counters are unsupported;
use the full sha256 mode when allowing such external mutation paths.
"""
import hashlib
from .policy import ApiPolicy
from moloch.learning_v2.train_inner import fingerprint


def contract(model):
    from .acting_validation import validate,STRICT
    value=(model.architecture,model.observation_protocol,
            getattr(model,'global_state_encoding','raw'),
            bool(getattr(model,'local_memory_joint_density',False)),validate(getattr(model,'acting_validation',STRICT)))
    from .read_target_sampling import validate as sampling_mode,FLAT
    mode=sampling_mode(getattr(model,'read_target_sampling',FLAT))
    if mode!=FLAT:value=value+(mode,)
    from .managed_data_service import profile
    extension=profile(getattr(model,'managed_data_profile',None))
    return value if extension is None else value+(extension,)


def require_contract(model,value):
    if value is None:
        from .read_target_sampling import validate as sampling_mode,FLAT
        from .managed_data_service import profile
        if (getattr(model,'local_memory_joint_density',False)or
                sampling_mode(getattr(model,'read_target_sampling',FLAT))!=FLAT or
                profile(getattr(model,'managed_data_profile',None))is not None):
            raise ValueError('EXPLICIT_BEHAVIOR_CONTRACT_REQUIRED')
    elif tuple(value)!=contract(model):raise ValueError('BEHAVIOR_ARCHITECTURE_OR_PROTOCOL_CHANGED')


def identity(model,mode='sha256'):
    if mode=='sha256':return fingerprint(model)
    if mode!='tensor_versions_v1':raise ValueError('EXPLICIT_ON_POLICY_GUARD_MODE')
    from .full_depth import FullDepthPolicy
    from .query_policy import QueryWindowPolicy
    from .joint_memory_policy import JointMemoryPolicy
    from .order_policy import OrderPagePolicy
    from .component_execution import ComponentPolicy
    from .bulk_quotes import BulkQuotePolicy
    from .api_affordances import ApiAffordancePolicy
    from .historical_neural import HistoricalNeuralPolicy
    if type(model)not in(ApiPolicy,FullDepthPolicy,QueryWindowPolicy,JointMemoryPolicy,OrderPagePolicy,ComponentPolicy,BulkQuotePolicy,ApiAffordancePolicy,HistoricalNeuralPolicy)or list(model.named_buffers()):raise ValueError('FAST_GUARD_REQUIRES_FIXED_BUFFER_FREE_RAW_POLICY')
    values=[id(model),model.training,contract(model)]
    for name,p in model.named_parameters():
        values.append((name,id(p),p.data_ptr(),tuple(p.shape),tuple(p.stride()),str(p.dtype),
                       str(p.device),p.requires_grad,p._version))
    return hashlib.sha256(repr(values).encode()).hexdigest()
