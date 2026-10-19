import unittest,torch
from .test_actions import environment
from .contract import OrderSpec,UNIT
from .passive import StrictPriorityFlow,DisplayedQueueFlow


def print_row(at=1000,quantity=7,price=400000,identity='trade1',available=None):
    return {'kind':'trade_flow','venue':'SIG','tournament_id':'cup','exchange_id':'x','market_id':'m',
        'match_id':identity,'quantity':quantity,'yes_price_micros':price,'executed_at_ms':at,
        'received_at_ms':at,'available_at_ms':at if available is None else available,
        'original_sha256':'a'*64,'aggressor_side_known':False}


def env():
    e=environment();e.passive=StrictPriorityFlow(e,'cup');return e


class PassiveTests(unittest.TestCase):
    def test_same_exchange_resting_commitment_uses_remaining_room_without_global_escrow(self):
        e=environment(UNIT)
        e._execute('ORDER',[OrderSpec('x','YES','BUY',3,200000)])
        e._execute('ORDER',[OrderSpec('x','YES','BUY',100,200000)])
        self.assertEqual(e.api_orders[2].remaining,2)
        self.assertEqual(sum(o.remaining*o.spec.price for o in e.api_orders.values()),UNIT)
        self.assertEqual(e.cash,UNIT)
    def test_connected_private_fills_share_one_publication_window(self):
        from .realtime import DerivedStream
        e=env();e.realtime=DerivedStream(e);e.private_fill_push=False
        e.keys['SIG'].append(('realtime_token',));e.request_observation('SIG',('realtime_token',));e.advance(101)
        e._execute('ORDER',[OrderSpec('x','YES','BUY',10,425000)])
        e.clock.schedule(1000,10,'api_counterparty_trade',print_row(quantity=2))
        e.clock.schedule(1200,10,'api_counterparty_trade',print_row(at=1200,quantity=2,identity='trade2'))
        e.advance(2099);self.assertEqual(e.known_cash,1000*UNIT)
        e.advance(2100);self.assertEqual(e.known_cash,1000*UNIT-4*425000)
        self.assertFalse(e.realtime.private_pending)

    def test_private_stream_receipts_coalesce_and_disconnect_requires_rest_reconciliation(self):
        from .realtime import DerivedStream
        e=env();e.realtime=DerivedStream(e);e.private_fill_push=False
        e.keys['SIG'].append(('realtime_token',));e.request_observation('SIG',('realtime_token',));e.advance(101)
        e._execute('ORDER',[OrderSpec('x','YES','BUY',5,425000)])
        e.clock.schedule(1000,10,'api_counterparty_trade',print_row())
        e.advance(1100);self.assertEqual(e.known_cash,1000*UNIT)
        e.realtime.disconnect();e.advance(3000)
        self.assertEqual(e.known_cash,1000*UNIT)
        e.request_observation('SIG',('account',));e.advance(3100)
        self.assertEqual(e.known_cash,e.cash)

    def queue(self):
        e=environment();e.passive=DisplayedQueueFlow(e,'cup');return e

    def test_displayed_queue_is_consumed_once_and_own_orders_remain_fifo(self):
        e=self.queue();s=OrderSpec('x','YES','BUY',10,400000)
        e._execute('ORDER',[s]);e._execute('ORDER',[s]);e.advance(1000)
        e.passive.trade(1000,print_row(quantity=105))
        self.assertEqual(e.held('x','YES'),5);self.assertEqual(e.api_orders[1].remaining,5)
        e.advance(1100);e.passive.trade(1100,print_row(at=1100,quantity=12,identity='trade2'))
        self.assertEqual(e.held('x','YES'),17);self.assertEqual(e.api_orders[2].remaining,3)
        self.assertEqual(e.passive.ahead_consumed,100);self.assertEqual(e.passive.intercepted_volume,17)

    def test_external_quantity_drops_never_give_free_cancel_credit(self):
        from dataclasses import replace
        e=self.queue();e._execute('ORDER',[OrderSpec('x','YES','BUY',10,400000)])
        world=e.books['x'];original=world.book
        world.update(replace(original,bids=()));e.passive.on_book('x')
        e.advance(500);e.passive.trade(500,print_row(at=500,quantity=10))
        self.assertFalse(e.api_fills)
        world.update(replace(original,bids=((400000,20),)));e.passive.on_book('x')
        self.assertEqual(e.passive.queues[('x',True,400000)]['ahead'],110)

    def test_better_quotes_and_equal_queue_share_the_same_print_not_two_copies(self):
        e=self.queue();e._execute('ORDER',[OrderSpec('x','YES','BUY',5,425000)])
        e._execute('ORDER',[OrderSpec('x','YES','BUY',100,400000)])
        e.advance(1000);e.passive.trade(1000,print_row(quantity=150))
        self.assertEqual(e.held('x','YES'),50)
        self.assertEqual(e.passive.intercepted_volume,50)
        self.assertEqual(e.passive.ahead_consumed,100)

    def test_unknown_initial_queue_and_rolled_back_atomic_quotes_cannot_get_priority(self):
        e=self.queue();world=e.books.pop('x')
        e._execute('ORDER',[OrderSpec('x','YES','BUY',10,400000)])
        e.books['x']=world;e.passive.on_book('x')
        e.advance(1000);e.passive.trade(1000,print_row(quantity=1000))
        self.assertFalse(e.api_fills)
        rollback=self.queue()
        with self.assertRaises(ValueError):
            rollback._execute('ALL_OR_NONE',[OrderSpec('x','YES','BUY',5,400000),OrderSpec('x','YES','BUY',5,450000)])
        self.assertFalse(rollback.passive.queues);self.assertFalse(rollback.api_fills)

    def test_strict_priority_shares_recorded_volume_fifo_and_delays_private_receipt(self):
        e=env();s=OrderSpec('x','YES','BUY',5,425000)
        e._execute('ORDER',[s]);e._execute('ORDER',[s])
        e.clock.schedule(1000,10,'api_counterparty_trade',print_row());e.advance(1000)
        self.assertEqual(e.held('x','YES'),7);self.assertEqual(e.api_orders[2].remaining,3)
        self.assertEqual(e.cash,1000*UNIT-7*425000);self.assertEqual(e.known_cash,1000*UNIT)
        e.advance(1100);self.assertEqual(e.known_cash,e.cash)
        self.assertEqual(e.passive.intercepted_volume,7)
        e.passive.trade(1000,print_row());self.assertEqual(e.held('x','YES'),7)
        with self.assertRaisesRegex(ValueError,'FACTS_CHANGED'):e.passive.trade(1000,print_row(quantity=8))

    def test_equal_price_or_same_timestamp_placement_never_invents_queue_priority(self):
        e=env();e._execute('ORDER',[OrderSpec('x','YES','BUY',10,400000)])
        e.advance(1000);e._execute('ORDER',[OrderSpec('x','YES','BUY',10,425000)])
        e.passive.trade(1000,print_row());self.assertFalse(e.api_fills)

    def test_no_side_and_covered_sales_use_yes_price_direction_and_own_limit(self):
        e=env();e._execute('ORDER',[OrderSpec('x','NO','BUY',3,575000)])
        e.advance(1000);e.passive.trade(1000,print_row(price=450000))
        self.assertEqual(e.held('x','NO'),3);self.assertEqual(e.cash,1000*UNIT-3*575000)
        sale=env();sale.lots={('x','YES'):[[3,300000]]}
        sale._execute('ORDER',[OrderSpec('x','YES','SELL',3,425000)])
        sale.advance(1000);sale.passive.trade(1000,print_row(price=450000))
        self.assertEqual(sale.cash,1000*UNIT+3*425000);self.assertEqual(sale.realized,3*125000)

    def test_crossing_liquidity_cannot_also_be_used_as_new_print_volume(self):
        e=env();e._execute('ORDER',[OrderSpec('x','YES','BUY',5,450000)])
        e._execute('ORDER',[OrderSpec('x','YES','BUY',10,425000)])
        e.advance(1000);e.passive.trade(1000,print_row())
        self.assertEqual(e.passive.intercepted_volume,2);self.assertEqual(e.held('x','YES'),7)
        rollback=env()
        with self.assertRaises(ValueError):
            rollback._execute('ALL_OR_NONE',[OrderSpec('x','YES','BUY',5,450000),OrderSpec('x','YES','BUY',5,200000)])
        self.assertFalse(rollback.passive.crossing_debt)
        self.assertFalse(rollback.api_fills)

    def test_expiry_owner_exclusions_and_original_scope_are_respected(self):
        e=env();e._execute('ORDER',[OrderSpec('x','YES','BUY',10,425000,1000)])
        e.advance(1000);e.passive.trade(1000,print_row());self.assertFalse(e.api_fills)
        with self.assertRaises(ValueError):StrictPriorityFlow(e,'cup',excluded_match_ids=['trade1'])
        excluded=env();excluded.passive=StrictPriorityFlow(excluded,'cup',excluded_match_ids=['trade1'],exclusion_source_sha256='a'*64)
        excluded._execute('ORDER',[OrderSpec('x','YES','BUY',10,425000)])
        excluded.advance(1000);excluded.passive.trade(1000,print_row())
        self.assertFalse(excluded.api_fills);self.assertEqual(excluded.passive.excluded_prints,1)

    def test_delayed_public_print_does_not_fill_order_posted_after_execution(self):
        from moloch.learning_realism.test_vector import tapes
        from moloch.learning_realism.test_replay import fixture
        from .engine import ApiTape
        _,source=tapes();row=print_row(at=500,available=5000)
        source.flow_rows=(row,);source.flow_times=(5000,)
        tape=ApiTape.from_loaded(source,split=source.split,role='train')
        tape.counterparty_replay_enabled=True
        e=tape.episode(fixture().mechanics,1,10000,initial_cash_micros=1000*UNIT,
            target_wealth_micros=10000*UNIT,burnin_ms=1)
        e.passive=StrictPriorityFlow(e,'cup');e.advance(1000)
        e._execute('ORDER',[OrderSpec('x','YES','BUY',10,425000)])
        e.advance(6000);self.assertFalse(e.api_fills)

    def test_late_public_reporting_inside_role_does_not_hide_preterminal_fills(self):
        from moloch.learning_realism.test_vector import tapes
        from moloch.learning_realism.test_replay import fixture
        from .engine import ApiTape
        _,source=tapes();row=print_row(at=1000,available=5000)
        source.flow_rows=(row,);source.flow_times=(5000,)
        tape=ApiTape.from_loaded(source,split=source.split,role='train')
        tape.counterparty_replay_enabled=True;tape.counterparty_information_bounds=(0,10000)
        e=tape.episode(fixture().mechanics,1,2000,initial_cash_micros=1000*UNIT,
            target_wealth_micros=10000*UNIT,burnin_ms=1)
        e.passive=StrictPriorityFlow(e,'cup');e._execute('ORDER',[OrderSpec('x','YES','BUY',5,425000)])
        e.advance(2000);self.assertEqual(e.held('x','YES'),5)
        # The future public print was not supplied to the actor before end.
        self.assertFalse(e.flow.rows)

    def test_future_hidden_fill_wakes_wait_only_when_its_receipt_arrives(self):
        from .stream import Stream
        from .policy import ApiPolicy
        torch.set_num_threads(1);e=env()
        e._execute('ORDER',[OrderSpec('x','YES','BUY',5,425000)])
        e.clock.schedule(1000,10,'api_counterparty_trade',print_row())
        stream=Stream(e,ApiPolicy(16),calibration={'decision_latency_ms':2})
        stream.step({'family':0,'market':0,'side':0,'price':0,'patience':0,
                     'target_fraction':0.,'child_fraction':1.})
        self.assertEqual(e.now,1100);self.assertEqual(e.held('x','YES'),5)
        self.assertEqual(e.known_cash,e.cash)


if __name__=='__main__':unittest.main()
