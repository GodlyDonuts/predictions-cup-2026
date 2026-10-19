"""Case-local draws for batched conditional heads, with original Torch kernels.

Categorical uses the same multinomial operation as Categorical.sample. Beta
uses its installed Dirichlet sampling kernel with an explicit generator. This
private Torch operation is covered by kernel/RNG regression checks; unsupported
Torch runtimes fail instead of substituting a different sampling law.
"""
import torch


class IndependentSampling:
    def __init__(self,generators,entity_counts):
        if not generators or len(generators)!=len(entity_counts)or any(type(n)is not int or n<1 for n in entity_counts):
            raise ValueError('ONE_GENERATOR_AND_ENTITY_COUNT_PER_CASE')
        self.generators=generators;self.entity_counts=entity_counts

    def categorical(self,dist,*,market=False):
        if torch.is_grad_enabled():raise ValueError('INDEPENDENT_SAMPLING_IS_ACTING_ONLY')
        probs=dist.probs
        if probs.ndim!=2 or probs.shape[0]!=len(self.generators):raise ValueError('EXACT_CASE_BATCH_REQUIRED')
        return torch.cat([torch.multinomial(probs[i:i+1,:self.entity_counts[i]]if market else probs[i:i+1],
            1,True,generator=g).reshape(1)for i,g in enumerate(self.generators)])

    def beta(self,dist):
        if torch.is_grad_enabled():raise ValueError('INDEPENDENT_SAMPLING_IS_ACTING_ONLY')
        concentration=torch.stack((dist.concentration1,dist.concentration0),-1)
        if concentration.shape!=(len(self.generators),2):raise ValueError('EXACT_SCALAR_BETA_CASE_BATCH_REQUIRED')
        return torch.cat([torch._sample_dirichlet(concentration[i:i+1],generator=g).select(-1,0)
                          for i,g in enumerate(self.generators)])
