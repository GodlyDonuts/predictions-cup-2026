import unittest
from copy import deepcopy
from turnover_opportunity_census import assess


class CensusTests(unittest.TestCase):
    def setUp(self):
        self.now=1792616400000
        self.review=dict(exchangeId='1043',marketId='354',side='NO',ordinaryProbability='0.895',
            exceptionalProbabilityAllowance='0.01',minimumExceptionalPayout='0',marginPerShare='0.01',
            reviewedAtMs=self.now-1000,expiresAtMs=self.now+10000,
            sources=[dict(observedAtMs=self.now-1000,maximumAgeMs=10000)],
            maximumPrincipal='1000',maximumQuantity=10000,sameSideTopUps=dict(version=3,maximumEntries=16))
        self.book=dict(exchangeId='1043',marketId='354',receivedAt='2026-10-21T21:00:00Z',
            sourceAt='2026-10-21T21:00:00Z',sequence='1',
            bids=[dict(price='0.175',quantity=100)],asks=[dict(price='0.185',quantity=100)])
        self.account=dict(cash='1000',reservedCash='0',positions={})

    def test_no_side_uses_complementary_bid_and_depth(self):
        row=assess(self.review,self.book,self.account,set(),self.now)
        self.assertEqual(row['bestAsk'],'0.825')
        self.assertEqual(row['entryCeiling'],'0.87605')
        self.assertEqual(row['proposedQuantity'],50)
        self.assertTrue(row['conditionalCandidate'])
        self.assertFalse(row['executionAuthority'])

    def test_existing_capital_is_charged_and_existing_order_blocks(self):
        self.account['positions']={'1043':{'NO':[dict(price='0.9',quantity=1100)]}}
        row=assess(self.review,self.book,self.account,{'1043'},self.now)
        self.assertEqual(row['proposedQuantity'],12)
        self.assertIn('existing_physical_or_native_liability',row['reasons'])
        self.assertFalse(row['conditionalCandidate'])

    def test_stale_and_future_clocks_do_not_count(self):
        for offset in [-1,10001]:
            row=assess(self.review,self.book,self.account,set(),self.now+offset)
            self.assertIn('stale_book',row['reasons'])
            self.assertFalse(row['conditionalCandidate'])

    def test_removing_discount_would_not_make_overpriced_market_profitable(self):
        r=deepcopy(self.review);r.update(side='YES',ordinaryProbability='0.755',exceptionalProbabilityAllowance='0.1')
        b=deepcopy(self.book);b.update(bids=[dict(price='0.755',quantity=100)],asks=[dict(price='0.765',quantity=100)])
        row=assess(r,b,self.account,set(),self.now)
        self.assertEqual(row['entryCeiling'],'0.6695')
        self.assertEqual(row['zeroAllowanceCeiling'],'0.745')
        self.assertIn('price_above_conditional_ceiling',row['reasons'])
        self.assertFalse(row['conditionalCandidate'])


if __name__=='__main__':unittest.main()
