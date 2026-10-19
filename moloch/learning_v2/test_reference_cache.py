import unittest
from .reference_cache import ReferenceCache


class MarketReferenceIndex(unittest.TestCase):
    def test_every_mapping_mutation_preserves_exact_rows_and_market_members(self):
        cache=ReferenceCache();ordinary={}
        operations=[('set',('a','K'),{'x':1}),('set',('b','P'),{'x':2}),('set',('a','P'),{'x':3}),
                    ('set',('a','K'),{'x':4}),('pop',('b','P'),None),('set',('b','K'),{'x':5})]
        for op,key,value in operations:
            if op=='set':cache[key]=value;ordinary[key]=value
            else:self.assertIs(cache.pop(key),ordinary.pop(key))
            self.assertEqual(dict(cache),ordinary)
            for market in ['a','b','missing']:
                self.assertEqual(dict(cache.for_market(market)),{k:v for k,v in ordinary.items()if k[0]==market})
        self.assertIs(cache[('a','K')],ordinary[('a','K')]);cache[('a','K')]['x']=9
        self.assertEqual(dict(cache.for_market('a'))[('a','K')]['x'],9)
        cache.update({('c','K'):{'x':10}});self.assertEqual(len(list(cache.for_market('c'))),1)
        cache.clear();self.assertEqual(dict(cache),{});self.assertEqual(list(cache.for_market('c')),[])

    def test_references_from_other_markets_are_never_touched(self):
        class Forbidden(dict):
            def __getitem__(self,key):raise AssertionError('unrelated row read')
            def get(self,key,default=None):raise AssertionError('unrelated row read')
        cache=ReferenceCache({('other','K'):Forbidden(),('selected','K'):{'price':1}})
        self.assertEqual([row['price']for key,row in cache.for_market('selected')],[1])
        self.assertEqual(cache.pop(('missing','K'),None),None)


if __name__=='__main__':unittest.main()
