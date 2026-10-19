import tempfile,unittest,math,hashlib
import torch
from pathlib import Path
from dataclasses import replace
from .test_actions import environment
from .contract import UNIT,OrderSpec
from .economics import snapshot,GrowthLedger
from .selection import IndependentTest
from moloch.learning_realism.protocol import DAY


class EconomicTests(unittest.TestCase):
    def test_busy_or_rate_limited_sig_does_not_suspend_other_policy_decisions(self):
        from .policy import ApiPolicy
        from .stream import Stream
        torch.set_num_threads(1);e=environment()
        e.dispatch('CANCEL_ALL',None,idempotency_key='inflight')
        stream=Stream(e,ApiPolicy(16),calibration={'decision_latency_ms':2})
        action={'family':4,'market':0,'side':0,'price':0,'patience':0,
                'target_fraction':0.,'child_fraction':0.}
        before=e.now;result=stream.step(action)
        self.assertEqual(result['status'],'BUSY');self.assertEqual(e.now,before+2)
        e.advance(301)
        for i in range(29):
            e.dispatch('CANCEL_ALL',None,idempotency_key='limit'+str(i));e.advance(e.now+300)
        before=e.now;result=stream.step(action)
        self.assertEqual(result['status'],'RATE_WAIT');self.assertEqual(e.now,before+2)

    def test_fused_ragged_sequence_matches_loop_values_probabilities_and_memory(self):
        from .policy import ApiPolicy
        from .stream import Stream
        from .training import collect_chunk,sequence_forward
        import torch.nn.functional as F
        torch.set_num_threads(1);torch.manual_seed(910)
        model=ApiPolicy(16);e=environment()
        with torch.no_grad():model.fractions.bias[2]=-10.;model.fractions.bias[3]=100.
        stream=Stream(e,model,calibration={'decision_latency_ms':1},record_training=True)
        collect_chunk(stream,4);chunk=collect_chunk(stream,8)
        chunk.frames[2]['contracts']=F.pad(chunk.frames[2]['contracts'],(0,0,0,3))
        chunk.frames[2]['present']=F.pad(chunk.frames[2]['present'],(0,3))
        chunk.frames[2]['masks']={k:F.pad(v,(0,3)if k=='market'else(0,0,0,3))
                                  for k,v in chunk.frames[2]['masks'].items()}
        hidden=chunk.initial_hidden;expected=[]
        for frame in chunk.frames:
            out=model.step(**{k:frame[k]for k in ('contracts','global_state','present','masks','reset')},
                hidden=hidden,actions=frame['actions'])
            expected.append(out);hidden=out['hidden']
        fused,last=sequence_forward(model,chunk.frames,chunk.initial_hidden)
        for key in ('logprob','value','entropy'):
            torch.testing.assert_close(fused[key],torch.stack([x[key]for x in expected]),atol=1e-5,rtol=1e-5)
        torch.testing.assert_close(last,hidden,atol=1e-6,rtol=1e-5)
        fused['logprob'].sum().backward()
        self.assertTrue(all(p.grad is None or torch.isfinite(p.grad).all()for p in model.parameters()))

    def test_final_compute_interval_keeps_terminal_reward_frame(self):
        from .policy import ApiPolicy
        from .stream import Stream
        from .training import collect_chunk
        torch.set_num_threads(1);e=environment();e.end=e.now+1
        ledger=GrowthLedger(e,development=True)
        stream=Stream(e,ApiPolicy(16),calibration={'decision_latency_ms':10},record_training=True)
        chunk=collect_chunk(stream,8,reward_ledger=ledger)
        self.assertEqual(len(chunk.frames),1);self.assertTrue(chunk.frames[0]['terminal'])
        self.assertEqual(chunk.frames[0]['economic_reward'],0.)
        self.assertFalse(e.budget.writes)

    def test_raw_calendar_terminal_excludes_next_role_source_observation(self):
        from moloch.learning_realism.test_vector import tapes
        from moloch.learning_realism.test_replay import fixture
        from .engine import ApiTape
        _,source=tapes();t=ApiTape.from_loaded(source,split=source.split,role='train')
        e=t.episode(fixture().mechanics,1,5000,initial_cash_micros=1000*UNIT,
                    target_wealth_micros=10000*UNIT,burnin_ms=1)
        e.advance(5000)
        self.assertEqual(e.books['x'].book.available_at_ms,1000)

    def test_mark_does_not_subtract_reservations_or_invent_execution_depth(self):
        e=environment();e.lots={('x','YES'):[[1000000,450000]]}
        value=snapshot(e)
        self.assertEqual(value.twice_micros,2*e.cash+850000*1000000)
        self.assertEqual(value.convention,'causal_SIG_midpoint_not_executable_liquidation')
        e.books['x'].invalidate()
        with self.assertRaisesRegex(ValueError,'INCOMPLETE_SIG'):snapshot(e)
        historical=snapshot(e,historical_last_mark=True)
        self.assertEqual(historical.twice_micros,value.twice_micros)
        self.assertEqual(historical.stale_exchanges,('x',))
        with self.assertRaisesRegex(ValueError,'DEVELOPMENT_ONLY'):
            GrowthLedger(e,historical_last_mark=True)

    def test_complement_netting_cash_deltas_equal_exact_wallet_change(self):
        e=environment();initial=e.cash
        e._execute('ORDER',[OrderSpec('x','YES','BUY',3,450000)])
        e._execute('ORDER',[OrderSpec('x','NO','BUY',3,600000)])
        self.assertFalse(any(e.lots.values()))
        self.assertEqual(sum(f['cash_delta_micros']for f in e.api_fills),e.cash-initial)

    def test_full_growth_score_and_dense_terminal_reward_telescope(self):
        e=environment();e.end=e.now+2*DAY
        ledger=GrowthLedger(e)
        e.cash+=10*UNIT;e.advance(e.start+DAY)
        a=ledger.transition()
        e.cash+=10*UNIT;e.advance(e.end)
        b=ledger.transition();result=ledger.finish()
        self.assertEqual(result['D'],2)
        # No trading opportunity can be manufactured by a fixture cash deposit.
        self.assertEqual(result['N'],0);self.assertEqual(result['reward'],0.)
        self.assertAlmostEqual(a+b+result['terminal_reward_correction'],result['reward'])
        with self.assertRaises(ValueError):ledger.finish()

    def test_actual_trade_cashflows_cluster_not_individual_fills(self):
        e=environment();e.end=e.now+2*DAY
        ledger=GrowthLedger(e,maximum_mark_age_ms=3*DAY)
        e._execute('ORDER',[OrderSpec('x','YES','BUY',10,450000)])
        book=e.books['x'].book
        e.books['x'].update(replace(book,bids=((550000,100),),asks=((600000,100),)))
        e._execute('ORDER',[OrderSpec('x','YES','SELL',5,550000)])
        e._execute('ORDER',[OrderSpec('x','YES','SELL',5,550000)])
        e.advance(e.start+DAY);ledger.transition()
        e.advance(e.end);ledger.transition();result=ledger.finish()
        self.assertEqual(result['N'],1);self.assertEqual(result['fill_count'],3)
        # One profitable day followed by flat does not pass the human's
        # consistency penalty, even though aggregate marked ROI is positive.
        self.assertLess(result['reward'],0)
        self.assertGreater(result['marked_roi'],0)
        self.assertEqual(result['realized_fifo_micros'],UNIT)

    def test_short_history_must_not_be_called_independent_days(self):
        e=environment()
        with self.assertRaises(ValueError):GrowthLedger(e)
        ledger=GrowthLedger(e,development=True);e.advance(e.end)
        ledger.transition();result=ledger.finish()
        self.assertFalse(result['eligible']);self.assertIsNone(result['D'])

    def test_independent_test_consumed_once_even_if_evaluator_crashes(self):
        with tempfile.TemporaryDirectory()as d:
            checkpoint=Path(d)/'fixture.bin';checkpoint.write_bytes(b'actual fixture checkpoint')
            sha=hashlib.sha256(checkpoint.read_bytes()).hexdigest()
            registry=IndependentTest(Path(d)/'registry','a'*64)
            with self.assertRaises(ValueError):registry.freeze(checkpoint,sha,{'role':'train'})
            registry.freeze(checkpoint,sha,{'role':'evolution','eligible':True,
                'checkpoint_sha256':sha,'reward':.01})
            with self.assertRaises(ValueError):registry.reserve('b'*64)
            registry.reserve(sha)
            with self.assertRaises(FileExistsError):registry.reserve(sha)
            registry.finish({'role':'test','checkpoint_sha256':sha,'reward':-.01})


if __name__=='__main__':unittest.main()
