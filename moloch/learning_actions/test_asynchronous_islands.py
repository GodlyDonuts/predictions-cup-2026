"""Actual CPU evolution and synthetic failure/migration glue, never ROI proof."""
import copy
from pathlib import Path
import tempfile
import sqlite3
import threading
import time
import unittest
from unittest.mock import patch
from types import SimpleNamespace
import torch
from moloch.learning_realism.protocol import digest
from .asynchronous_islands import configurations, AsynchronousIslands, ChampionBridge
from .population import ResearchArchive
from .test_population_experiment import FixtureExperiment
from .test_neural_sessions import tree_sha


def base():
    return dict(mode='inspected_development',device='cpu',seed=81,members=4,cycles=2,
        fresh_founders=4,hidden_size=16,development_episode_ms=200,compute_chunk_decisions=4,
        learning_rate=1e-4,maximum_mark_age_ms=120000,training_snapshot_every_updates=4,
        on_policy_guard='tensor_versions_v1',checkpoint_entity_encoder=True,
        initial_entropy_coefficients=[0.,.001],test_finalist_once=True,
        population_search=dict(islands=2,probe_decisions=4,archive_size=8,development_selection_blocks=2))


def factory(config, path):
    path.mkdir(); owner = FixtureExperiment(path)
    owner.config = copy.deepcopy(config); owner.search_config = copy.deepcopy(config['population_search'])
    owner.spec_sha = digest(config); owner.archive = ResearchArchive(owner.dataset_sha,owner.spec_sha,8)
    owner.frozen = dict(source_sha256={'mechanics-fixture':'c'*64},mechanics_sha256='d'*64)
    return owner


