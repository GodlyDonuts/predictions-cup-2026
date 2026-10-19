import unittest
import torch
from .evaluation_batch import EvaluationBatch
from .policy import ApiPolicy
from .full_depth import FullDepthPolicy,FullDepthEpisode
from .stream import Stream
from .economics import GrowthLedger
from .test_actions import environment
from .test_evaluation_valuation import world,print_before_book


class EvaluationBatchTests(unittest.TestCase):
    def setUp(self):torch.set_num_threads(1);torch.manual_seed(28)

    def compare(self,kind,deterministic,batch_heads=False,actions_only=False):
        model=kind(32);seeds=[41,72,99];serial=[environment()for _ in seeds];parallel=[environment()for _ in seeds]
        if issubclass(kind,FullDepthPolicy):
            for e in serial+parallel:e.__class__=FullDepthEpisode;e.observation_protocol=model.observation_protocol
        # Unequal entity counts exercise per-case trimming before sampling.
        serial[1].scope['fixture-extra']=[];parallel[1].scope['fixture-extra']=[]
        for j,e in enumerate(serial+parallel):e.end=100+(j%3)*25
        traces=[];hidden=[];states=[]
        for seed,e in zip(seeds,serial):
            torch.manual_seed(seed);stream=Stream(e,model,calibration={'decision_latency_ms':2},actions_only=actions_only)
            trace=[]
            while e.now<e.end:trace.append(stream.step(deterministic=deterministic))
            traces.append(trace);hidden.append(stream.hidden);states.append(torch.get_rng_state())
        batch=EvaluationBatch(parallel,model,seeds,calibration={'decision_latency_ms':2},batch_action_heads=batch_heads,actions_only=actions_only)
        global_rng=torch.get_rng_state().clone();actual=[[]for _ in seeds]
        while batch.step(deterministic=deterministic):
            for row in batch.last_rows:actual[row['case']].append(row)
        torch.testing.assert_close(torch.get_rng_state(),global_rng,atol=0,rtol=0)
        for i,(a,b)in enumerate(zip(traces,actual)):
            self.assertEqual(len(a),len(b))
            for x,y in zip(a,b):
                for key in ('at_ms','next_ms','status','terminal'):self.assertEqual(x[key],y[key])
                for key in x['action']:
                    if isinstance(x['action'][key],float):self.assertAlmostEqual(x['action'][key],y['action'][key],places=5)
                    else:self.assertEqual(x['action'][key],y['action'][key])
            self.assertEqual(serial[i].cash,parallel[i].cash);self.assertEqual(serial[i].api_fills,parallel[i].api_fills)
            torch.testing.assert_close(hidden[i][0],batch.hidden[i],atol=2e-6,rtol=1e-5)
            torch.testing.assert_close(states[i],batch.rng[i],atol=0,rtol=0)

    def test_cpu_case_rngs_match_serial_stochastic_and_deterministic_paths(self):
        for kind in (ApiPolicy,FullDepthPolicy):
            for deterministic in (False,True):self.compare(kind,deterministic)

    def test_actions_only_keeps_case_rngs_and_both_batch_sampling_paths(self):
        from .joint_memory_policy import JointMemoryPolicy
        for heads in(False,True):self.compare(JointMemoryPolicy,False,batch_heads=heads,actions_only=True)

    def test_batched_action_heads_match_serial_paths_and_rng_with_full_joint_memory(self):
        from .joint_memory_policy import JointMemoryPolicy
        from .query_policy import QueryWindowPolicy
        for kind in(ApiPolicy,FullDepthPolicy,QueryWindowPolicy,JointMemoryPolicy):
            for deterministic in(False,True):self.compare(kind,deterministic,True)

    def test_frozen_model_guard_and_input_validation(self):
        model=ApiPolicy(16);batch=EvaluationBatch([environment()],model,[1],calibration={'decision_latency_ms':1})
        with torch.no_grad():model.value.bias.add_(1)
        with self.assertRaisesRegex(ValueError,'FROZEN_EVALUATION'):batch.run_chunk(1)
        e=environment()
        with self.assertRaisesRegex(ValueError,'INDEPENDENT_WORLD'):EvaluationBatch([e,e],model,[1,2],calibration={'decision_latency_ms':1})
        with self.assertRaisesRegex(ValueError,'NONZERO_MEASURED'):EvaluationBatch([e],model,[1],calibration={'decision_latency_ms':0})

    def test_bad_compute_profile_is_rejected_before_dataset_or_output_creation(self):
        import tempfile
        from pathlib import Path
        from .experiment import Experiment
        with tempfile.TemporaryDirectory()as d:
            target=Path(d)/'must-not-open'
            for config in ({'batched_evaluation_action_heads':True},{'batched_evaluation':1}):
                with self.assertRaisesRegex(ValueError,'EXPLICIT_BATCH_ACTION_HEADS'):Experiment(config,target)
                self.assertFalse(target.exists())

    def test_evaluation_never_requests_intermediate_mark_for_early_fill(self):
        model=ApiPolicy(16)
        with torch.no_grad():
            for head in model.heads.values():head.weight.zero_();head.bias.zero_()
            model.heads['family'].bias[11]=100.
        e=world();ledger=GrowthLedger(e,development=True);print_before_book(e)
        batch=EvaluationBatch([e],model,[1],calibration={'decision_latency_ms':1000})
        while batch.run_chunk(2,deterministic=True):pass
        result=ledger.finish();self.assertEqual(result['fill_count'],1);self.assertAlmostEqual(result['marked_roi'],-.00025)
        self.assertEqual(ledger.reward_sum,0.)

    def test_experiment_batch_runner_keeps_strict_terminal_marks_and_role(self):
        from .test_evaluation_valuation import EvaluationValuationTests
        from types import SimpleNamespace
        for terminal in (False,True):
            x=EvaluationValuationTests().experiment(terminal);x.config['batched_evaluation']=True
            x.stream=lambda model,e:SimpleNamespace(calibration={'decision_latency_ms':1000})
            def make_world(*args):
                e=world(terminal)
                e.clock.schedule(1000,10,'api_counterparty_trade',dict(tournament_id='cup',exchange_id='860',
                    market_id='m',match_id='fixture',executed_at_ms=1000,yes_price_micros=450000,quantity=5))
                return e
            x.world=make_world;model=ApiPolicy(16)
            with torch.no_grad():
                model.heads['family'].weight.zero_();model.heads['family'].bias.zero_();model.heads['family'].bias[11]=100.
            if not terminal:
                with self.assertRaisesRegex(ValueError,'INCOMPLETE_SIG_WEALTH_MARK'):x.evaluate(model,{'sha256':'a'*64})
            else:
                result=x.evaluate(model,{'sha256':'a'*64})
                self.assertEqual(result['role'],'development_selection')
                self.assertTrue(all(c['evaluation_compute_mode']=='batch_encoding_case_local_rng'for c in result['cases']))
