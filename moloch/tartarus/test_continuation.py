import json
import hashlib
import unittest
from copy import deepcopy
from .continuation import residual,rebase_fifo,digest


class ContinuationTests(unittest.TestCase):
    def fixture(self,filled):
        legs=[{'ex':'x','side':'YES','action':'SELL','quantity':10,'price':600000},
              {'ex':'y','side':'NO','action':'BUY','quantity':5,'price':100000}]
        nom={'account':{'actualFillsSha256':digest([])},'action':{'exchangeId':'x','side':'YES','action':'SELL','quantity':10,'limitPrice':'0.600000'}}
        raw=json.dumps(nom).encode();q={'id':7,'exchangeId':'x','side':'YES','action':'sell','quantity':10,'price':'0.600000',
               'filled':filled,'retired':True,'reviewedRankCampaign':{'campaignSha256':digest(nom)}}
        fs=[]if not filled else[{'sourceId':'actual-1','orderId':7,'actualOrderId':'701','exchangeId':'x','side':'YES','action':'sell','quantity':filled,'price':'0.605000','counterfactual':False}]
        return raw,hashlib.sha256(raw).hexdigest(),legs,fs,0,q,'701'

    def test_full_and_partial_fills_do_not_credit_future_cash(self):
        full=residual(*self.fixture(10),physical_terminal=True);self.assertEqual(full['remainingLegs'][0]['ex'],'y')
        part=residual(*self.fixture(4),physical_terminal=True);self.assertEqual(part['remainingLegs'][0]['quantity'],6)
        self.assertFalse(part['actualSaleProceedsCreditedByThisFunction']);self.assertFalse(part['executionAuthority'])
        self.assertNotIn('cash',part)

    def test_unknown_nonterminal_counterfactual_and_wrong_fill_are_rejected(self):
        with self.assertRaises(ValueError):residual(*self.fixture(4),physical_terminal=False)
        for field,value in [('counterfactual',True),('actualOrderId','702'),('price','0.595000'),('quantity',5)]:
            args=list(self.fixture(4));args[3][0][field]=value
            with self.assertRaises(ValueError):residual(*args,physical_terminal=True)

    def test_fifo_rebase_retains_originals_and_rejects_unfunded_inventory(self):
        positions={'x':{'YES':[{'quantity':3,'price':'0.400000'},{'quantity':3,'price':'0.500000'}],'NO':[]}}
        old=deepcopy(positions);legs=residual(*self.fixture(4),physical_terminal=True)['remainingLegs'];new=rebase_fifo(positions,legs)
        self.assertEqual(new[0]['basis'],2700000);self.assertEqual(positions,old)
        positions['x']['YES'][0]['quantity']=2
        with self.assertRaises(ValueError):rebase_fifo(positions,legs)


if __name__=='__main__':unittest.main()
