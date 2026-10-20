import hashlib
import json
import unittest
from . import test_audit_sig_trade_history as fixtures
from .audit_sig_history import clock
from .historical_sig_trades import build,SigTradeHistory,SigTradePager
from .historical_samples import Delivery


class SigTradeHistoryTests(unittest.TestCase):
    def setUp(self):
        self.fixture=fixtures.TradeAuditTest();self.fixture.setUp()
        self.addCleanup(self.fixture.tearDown)
        self.f=self.fixture.fixture

    def compile(self):
        proof=self.fixture.run_audit();path=self.f.root/'audit.json'
        path.write_text(json.dumps(proof));out=self.f.root/'compiled'
        result=build(self.f.root,path,hashlib.sha256(path.read_bytes()).hexdigest(),self.f.scope,out)
        self.out=out;self.sha=hashlib.sha256((out/'complete.json').read_bytes()).hexdigest()
        return result

    def reader(self):
        return SigTradeHistory(self.out,self.sha,scoring_bounds=(clock(self.f.first),clock(self.f.end)),
            information_start_ms=clock(self.f.start),delivery=Delivery(1000,1,0,120000))

    def test_causal_delivery_precision_and_reused_ids(self):
        result=self.compile();self.assertEqual(result['records'],2)
        self.assertEqual(result['reusedLegacyEngineIdsRetainedAsDistinctFacts'],1)
        t=clock(self.f.first)
        with self.reader() as reader:
            self.assertEqual(reader.page('840',t+999),[])
            rows=reader.page('840',clock(self.f.split)+1000)
            self.assertEqual(len(rows),2)
            self.assertEqual({r['trade']['id'] for r in rows},{'4'})
            self.assertEqual(len({r['fact_sha256'] for r in rows}),2)
            self.assertEqual(rows[0]['trade']['price'],'0.0600000000000001')
            self.assertFalse(rows[0]['execution_authority'])
            with self.assertRaises(ValueError):reader.page('840',clock(self.f.end))
            with self.assertRaises(ValueError):reader.page('840',t+1000,limit=201)

    def test_null_and_unknown_are_not_zero_prices(self):
        self.fixture.edit_page(1,lambda r:r['data']['data'][0].update(price=None))
        self.compile()
        with self.reader() as reader:
            self.assertIsNone(reader.page('840',clock(self.f.first)+1000)[0]['trade']['price'])

    def test_changed_database_rejected(self):
        self.compile()
        with (self.out/'history.sqlite').open('ab') as out:out.write(b'tampered')
        with self.assertRaisesRegex(ValueError,'DATABASE_CHANGED'):self.reader()

    def test_continuation_keeps_original_watermark_and_filters(self):
        self.compile()
        with self.reader()as reader:
            pager=SigTradePager(reader,'840');early=clock(self.f.first)+1000
            later=clock(self.f.split)+1000
            first=pager(early,trade_limit=1)
            self.assertEqual(len(first),1)
            self.assertEqual(pager(later,continue_trade_page=True),[])
            newest=pager(later,trade_limit=1,trade_from_ms=clock(self.f.split))
            self.assertEqual(len(newest),1)
            self.assertNotEqual(first[0]['fact_sha256'],newest[0]['fact_sha256'])
            self.assertEqual(pager(later,continue_trade_page=True,trade_from_ms=0),[])
            with self.assertRaisesRegex(ValueError,'OUTSIDE_ROLE'):pager(clock(self.f.end),continue_trade_page=True)

    def test_same_timestamp_facts_are_neither_skipped_nor_duplicated(self):
        for page in(0,1):self.fixture.edit_page(page,lambda r,p=page:r['data']['data'][0].update(createdAt=self.f.first,id=str(p+20)))
        self.compile()
        with self.reader()as reader:
            now=clock(self.f.split)+1000;pager=SigTradePager(reader,'840')
            expected=reader.page('840',now)
            actual=pager(now,trade_limit=1)+pager(now,continue_trade_page=True)
            self.assertEqual(actual,expected)
            self.assertEqual(len(actual),2)
            self.assertEqual(pager(now,continue_trade_page=True),[])

    def test_submillisecond_event_is_never_visible_early(self):
        from datetime import datetime,timedelta
        original=datetime.fromisoformat(self.f.first.replace('Z','+00:00'))
        stamp=(original+timedelta(microseconds=500)).isoformat()
        self.fixture.edit_page(1,lambda r:r['data']['data'][0].update(createdAt=stamp))
        self.compile();t=clock(self.f.first)
        with self.reader() as reader:
            self.assertEqual(reader.page('840',t+1000),[])
            self.assertEqual(len(reader.page('840',t+1001)),1)
