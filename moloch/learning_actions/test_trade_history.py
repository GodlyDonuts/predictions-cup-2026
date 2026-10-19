import unittest
from .trade_history import TradeHistory
from .test_actions import environment


def trade(i, at=1, executed=0):
    return {'kind':'trade_flow','venue':'SIG','exchange_id':'x','market_id':'m',
            'tournament_id':'t','match_id':str(i),'available_at_ms':at,
            'received_at_ms':at,'executed_at_ms':executed,'quantity':i+1,
            'yes_price_micros':400000,'original_sha256':'a'*64}


class TradeHistoryTests(unittest.TestCase):
    def test_max_default_limits_and_half_open_execution_range(self):
        h=TradeHistory()
        for i in range(450):h.accept(10,trade(i,10,i%10))
        self.assertEqual(len(h.page('x',11)['data']),50)
        self.assertEqual(len(h.page('x',11,limit=200)['data']),200)
        for bad in (0,201,True,1.5):
            with self.assertRaises(ValueError):h.page('x',11,limit=bad)
        p=h.page('x',11,from_ms=3,to_ms=4)
        self.assertEqual(len(p['data']),45)
        self.assertTrue(all(r['executed_at_ms']==3 for r in p['data']))
        self.assertFalse(p['coverage']['complete'])

    def test_cursor_frozen_before_new_print_or_late_report(self):
        h=TradeHistory()
        for i in range(450):h.accept(1,trade(i))
        first=h.page('x',2,limit=200)
        h.accept(3,trade(999,3,0))
        second=h.page('x',4,limit=200,cursor=first['pagination']['nextCursor'])
        third=h.page('x',5,limit=200,cursor=second['pagination']['nextCursor'])
        ids=[r['match_id']for p in(first,second,third)for r in p['data']]
        self.assertEqual(len(ids),450);self.assertEqual(len(set(ids)),450)
        self.assertNotIn('999',ids);self.assertFalse(third['pagination']['hasMore'])
        self.assertIsNone(third['pagination']['nextCursor'])
        for ex,at in (('other',4),('x',0)):
            with self.assertRaises(ValueError):h.page(ex,at,cursor=first['pagination']['nextCursor'])

    def test_source_facts_immutable_and_future_availability_excluded(self):
        h=TradeHistory();row=trade(0);h.accept(1,row);row['quantity']=99
        self.assertEqual(h.page('x',2)['data'][0]['quantity'],1)
        h.accept(3,trade(1,3,0))
        self.assertEqual(len(h.page('x',2)['data']),1)
        with self.assertRaises(ValueError):h.accept(3,{**trade(0,3),'quantity':99})

    def test_paid_pages_keep_backlog_and_publish_only_on_receipt(self):
        e=environment();key=('trade_flow','x')
        for i in range(450):e._event(e.now,'trade_flow',trade(i))
        self.assertNotIn('x',e.known_trade_pages);self.assertFalse(e.flow.seen)
        for size in (200,200,50):
            self.assertEqual(e.request_observation('SIG',key,trade_limit=200)['status'],'IN_FLIGHT')
            self.assertEqual(e.clock.next_observable(),e.busy_until)
            self.assertEqual(e.request_observation('SIG',key)['status'],'BUSY')
            e.advance(e.now+100)
            self.assertEqual(len(e.known_trade_pages['x']['data']),size)
        self.assertEqual(len(e.budget.reads),3);self.assertFalse(e.budget.writes)
        self.assertEqual(len(e.flow.seen),450)
        self.assertEqual(e.feed_request_counts['SIG'],3)
        self.assertEqual(e.feed_receipt_counts['SIG'],3)

    def test_targets_do_not_reveal_unpaid_print_activity_and_raw_rows_are_visible(self):
        e=environment();before=list(e.observe()['entities'])
        e._event(e.now,'trade_flow',trade(0))
        self.assertEqual(e.observe()['entities'],before)
        e.request_observation('SIG',('trade_flow','x'));e.advance(e.now+100)
        obs=e.observe();i=obs['entities'].index(('trade_print',('x',0)))
        self.assertFalse(obs['masks']['market'][i]);self.assertEqual(obs['contracts'][i,24],7.)
        self.assertAlmostEqual(float(obs['contracts'][i,0]),.4)

    def test_rate_budget_applies_and_new_query_can_restart(self):
        e=environment();key=('trade_flow','x')
        for i in range(250):e._event(e.now,'trade_flow',trade(i))
        e.request_observation('SIG',key,trade_limit=200);e.advance(e.now+100)
        first=e.known_trade_pages['x']['data']
        e.request_observation('SIG',key,trade_limit=200,continue_trade_page=False);e.advance(e.now+100)
        self.assertEqual(e.known_trade_pages['x']['data'],first)
        for _ in range(98):
            e.request_observation('SIG',key,trade_limit=1);e.advance(e.now+100)
        self.assertEqual(len(e.budget.reads),100)
        self.assertEqual(e.request_observation('SIG',key)['status'],'RATE_WAIT')

    def test_query_choices_have_density_and_survive_fused_and_batched_paths(self):
        import torch
        from .policy import ApiPolicy
        from .stream import tensors
        from .batched import inputs
        from .training import sequence_forward
        torch.manual_seed(71);torch.set_num_threads(1)
        e=environment();model=ApiPolicy(hidden_size=16);obs=e.observe()
        data=tensors(obs,'cpu');n=obs['entities'].index(('read',('SIG',('trade_flow','x'))))
        actions={k:torch.tensor([v],dtype=torch.float64 if 'fraction'in k else torch.int64)
                 for k,v in {'market':n,'family':10,'side':0,'price':0,'patience':0,
                             'target_fraction':.2,'child_fraction':.5}.items()}
        hidden=model.initial_state(1);reset=torch.tensor([True])
        a=model.step(**data,hidden=hidden,reset=reset,actions=actions)
        b=model.step(**data,hidden=hidden,reset=reset,actions={**actions,'target_fraction':torch.tensor([.8],dtype=torch.float64)})
        self.assertNotEqual(float(a['logprob'].detach()),float(b['logprob'].detach()))
        frame={**data,'actions':actions,'reset':reset}
        fused,_=sequence_forward(model,[frame],hidden)
        self.assertTrue(torch.allclose(fused['logprob'][0],a['logprob'],atol=1e-6))
        e._event(e.now,'trade_flow',trade(0));e.request_observation('SIG',('trade_flow','x'));e.advance(e.now+100)
        padded=inputs([obs,e.observe()],'cpu')
        self.assertEqual(padded['masks']['trade_query'].shape[0],2)
        self.assertEqual(padded['masks']['trade_query'].shape[1],padded['contracts'].shape[1])

    def test_paid_older_history_is_available_without_crossing_roles_or_filling(self):
        from types import SimpleNamespace
        from .trade_history import seed_role_history
        from moloch.learning_realism.protocol import Split
        e=environment();rows=[trade(i,at,0)for i,at in enumerate((0,2,4,6,8))]
        tape=SimpleNamespace(flow_rows=rows,flow_times=tuple(r['available_at_ms']for r in rows),
            counterparty_information_bounds=(2,8),split=Split(0,1000000000),role='train')
        self.assertEqual(seed_role_history(tape,e,6),2)
        self.assertFalse(e.flow.seen);self.assertFalse(e.known_trade_pages);self.assertFalse(e.api_fills)
        self.assertEqual({r['match_id']for r in e.trade_history.page('x',7)['data']},{'1','2'})
        with self.assertRaises(ValueError):seed_role_history(tape,e,8)


if __name__=='__main__':unittest.main()
