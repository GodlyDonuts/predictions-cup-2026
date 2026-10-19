"""Coalesce inference tensors by dtype into one device transfer per dtype.

No float conversion or inference math changes. Used only for detached actor
observations/results, not differentiable model parameters or training graphs.
"""
import torch
import numpy as np


def ipc_pack(tree):
    """Own exact CPU bytes before Queue's asynchronous serializer sees them.

    Per-frame Torch storage handles are expensive on Windows. NumPy payloads
    use ordinary bounded copies without a handle exchange for every small leaf.
    This is transport only: no dtype, shape, inference or accounting change.
    """
    if isinstance(tree,dict):return {k:ipc_pack(v)for k,v in tree.items()}
    if not isinstance(tree,torch.Tensor)or tree.device.type!='cpu':raise TypeError('CPU_TENSOR_TREE_REQUIRED')
    return tree.detach().numpy().copy()


def ipc_unpack(tree):
    if isinstance(tree,dict):return {k:ipc_unpack(v)for k,v in tree.items()}
    if not isinstance(tree,np.ndarray)or tree.dtype.kind not in 'biuf':raise TypeError('NUMERIC_ARRAY_TREE_REQUIRED')
    return torch.from_numpy(tree)


def transfer(tree,device):
    groups={};layout={}
    def collect(node,path=()):
        if isinstance(node,dict):
            for key,value in node.items():collect(value,path+(key,))
        elif isinstance(node,torch.Tensor):
            group=groups.setdefault(node.dtype,[]);layout[path]=(node.dtype,len(group),tuple(node.shape),node.numel())
            group.append(node.detach().reshape(-1))
        else:raise TypeError('TENSOR_TREE_REQUIRED')
    collect(tree);moved={}
    for dtype,values in groups.items():
        packed=torch.cat(values).to(device);offset=0;views=[]
        for value in values:views.append(packed[offset:offset+value.numel()]);offset+=value.numel()
        moved[dtype]=views
    def restore(node,path=()):
        if isinstance(node,dict):return {key:restore(value,path+(key,))for key,value in node.items()}
        dtype,index,shape,_=layout[path];return moved[dtype][index].reshape(shape)
    return restore(tree)
