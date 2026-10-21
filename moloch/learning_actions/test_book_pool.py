"""Immutable replay reuse must not share economic or information authority."""
import gc
from types import SimpleNamespace
import unittest
from weakref import WeakValueDictionary
from .disk_tape import Books
from .test_disk_tape import book
from moloch.learning_v2.liquidity import SharedLiquidity


def row(at=100):
    value=book(at);value['_disk_record']=('market',1);return value


class BookPoolTests(unittest.TestCase):
    def test_same_original_clock_reuses_book_but_not_counterfactual_depletion(self):
        store=SimpleNamespace(book_pool=WeakValueDictionary())
        world=Books(store,world=True);info=Books(store)
        original=row();a=world.from_row(original);b=info.from_row(original)
        self.assertIs(a,b)
        x,y=SharedLiquidity(a),SharedLiquidity(b)
        self.assertEqual(x.cross('YES','BUY',450000,7,100,100),((450000,7),))
        self.assertEqual(y.depleted,{})
        x.invalidate();self.assertFalse(y.invalidated)
        original['asks'][0][1]=1
        self.assertEqual(a.asks,((450000,100),))

    def test_original_information_clock_and_hidden_world_clock_stay_distinct(self):
        store=SimpleNamespace(book_pool=WeakValueDictionary())
        original=row();original.pop('snapshot_origin')
        original.update(available_at_ms=200,native_event_received_at_ms=100,
                        native_processed_at_ms=150)
        a=Books(store,world=True).from_row(original)
        b=Books(store).from_row(original)
        self.assertIsNot(a,b)
        self.assertEqual((a.available_at_ms,b.available_at_ms),(100,200))
        self.assertEqual(a.asks,b.asks)

    def test_different_verified_stores_cannot_collide_on_record_number(self):
        a,b=(SimpleNamespace(book_pool=WeakValueDictionary())for _ in range(2))
        first=row();second=row();second['asks'][0][0]=460000
        x=Books(a).from_row(first);y=Books(b).from_row(second)
        self.assertIsNot(x,y);self.assertNotEqual(x.asks,y.asks)

    def test_weak_pool_does_not_keep_evicted_books_or_depth_alive(self):
        store=SimpleNamespace(book_pool=WeakValueDictionary())
        view=Books(store,capacity=1)
        first=view.from_row(row());self.assertEqual(len(store.book_pool),1)
        second=row(101);second['_disk_record']=('market',2)
        view.from_row(second);self.assertEqual(len(store.book_pool),2)
        del first;gc.collect();self.assertEqual(len(store.book_pool),1)
        view.cache.clear();gc.collect();self.assertEqual(len(store.book_pool),0)

    def test_legacy_fixture_store_without_pool_keeps_value_behavior(self):
        a=Books(SimpleNamespace()).from_row(row())
        b=Books(SimpleNamespace()).from_row(row())
        self.assertIsNot(a,b);self.assertEqual(a,b)
