import unittest
import torch
from moloch.learning_realism.test_vector import tapes
from moloch.learning_realism.test_replay import fixture
from .contract import OrderSpec,MAX_QUANTITY,UNIT
from .engine import ApiTape
from .policy import ApiPolicy
from .stream import Stream,tensors


def environment(cash=1000*UNIT):
    _,base=tapes();t=ApiTape.from_loaded(base,split=base.split,role='train')
    return t.episode(fixture().mechanics,1,100000,initial_cash_micros=cash,
                     target_wealth_micros=11000*UNIT,burnin_ms=1)


class ActionsTests(unittest.TestCase):
    def test_whole_exchange_price_quantity_and_expiration_domain(self):
        for tick in range(1,200):OrderSpec('x','YES','BUY',MAX_QUANTITY,tick*5000,999999999).validate(0)
        OrderSpec('x','YES','BUY',1,None).validate(0)
        with self.assertRaises(ValueError):OrderSpec('x','YES','BUY',1,5100).validate(0)
        with self.assertRaises(ValueError):OrderSpec('x','YES','BUY',MAX_QUANTITY+1,5000).validate(0)
        with self.assertRaises(ValueError):OrderSpec('x','YES','BUY',1,None,200).validate(0)

    def test_raw_orders_have_no_old_quantity_market_or_portfolio_cap(self):
        e=environment(100000*UNIT)
        r=e.dispatch('ORDER',[OrderSpec('x','YES','BUY',100001,5000)],idempotency_key='raw')
        self.assertEqual(r['status'],'IN_FLIGHT');e.advance(301)
        self.assertEqual(e.api_orders[1].remaining,100001)
        self.assertEqual(e.cash,100000*UNIT)
        self.assertEqual(e.budget.config.writes_per_minute,30)
        self.assertEqual(e.budget.config.write_completion_gap_ms,0)
        self.assertFalse(e.budget.reads)  # no invented 16-read preflight

    def test_reads_are_policy_selected_and_charged(self):
        e=environment();self.assertFalse(e.actor_books)
        e.advance(500);self.assertFalse(e.budget.reads)
        key=next(k for k in e.keys['SIG']if k[0]=='book');e.request_observation('SIG',key)
        e.advance(599);self.assertFalse(e.actor_books)
        e.advance(600);self.assertIn('x',e.actor_books)
        e.advance(2000);self.assertEqual(len(e.budget.reads),1)
        self.assertFalse(e.budget.writes)

    def test_flat_sell_opens_complement_and_receipt_hides_cash_until_delivery(self):
        e=environment();e.dispatch('ORDER',[OrderSpec('x','YES','SELL',5,400000)],idempotency_key='sell')
        e.advance(201)
        self.assertEqual(e.held('x','NO'),5);self.assertEqual(e.cash,997*UNIT)
        self.assertEqual(e.known_cash,1000*UNIT);self.assertFalse(e.known_lots)
        e.advance(301);self.assertEqual(e.known_cash,997*UNIT)

    def test_atomic_failure_rolls_back_cash_positions_and_shared_depth(self):
        e=environment();e.dispatch('ALL_OR_NONE',[OrderSpec('x','YES','BUY',5,450000),
            OrderSpec('x','YES','BUY',5,200000)],idempotency_key='atomic')
        e.advance(301)
        self.assertEqual(e.cash,1000*UNIT);self.assertFalse(e.lots)
        self.assertFalse(e.api_fills);self.assertFalse(e.books['x'].depleted)
        self.assertEqual(e.idempotency['atomic']['status'],'REJECTED')
        self.assertEqual(len(e.budget.writes),1)

    def test_one_batch_is_one_write_and_idempotent_retry_has_no_second_effect(self):
        e=environment();specs=[OrderSpec('x','YES','BUY',1,450000)]*3
        e.dispatch('BATCH',specs,idempotency_key='batch');e.advance(301)
        self.assertEqual(e.held('x','YES'),3);self.assertEqual(len(e.budget.writes),1)
        e.dispatch('BATCH',specs,idempotency_key='batch');self.assertEqual(e.held('x','YES'),3)
        with self.assertRaises(ValueError):e.dispatch('BATCH',specs[:1],idempotency_key='batch')

    def test_market_and_arbitrary_server_expiration_not_patience_menu(self):
        e=environment();e.dispatch('ORDER',[OrderSpec('x','YES','BUY',1000,None)],idempotency_key='market')
        e.advance(301);self.assertFalse(e.api_orders);self.assertEqual(e.held('x','YES'),100)
        e.dispatch('ORDER',[OrderSpec('x','YES','BUY',5,250000,2713)],idempotency_key='expiry')
        e.advance(601);self.assertTrue(e.api_orders)
        e.advance(2713);self.assertFalse(e.api_orders);self.assertEqual(len(e.budget.writes),2)

    def test_all_buying_power_requires_verified_floor_not_reference_mark(self):
        e=environment(UNIT);self.assertEqual(e.advance_capacity(),0)
        # Explicit synthetic guaranteed-floor fixture, not a statistical model.
        e.lots={('x','YES'):[[20,300000]],('y','NO'):[[20,300000]]}
        e.collateral_oracle=lambda lots:min(sum(n for n,p in lots.get(('x','YES'),[])),
                                          sum(n for n,p in lots.get(('y','NO'),[])))*UNIT
        e.dispatch('ORDER',[OrderSpec('x','YES','BUY',10,450000)],idempotency_key='advance')
        e.advance(301);self.assertEqual(e.cash,-3500000);self.assertEqual(e.held('x','YES'),30)

    def test_dynamic_policy_and_stream_chunks_preserve_state_without_1hz_cap(self):
        torch.set_num_threads(1);torch.manual_seed(3)
        e=environment();model=ApiPolicy(hidden_size=32)
        stream=Stream(e,model,calibration={'decision_latency_ms':2,'assumption':'synthetic_fixture'})
        o=e.observe();inputs=tensors(o,'cpu')
        out=model.step(**inputs,hidden=model.initial_state(1),reset=torch.tensor([True]))
        self.assertEqual(out['actions']['target_fraction'].dtype,torch.float64)
        self.assertEqual(model.heads['price'].out_features,200)
        action={'family':5,'market':1,'side':0,'price':1,'patience':0,
                'target_fraction':.05,'child_fraction':.01}
        a=stream.step(action);b=stream.step(action)
        self.assertEqual(len(e.builder),2);self.assertLess(b['next_ms']-a['at_ms'],1000)
        self.assertEqual(stream.decisions,2);self.assertFalse(b['reset'])
        self.assertFalse(e.budget.writes)

    def test_compute_chunks_do_not_end_episode_or_cap_lifetime_decisions(self):
        torch.set_num_threads(1);e=environment();model=ApiPolicy(hidden_size=16)
        s=Stream(e,model,calibration={'decision_latency_ms':1,'assumption':'synthetic_fixture'})
        def action(stream):return {'family':11,'market':0,'side':0,'price':0,'patience':0,
                                  'target_fraction':0.,'child_fraction':0.}
        a=s.run_chunk(1030,action_provider=action)
        self.assertFalse(a['terminal']);self.assertEqual(a['total_decisions'],1030)
        b=s.run_chunk(20,action_provider=action)
        self.assertEqual(b['total_decisions'],1050);self.assertFalse(b['terminal'])
        self.assertEqual(e.now,1051);self.assertFalse(e.budget.writes)

    def test_atomic_full_quantity_rejects_funding_clipped_leg(self):
        e=environment(UNIT)
        e.dispatch('ALL_OR_NONE',[OrderSpec('x','YES','BUY',10,450000)],idempotency_key='too-big')
        e.advance(301);self.assertEqual(e.cash,UNIT);self.assertFalse(e.lots)
        self.assertEqual(e.idempotency['too-big']['status'],'REJECTED')

    def test_distant_wait_is_interrupted_by_observable_receipt(self):
        e=environment();e.request_observation('SIG',e.keys['SIG'][0])
        self.assertEqual(e.decision_times(99999),101)

    def test_new_policy_can_train_from_continuing_chunks_without_inventing_rewards(self):
        from .training import collect_chunk,update
        torch.manual_seed(33);torch.set_num_threads(1)
        model=ApiPolicy(hidden_size=16);e=environment()
        stream=Stream(e,model,calibration={'decision_latency_ms':1,'assumption':'synthetic_fixture'},record_training=True)
        chunk=collect_chunk(stream,8)
        with self.assertRaises(ValueError):update(model,torch.optim.Adam(model.parameters()),chunk,[None]*len(chunk.frames),role='train')
        result=update(model,torch.optim.Adam(model.parameters(),lr=1e-5),chunk,
                      [0.]*len(chunk.frames),role='train',epochs=1)
        self.assertTrue(result[0]['accepted'])
        self.assertGreater(stream.decisions,0)
        e.role='test'
        with self.assertRaises(ValueError):collect_chunk(stream,1)

    def test_rate_budget_is_real_rolling_limit_without_completion_headroom(self):
        e=environment()
        for i in range(30):
            r=e.dispatch('CANCEL_ALL',None,idempotency_key=str(i));self.assertEqual(r['status'],'IN_FLIGHT')
            e.advance(e.now+300)
        r=e.dispatch('CANCEL_ALL',None,idempotency_key='31')
        self.assertEqual(r['status'],'RATE_WAIT');self.assertGreaterEqual(r['ready_ms'],60001)
        e.budget.unknown.add('unresolved')
        self.assertEqual(e.dispatch('CANCEL_ALL',None,idempotency_key='32')['status'],'UNRESOLVED_EXECUTION')
        self.assertEqual(e.budget.unknown,{'unresolved'})

    def test_collateral_cannot_be_created_from_empty_inventory(self):
        e=environment();e.collateral_oracle=lambda lots:UNIT
        with self.assertRaises(ValueError):e.advance_capacity()

    def test_late_account_snapshot_cannot_roll_back_known_fill_or_idempotent_response(self):
        e=environment();old=e._capture_account()
        e.dispatch('ORDER',[OrderSpec('x','YES','BUY',1,450000)],idempotency_key='known')
        e.advance(301);cash=e.known_cash;e._publish_account(old)
        self.assertEqual(e.known_cash,cash)

    def test_relationship_constraint_requires_known_authoritative_validator(self):
        e=environment()
        e.dispatch('MULTI',[OrderSpec('x','YES','BUY',1,450000)],idempotency_key='constraint',relationship_constraint='r')
        e.advance(301)
        self.assertEqual(e.idempotency['constraint']['error'],'RELATIONSHIP_DATA_UNAVAILABLE')
        self.assertFalse(e.lots)

    def test_all_three_observation_venues_are_available_with_sig_only_execution(self):
        e=environment()
        for venue in ('SIG','Kalshi','Polymarket'):
            key=next(k for k in e.keys[venue]if k[0]=='book')if venue=='SIG'else e.keys[venue][0]
            self.assertEqual(e.request_observation(venue,key)['status'],'IN_FLIGHT')
        e.advance(151)
        self.assertTrue(e.actor_books)
        self.assertEqual({r['venue']for r in e.reference_cache.values()},{'Kalshi','Polymarket'})
        self.assertFalse(e.api_orders);self.assertFalse(e.budget.writes)

    def test_policy_sees_validation_outcome_only_after_response_arrives(self):
        e=environment();e.dispatch('ORDER',[OrderSpec('x','YES','BUY',1,5100)],idempotency_key='bad-tick')
        e.advance(201);self.assertEqual(e.observe()['global_state'][10],0.)
        e.advance(301);self.assertEqual(e.observe()['global_state'][10],1.)

    def test_frozen_checkpoint_is_verified_and_legacy_weights_are_not_relabelled(self):
        from .training import save_checkpoint,load_checkpoint
        from .entry import open_experiment
        from moloch.learning_v2.train_inner import fingerprint
        import tempfile
        from pathlib import Path
        model=ApiPolicy(hidden_size=16);optimizer=torch.optim.Adam(model.parameters())
        with tempfile.TemporaryDirectory()as d:
            path=Path(d)/'fixture.pt'
            sha=save_checkpoint(path,model,optimizer,dataset_manifest_sha256='e'*64,split_specification_sha256='b'*64)
            restored,_=load_checkpoint(path,sha)
            self.assertEqual(fingerprint(restored),fingerprint(model))
            with path.open('ab')as f:f.write(b'tamper')
            with self.assertRaises(ValueError):load_checkpoint(path,sha)
        with self.assertRaises(ValueError):
            open_experiment(None,None,None,'test',start_ms=1,end_ms=2,
                initial_cash_micros=UNIT,target_wealth_micros=UNIT,frozen_finalist_sha256='b'*64)


if __name__=='__main__':unittest.main()
