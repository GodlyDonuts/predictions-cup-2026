"""Feature memoization must never freeze dynamic evidence or truncate entities."""
import hashlib
import unittest
import numpy as np
import torch
from .feature_cache import identity,log_quantity
from .test_actions import environment
from .contract import OrderSpec
from .full_depth import extend_observation
from .stream import tensors
from .batched import inputs
from moloch.learning_v2.liquidity import Book,SharedLiquidity


class FeatureCacheTests(unittest.TestCase):
    def test_eviction_and_immutable_identity_are_exact(self):
        for i in range(8200):
            expected=np.frombuffer(hashlib.sha256(repr(('contract',str(i))).encode()).digest()[:7],np.uint8)/255.
            np.testing.assert_array_equal(identity('contract',str(i)),expected)
        with self.assertRaises(ValueError):identity('contract','0')[0]=0
        np.testing.assert_array_equal(identity('contract','0'),np.frombuffer(
            hashlib.sha256(repr(('contract','0')).encode()).digest()[:7],np.uint8)/255.)
        for n in (0,1,10,2147483648,2**53-1):self.assertEqual(log_quantity(n),np.log1p(n))

    def test_age_invalidation_lots_and_book_replacement_stay_dynamic(self):
        e=environment();b=Book('x',0,0,((400000,100),),((450000,100),),'a'*64)
        e.actor_books['x']=SharedLiquidity(b)
        a=e.observe();i=a['entities'].index(('contract','x'));frozen=a['contracts'].copy()
        e.advance(e.now+17);e.actor_books['x'].invalidated=True;e.known_lots[('x','YES')]=[[7,400000]]
        z=e.observe();self.assertEqual(z['contracts'][i,3],np.float32(e.now/1000))
        self.assertEqual(z['contracts'][i,7],0);self.assertEqual(z['contracts'][i,14],np.float32(np.log1p(7)))
        np.testing.assert_array_equal(a['contracts'],frozen)
        z['contracts'][i,0]=0;self.assertEqual(e.observe()['contracts'][i,0],np.float32(.4))
        e.actor_books['x'].update(Book('x',e.now,e.now,((300000,55),),((600000,88),),'b'*64))
        z=e.observe();self.assertEqual(z['contracts'][i,0],np.float32(.3))
        self.assertEqual(z['contracts'][i,4],np.float32(np.log1p(55)))

    def test_cpu_staging_and_variable_entity_padding_preserve_every_value(self):
        e=environment();a=extend_observation(e,e.observe())
        e.builder.append(OrderSpec('x','YES','BUY',123456,400000,50000))
        b=extend_observation(e,e.observe());values=[a,b];batch=inputs(values,'cpu')
        self.assertEqual(batch['contracts'].shape[1],len(b['entities']))
        for j,o in enumerate(values):
            single=tensors(o,'cpu');n=len(o['entities'])
            for k in ('contracts','present'):
                self.assertTrue(torch.equal(batch[k][j,:n],single[k][0]))
                self.assertFalse(batch[k][j,n:].any())
            self.assertTrue(torch.equal(batch['global_state'][j],single['global_state'][0]))
            for k in o['masks']:
                self.assertTrue(torch.equal(batch['masks'][k][j,:n],single['masks'][k][0]))
                self.assertFalse(batch['masks'][k][j,n:].any())
