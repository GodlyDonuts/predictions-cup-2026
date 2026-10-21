import json,os,subprocess,sys,tempfile,time,unittest
from pathlib import Path
import torch
from types import SimpleNamespace
from unittest.mock import patch
from .population_handoff import boundary,select_population,next_configuration
from .test_population_experiment import FixtureExperiment,RegressingExperiment
from .population import ResearchArchive
from .selection import frozen
from .training_process import TrainingProcess
from .continue_population import run as run_handoff


class PopulationHandoffTests(unittest.TestCase):
    def test_later_handoff_keeps_historical_elites_as_well_as_mutated_children(self):
        torch.set_num_threads(1)
        with tempfile.TemporaryDirectory()as directory:
            x=RegressingExperiment(Path(directory));x.config.update(mode='inspected_development',cycles=3)
            x.spec_sha=frozen(x.output/'experiment.json',{'config':x.config,'production_authority':False,
                'full_history_experiment':False,'independent_test_used':False,'dataset_manifest_sha256':x.dataset_sha})
            x.archive=ResearchArchive(x.dataset_sha,x.spec_sha,8);x.run()
            (x.output/'progress.jsonl').write_text('\n'.join(json.dumps(e)for e in x.events)+'\n')
            plan=select_population(x.output,cycle=1,members=4,hidden_size=16)
            self.assertEqual(len(plan['retentions']),2)
            retained={r['slot']:r['retained_checkpoint']for r in plan['retentions']}
            children={r['loser']:r['checkpoint']for r in plan['replacements']}
            self.assertFalse(set(retained)&set(children))
            for p in plan['parents']:
                expected=(children if p['member']in children else retained)[p['member']]
                self.assertEqual(p['sha256'],expected['sha256'])
            self.assertEqual(sum(p['origin']=='checked_historical_island_champion'for p in plan['parents']),2)
            path=x.output/'island-champions-001.json';proof=json.loads(path.read_text())
            proof['champions']['0']['checkpoint']['sha256']='f'*64;path.write_text(json.dumps(proof))
            with self.assertRaisesRegex(ValueError,'RETAINED_CHAMPION_CHECKPOINT_CHANGED'):
                select_population(x.output,cycle=1,members=4,hidden_size=16)

    def test_complete_generation_keeps_actual_children_and_parent_hyperparameters(self):
        torch.set_num_threads(1)
        with tempfile.TemporaryDirectory()as directory:
            root=Path(directory);x=FixtureExperiment(root);x.config['mode']='inspected_development'
            x.config['initial_learning_rates']=[1e-5,2e-5,3e-5,4e-5]
            x.spec_sha=frozen(x.output/'experiment.json',{'config':x.config,'production_authority':False,
                'full_history_experiment':False,'independent_test_used':False,'dataset_manifest_sha256':x.dataset_sha})
            x.archive=ResearchArchive(x.dataset_sha,x.spec_sha,8);x.run()
            progress=x.output/'progress.jsonl'
            progress.write_text('\n'.join(json.dumps(e)for e in x.events if e['event']!='cycle_complete')+'\n')
            self.assertIsNone(boundary(x.output));self.assertIsNone(select_population(x.output,members=4,hidden_size=16))
            progress.write_text('\n'.join(json.dumps(e)for e in x.events)+'\n')
            plan=select_population(x.output,members=4,hidden_size=16)
            self.assertEqual(len(plan['parents']),4)
            children={r['loser']:r for r in plan['replacements']}
            self.assertEqual(len(children),2)
            for p in plan['parents']:
                if p['member']in children:
                    child=children[p['member']]
                    self.assertEqual(p['sha256'],child['checkpoint']['sha256'])
                    self.assertEqual(p['learning_rate'],child['child_learning_rate'])
                    self.assertEqual(p['entropy_coefficient'],child['child_entropy_coefficient'])
            config=next_configuration({'mode':'inspected_development','members':4},plan,compute_steps=64)
            self.assertEqual(config['initial_learning_rates'],[p['learning_rate']for p in plan['parents']])
            self.assertEqual(config['initial_entropy_coefficients'],[p['entropy_coefficient']for p in plan['parents']])
            self.assertEqual(config['fresh_founders'],0);self.assertFalse(plan['oldScoresReusedForNewSelection'])
            p=Path(plan['parents'][0]['path']);raw=p.read_bytes();p.write_bytes(raw+b'changed')
            with self.assertRaisesRegex(ValueError,'FROZEN_RAW_API_CHECKPOINT_CHANGED'):
                select_population(x.output,members=4,hidden_size=16)

    def test_founder_rates_are_explicit_finite_and_do_not_change_defaults(self):
        x=FixtureExperiment.__new__(FixtureExperiment);x.config={}
        self.assertEqual(x.founder_hyperparameters(0),(1e-5,.01))
        for value in(0,-1,True,float('nan')):
            x.config={'initial_learning_rates':[value]}
            with self.assertRaisesRegex(ValueError,'POSITIVE_FINITE_FOUNDER'):x.founder_hyperparameters(0)

    def driver_fixture(self,root):
        from .population_handoff import sha_file
        source=root/'source';(source/'moloch').mkdir(parents=True)
        py=source/'moloch/fixture.py';py.write_text('# SOFTWARE FIXTURE ONLY\n')
        frozen(source/'source-snapshot.json',{'commit':'a'*40,'files':{'moloch/fixture.py':sha_file(py)}})
        data=root/'data';base=data/'base';base.mkdir(parents=True)
        (base/'observations.jsonl.gz').write_bytes(b'fixture-bytes-not-a-learning-dataset')
        (data/'trade-flow.jsonl.gz').write_bytes(b'fixture')
        base_sha=frozen(base/'dataset-manifest.json',{'observationsSha256':sha_file(base/'observations.jsonl.gz')})
        dataset_sha=frozen(data/'dataset-manifest.json',{'baseDataset':str(base),'baseManifestSha256':base_sha,
            'tradeRowsSha256':sha_file(data/'trade-flow.jsonl.gz')})
        (data/'COMPLETE.json').write_text(json.dumps({'manifestSha256':dataset_sha}))
        spec={'sourceRoot':str(source),'sourceCommit':'a'*40,'sourceManifestSha256':sha_file(source/'source-snapshot.json'),
            'datasetManifestSha256':dataset_sha,'oldPid':-987654,'oldCreation':1,'oldRun':str(root/'old-run'),
            'oldConfig':str(root/'old.json'),'newRun':str(root/'new-run'),'cycle':0,'python':'NEVER_EXECUTE_THIS_FIXTURE',
            'capacityChunks':[64,32],'template':{'mode':'inspected_development','dataset':str(data),'members':4,'hidden_size':16,
                'training_worlds':8,'cuda_memory_fraction':.85}}
        path=root/'spec.json';path.write_text(json.dumps(spec))
        plan={'parents':[{'member':i,'path':'fixture','sha256':'b'*64,'learning_rate':1e-5*(i+1),
                         'entropy_coefficient':.001*i,'origin':'software_fixture'}for i in range(4)],
              'sourceExperimentSha256':'c'*64,'cycle':0}
        return path,spec,plan

    def test_driver_preserves_old_worker_until_checks_then_adapts_only_memory_failure(self):
        with tempfile.TemporaryDirectory()as directory:
            root=Path(directory);path,spec,plan=self.driver_fixture(root);state={'stopped':False,'launches':0,'probes':[]}
            class Handle:
                def __init__(self,*args,**kwargs):pass
                def alive(self):return not state['stopped']
                def terminate_for_handoff(self):state['stopped']=True
                def close(self):pass
            def probe(argv,**kwargs):
                self.assertTrue(state['stopped']);steps=int(argv[argv.index('--steps')+1]);state['probes'].append(steps)
                if steps==64:kwargs['stdout'].write('CUDA out of memory\n');return SimpleNamespace(returncode=1)
                output=Path(argv[argv.index('--output')+1]);output.mkdir()
                (output/'RESULT.json').write_text(json.dumps({'device':'cuda:0','identical_wallets_orders_fills_and_clocks':True,
                    'ppo':[{'accepted':True}],'on_policy_guard_changed_after_optimizer_step':True}))
                return SimpleNamespace(returncode=0)
            def launch(*args,**kwargs):
                self.assertEqual(state['probes'],[64,32]);state['launches']+=1
                output=Path(spec['newRun']);output.mkdir()
                (output/'progress.jsonl').write_text('{"event":"parallel_train_chunk","total_decisions":128,"updates":[{"accepted":true}]}\n')
                (output/'experiment.json').write_text(json.dumps({'config':json.loads((root/'next-config.json').read_text()),
                    'dataset_manifest_sha256':spec['datasetManifestSha256'],'production_authority':False,'source_sha256':{}}))
                return SimpleNamespace(pid=-123456,poll=lambda:None)
            with patch('moloch.learning_actions.continue_population.TrainingProcess',Handle),\
                 patch('moloch.learning_actions.continue_population.boundary',return_value={'cycle':0}),\
                 patch('moloch.learning_actions.continue_population.select_population',return_value=plan),\
                 patch('moloch.learning_actions.continue_population.subprocess.run',side_effect=probe),\
                 patch('moloch.learning_actions.continue_population.subprocess.Popen',side_effect=launch):
                run_handoff(path)
            self.assertEqual(state['launches'],1)
            self.assertEqual(json.loads((root/'status.json').read_text())['phase'],'new_population_training_verified')
            config=json.loads((root/'next-config.json').read_text())
            self.assertEqual(config['compute_chunk_decisions'],32)
            self.assertEqual(config['initial_learning_rates'],[1e-5*(i+1)for i in range(4)])

    def test_changed_source_stops_before_any_process_control(self):
        with tempfile.TemporaryDirectory()as directory:
            root=Path(directory);path,spec,_=self.driver_fixture(root)
            (Path(spec['sourceRoot'])/'moloch/fixture.py').write_text('# altered\n')
            with patch('moloch.learning_actions.continue_population.TrainingProcess')as process:
                with self.assertRaisesRegex(ValueError,'FROZEN_SOURCE_CHANGED'):run_handoff(path)
                process.assert_not_called()

    def test_process_driver_routes_to_process_probe_and_rejects_local_only_result(self):
        with tempfile.TemporaryDirectory()as directory:
            root=Path(directory);path,spec,plan=self.driver_fixture(root)
            spec['template']['process_source_worlds']=True;spec['capacityChunks']=[8]
            path.write_text(json.dumps(spec));state={'stopped':False,'module':None}
            class Handle:
                def __init__(self,*args,**kwargs):pass
                def alive(self):return not state['stopped']
                def terminate_for_handoff(self):state['stopped']=True
                def close(self):pass
            def probe(argv,**kwargs):
                self.assertTrue(state['stopped']);state['module']=argv[2]
                output=Path(argv[argv.index('--output')+1]);output.mkdir()
                (output/'RESULT.json').write_text(json.dumps({'device':'cuda:0',
                    'identical_wallets_orders_fills_and_clocks':True,'ppo':[{'accepted':True}],
                    'on_policy_guard_changed_after_optimizer_step':True}))
                return SimpleNamespace(returncode=0)
            with patch('moloch.learning_actions.continue_population.TrainingProcess',Handle),\
                 patch('moloch.learning_actions.continue_population.boundary',return_value={'cycle':0}),\
                 patch('moloch.learning_actions.continue_population.select_population',return_value=plan),\
                 patch('moloch.learning_actions.gpu_diagnostic_guard.require_idle_gpu',return_value=None),\
                 patch('moloch.learning_actions.continue_population.subprocess.run',side_effect=probe),\
                 patch('moloch.learning_actions.continue_population.subprocess.Popen',side_effect=AssertionError('must not launch')):
                with self.assertRaisesRegex(ValueError,'ACTUAL_PROCESS_BACKEND'):
                    run_handoff(path)
            self.assertEqual(state['module'],'moloch.learning_actions.process_capacity')
            self.assertTrue((root/'population-plan.json').exists())
            self.assertFalse((root/'next-config.json').exists())

    def test_closed_return_handoff_requires_extra_complete_episode_probe_before_launch(self):
        for accepted in(False,True):
            with tempfile.TemporaryDirectory()as directory:
                root=Path(directory);path,spec,plan=self.driver_fixture(root)
                spec['capacityChunks']=[32];spec['template']['closed_episode_return_replay']=True
                path.write_text(json.dumps(spec));state={'stopped':False,'closed_probe':False}
                class Handle:
                    def __init__(self,*args,**kwargs):pass
                    def alive(self):return not state['stopped']
                    def terminate_for_handoff(self):state['stopped']=True
                    def close(self):pass
                def probe(argv,**kwargs):
                    self.assertTrue(state['stopped']);output=Path(argv[argv.index('--output')+1]);output.mkdir()
                    if 'moloch.learning_actions.closed_episode_capacity'in argv:
                        state['closed_probe']=True
                        proof={'device':'cuda:0','fullReturnTargets':accepted,'optimizerChangedWeights':True,
                               'acceptedUpdates':1,'worlds':8}
                    else:proof={'device':'cuda:0','identical_wallets_orders_fills_and_clocks':True,
                        'ppo':[{'accepted':True}],'on_policy_guard_changed_after_optimizer_step':True}
                    (output/'RESULT.json').write_text(json.dumps(proof));return SimpleNamespace(returncode=0)
                def launch(*args,**kwargs):
                    self.assertTrue(state['closed_probe']);self.assertTrue(accepted)
                    output=Path(spec['newRun']);output.mkdir()
                    (output/'progress.jsonl').write_text('{"event":"parallel_train_chunk","total_decisions":128,"updates":[{"accepted":true}]}\n')
                    (output/'experiment.json').write_text(json.dumps({'config':json.loads((root/'next-config.json').read_text()),
                        'dataset_manifest_sha256':spec['datasetManifestSha256'],'production_authority':False,'source_sha256':{}}))
                    return SimpleNamespace(pid=-123456,poll=lambda:None)
                with patch('moloch.learning_actions.continue_population.TrainingProcess',Handle),\
                     patch('moloch.learning_actions.continue_population.boundary',return_value={'cycle':0}),\
                     patch('moloch.learning_actions.continue_population.select_population',return_value=plan),\
                     patch('moloch.learning_actions.continue_population.subprocess.run',side_effect=probe),\
                     patch('moloch.learning_actions.continue_population.subprocess.Popen',side_effect=launch)as launched:
                    if accepted:run_handoff(path);self.assertTrue(launched.called)
                    else:
                        with self.assertRaisesRegex(ValueError,'COMPLETE_EPISODE_GPU_CAPACITY_PROOF_REQUIRED'):run_handoff(path)
                        launched.assert_not_called()

    @unittest.skipUnless(os.name=='nt','Exact retained Windows process handle')
    def test_handle_targets_only_own_fixture_and_rejects_wrong_output_and_creation(self):
        with tempfile.TemporaryDirectory()as directory:
            root=Path(directory);package=root/'moloch/learning_actions';package.mkdir(parents=True)
            (root/'moloch/__init__.py').write_text('');(package/'__init__.py').write_text('')
            (package/'experiment.py').write_text("import time,pathlib;pathlib.Path('ready').write_text('ready');time.sleep(60)")
            config=root/'fixture.json';config.write_text('{}');run=root/'output'
            process=subprocess.Popen([sys._base_executable,'-m','moloch.learning_actions.experiment','--config',str(config),'--output',str(run)],
                cwd=root,creationflags=subprocess.CREATE_NO_WINDOW)
            handle=None
            try:
                deadline=time.monotonic()+10
                while not(root/'ready').exists()and time.monotonic()<deadline:time.sleep(.05)
                self.assertTrue((root/'ready').exists())
                with self.assertRaisesRegex(ValueError,'NOT_EXPECTED_TRAINING_RUN'):TrainingProcess(process.pid,root/'wrong',config)
                self.assertIsNone(process.poll())
                handle=TrainingProcess(process.pid,run,config)
                with self.assertRaisesRegex(ValueError,'CREATION_CHANGED'):
                    TrainingProcess(process.pid,run,config,expected_creation=handle.creation+1)
                self.assertTrue(handle.alive());handle.terminate_for_handoff();process.wait(timeout=10)
                self.assertFalse(handle.alive());self.assertEqual(process.returncode,42)
            finally:
                if handle is not None:handle.close()
                if process.poll()is None:process.terminate();process.wait(timeout=10)
