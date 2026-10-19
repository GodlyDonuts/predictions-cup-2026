from dataclasses import replace
import unittest
from moloch.learning_flow.episodes import FlowTape
from moloch.learning_realism.protocol import Split,DAY
from moloch.learning_realism.test_replay import fixture
from .engine import ApiTape
from .contract import OrderSpec,UNIT
from .realtime import DerivedStream
from .snapshot_clock import prepare


def book(received,available,ask=450000):
    return {'venue':'SIG','kind':'book','market_id':'m','exchange_id':'x','available_at_ms':available,
        'received_at_ms':received,'source_at_ms':received,'native_event_received_at_ms':received,
        'native_processed_at_ms':available,'original_sha256':('%064x'%(available+1)),
        'bids':[[400000,100]],'asks':[[ask,100]]}


def tape(rows,bounds=(0,5000)):
    original=FlowTape.__new__(FlowTape);original.rows=tuple(sorted(rows+[book(100000,100000)],key=lambda r:r['available_at_ms']))
    original.times=tuple(r['available_at_ms']for r in original.rows)
    original.exchanges=('__GLOBAL_WAIT__','x');original.index={'__GLOBAL_WAIT__':0,'x':1}
    original.flow_rows=();original.flow_times=();original.manifest={};original.manifest_sha256='a'*64
    t=ApiTape.from_loaded(original,split=Split(0,10*DAY),role='train');prepare(t,bounds);return t


def episode(t,read_latency=None,end=5000):
    mechanics=fixture().mechanics
    if read_latency is not None:mechanics=replace(mechanics,read_latency_ms=read_latency)
    return t.episode(mechanics,1,end,initial_cash_micros=1000*UNIT,target_wealth_micros=10000*UNIT,burnin_ms=1)


class SnapshotClockTests(unittest.TestCase):
    def test_hidden_world_changes_execution_before_delayed_information_without_waking_actor(self):
        e=episode(tape([book(0,1000),book(200,2000,600000)]))
        self.assertNotIn('x',e.actor_books);revision=e.observation_revision
        self.assertEqual(e.advance_until_observable(500),500)
        self.assertEqual(e.observation_revision,revision)
        self.assertEqual(e.books['x'].book.asks[0][0],600000)
        result=e._execute('ORDER',[OrderSpec('x','YES','BUY',1,None)])[0]
        self.assertEqual(result.remaining,0);self.assertEqual(e.api_fills[-1]['price'],600000)
        self.assertNotIn('x',e.actor_books)
        self.assertTrue(all(e.observe()['contracts'][i,:8].sum()==0 for i,(kind,_)in enumerate(e.observe()['entities'])if kind=='contract'))

    def test_delayed_stream_uses_its_original_snapshot_and_cannot_regress_fresher_rest(self):
        e=episode(tape([book(0,2000),book(200,3000,600000)]))
        e.realtime=DerivedStream(e);e.realtime.mint();e.advance(300)
        # Known contract metadata is not a price observation.
        e._remember('SIG',('book','x'),{'kind':'book','exchange_id':'x'})
        e.request_observation('SIG',('book','x'));e.advance(400)
        self.assertEqual(e.actor_books['x'].source_state_at_ms,200)
        e.advance(2000)
        self.assertEqual(e.actor_books['x'].book.asks[0][0],600000)
        self.assertEqual(e.realtime.deliveries,0)
        e.advance(3000);self.assertEqual(e.realtime.deliveries,1)

    def test_late_read_receipt_completes_budget_without_overwriting_newer_stream(self):
        e=episode(tape([book(0,0),book(200,300,600000)]),read_latency=1000)
        e.realtime=DerivedStream(e);e.realtime.enabled=True;e.realtime.expires_at=100000;e.realtime.synced['x']=0
        e.advance(100);e.request_observation('SIG',('book','x'))
        e.advance(300);self.assertEqual(e.actor_books['x'].source_state_at_ms,200)
        e.advance(1100)
        self.assertIsNone(e.budget.active);self.assertEqual(len(e.budget.reads),1)
        self.assertEqual(e.actor_books['x'].book.asks[0][0],600000)
        self.assertEqual(e.realtime.synced['x'],0)  # stale receipt is not a new REST baseline

    def test_old_gap_notice_cannot_invalidate_a_post_gap_baseline(self):
        gap={'venue':'SIG','kind':'gap','market_id':'m','exchange_id':'x','available_at_ms':1000,
             'native_event_received_at_ms':100,'native_processed_at_ms':1000,'original_sha256':'d'*64}
        e=episode(tape([book(0,0),book(200,300,600000),gap]))
        e.realtime=DerivedStream(e);e.realtime.mint();e.advance(400)
        e.request_observation('SIG',('book','x'));e.advance(500)
        self.assertFalse(e.actor_books['x'].invalidated)
        baseline=e.realtime.synced['x'];e.advance(1000)
        self.assertFalse(e.books['x'].invalidated);self.assertFalse(e.actor_books['x'].invalidated)
        self.assertEqual(e.realtime.synced['x'],baseline)

    def test_later_role_snapshot_cannot_change_earlier_hidden_world(self):
        later=book(200,2000,600000)
        t=tape([book(0,0),later],bounds=(0,1500));e=episode(t,end=1000)
        e.advance(500);self.assertEqual(e.books['x'].book.asks[0][0],450000)
        with self.assertRaisesRegex(ValueError,'IN_INFORMATION_ROLE'):episode(t)

    def test_missing_or_inconsistent_source_clocks_fail_instead_of_retiming(self):
        missing=book(0,1000);missing.pop('native_processed_at_ms')
        with self.assertRaises(KeyError):tape([missing])
        wrong=book(200,100);wrong['source_at_ms']=0
        with self.assertRaisesRegex(ValueError,'CLOCK_ORDER'):tape([wrong])

    def test_hidden_fill_and_cancel_race_is_not_reexecuted_by_late_information(self):
        first=book(0,1000,600000);next_book=book(200,2000,450000);next_book['asks'][0][1]=1
        e=episode(tape([first,next_book]))
        order=e._execute('ORDER',[OrderSpec('x','YES','BUY',2,500000)])[0]
        self.assertEqual(order.remaining,2)
        e.advance(200);self.assertEqual(order.remaining,1);self.assertEqual(len(e.api_fills),1)
        e.advance(250);cancelled=e._execute('CANCEL',order.id)
        self.assertEqual(cancelled.remaining,1);e.advance(2000)
        self.assertEqual(len(e.api_fills),1);self.assertEqual(e.cash,1000*UNIT-450000)
        self.assertEqual(e.books['x'].book.available_at_ms,200)

    def test_late_old_information_cannot_revert_the_hidden_matching_book(self):
        e=episode(tape([book(0,1000,600000),book(200,2000,450000)]))
        e.advance(1000)
        self.assertEqual(e.books['x'].book.available_at_ms,200)
        self.assertEqual(e.books['x'].book.asks[0][0],450000)
