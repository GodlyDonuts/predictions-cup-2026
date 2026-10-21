"""Spawned evolution must preserve actions, clocks, paid receipts and wealth."""
from pathlib import Path
import tempfile
import unittest
import torch
from .process_worlds import ProcessCohort
from .evaluation_batch import EvaluationBatch
from .policy import ApiPolicy
from .episode_tape import financial_digest,tensor_digest
from .stream import tensors
from . import test_process_worlds as fixtures
from .economics import GrowthLedger


def evaluation_factory(index,daily=False):
    env,ledger,owner=fixtures.factory(index,daily);env.role='evolution'
    return env,ledger,owner


def early_fill_factory(terminal):
    from .test_evaluation_valuation import world
    env=world(terminal);env.role='evolution'
    env.clock.schedule(1000,10,'api_counterparty_trade',dict(tournament_id='cup',exchange_id='860',
        market_id='m',match_id='fixture',executed_at_ms=1000,yes_price_micros=450000,quantity=5))
    return env,GrowthLedger(env,development=True),None


class ProcessEvaluationTests(unittest.TestCase):
    def run_case(self,remote,daily,heads):
        torch.set_num_threads(1);torch.manual_seed(616)
        model=ApiPolicy(16);model.acting_validation='consolidated_v1'
        cohort=None
        try:
            if remote:
                cohort=ProcessCohort([(evaluation_factory,(i,daily))for i in range(3)],mode='evaluation')
                envs=cohort.worlds;ledgers=None
                self.assertTrue(all(not v['cudaInitialized']for v in cohort.identities))
            else:
                values=[evaluation_factory(i,daily)for i in range(3)]
                envs=[v[0]for v in values];ledgers=[v[1]for v in values]
            from moloch.learning_realism.protocol import DAY
            batch=EvaluationBatch(envs,model,[27,37,57],calibration={'decision_latency_ms':DAY//3 if daily else 100},
                                  batch_action_heads=heads,actions_only=True,world_backend=cohort)
            traces=[]
            while batch.step():traces.extend(batch.last_rows)
            if remote:
                finals=[v['final']for v in cohort.exchange(envs,[('evaluation_finish',None)]*3)]
                audits=[cohort.request(e,'audit',None)for e in envs]
            else:
                self.assertTrue(all(l.reward_sum==0. for l in ledgers))
                finals=[l.finish()for l in ledgers]
                audits=[dict(financialSha256=financial_digest(e),observationSha256=tensor_digest(tensors(e.observe(),'cpu')),
                    feedRequests=list(e.feed_requests),feedReceipts=list(e.feed_receipts),paidRequests=dict(e.feed_request_counts))for e in envs]
            audits=[{k:v[k]for k in('financialSha256','observationSha256','feedRequests','feedReceipts','paidRequests')}for v in audits]
            return traces,finals,audits,batch.hidden,batch.rng,batch.decisions
        finally:
            if cohort is not None:
                cohort.close();self.assertTrue(all(not p.is_alive()for p,_ in cohort.handles))

    def test_exact_stochastic_actions_clocks_wallets_receipts_daily_rewards_and_rng(self):
        compare=fixtures.ProcessWorldTests().equal_tree
        for daily in(False,True):
            for heads in(False,True):
                with self.subTest(daily=daily,heads=heads):
                    compare(self.run_case(False,daily,heads),self.run_case(True,daily,heads))

    def test_evaluation_does_not_require_intermediate_mark_but_keeps_terminal_requirement(self):
        for terminal in(False,True):
            cohort=ProcessCohort([(early_fill_factory,(terminal,))],mode='evaluation')
            try:
                model=ApiPolicy(16)
                with torch.no_grad():
                    model.heads['family'].weight.zero_();model.heads['family'].bias.zero_();model.heads['family'].bias[11]=100.
                batch=EvaluationBatch(cohort.worlds,model,[1],calibration={'decision_latency_ms':1000},world_backend=cohort)
                while batch.run_chunk(2,deterministic=True):pass
                if terminal:
                    result=cohort.request(cohort.worlds[0],'evaluation_finish',None)['final']
                    self.assertEqual(result['fill_count'],1);self.assertAlmostEqual(result['marked_roi'],-.00025)
                else:
                    with self.assertRaisesRegex(RuntimeError,'INCOMPLETE_SIG_WEALTH_MARK'):
                        cohort.request(cohort.worlds[0],'evaluation_finish',None)
            finally:cohort.close()

    def test_independent_test_world_and_mixed_training_transitions_are_rejected(self):
        with self.assertRaisesRegex(RuntimeError,'CANNOT_OPEN_INDEPENDENT_TEST'):
            ProcessCohort([(fixtures.wrong_role,())],mode='evaluation')
        cohort=ProcessCohort([(evaluation_factory,(0,))],mode='evaluation')
        try:
            with self.assertRaisesRegex(ValueError,'TRAIN_TRANSITIONS_REQUIRE_TRAIN'):
                cohort.transitions(cohort.worlds,[True],[{}],1)
        finally:cohort.close()

    def test_profile_validation_precedes_output_or_source_initialization(self):
        from .experiment import Experiment
        with tempfile.TemporaryDirectory()as td:
            for config in({'process_evaluation_worlds':1}, {'process_evaluation_worlds':True}):
                with self.assertRaisesRegex(ValueError,'PROCESS_EVALUATION_REQUIRES_EXPLICIT'):
                    Experiment(config,Path(td)/'unused')
                self.assertFalse((Path(td)/'unused').exists())

    def test_full_experiment_keeps_original_case_results_and_financial_scores(self):
        from .experiment import Experiment
        from .test_disk_tape import dataset
        from .disk_tape import build
        from .snapshot_clock import MODE
        with tempfile.TemporaryDirectory()as td:
            root=Path(td);source=dataset(root);cache=root/'cache';digest=build(source,cache)
            mechanics=Path(__file__).resolve().parents[2]/'research/tartarus-learning-v2/development-mechanics-v2.json'
            config={'mode':'inspected_development','device':'cpu','cpu_threads':1,
                'dataset':str(source),'mechanics':str(mechanics),
                'replay_cache':{'path':str(cache),'manifest_sha256':digest},
                'development_train_ms':[1,1001],'development_selection_ms':[1001,5000],
                'development_episode_ms':200,'initial_world_history_ms':0,'snapshot_world_clock':MODE,
                'compute_chunk_decisions':3,'batched_evaluation':True,'batched_evaluation_action_heads':True,
                'evaluation_actions_only':True,'seed':42}
            results=[]
            for remote in(False,True):
                owner=Experiment(dict(config,process_evaluation_worlds=remote),root/('run-'+str(remote)))
                try:
                    owner.calibration={'decision_latency_ms':20};model=ApiPolicy(16)
                    result=owner.evaluate(model,{'sha256':'a'*64})
                    for case in result['cases']:case.pop('source_compute_mode',None)
                    results.append(result)
                finally:owner.close()
            fixtures.ProcessWorldTests().equal_tree(*results)
