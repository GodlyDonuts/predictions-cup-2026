"""Installed-kernel and RNG identity checks; no financial data is used."""
import unittest
import torch
from torch.distributions import Categorical,Beta
from .independent_sampling import IndependentSampling


class IndependentSamplingTests(unittest.TestCase):
    def check_device(self,device):
        counts=[2,7,4];generators=[torch.Generator(device=device).manual_seed(i+808)for i in range(3)]
        sampler=IndependentSampling(generators,counts);global_generator=torch.Generator(device=device).manual_seed(1)
        def get():return torch.cuda.get_rng_state(device)if device.startswith('cuda')else torch.get_rng_state()
        def put(v):torch.cuda.set_rng_state(v,device)if device.startswith('cuda')else torch.set_rng_state(v)
        original=get().clone();expected=[];final=[]
        logits=torch.arange(21,device=device,dtype=torch.float32).reshape(3,7)/20
        for i,n in enumerate(counts):logits[i,n:]=-1e9
        a=torch.tensor([1.01,4.2,100.],device=device,dtype=torch.float64)
        b=torch.tensor([9.,1.05,100.],device=device,dtype=torch.float64)
        try:
            for i in range(3):
                put(generators[i].get_state());row=[]
                for _ in range(10):
                    row.append((Categorical(logits=logits[i:i+1,:counts[i]]).sample(),Beta(a[i:i+1],b[i:i+1]).sample()))
                expected.append(row);final.append(get().clone())
            put(original)
            with torch.no_grad():
                for step in range(10):
                    categories=sampler.categorical(Categorical(logits=logits),market=True)
                    values=sampler.beta(Beta(a,b))
                    for i in range(3):
                        self.assertTrue(torch.equal(categories[i:i+1],expected[i][step][0]))
                        torch.testing.assert_close(values[i:i+1],expected[i][step][1],atol=0,rtol=0)
            self.assertTrue(torch.equal(get(),original))
            for i,g in enumerate(generators):self.assertTrue(torch.equal(g.get_state(),final[i]))
        finally:put(original)

    def test_cpu_draws_and_states_match_original_distribution_kernels(self):self.check_device('cpu')

    @unittest.skipUnless(torch.cuda.is_available(),'CUDA unavailable')
    def test_cuda_draws_and_states_match_original_distribution_kernels(self):self.check_device('cuda:0')

    def test_acting_only_and_shape_validation(self):
        with self.assertRaisesRegex(ValueError,'ONE_GENERATOR'):IndependentSampling([],[])
        sampler=IndependentSampling([torch.Generator().manual_seed(1)],[2])
        with self.assertRaisesRegex(ValueError,'ACTING_ONLY'):sampler.beta(Beta(torch.ones(1),torch.ones(1)))
        with torch.no_grad():
            with self.assertRaisesRegex(ValueError,'EXACT_CASE'):sampler.categorical(Categorical(logits=torch.zeros(2,2)))
