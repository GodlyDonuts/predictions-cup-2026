"""Causal paid information regressions, not a strategy profitability test."""
import tempfile
import unittest
from pathlib import Path
import torch
from .test_historical_neural import environment,trade,KEY
from .historical_delivery import register_kalshi,register_polymarket
from .historical_memory import LATEST
from .managed_data_service import ManagedDataService,INTENT_PROFILE,HISTORY_PROFILE
from .historical_neural import HistoricalNeuralPolicy,OLD
from .training import save_checkpoint,load_checkpoint
from .state_encoding import matching
from .behavior_guard import contract,require_contract
from .test_managed_data_service import action
from moloch.learning_v2.clock import RateBudget,BudgetConfig


class Reader:
    tickers={'A'}
    def __init__(self):self.calls=[]
    def page(self,ticker,now,*,limit=1000):
        self.calls.append((now,limit))
        return [trade(i)for i in range(3)][:limit]


def install(mode=HISTORY_PROFILE):
    env=environment();env.historical_memory_profile=LATEST
    env.keys['Kalshi']=[];env.keys['Polymarket']=[]
    reader=Reader();register_kalshi(env,reader,{'A':['m','YES']})
    class Samples:
        def at(self,asset,now):
            return {'source_at_ms':0,'simulated_available_at_ms':1,'price':.35}
    register_polymarket(env,Samples(),{'P':['m','YES']})
    service=ManagedDataService(env,{'profile':mode,'initially_enabled':True})
    return env,reader,service


class ManagedHistoryTests(unittest.TestCase):
    def setUp(self):torch.set_num_threads(1)

    def test_missing_live_reference_gets_latest_paid_record_after_receipt(self):
        env,reader,service=install();cash=env.cash
        env.advance(1)
        self.assertEqual(service.requests,{'SIG':1,'Kalshi':1,'Polymarket':1})
        self.assertFalse(env.known_historical_information)
        self.assertEqual(reader.calls,[(1,1)])
        env.advance(150);self.assertFalse(env.known_historical_information)
        env.advance(151)
        receipt=env.known_historical_information['Kalshi',KEY]
        self.assertEqual((receipt['requested_at_ms'],receipt['received_at_ms']),(1,151))
        self.assertEqual(len(receipt['payload']),1)
        observed=env.observe();i=observed['entities'].index(('read',('Kalshi',KEY)))
        self.assertEqual(observed['contracts'][i,OLD+17],.125)
        self.assertEqual(env.cash,cash);self.assertFalse(env.api_fills);self.assertFalse(env.budget.writes)
        self.assertNotIn(KEY,env.latest)
        self.assertNotIn(('historical_sample','P','m','YES'),env.latest)
        self.assertNotIn('A',env.books);self.assertNotIn('P',env.books)

    def test_full_actor_history_query_survives_and_shares_budget(self):
        env,reader,service=install();env.advance(1)
        self.assertEqual(env.request_observation('Kalshi',KEY)['status'],'RATE_WAIT')
        service.toggle();env.advance(51)
        self.assertEqual(env.request_observation('Kalshi',KEY)['status'],'IN_FLIGHT')
        self.assertEqual(reader.calls,[(1,1),(51,1000)])
        env.advance(201)
        self.assertEqual(len(env.known_historical_information['Kalshi',KEY]['payload']),3)
        with self.assertRaisesRegex(ValueError,'REGISTERED_BACKGROUND'):
            env.request_observation('Kalshi',KEY,history_latest_only=True)

    def test_rolling_limits_pause_and_resume_keep_original_admissions(self):
        env,reader,service=install();env.advance(5000)
        for venue,cap in [('Kalshi',20),('Polymarket',30)]:
            times=[r['at_ms']for r in env.feed_requests if r['venue']==venue]
            self.assertGreater(len(times),cap*4)
            left=0
            for right,at in enumerate(times):
                while at-times[left]>=1000:left+=1
                self.assertLessEqual(right-left+1,cap)
        before=dict(service.requests);service.toggle();env.advance(5500)
        self.assertEqual(service.requests,before)
        service.toggle();env.advance(5700)
        self.assertGreater(service.requests['Kalshi'],before['Kalshi'])
        self.assertFalse(env.api_fills)

    def test_old_profile_preserves_actor_only_history(self):
        env,reader,service=install(INTENT_PROFILE);env.advance(1000)
        self.assertEqual(service.requests['Kalshi'],0);self.assertEqual(service.requests['Polymarket'],0)
        self.assertFalse(reader.calls);self.assertFalse(env.known_historical_information)
        self.assertEqual(env.request_observation('Kalshi',KEY)['status'],'IN_FLIGHT')
        self.assertEqual(reader.calls,[(1000,1000)])

    def test_live_reference_does_not_silently_replace_historical_outcome(self):
        env,reader,service=install();env.advance(1)
        live=('reference','Kalshi','m','A');other=('reference','Kalshi','m','B')
        env.keys['Kalshi'] += [live,other];env.latest[live]={};env.latest[other]={}
        self.assertEqual(service.targets('Kalshi'),(live,other,KEY))
        del env.latest[live]
        self.assertEqual(service.targets('Kalshi'),(other,KEY))

    def test_foreground_intent_still_gets_next_sig_turn(self):
        env,reader,service=install();env.budget=RateBudget(BudgetConfig(100,600,24,2500,'a'*64))
        env.advance(101);self.assertEqual(action(env,4)['status'],'RATE_WAIT')
        env.advance(701);self.assertTrue(service.waiting_actor)
        self.assertEqual(action(env,4)['status'],'IN_FLIGHT')
        self.assertEqual(len(env.budget.writes),1)
        self.assertGreater(service.requests['Kalshi'],0)

    def test_history_profile_survives_checkpoint_and_rejects_old_behavior(self):
        env,reader,service=install();model=HistoricalNeuralPolicy(16)
        model.managed_data_profile=INTENT_PROFILE;old=contract(model)
        model.managed_data_profile=HISTORY_PROFILE
        model.historical_memory_profile=LATEST;matching(env,model)
        with self.assertRaisesRegex(ValueError,'PROTOCOL_CHANGED'):require_contract(model,old)
        with tempfile.TemporaryDirectory()as d:
            p=Path(d)/'policy.pt'
            pin=save_checkpoint(p,model,torch.optim.Adam(model.parameters()),dataset_manifest_sha256='a'*64,split_specification_sha256='b'*64)
            loaded,_=load_checkpoint(p,pin,'cpu')
            self.assertEqual(loaded.managed_data_profile,HISTORY_PROFILE);matching(env,loaded)
