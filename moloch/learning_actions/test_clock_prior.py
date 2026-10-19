import unittest
import torch
from .policy import ApiPolicy
from .clock_prior import initialize_wait_only,migrate_shared_prior


class ClockPriorTests(unittest.TestCase):
    def setUp(self):torch.manual_seed(55);torch.set_num_threads(1)

    def raw(self,model,x,family):
        return model.fractions(x+model.conditioners['family'].weight[family])

    def test_wait_only_initialization_leaves_all_order_parameters_unchanged(self):
        model=ApiPolicy(16);x=torch.randn(7,272)
        old=[self.raw(model,x,f).detach().clone()for f in range(12)]
        bias=model.fractions.bias.detach().clone();report=initialize_wait_only(model)
        for family in range(1,12):torch.testing.assert_close(old[family],self.raw(model,x,family),atol=0,rtol=0)
        torch.testing.assert_close(bias,model.fractions.bias,atol=0,rtol=0)
        shift=self.raw(model,x,0)-old[0]
        expected=torch.tensor([0.,0.,-3.-float(bias[2]),9.-float(bias[3])]).expand_as(shift)
        torch.testing.assert_close(shift,expected,atol=1e-5,rtol=1e-5)
        self.assertFalse(report['legal_deadline_domain_changed'])

    def test_migration_preserves_wait_distribution_and_order_quantities(self):
        model=ApiPolicy(16);x=torch.randn(7,272)
        with torch.no_grad():model.fractions.bias[2]=-3.;model.fractions.bias[3]=9.
        wait=self.raw(model,x,0).detach().clone();order=self.raw(model,x,1).detach().clone()
        report=migrate_shared_prior(model)
        torch.testing.assert_close(wait,self.raw(model,x,0),atol=1e-5,rtol=1e-5)
        torch.testing.assert_close(order[:,:2],self.raw(model,x,1)[:,:2],atol=0,rtol=0)
        torch.testing.assert_close(self.raw(model,x,1)[:,2:]-order[:,2:],torch.tensor([3.,-9.]).expand(7,2),atol=1e-5,rtol=1e-5)
        self.assertTrue(report['optimizer_moments_must_be_reset'])
