import copy
from decimal import Decimal
import importlib.util
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('calibration_audit', Path(__file__).with_name('analyze.py'))
A = importlib.util.module_from_spec(spec)
spec.loader.exec_module(A)


def at(ms):
    return f'2026-10-17T00:00:{ms//1000:02d}.{ms%1000:03d}Z'


def original_pair(total=0):
    q = dict(exchangeId='10', marketId='20', side='NO', action='buy', price='0.400000')
    order = dict(endpoint='/orders/30', status=200, requestStartedAt=at(2000), receivedAt=at(2100),
        data=dict(id=30, exchangeId='10', tournamentId='scope', side='no', action='buy',
                  priceLimit=Decimal('.4'), open=False, quantity=5, quantityFilled=total))
    rows = [] if not total else [dict(id='40', quantity=-total, side='no', price=Decimal('.4'), filledAt=at(1000))]
    tape = dict(endpoint='/orders/30/fills?limit=200', status=200, requestStartedAt=at(2200), receivedAt=at(2300),
        data=dict(orderId=30, exchangeId='10', tournamentId='scope', coverage=dict(complete=True),
                  pagination=dict(hasMore=False, nextCursor=None), totalQuantityFilled=-total, data=rows))
    return q, order, tape


def book(source, received=None, processed=None, quantity=10, seq=1, invalid=False):
    received = source+20 if received is None else received
    processed = received+20 if processed is None else processed
    return dict(sequence=seq, hash='a'*64, event=dict(type='book', receivedAt=at(received), processedAt=at(processed),
        sourceRef=dict(id='synthetic-source',sequence=1,hash='b'*64),
        book=dict(marketId='20', exchangeId='10', tournamentId='scope', sequence=str(seq), sourceAt=at(source),
                  requestStartedAt=at(max(0, source-10)), receivedAt=at(received), invalid=invalid,
                  bids=[dict(price=Decimal('.55'), quantity=quantity)],
                  asks=[dict(price=Decimal('.60'), quantity=quantity)])))


