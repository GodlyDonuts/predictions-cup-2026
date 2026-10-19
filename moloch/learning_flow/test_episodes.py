import unittest
import numpy as np
from moloch.learning_v2.test_interval_episodes import PartialValuationEpisodes
from .episodes import FlowEpisode
from .test_features import FlowTests


class FlowEpisodeTests(unittest.TestCase):
    def test_prints_cannot_change_money_depth_masks_or_base_observation(self):
        fixture=PartialValuationEpisodes();base=fixture.fixture();flow=fixture.fixture(FlowEpisode)
        row=FlowTests().row(available=1000,received=1000);row['exchange_id']='x'
        flow.clock.schedule(1000,10,'trade_flow',row);base.advance(1000);flow.advance(1000)
        a,b=base.observe(),flow.observe()
        np.testing.assert_array_equal(a['contracts'],b['contracts'][:,:32]);np.testing.assert_array_equal(a['global_state'],b['global_state'])
        for name in a['masks']:np.testing.assert_array_equal(a['masks'][name],b['masks'][name])
        self.assertEqual(base.account(),flow.account());self.assertEqual(base.fill_events,flow.fill_events)
        self.assertEqual(b['contracts'].shape,(3,40));self.assertEqual(b['contracts'][1,37],1.)
        self.assertIsNone(b['wealth'])

    def test_separate_actor_schema_accepts_exact_flow_features(self):
        import torch
        from .policy import FlowPolicy
        e=PartialValuationEpisodes().fixture(FlowEpisode);o=e.observe();model=FlowPolicy(3,hidden_size=32,market_hidden_size=8)
        tensors={k:torch.as_tensor(o[k]).unsqueeze(0)for k in ['contracts','global_state','present']}
        masks={k:torch.as_tensor(v).unsqueeze(0)for k,v in o['masks'].items()}
        with torch.no_grad():r=model.step(**tensors,masks=masks,hidden=model.initial_state(1),reset=torch.tensor([True]))
        self.assertEqual(model.contract_features,40);self.assertEqual(r['hidden'].shape,(1,59))


if __name__=='__main__':unittest.main()
