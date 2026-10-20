import tempfile,unittest
from pathlib import Path
import numpy as np
import torch
from .price_parameterization import ABSOLUTE,QUOTE_RANK,permutation,tick
from .test_full_depth import depth_environment
from .full_depth import FullDepthPolicy
from .training import save_checkpoint,load_checkpoint
from .behavior_guard import contract,require_contract
from .state_encoding import matching
from .exploration_prior import mutate_price_logits
from moloch.learning_v2.liquidity import Book


class PriceVocabularyTests(unittest.TestCase):
    def setUp(self):torch.set_num_threads(1);torch.manual_seed(49)

    def feature(self):
        x=np.zeros(438,np.float32);x[40+80-1]=1;x[239+90-1]=1;return x

    def test_every_tick_and_market_order_remain_available_for_both_outcomes(self):
        x=self.feature()
        for side in(0,1):
            for family in(1,2,5,6):
                p=permutation(x,side,family);self.assertEqual(sorted(p),list(range(200)))
                self.assertEqual(len(set(p)),200);self.assertEqual(tick(QUOTE_RANK,0,x,side,family),0)
        self.assertEqual(permutation(x,0,1)[:3],[0,80,90])
        self.assertEqual(permutation(x,0,2)[:3],[0,90,80])
        self.assertEqual(permutation(x,1,1)[:3],[0,110,120])
        self.assertEqual(permutation(x,1,2)[:3],[0,120,110])
        self.assertEqual(permutation(np.zeros(438),0,1),list(range(200)))
        for rank in range(200):self.assertEqual(tick(ABSOLUTE,rank,x,1,2),rank)

    def test_mapping_uses_exact_actor_observation_not_later_book_updates(self):
        e=depth_environment();e.price_parameterization=QUOTE_RANK
        e.request_observation('SIG',('book','x'));e.advance(101);o=e.observe()
        i=o['entities'].index(('contract','x'))
        e.actor_books['x'].update(Book('x',101,101,((800000,100),),((850000,100),),'f'*64))
        action={'family':5,'market':i,'side':0,'price':1,'patience':0,'target_fraction':0.,'child_fraction':0.}
        self.assertEqual(e.act_raw(action,o)['status'],'BUILDER_UPDATED')
        self.assertEqual(e.builder[-1].price,400000)
        e.price_parameterization=ABSOLUTE;e.act_raw(action,o);self.assertEqual(e.builder[-1].price,5000)

    def test_checkpoint_contract_and_environment_pin_the_parameterization(self):
        e=depth_environment();m=FullDepthPolicy(16);old=contract(m);m.price_parameterization=QUOTE_RANK
        with self.assertRaisesRegex(ValueError,'PROTOCOL_CHANGED'):require_contract(m,old)
        with self.assertRaisesRegex(ValueError,'EXPLICIT_BEHAVIOR'):require_contract(m,None)
        with self.assertRaisesRegex(ValueError,'PRICE_PARAMETERIZATION_MISMATCH'):matching(e,m)
        e.price_parameterization=QUOTE_RANK;matching(e,m)
        with tempfile.TemporaryDirectory()as d:
            p=Path(d)/'model.pt';h=save_checkpoint(p,m,torch.optim.Adam(m.parameters()),dataset_manifest_sha256='a'*64,split_specification_sha256='b'*64)
            loaded,_=load_checkpoint(p,h);self.assertEqual(loaded.price_parameterization,QUOTE_RANK)

    def test_initial_price_preferences_change_only_the_trainable_price_bias(self):
        m=FullDepthPolicy(16);before={k:v.clone()for k,v in m.state_dict().items()};delta=[0.]*200;delta[1]=4.
        result=mutate_price_logits(m,delta)
        self.assertFalse(result['legal_price_support_changed'])
        for name,value in m.state_dict().items():
            if name!='heads.price.bias':self.assertTrue(torch.equal(value,before[name]),name)
        self.assertTrue(bool((m.heads['price'].bias.softmax(0)>0).all()))
        self.assertAlmostEqual(float((m.heads['price'].bias-before['heads.price.bias'])[1].detach()),4.)


if __name__=='__main__':unittest.main()