class AuditChecks(unittest.TestCase):
    def test_zero_requires_original_complete_later_tape(self):
        q,o,t = original_pair()
        self.assertEqual(A.terminal_pair(o,t,'30',q,'scope',5,A.clock(at(5000)))[0], {})
        for mutate in (lambda x:x['data']['coverage'].update(complete=False),
                       lambda x:x.update(requestStartedAt=at(2000)),
                       lambda x:x['data']['pagination'].update(hasMore=True)):
            bad=copy.deepcopy(t);mutate(bad)
            with self.assertRaises(A.RAW.Rejected): A.terminal_pair(o,bad,'30',q,'scope',5,A.clock(at(5000)))

    def test_partial_no_sign_is_side_not_sale(self):
        q,o,t=original_pair(2)
        rows,_=A.terminal_pair(o,t,'30',q,'scope',5,A.clock(at(5000)))
        self.assertEqual(rows['40'][:2],(2,400000))
        t['data']['data'][0]['quantity']=2
        with self.assertRaises(A.RAW.Rejected): A.terminal_pair(o,t,'30',q,'scope',5,A.clock(at(5000)))

    def test_canonical_endpoint_scope_and_full_quantity_are_bound(self):
        q,o,t=original_pair(2)
        for endpoint in ('/orders/30/fills?limit=200&tournamentId=scope','/orders/31/fills?limit=200'):
            bad=copy.deepcopy(t);bad['endpoint']=endpoint
            with self.assertRaises(A.RAW.Rejected): A.terminal_pair(o,bad,'30',q,'scope',5,A.clock(at(5000)))
        t['data']['tournamentId']='different'
        with self.assertRaises(A.RAW.Rejected): A.terminal_pair(o,t,'30',q,'scope',5,A.clock(at(5000)))

    def test_no_complement_uses_full_depth_not_top_price(self):
        self.assertEqual(A.depth_value([(550000,10)],[(600000,2),(650000,3)],'NO','sell',5),1850000)
        self.assertIsNone(A.depth_value([(550000,10)],[(600000,2)],'NO','sell',5))

    def fixture(self):
        q=dict(marketId='20',exchangeId='10',side='YES',action='buy')
        o=dict(q=q,ack_start=A.clock(at(900)),pre=None,pre_status='no_causal_book')
        f=dict(order=o,n=5,price=550000,fee=0,executed=A.clock(at(1000)),observed=A.clock(at(4000)),posts={})
        return o,f

    def test_features_strictly_before_post_not_response(self):
        o,f=self.fixture()
        A.book_observations([book(800,850,900)], [o],[f],A.clock(at(10000)),
            dict(maximumSourceAgeMs=10000,postSourceAndReceiptWindowMs=5000))
        self.assertIsNone(o['pre'])
        o,f=self.fixture()
        A.book_observations([book(800,850,899)], [o],[f],A.clock(at(10000)),
            dict(maximumSourceAgeMs=10000,postSourceAndReceiptWindowMs=5000))
        self.assertEqual(o['pre_status'],'available')

    def test_first_shallow_label_cannot_be_replaced_favorably(self):
        o,f=self.fixture()
        A.book_observations([book(2000,quantity=2),book(2100,quantity=50,seq=2)], [o],[f],A.clock(at(10000)),
            dict(maximumSourceAgeMs=10000,postSourceAndReceiptWindowMs=5000))
        self.assertEqual(f['posts'][1000]['status'],'shallow_depth')
        self.assertIsNone(f['posts'][1000]['gain'])
        self.assertEqual(f['posts'][120000]['status'],'right_censored')

    def test_late_invalid_and_conflicting_book_not_zero_labels(self):
        for rows,expected in (([book(2000,received=8000,processed=8010)],'late_receipt'),
                              ([book(2000,invalid=True)],'explicit_invalid_or_gap'),
                              ([book(1900),book(2000,seq=0)],'version_rollback')):
            o,f=self.fixture()
            A.book_observations(rows,[o],[f],A.clock(at(10000)),dict(maximumSourceAgeMs=10000,postSourceAndReceiptWindowMs=5000))
            self.assertEqual(f['posts'][1000]['status'],expected)
            self.assertIsNone(f['posts'][1000]['gain'])

    def test_wrong_source_pin_refuses_before_data_analysis(self):
        with tempfile.TemporaryDirectory() as directory:
            p=Path(directory);(p/'SHA256SUMS.json').write_bytes(b'{}')
            with self.assertRaisesRegex(A.RAW.Rejected,'SOURCE_MANIFEST_PIN'): A.pinned_sources(p,'0'*64)

    def test_source_reference_and_tournament_are_not_inferred(self):
        for mutation,expected in ((lambda e:e.pop('sourceRef'),'missing_recorded_source_ref'),
                                  (lambda e:e['book'].update(tournamentId='wrong'),'wrong_tournament_scope')):
            o,f=self.fixture();row=book(2000);mutation(row['event'])
            A.book_observations([row],[o],[f],A.clock(at(10000)),dict(maximumSourceAgeMs=10000,
                postSourceAndReceiptWindowMs=5000,tournamentId='scope'))
            self.assertEqual(f['posts'][1000]['status'],expected)

    def test_known_own_resting_depth_not_external_and_ack_not_terminal(self):
        q=dict(marketId='20',exchangeId='10',side='NO',action='buy',price='0.400000')
        o=dict(qid='1',q=q,admitted=4,native_ack_available=100,terminal_available=1000)
        # NO bid .40 is YES ask .60. A cancel acknowledgement is deliberately
        # not an input capable of releasing these four shares.
        bids,asks,state,removed=A.external_depth([(550000,10)],[(600000,10)],('20','10'),[o],[],500)
        self.assertEqual((asks,state,removed),([(600000,6)],'valid',4))
        _,asks,_,removed=A.external_depth([(550000,10)],[(600000,10)],('20','10'),[o],[],1001)
        self.assertEqual((asks,removed),([(600000,10)],0))

    def test_future_fill_cannot_improve_causal_own_liquidity_feature(self):
        q=dict(marketId='20',exchangeId='10',side='YES',action='buy',price='0.550000')
        o=dict(qid='1',q=q,admitted=4,native_ack_available=100,terminal_available=1000)
        future=dict(qid='1',observed=600,n=3)
        _,_,state,_=A.external_depth([(550000,2)],[(600000,10)],('20','10'),[o],[future],500)
        self.assertEqual(state,'own_depth_quantity_uncertain')
        bids,_,state,_=A.external_depth([(550000,2)],[(600000,10)],('20','10'),[o],[future],700)
        self.assertEqual((bids,state),([(550000,1)],'valid'))

    def test_microprice_tick_artifact_and_duplicate_json_keys(self):
        self.assertEqual(A.levels([dict(price=Decimal('0.06000000000000005'),quantity=1)],True),[(60000,1)])
        with self.assertRaises(A.RAW.Rejected): A.load('{"x":1,"x":2}')


if __name__=='__main__': unittest.main()
