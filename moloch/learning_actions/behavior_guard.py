"""Process-local guards for on-policy chunks; checkpoints retain full hashes.

The opt-in fast guard supports the fixed, buffer-free ApiPolicy and ordinary
PyTorch in-place updates/load_state_dict/parameter replacement. Tensor.data or
external storage writes that bypass PyTorch version counters are unsupported;
use the full sha256 mode when allowing such external mutation paths.
"""
import hashlib
from .policy import ApiPolicy
from moloch.learning_v2.train_inner import fingerprint


def identity(model,mode='sha256'):
    if mode=='sha256':return fingerprint(model)
    if mode!='tensor_versions_v1':raise ValueError('EXPLICIT_ON_POLICY_GUARD_MODE')
    if type(model)is not ApiPolicy or list(model.named_buffers()):raise ValueError('FAST_GUARD_REQUIRES_FIXED_BUFFER_FREE_RAW_POLICY')
    values=[id(model),model.training]
    for name,p in model.named_parameters():
        values.append((name,id(p),p.data_ptr(),tuple(p.shape),tuple(p.stride()),str(p.dtype),
                       str(p.device),p.requires_grad,p._version))
    return hashlib.sha256(repr(values).encode()).hexdigest()
