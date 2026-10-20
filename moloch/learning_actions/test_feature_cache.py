"""Feature memoization must never freeze dynamic evidence or truncate entities."""
import hashlib
import unittest
import numpy as np
import torch
from .feature_cache import identity,log_quantity,entity_identities
from .test_actions import environment
from .contract import OrderSpec
from .full_depth import extend_observation
from .stream import tensors
from .batched import inputs
from moloch.learning_v2.liquidity import Book,SharedLiquidity


class FeatureCacheTests(unittest.TestCase):
    def test_entity_matrix_is_exact_immutable_and_invalidates_on_order_or_membership(self):
        e=environment();entities=[('global',None),('contract','x'),('read',('SIG',('book','x')))]
        first=entity_identities(e,entities)
        np.testing.assert_array_equal(first,np.asarray([identity(*key)for key in entities],dtype=np.float32))
        self.assertIs(first,entity_identities(e,list(entities)))
        with self.assertRaises(ValueError):first[0,0]=0
        swapped=entities[::-1];second=entity_identities(e,swapped)
        np.testing.assert_array_equal(second,first[::-1]);self.assertIsNot(first,second)
        extra=swapped+[('order',17)];third=entity_identities(e,extra)
        self.assertEqual(third.shape,(4,7));np.testing.assert_array_equal(third[-1],identity('order',17).astype(np.float32))
        self.assertEqual(entity_identities(e,[]).shape,(0,7))

    def test_receipt_identity_overrides_do_not_become_stale_with_same_layout(self):
        e=environment()
        e.known_acknowledgments=[{'quantity':1,'remaining':1,'quantity_traded':0,'total_cost_micros':0,
            'last_price_micros':None,'side':'YES','action':'BUY','at_ms':e.now,'exchange':'x','order':7,'open':True}]
        e.known_private_fills=[{'yes_price_micros':400000,'quantity':3,'executed_at_ms':e.now,'exchange':'x','order':7}]
        before=e.observe();cached=e._entity_identity_matrix_cache[1]
        for row in(e.known_acknowledgments[0],e.known_private_fills[0]):row['exchange']='y'
        after=e.observe();self.assertEqual(before['entities'],after['entities'])
        self.assertIs(cached,e._entity_identity_matrix_cache[1])
        for kind in('execution_ack','private_fill'):
            i=after['entities'].index((kind,0))
            np.testing.assert_array_equal(before['contracts'][i,33:40],identity('contract','x').astype(np.float32))
            np.testing.assert_array_equal(after['contracts'][i,33:40],identity('contract','y').astype(np.float32))

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
