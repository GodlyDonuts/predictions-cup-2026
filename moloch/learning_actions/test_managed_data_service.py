"""Synthetic paid-delivery and scheduling fixtures, never profitability proof."""
import tempfile,unittest
from pathlib import Path
import torch
from .managed_data_service import ManagedDataService,PROFILE,advance_alarm,specification
from .test_actions import environment
from .contract import UNIT
from .policy import ApiPolicy
from .stream import tensors
from .training import save_checkpoint,load_checkpoint
from .state_encoding import matching
from .behavior_guard import contract,require_contract
from moloch.learning_v2.clock import RateBudget,BudgetConfig


def install(env,enabled=True):return ManagedDataService(env,{'profile':PROFILE,'initially_enabled':enabled})


def action(env,family,kind='global',patience=0,clock=0.):
    o=env.observe();i=next(i for i,(k,_)in enumerate(o['entities'])if k==kind)
    return env.act_raw({'family':family,'market':i,'side':0,'price':1,'patience':patience,
                       'target_fraction':0.,'child_fraction':clock},o)


class ManagedDataTests(unittest.TestCase):
    def setUp(self):torch.set_num_threads(1);torch.manual_seed(112)

    def test_no_free_book_until_original_paid_receipt_and_no_financial_change(self):
        e=environment();cash=e.cash;s=install(e);self.assertFalse(e.actor_books)
        e.advance(1);self.assertEqual(s.requests['SIG'],1);self.assertFalse(e.actor_books)
        e.advance(100);self.assertFalse(e.actor_books);e.advance(101)
        self.assertFalse(e.actor_books['x'].invalidated);self.assertEqual(len(e.budget.reads),1)
        self.assertEqual(e.cash,cash);self.assertFalse(e.api_fills);self.assertFalse(e.budget.writes)
        self.assertTrue(all(r['request_owner']=='managed_data_service'for r in e.feed_requests))
        e.advance(9000);self.assertLess(e.actor_books['x'].book.available_at_ms,e.now)

    def test_published_venue_limits_are_shared_with_explicit_reads(self):
        e=environment();s=install(e);e.advance(59999)
        self.assertLessEqual(s.requests['SIG'],100)
        self.assertLessEqual(s.requests['Kalshi'],20*60);self.assertLessEqual(s.requests['Polymarket'],30*60)
        for venue,cap in(('Kalshi',20),('Polymarket',30)):
            times=[r['at_ms']for r in e.feed_requests if r['venue']==venue]
            left=0
            for right,at in enumerate(times):
                while at-times[left]>=1000:left+=1
                self.assertLessEqual(right-left+1,cap)
        before=len(e.budget.reads);result=e.request_observation('SIG',('book','x'))
        self.assertIn(result['status'],('RATE_WAIT','BUSY'));self.assertEqual(len(e.budget.reads),before)

    def test_foreground_write_gets_priority_without_an_extra_admission(self):
        e=environment();e.budget=RateBudget(BudgetConfig(100,600,24,2500,'a'*64));s=install(e)
        e.advance(101);r=e.dispatch('CANCEL_ALL',None,idempotency_key='foreground')
        self.assertEqual(r,{'status':'RATE_WAIT','ready_ms':701})
        e.advance(701);self.assertTrue(s.waiting_actor);self.assertIsNone(e.budget.active)
        r=action(e,4);self.assertEqual(r['status'],'IN_FLIGHT')
        e.advance(702);self.assertEqual(s.requests['SIG'],1);self.assertEqual(len(e.budget.writes),1)
        e.advance(3602);self.assertGreaterEqual(s.requests['SIG'],2)

    def test_abandoned_foreground_request_does_not_freeze_collection(self):
        e=environment();e.budget=RateBudget(BudgetConfig(100,600,24,2500,'a'*64));s=install(e)
        e.advance(101);e.dispatch('CANCEL_ALL',None,idempotency_key='not-a-durable-intent')
        result=action(e,0,patience=1,clock=.5);self.assertFalse(result['wake_on_observation'])
        advance_alarm(e,101,{**result,'ready_ms':1000})
        self.assertEqual(e.now,1000);self.assertGreaterEqual(s.requests['SIG'],2);self.assertFalse(e.budget.writes)

    def test_pause_fences_future_polls_but_delivers_already_paid_requests(self):
        e=environment();s=install(e);e.advance(101);before=dict(s.requests);received=dict(e.feed_receipt_counts)
        self.assertEqual(action(e,11,'data_service_control')['status'],'DATA_SERVICE_PAUSED')
        e.advance(500);self.assertEqual(s.requests,before)
        self.assertGreater(e.feed_receipt_counts['Polymarket'],received.get('Polymarket',0))
        self.assertEqual(action(e,11,'data_service_control')['status'],'DATA_SERVICE_STARTED')
        e.advance(601);self.assertGreater(s.requests['SIG'],before['SIG'])

    def test_unknown_acceptance_is_preserved_and_blocks_background_sig_reads(self):
        e=environment();e.budget.unknown.add('unresolved');s=install(e);e.advance(500)
        self.assertEqual(s.requests['SIG'],0);self.assertEqual(e.budget.unknown,{'unresolved'})
        self.assertGreater(s.requests['Kalshi'],0);e.budget.unknown.clear();e.advance(1101)
        self.assertGreater(s.requests['SIG'],0)

    def test_wait_choice_controls_wake_without_stopping_feed_clocks(self):
        interrupted=environment();a=install(interrupted);interrupted.advance(1)
        advance_alarm(interrupted,1,{'status':'ALARM','ready_ms':800,'wake_on_observation':True})
        self.assertLess(interrupted.now,800)
        strict=environment();b=install(strict);strict.advance(1)
        advance_alarm(strict,1,{'status':'ALARM','ready_ms':800,'wake_on_observation':False})
        self.assertEqual(strict.now,800);self.assertGreater(b.requests['SIG'],a.requests['SIG'])
        self.assertFalse(strict.api_fills);self.assertFalse(strict.budget.writes)

    def test_optional_finance_reads_update_only_their_paid_scoped_receipts(self):
        e=environment();s=ManagedDataService(e,{'profile':PROFILE,'initially_enabled':True,'finance_period_ms':1000})
        e.cash-=UNIT;e.finance_revision+=1;e.lots[('x','YES')]=[[1,UNIT]]
        self.assertEqual(e.known_cash,1000*UNIT);self.assertFalse(e.known_lots)
        e.advance(100);self.assertEqual(e.known_cash,1000*UNIT)
        e.advance(101);self.assertEqual(e.known_cash,999*UNIT);self.assertFalse(e.known_lots)
        e.advance(202);self.assertEqual(e.known_lots,{('x','YES'):[[1,UNIT]]})
        e.advance(1000);self.assertEqual(s.target_requests['SIG/account'],1)
        self.assertGreater(s.target_requests['SIG/book'],0);self.assertFalse(e.budget.writes)

    def test_uninterrupted_wait_still_captures_each_daily_reward_boundary(self):
        from .economics import GrowthLedger
        from moloch.learning_realism.protocol import DAY
        e=environment();e.end=e.now+2*DAY;install(e,False);ledger=GrowthLedger(e)
        advance_alarm(e,e.now,{'status':'ALARM','ready_ms':e.end,'wake_on_observation':False})
        ledger.transition();result=ledger.finish()
        self.assertEqual(result['D'],2);self.assertEqual(result['N'],0);self.assertEqual(result['reward'],0.)
        self.assertEqual([b.at_ms for b in ledger.boundaries],[1,1+DAY,1+2*DAY])

    def test_profile_contract_checkpoint_and_wait_density_are_explicit(self):
        e=environment();install(e,False);m=ApiPolicy(16);legacy=contract(m)
        m.managed_data_profile=PROFILE;matching(e,m)
        with self.assertRaisesRegex(ValueError,'PROTOCOL_CHANGED'):require_contract(m,legacy)
        with self.assertRaisesRegex(ValueError,'EXPLICIT_BEHAVIOR'):require_contract(m,None)
        with torch.no_grad():
            m.heads['patience'].weight.zero_();m.heads['patience'].bias.copy_(torch.tensor([-1.,2.]));m.conditioners['patience'].weight.zero_()
        x=tensors(e.observe(),'cpu');h=m.initial_state(1);reset=torch.ones(1,dtype=torch.bool)
        sampled=m.step(**x,hidden=h,reset=reset,deterministic=True)['actions'];sampled['family'][:]=0;sampled['market'][:]=0
        out=[]
        for bit in(0,1):
            a={k:v.clone()for k,v in sampled.items()};a['patience'][:]=bit
            out.append(m.step(**x,hidden=h,reset=reset,actions=a)['logprob'])
        self.assertAlmostEqual(float((out[1]-out[0]).detach()),3.,places=6)
        with tempfile.TemporaryDirectory()as d:
            p=Path(d)/'checkpoint.pt';digest=save_checkpoint(p,m,torch.optim.Adam(m.parameters()),dataset_manifest_sha256='a'*64,split_specification_sha256='b'*64)
            loaded,_=load_checkpoint(p,digest);self.assertEqual(loaded.managed_data_profile,PROFILE)
        m.managed_data_profile=None
        with self.assertRaisesRegex(ValueError,'MANAGED_DATA_MISMATCH'):matching(e,m)
        with self.assertRaisesRegex(ValueError,'CONFIGURATION'):specification({'profile':PROFILE})

    def test_sealed_episode_replays_paid_service_and_strict_wait_exactly(self):
        from .test_episode_tape import worlds
        from .batched import BatchStream
        from .economics import GrowthLedger
        from .episode_tape import EpisodeWriter,EpisodeReader
        def setup():
            envs=worlds()
            for e in envs:install(e)
            return envs,[GrowthLedger(e,development=True)for e in envs]
        envs,ledgers=setup();m=ApiPolicy(16);m.managed_data_profile=PROFILE
        with torch.no_grad():
            m.heads['family'].weight.zero_();m.heads['family'].bias.zero_();m.heads['family'].bias[0]=100
            m.heads['patience'].weight.zero_();m.heads['patience'].bias.copy_(torch.tensor([-100.,100.]))
        stream=BatchStream(envs,m,ledgers,calibration={'decision_latency_ms':10},compact_finished=True)
        with tempfile.TemporaryDirectory()as d:
            p=Path(d)/'episode';writer=EpisodeWriter(p,stream,dataset_sha256='a'*64,specification_sha256='b'*64)
            while any(e.now<e.end for e in envs):writer.record(stream.collect(3,bootstrap=False))
            pin=writer.seal();replayed,new_ledgers=setup();reader=EpisodeReader(p,pin)
            try:
                chunks=list(reader.replay(replayed,new_ledgers))
                self.assertTrue(chunks);self.assertTrue(all(e.now==e.end for e in replayed))
                self.assertEqual([e.managed_data_service.requests for e in envs],[e.managed_data_service.requests for e in replayed])
            finally:reader.close()


if __name__=='__main__':unittest.main()
