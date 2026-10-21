"""Construct padded actor inputs directly in one owned CPU buffer per dtype.

Padding, feature values and mask order match the original pad/cat path. This
changes copies only, never observations, action masks or simulated clocks.
"""
import torch


def padded(observations):
    """Original path retained for CPU replay and smaller copy workloads."""
    import torch.nn.functional as F
    from .stream import tensors
    values=[tensors(o,'cpu')for o in observations]
    n=max(v['contracts'].shape[1]for v in values)
    return {'contracts':torch.cat([F.pad(v['contracts'],(0,0,0,n-v['contracts'].shape[1]))for v in values]),
        'global_state':torch.cat([v['global_state']for v in values]),
        'present':torch.cat([F.pad(v['present'],(0,n-v['contracts'].shape[1]))for v in values]),
        'masks':{k:torch.cat([F.pad(v['masks'][k],(0,n-v['contracts'].shape[1])if k=='market'
                    else(0,0,0,n-v['contracts'].shape[1]))for v in values])for k in values[0]['masks']}}


def assemble(observations):
    if not observations:raise ValueError('NONEMPTY_OBSERVATION_BATCH_REQUIRED')
    keys=tuple(observations[0]['masks'])
    specs=[]
    for name in ('contracts','global_state','present',*keys):
        mask=name in keys and name not in ('contracts','global_state','present')
        values=[torch.as_tensor(o['masks'][name]if mask else o[name],device='cpu')
                for o in observations]
        # Contract/entity dimensions are padded; global state is already fixed.
        padded=name!='global_state'
        tails=[tuple(v.shape[1:] if padded else v.shape)for v in values]
        if any(t!=tails[0]for t in tails):raise ValueError('MATCHING_OBSERVATION_FEATURE_SHAPES_REQUIRED')
        dtype=values[0].dtype
        for value in values[1:]:dtype=torch.promote_types(dtype,value.dtype)
        shape=((len(values),max(v.shape[0]for v in values),*tails[0])if padded
               else(len(values),*tails[0]))
        specs.append((name,mask,values,dtype,shape,padded))
    # Include every row of every mask, including false padding. No feature is
    # summarized, omitted or cast to a lower-precision type.
    lengths={}
    for _,_,_,dtype,shape,_ in specs:
        lengths[dtype]=lengths.get(dtype,0)+torch.Size(shape).numel()
    buffers={dtype:torch.zeros(n,dtype=dtype)for dtype,n in lengths.items()}
    offsets={dtype:0 for dtype in lengths};out={};masks={}
    for name,mask,values,dtype,shape,padded in specs:
        n=torch.Size(shape).numel();start=offsets[dtype]
        target=buffers[dtype][start:start+n].reshape(shape);offsets[dtype]+=n
        for i,value in enumerate(values):
            if padded:target[i,:value.shape[0]].copy_(value)
            else:target[i].copy_(value)
        (masks if mask else out)[name]=target
    out['masks']=masks
    return out