class IslandTests(unittest.TestCase):
    def setUp(self): torch.set_num_threads(1)

    def test_global_population_profiles_and_world_counts_are_not_reduced(self):
        c=base(); c.update(members=32,fresh_founders=0,training_worlds=8)
        c['population_search']['islands']=4
        c['development_initial_checkpoints']=[dict(path=f'parent-{i}',sha256=str(i))for i in range(32)]
        c['inherited_wait_logit_shifts']=[[i,0]for i in range(32)]
        c['initial_learning_rates']=[1e-4,2e-4,3e-4]
        variants=configurations(c)
        self.assertEqual(sorted(i for x in variants for i in x['asynchronous_global_member_ids']),list(range(32)))
        for x in variants:
            ids=x['asynchronous_global_member_ids'];self.assertEqual(x['members'],8)
            self.assertEqual(x['training_worlds'],8);self.assertEqual(x['population_search']['islands'],1)
            self.assertEqual(x['development_initial_checkpoints'],[c['development_initial_checkpoints'][i]for i in ids])
            self.assertEqual(x['inherited_wait_logit_shifts'],[[i,0]for i in ids])
            self.assertEqual(x['initial_learning_rates'],[c['initial_learning_rates'][i%3]for i in ids])
            self.assertFalse(x['test_finalist_once'])
        c['fresh_founders']=3;c['development_initial_checkpoints']=c['development_initial_checkpoints'][:-3]
        c['inherited_wait_logit_shifts'][-3:]=[None]*3
        self.assertEqual(sum(x['fresh_founders']for x in configurations(c)),3)
        c['development_initial_checkpoints'].pop()
        with self.assertRaisesRegex(ValueError,'EXPLICIT_PARENT'):configurations(c)

    def signature(self, owner, summary):
        bundles=[torch.load(x['path'],map_location='cpu',weights_only=True)for x in owner.population]
        return dict(models=[tree_sha(x['model'])for x in bundles],optimizers=[tree_sha(x['optimizer'])for x in bundles],
                    decisions=summary['total_decisions'],updates=summary['total_accepted_updates'],
                    cases=summary['best']['selection']['cases'],score=summary['best']['selection']['reward'])

    def test_runner_matches_serial_islands_and_never_opens_development_holdout(self):
        c=base(); variants=configurations(c)
        with tempfile.TemporaryDirectory()as d:
            root=Path(d);expected=[]
            for i,config in enumerate(variants):
                owner=factory(config,root/f'serial-{i}');summary=owner.run();expected.append(self.signature(owner,summary))
            original=torch.get_rng_state().clone()
            runner=AsynchronousIslands(c,root/'parallel',{'decision_latency_ms':2},workers=2,
                                      experiment_factory=factory,migrate=False)
            result=runner.run()
            self.assertTrue(torch.equal(original,torch.get_rng_state()))
            self.assertFalse(result['independentTestUsed']);self.assertIsNone(result['independentResult'])
            self.assertEqual(expected,[self.signature(owner,result['islands'][i])for i,owner in enumerate(runner.owners)])
            self.assertEqual(len(runner.bridge.champions),2)

    def test_one_waiting_island_does_not_block_another_completed_evolution_cycle(self):
        c=base();waiting=threading.Event();advanced=threading.Event()
        def make(config,path):
            owner=factory(config,path);original_train=owner.train_episode;original_log=owner.log
            if config['asynchronous_global_member_ids'][0]==0:
                def train(*a,**kw):
                    waiting.set()
                    if not advanced.wait(15):raise AssertionError('other island could not complete its cycle')
                    return original_train(*a,**kw)
                owner.train_episode=train
            else:
                def log(event):
                    original_log(event)
                    if event['event']=='cycle_complete':advanced.set()
                owner.log=log
            return owner
        with tempfile.TemporaryDirectory()as d:
            runner=AsynchronousIslands(c,Path(d)/'parallel',{'decision_latency_ms':2},
                                      workers=2,experiment_factory=make,migrate=False)
            result=runner.run();self.assertTrue(waiting.is_set()and advanced.is_set())
            self.assertEqual(len(result['islands']),2)

    def test_sqlite_sources_are_created_used_and_closed_in_the_same_island_thread(self):
        threads=[]
        def make(config,path):
            owner=factory(config,path);created=threading.get_ident();db=sqlite3.connect(':memory:')
            db.execute('create table evidence(value integer)');db.execute('insert into evidence values(1)')
            world=owner.world;close=owner.close
            def observed_world(*a,**kw):
                self.assertEqual(created,threading.get_ident())
                self.assertEqual(db.execute('select value from evidence').fetchone()[0],1)
                return world(*a,**kw)
            def observed_close():
                self.assertEqual(created,threading.get_ident());db.close();threads.append(created);close()
            owner.world=observed_world;owner.close=observed_close;return owner
        with tempfile.TemporaryDirectory()as d:
            main=threading.get_ident();runner=AsynchronousIslands(base(),Path(d)/'parallel',
                {'decision_latency_ms':2},workers=2,experiment_factory=make,migrate=False)
            runner.run();self.assertEqual(len(threads),2)
            self.assertEqual(len(set(threads)),2);self.assertNotIn(main,threads)

    def test_actual_sqlite_experiment_and_spawned_worlds_complete_in_parallel_islands(self):
        from .test_disk_tape import dataset
        from .disk_tape import build
        from .snapshot_clock import MODE
        with tempfile.TemporaryDirectory()as d:
            root=Path(d);source=dataset(root);cache=root/'cache';manifest=build(source,cache)
            mechanics=Path(__file__).resolve().parents[2]/'research/tartarus-learning-v2/development-mechanics-v2.json'
            config=dict(mode='inspected_development',device='cpu',cpu_threads=1,seed=81,
                dataset=str(source),mechanics=str(mechanics),hidden_size=16,
                replay_cache=dict(path=str(cache),manifest_sha256=manifest),
                development_train_ms=[1,1001],development_selection_ms=[1001,5000],
                development_episode_ms=200,initial_world_history_ms=0,
                training_worlds=2,compute_chunk_decisions=2,snapshot_world_clock=MODE,
                on_policy_guard='tensor_versions_v1',compact_finished_worlds=True,
                process_source_worlds=True,members=4,cycles=1,fresh_founders=4,
                population_search=dict(islands=2,probe_decisions=2,archive_size=4))
            runner=AsynchronousIslands(config,root/'parallel',{'decision_latency_ms':20},migrate=False)
            result=runner.run()
            self.assertEqual(len(result['islands']),2);self.assertTrue(all(runner.closed))
            self.assertTrue(all(x['total_accepted_updates']>0 for x in result['islands'].values()))
            self.assertTrue(all(owner.original.disk_store.closed for owner in runner.owners))
            self.assertFalse(result['independentTestUsed']);self.assertFalse(result['productionChanged'])

    def test_negative_champion_migration_uses_matching_context_and_fresh_moments(self):
        with tempfile.TemporaryDirectory()as d:
            root=Path(d);owners=[factory(config,root/f'island-{i}')for i,config in enumerate(configurations(base()))]
            summaries=[owner.run()for owner in owners]
            rows=[copy.deepcopy(summary['best'])for summary in summaries]
            # Synthetic negative fitness exercises ranking glue, not the market reward.
            for row,score in zip(rows,[-.1,-.2]):
                row['selection']['reward']=score
                for case in row['selection']['cases']:case['reward']=score
            bridge=ChampionBridge(root,'e'*64,islands=2,migration_every=1)
            def ranking(row):
                result=[copy.deepcopy(row)for _ in range(2)]
                for i,x in enumerate(result):x['member']=i
                return result
            bridge.publish(0,owners[0],0,ranking(rows[0]),rows[0])
            before=copy.deepcopy(owners[1].population)
            bridge.publish(1,owners[1],1,ranking(rows[1]),rows[1])
            self.assertEqual(len(bridge.events),1);event=bridge.events[0]
            self.assertTrue(event['newFitnessRequired']and event['optimizerMomentsReset'])
            donor=torch.load(rows[0]['checkpoint']['path'],map_location='cpu',weights_only=True)
            child=torch.load(event['checkpoint']['path'],map_location='cpu',weights_only=True)
            self.assertEqual(tree_sha(donor['model']),tree_sha(child['model']))
            self.assertEqual(child['optimizer']['state'],{})
            self.assertEqual(child['split_specification_sha256'],owners[1].spec_sha)
            self.assertTrue(all(Path(x['path']).exists()for x in before))
            owners[1].calibration={'decision_latency_ms':3}
            with self.assertRaisesRegex(ValueError,'CONTEXT_CHANGED'):
                bridge.publish(1,owners[1],2,ranking(rows[1]),rows[1])

    def test_owned_failure_cancels_remaining_compute_and_preserves_failure_records(self):
        c=base();barrier=threading.Barrier(2);shared={};closed=[]
        class Owner:
            def __init__(self,config,path):self.output=path;path.mkdir();self.index=config['asynchronous_global_member_ids'][0]
            def close(self):closed.append(self.index)
            def run(self,**kwargs):
                barrier.wait(timeout=10)
                if self.index==0:raise ValueError('original owned fixture failure')
                until=time.monotonic()+10
                while shared['runner'].coordinator.cancelled_reason is None:
                    if time.monotonic()>until:raise AssertionError('owned failure did not cancel compute')
                    time.sleep(.01)
                from .neural_sessions import neural_section
                with neural_section():raise AssertionError('cancelled compute resumed')
        with tempfile.TemporaryDirectory()as d:
            runner=AsynchronousIslands(c,Path(d)/'parallel',{'decision_latency_ms':2},experiment_factory=Owner)
            shared['runner']=runner
            with self.assertRaises((ValueError,RuntimeError)):runner.run()
            self.assertEqual(sorted(closed),[0,1]);self.assertTrue((runner.output/'FAILED.json').exists())
            self.assertFalse((runner.output/'SUMMARY.json').exists())
            self.assertIn('original owned fixture failure',(runner.output/'island-000-failed.json').read_text())

    def test_source_close_failure_cancels_other_island_and_never_claims_clean_completion(self):
        barrier=threading.Barrier(2);shared={}
        class Owner:
            def __init__(self,config,path):self.output=path;path.mkdir();self.index=config['asynchronous_global_member_ids'][0]
            def close(self):
                if self.index==0:raise OSError('original fixture source close failure')
            def run(self,**kwargs):
                barrier.wait(timeout=10)
                if self.index==0:return {'syntheticSourceCloseFixture':True}
                until=time.monotonic()+10
                while shared['runner'].coordinator.cancelled_reason is None:
                    if time.monotonic()>until:raise AssertionError('close failure did not cancel other island')
                    time.sleep(.01)
                from .neural_sessions import neural_section
                with neural_section():raise AssertionError('closed-source cancellation ignored')
        with tempfile.TemporaryDirectory()as d:
            runner=AsynchronousIslands(base(),Path(d)/'parallel',{'decision_latency_ms':2},experiment_factory=Owner)
            shared['runner']=runner
            with self.assertRaises((OSError,RuntimeError)):runner.run()
            self.assertEqual(runner.closed,[False,True]);self.assertFalse((runner.output/'SUMMARY.json').exists())
            self.assertIn('original fixture source close failure',(runner.output/'island-000-close-failed.json').read_text())

    def finalist_factory(self, registry, accesses, qualified):
        def make(config,path):
            owner=factory(config,path);original=owner.evaluate
            owner.original=SimpleNamespace(manifest={'synthetic_dispatch_fixture':True})
            owner.split=SimpleNamespace(bounds=lambda role:(2001,3001))
            def evaluate(model,checkpoint,role='evolution',finalist=None,**kwargs):
                if role=='test':
                    self.assertEqual(len(list(registry.rglob('test-access-consumed.json'))),1)
                    accesses.append(checkpoint['sha256'])
                    return dict(role='test',checkpoint_sha256=checkpoint['sha256'],reward=-.1,
                                syntheticDispatchOnly=True,marketPriceRowsRead=0)
                result=original(model,checkpoint,role,finalist,**kwargs)
                # Synthetic score/eligibility metadata tests final-test routing;
                # it does not turn the short fixture into qualified market data.
                result['reward']=.01
                for case in result['cases']:case['reward']=.01
                if qualified:result.update(role='evolution',eligible=True)
                return result
            owner.evaluate=evaluate;return owner
        return make

    def test_positive_development_metadata_cannot_authorize_global_test(self):
        with tempfile.TemporaryDirectory()as d:
            root=Path(d);registry=root/'heldout';accesses=[];c=base()
            c.update(mode='full_history',replay_cache={'synthetic_dispatch_fixture':True},test_registry=str(registry))
            runner=AsynchronousIslands(c,root/'parallel',{'decision_latency_ms':2},
                experiment_factory=self.finalist_factory(registry,accesses,False),migrate=False)
            with patch('moloch.learning_actions.asynchronous_islands.independent_test_claim',return_value=True):
                with self.assertRaisesRegex(ValueError,'QUALIFIED_EVOLUTION'):runner.run()
            self.assertEqual(accesses,[]);self.assertEqual(list(registry.rglob('test-access-consumed.json')),[])

    def test_global_test_consumes_once_before_access_and_does_not_retry_a_negative_result(self):
        with tempfile.TemporaryDirectory()as d:
            root=Path(d);registry=root/'heldout';accesses=[];c=base()
            c.update(mode='full_history',replay_cache={'synthetic_dispatch_fixture':True},test_registry=str(registry))
            runner=AsynchronousIslands(c,root/'parallel',{'decision_latency_ms':2},
                experiment_factory=self.finalist_factory(registry,accesses,True),migrate=False)
            with patch('moloch.learning_actions.asynchronous_islands.independent_test_claim',return_value=True):
                result=runner.run()
            self.assertEqual(len(accesses),1);self.assertTrue(result['independentTestUsed'])
            self.assertEqual(result['independentResult']['reward'],-.1)
            self.assertEqual(result['independentResult']['marketPriceRowsRead'],0)
            self.assertEqual(len(list(registry.rglob('test-access-consumed.json'))),1)


if __name__=='__main__':unittest.main()
