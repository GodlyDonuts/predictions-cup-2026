import unittest
import torch
from .policy import ApiPolicy
from .exploration_prior import mutate_family_logits


class ExplorationPriorTests(unittest.TestCase):
    def setUp(self):torch.manual_seed(71);torch.set_num_threads(1)

    def test_changes_only_trainable_family_bias_with_full_finite_choice_support(self):
        m=ApiPolicy(16);x=torch.randn(8,272);before={k:v.clone()for k,v in m.state_dict().items()}
        logits=m.heads['family'](x).detach();shifts=[0.]*12;shifts[0]=4.;shifts[10]=1.
        r=mutate_family_logits(m,shifts)
        torch.testing.assert_close(m.heads['family'](x)-logits,torch.tensor(shifts).expand(8,12),atol=1e-6,rtol=1e-6)
        for k,v in m.state_dict().items():
            if k!='heads.family.bias':torch.testing.assert_close(v,before[k],atol=0,rtol=0)
        self.assertTrue(m.heads['family'].bias.requires_grad)
        self.assertTrue((m.heads['family'](x).softmax(-1)>0).all())
        self.assertFalse(r['action_masks_changed']);self.assertFalse(r['decision_cap_added'])

    def test_zero_and_invalid_mutations_preserve_original_weights(self):
        m=ApiPolicy(16);before={k:v.clone()for k,v in m.state_dict().items()}
        mutate_family_logits(m,[0.]*12)
        for bad in([0.]*11,[True]*12,[float('nan')]*12,[1e300]*12):
            with self.assertRaises(ValueError):mutate_family_logits(m,bad)
        for k,v in m.state_dict().items():torch.testing.assert_close(v,before[k],atol=0,rtol=0)
