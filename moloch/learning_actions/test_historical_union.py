import copy,tempfile,unittest
from pathlib import Path
from .historical_union import KalshiUnion,SampleUnion,open_indexes
from .historical_binding import validate
from .test_historical_binding import config
from .test_historical_delivery import environment
from .historical_delivery import register_kalshi
from .historical_samples import Samples,Delivery
from . import test_historical_samples as fixtures


def trade(identifier,at,receipt='b'*64):
    return {'trade':{'trade_id':identifier,'yes_price_dollars':'0.45'},'source_at_ms':at,
            'simulated_available_at_ms':at,'original_receipt_sha256':receipt,'actual_retrieved_at':'original-clock'}


class PageReader:
    tickers={'t'}
    def __init__(self,rows):self.rows=rows;self.calls=0;self.closed=False
    def page(self,ticker,now,*,limit=1000,**kwargs):
        self.calls+=1
        return sorted((r for r in self.rows if r['source_at_ms']<=now),key=lambda r:(-r['source_at_ms'],r['trade']['trade_id']))[:limit]
    def close(self):self.closed=True


class HistoricalUnionTests(unittest.TestCase):
    def test_causal_top_page_deduplication_and_original_provenance(self):
        a=PageReader([trade('c',3),trade('b',2)]);b=PageReader([trade('c',3,'a'*64),trade('a',1)])
        u=KalshiUnion([a,b]);page=u.page('t',3,limit=2)
        self.assertEqual([r['trade']['trade_id']for r in page],['c','b'])
        self.assertEqual(page[0]['original_receipt_sha256'],'a'*64)
        self.assertEqual([r['trade']['trade_id']for r in u.page('t',2,limit=2)],['b','a'])
        changed=copy.deepcopy(b.rows[0]);changed['trade']['yes_price_dollars']='0.46';b.rows[0]=changed
        with self.assertRaisesRegex(ValueError,'FACT_CONFLICT'):u.page('t',3)
        u.close();self.assertTrue(a.closed and b.closed)

    def test_multiple_storage_indexes_still_cost_one_external_request(self):
        e=environment();a=PageReader([trade('a',0)]);b=PageReader([trade('b',0)])
        register_kalshi(e,KalshiUnion([a,b]),{'t':['m','YES']})
        key=('historical_trades','t','m','YES');cash=e.cash
        self.assertEqual(e.request_observation('Kalshi',key)['status'],'IN_FLIGHT')
        self.assertEqual((a.calls,b.calls),(1,1));self.assertEqual(e.feed_request_counts['Kalshi'],1)
        self.assertNotIn(('Kalshi',key),e.known_historical_information)
        e.advance(151);self.assertEqual(len(e.known_historical_information['Kalshi',key]['payload']),2)
        self.assertEqual(e.cash,cash);self.assertFalse(e.api_fills);self.assertFalse(e.actor_books)

    def test_real_packed_sample_readers_keep_each_publication_frontier(self):
        with tempfile.TemporaryDirectory()as d:
            root=Path(d);a=root/'a';b=root/'b';a.mkdir();b.mkdir();f=fixtures.HistoricalSamplesTests()
            pins=[f.fixture(a,rows=[(1000,.2,0,1)]),f.fixture(b,rows=[(2000,.9,0,1)])]
            readers=[Samples(p,s,scoring_bounds=(2000,5000),information_start_ms=1000,
                delivery=Delivery(250,1000,0,3000))for p,s in zip((a,b),pins)]
            u=SampleUnion(readers)
            try:
                self.assertEqual(u.at('A',2999)['source_at_ms'],1000)
                self.assertEqual(u.at('A',3000)['price'],.9)
                self.assertEqual(u.at('A',3000)['actual_retrieved_at'],'2026-10-20T06:00:00Z')
                with self.assertRaisesRegex(ValueError,'OUTSIDE_SCORING_ROLE'):u.at('A',5000)
            finally:u.close()

    def test_conflicting_same_time_sample_is_rejected(self):
        class Reader:
            def __init__(self,price):self.price=price
            def at(self,*_):return {'price':self.price,'source_at_ms':1,'simulated_available_at_ms':2,'original_receipt_sha256':'a'*64}
        with self.assertRaisesRegex(ValueError,'FACT_CONFLICT'):SampleUnion([Reader(.1),Reader(.2)]).at('A',3)

    def test_pins_are_explicit_and_partial_open_failure_closes_children(self):
        c=config();s=c['roles']['train']['Kalshi'];s['additional_indexes']=[{'path':'extra','manifest_sha256':'b'*64}];validate(c)
        s['additional_indexes']=[{'path':s['path'],'manifest_sha256':s['manifest_sha256']}]
        with self.assertRaisesRegex(ValueError,'DUPLICATE_HISTORY_INDEX'):validate(c)
        first=PageReader([])
        def factory(path,*args,**kwargs):
            if path=='primary':return first
            raise ValueError('BAD_SECOND_SOURCE')
        spec={'path':'primary','manifest_sha256':'a'*64,'information_start_ms':0,
              'additional_indexes':[{'path':'bad','manifest_sha256':'b'*64}]}
        with self.assertRaisesRegex(ValueError,'BAD_SECOND_SOURCE'):
            open_indexes(factory,'Kalshi',spec,bounds=(1,2),delivery=Delivery(0,1,0,1))
        self.assertTrue(first.closed)


if __name__=='__main__':unittest.main()
