"""Coalesce inference tensors by dtype into one device transfer per dtype.

No float conversion or inference math changes. Used only for detached actor
observations/results, not differentiable model parameters or training graphs.
"""
import torch


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
