import unittest
import torch
from .policy import RecurrentPolicy, masked_categorical, HEAD_SIZES


def masks(batch=2, markets=3):
    return {'market': torch.ones(batch, markets, dtype=torch.bool),
            'family': torch.ones(batch, markets, HEAD_SIZES['family'], dtype=torch.bool),
            'side': torch.ones(batch, markets, HEAD_SIZES['family'], 2, dtype=torch.bool),
            **{name:torch.ones(batch, markets, HEAD_SIZES['family'], 2, HEAD_SIZES[name], dtype=torch.bool)
               for name in ['price', 'patience']}}


class RecurrentActor(unittest.TestCase):
    def setUp(self):
        torch.manual_seed(107)
        torch.set_num_threads(2)
        self.p = RecurrentPolicy()
        self.x = torch.randn(2, 3, 16)
        self.g = torch.randn(2, 12)
        self.present = torch.ones(2, 3, dtype=torch.bool)
        self.reset = torch.ones(2, dtype=torch.bool)
        self.m = masks()

    def step(self, **kw):
        return self.p.step(self.x, self.g, self.present, self.m,
                           self.p.initial_state(2), self.reset, **kw)

    def test_measured_candidate_parameter_range(self):
        self.assertTrue(500_000 <= sum(p.numel() for p in self.p.parameters()) <= 3_000_000)

    def test_sampled_likelihood_recomputed_for_identical_policy(self):
        a = self.step()
        b = self.step(actions=a['actions'])
        self.assertTrue(torch.allclose(a['logprob'], b['logprob'], atol=1e-6))
        self.assertTrue(torch.isfinite(a['logprob']).all())
        self.assertEqual(tuple(a['monetary_quantiles'].shape), (2, 9))

    def test_masked_action_never_selected(self):
        self.m['market'][:, :2] = False
        self.m['family'][:, :, 1:] = False
        out = self.step()['actions']
        self.assertEqual(out['market'].tolist(), [2, 2])
        self.assertEqual(out['family'].tolist(), [0, 0])

    def test_side_mask_really_depends_on_selected_family(self):
        self.m['family'][:] = False
        self.m['family'][:, :, 2] = True
        self.m['side'][:, :, 2, 0] = False
        out = self.step()['actions']
        self.assertEqual(out['family'].tolist(), [2, 2])
        self.assertEqual(out['side'].tolist(), [1, 1])

    def test_all_false_mask_fails_instead_of_unmasking_impossible_trade(self):
        with self.assertRaisesRegex(ValueError, 'NO_FEASIBLE_ACTION'):
            masked_categorical(torch.zeros(2, 3), torch.zeros(2, 3, dtype=torch.bool))

    def test_hidden_state_resets_at_real_episode_boundary(self):
        a = self.p.encode(self.x, self.g, self.present, torch.randn(2, 512), self.reset)[1]
        b = self.p.encode(self.x, self.g, self.present, torch.zeros(2, 512), self.reset)[1]
        self.assertTrue(torch.equal(a, b))

    def test_set_encoder_does_not_depend_on_contract_order(self):
        _, a = self.p.encode(self.x, self.g, self.present, self.p.initial_state(2), self.reset)
        _, b = self.p.encode(self.x[:, [2, 0, 1]], self.g, self.present, self.p.initial_state(2), self.reset)
        self.assertTrue(torch.allclose(a, b, atol=1e-6))

    def test_masked_padding_does_not_change_portfolio_encoding(self):
        _, a = self.p.encode(self.x, self.g, self.present, self.p.initial_state(2), self.reset)
        extra = torch.cat([self.x, torch.randn(2, 1, 16)*100], 1)
        mask = torch.cat([self.present, torch.zeros(2, 1, dtype=torch.bool)], 1)
        _, b = self.p.encode(extra, self.g, mask, self.p.initial_state(2), self.reset)
        self.assertTrue(torch.allclose(a, b, atol=1e-6))

    def test_gradient_reaches_encoder_recurrent_and_action_heads(self):
        out = self.step()
        loss = -out['logprob'].mean() + out['value'].square().mean()
        loss.backward()
        for module in [self.p.encoder[0], self.p.recurrent, self.p.heads['family']]:
            grad = next(module.parameters()).grad
            self.assertIsNotNone(grad)
            self.assertTrue(torch.isfinite(grad).all())


if __name__ == '__main__':
    unittest.main()
