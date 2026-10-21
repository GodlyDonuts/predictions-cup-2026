import unittest
from copy import deepcopy
from moloch.learning_v2.liquidity import Book,SharedLiquidity
from .liquidity_snapshot import receipt_snapshot


def world():return SharedLiquidity(Book('x',1,1,((400000,100),),((450000,100),),'a'*64))


class ReceiptSnapshotTests(unittest.TestCase):
    def test_preserves_values_and_isolates_world_and_two_delayed_receipts(self):
        w=world();w.source_state_at_ms=1;w.depleted[('asks',450000)]=7
        a=receipt_snapshot(w);b=receipt_snapshot(w);old=deepcopy(w)
        self.assertEqual(vars(a),vars(old));self.assertIs(a.book,w.book)
        self.assertIsNot(a.depleted,w.depleted);self.assertIsNot(a.depleted,b.depleted)
        self.assertEqual(a.cross('YES','BUY',450000,10,1,100),old.cross('YES','BUY',450000,10,1,100))
        self.assertEqual(a.depleted,old.depleted);self.assertEqual(w.depleted[('asks',450000)],7)
        self.assertEqual(b.depleted[('asks',450000)],7)
        w.invalidate();w.update(Book('x',2,2,((500000,50),),((550000,50),),'b'*64))
        self.assertEqual(a.book.available_at_ms,1);self.assertFalse(a.invalidated)
        a.invalidate();self.assertFalse(b.invalidated)
        self.assertEqual(a.source_state_at_ms,1)

    def test_mutable_depth_and_unexpected_attributes_use_full_deepcopy(self):
        for change in ('list_depth','extra_wrapper','extra_book','mutable_depletion'):
            with self.subTest(change=change):
                w=world()
                if change=='list_depth':object.__setattr__(w.book,'bids',[[400000,100]])
                elif change=='extra_wrapper':w.custom={'rows':[1]}
                elif change=='extra_book':object.__setattr__(w.book,'custom',{'rows':[1]})
                else:w.depleted[('asks',450000)]=[7]
                c=receipt_snapshot(w);self.assertEqual(vars(c),vars(deepcopy(w)))
                self.assertIsNot(c.book,w.book)
                if change=='list_depth':w.book.bids[0][1]=99;self.assertEqual(c.book.bids[0][1],100)
                elif change=='extra_wrapper':w.custom['rows'].append(2);self.assertEqual(c.custom['rows'],[1])
                elif change=='extra_book':w.book.custom['rows'].append(2);self.assertEqual(c.book.custom['rows'],[1])
                else:w.depleted[('asks',450000)].append(8);self.assertEqual(c.depleted[('asks',450000)],[7])

    def test_subclasses_keep_original_deepcopy_protocol(self):
        class Special(SharedLiquidity):
            def __deepcopy__(self,memo):return 'custom-copy'
        self.assertEqual(receipt_snapshot(Special(world().book)),'custom-copy')
        class SpecialBook(Book):pass
        w=world();w.book=SpecialBook('x',1,1,((400000,100),),((450000,100),),'a'*64)
        c=receipt_snapshot(w);self.assertIsNot(c.book,w.book);self.assertIs(type(c.book),SpecialBook)

    def test_invalidated_and_empty_depth_keep_exact_state(self):
        w=SharedLiquidity(Book('x',1,None,(),(),'a'*64));w.invalidate()
        c=receipt_snapshot(w);self.assertEqual(vars(c),vars(deepcopy(w)))
        self.assertTrue(c.invalidated);self.assertIs(c.book,w.book)
