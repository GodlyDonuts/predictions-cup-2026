"""Public source normalization fixtures; no claim of execution calibration."""
from copy import deepcopy
from datetime import datetime,timezone
import unittest
from .prospective_source import ProspectiveDecoder
from .snapshot_clock import source_clock,REST_ORIGIN


def iso(at):return datetime.fromtimestamp(at/1000,timezone.utc).isoformat(timespec='milliseconds')


def book(seq=10,source=900,ask=.45):
    return dict(exchangeId='17',asOf={'sequence':seq,'at':iso(source)},
        bids=[{'price':.4,'quantity':5}],asks=[{'price':ask,'quantity':7}])


def rest(at=1000,value=None):
    return dict(source='official_sig_api_v1',kind='book',status=200,marketId='1',tournamentId='cup',
        requestStartedAt=iso(at-50),receivedAt=iso(at),endpoint='/markets/1/orderbook?tournamentId=cup&depth=200',
        data={'contexts':[{'type':'tournament','tournament':{'id':'cup'},'orderbook':{'exchanges':[value or book()]}}]})


def ws(at=1100,value=None):
    return dict(source='official_sig_realtime',kind='broadcast',event='market_batch',marketId='1',tournamentId='cup',
        receivedAt=iso(at),duplicate=False,gap=False,requiresRestResync=False,payload={
            'delivery':{'model':'best-effort-authoritative-resync','revision':1,'previousRevision':0},
            'resyncRequired':False,'books':[value or book()],'trades':[],'marketSettled':[]})


class ProspectiveSourceTests(unittest.TestCase):
    def decoder(self):return ProspectiveDecoder('cup',(0,10000),'fit',
        subscription_binding={'tournamentId':'cup','manifestSha256':'f'*64})

    def test_integer_epoch_conversion_keeps_exact_millisecond_and_offset(self):
        from moloch.learning_v2.reference_adapters import millis
        for at in(1,1001,1792533419094):self.assertEqual(millis(iso(at)),at)
        self.assertEqual(millis('1970-01-01T01:00:01.001999+01:00'),1001)
        self.assertEqual(millis('1969-12-31T23:59:59.999999Z'),-1)

    def test_rest_keeps_original_clocks_scope_and_distinct_origin(self):
        d=self.decoder();rows,prints=d.rest_book(rest(),'a'*64);self.assertFalse(prints);self.assertEqual(len(rows),1)
        r=rows[0];self.assertEqual((r['available_at_ms'],r['source_at_ms'],source_clock(r)),(1000,900,1000))
        self.assertEqual(r['snapshot_origin'],REST_ORIGIN);self.assertEqual(r['original_sha256'],'a'*64)
        self.assertNotIn('native_processed_at_ms',r);self.assertNotIn('original_realtime_received_at_ms',r)
        self.assertEqual(r['asks'],[[450000,7]])
        r['received_at_ms']=1001
        with self.assertRaisesRegex(ValueError,'ORIGINAL_REST_SNAPSHOT_CLOCK'):source_clock(r)

    def test_same_versions_and_conflicts_share_one_rest_ws_version_domain(self):
        d=self.decoder();d.rest_book(rest(),'a'*64)
        self.assertEqual(d.websocket(ws(),'b'*64),([],[]))
        newer=ws(1200,book(11,1100));newer['payload']['delivery'].update(revision=2,previousRevision=1)
        rows,_=d.websocket(newer,'c'*64);self.assertEqual(rows[0]['sequence'],'11')
        with self.assertRaisesRegex(ValueError,'SAME_SOURCE_BOOK_CONFLICT'):
            d.rest_book(rest(1300,book(11,1100,.5)),'d'*64)
        self.assertEqual(d.rest_book(rest(1400,book(9,800)),'e'*64),([],[]))

    def test_subscription_and_transport_gaps_survive_full_snapshot_delivery(self):
        d=self.decoder();d.rest_book(rest(),'a'*64)
        subscribe=dict(source='official_sig_realtime',kind='subscribed',marketId='1',receivedAt=iso(1050))
        with self.assertRaisesRegex(ValueError,'CAPTURE_TOURNAMENT_SCOPE'):
            ProspectiveDecoder('cup',(0,10000),'fit').websocket(subscribe,'b'*64)
        rows,_=d.websocket(subscribe,'b'*64);self.assertEqual(rows[0]['kind'],'gap');self.assertIn('17',d.decoder.needs_baseline)
        self.assertEqual(rows[0]['subscription_session_manifest_sha256'],'f'*64)
        event=ws();event['gap']=True;event['requiresRestResync']=True
        rows,_=d.websocket(event,'c'*64);self.assertEqual([r['kind']for r in rows],['gap','book'])
        self.assertTrue(rows[1]['original_requires_rest_resync']);self.assertEqual(d.decoder.needs_baseline,set())

    def test_exact_scope_endpoint_receipt_bounds_and_future_source_are_required(self):
        for mutate in (lambda e:e.update(tournamentId='other'),lambda e:e.update(status=500),
                       lambda e:e.update(endpoint='/markets/2/orderbook?tournamentId=cup&depth=200'),
                       lambda e:e.update(requestStartedAt=iso(1001)),
                       lambda e:e['data']['contexts'][0]['tournament'].update(id='other'),
                       lambda e:e['data']['contexts'][0]['orderbook']['exchanges'][0]['asOf'].update(at=iso(1001))):
            event=rest();mutate(event)
            with self.assertRaises(ValueError):self.decoder().rest_book(event,'a'*64)
        d=ProspectiveDecoder('cup',(1000,1100),'search');d.rest_book(rest(),'a'*64)
        with self.assertRaisesRegex(ValueError,'RECEIPT_CALENDAR_OR_ORDER'):d.websocket(ws(),'b'*64)
        with self.assertRaisesRegex(ValueError,'RECEIPT_CALENDAR_OR_ORDER'):d.rest_book(rest(999),'b'*64)
        with self.assertRaisesRegex(ValueError,'VERIFIED_ORIGINAL'):self.decoder().rest_book(rest(),'invalid')

    def test_original_transport_payload_is_not_rewritten(self):
        event=ws();original=deepcopy(event);self.decoder().websocket(event,'a'*64);self.assertEqual(event,original)
