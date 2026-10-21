"""Capacity entrypoint and proof routing; CPU tests never claim CUDA capacity."""
import copy,json,tempfile,unittest
from pathlib import Path
from .continue_population import require_process_capacity
from .population_handoff import sha_file


class ProcessCapacityTests(unittest.TestCase):
    def test_old_local_proof_cannot_admit_process_backend(self):
        with tempfile.TemporaryDirectory()as td:
            path=Path(td)/'config.json';path.write_text('{}')
            config={'process_source_worlds':True,'training_worlds':2,'cuda_memory_fraction':.8}
            old={'device':'cuda:0','ppo':[{'accepted':True}],
                 'identical_wallets_orders_fills_and_clocks':True,'on_policy_guard_changed_after_optimizer_step':True}
            with self.assertRaisesRegex(ValueError,'ACTUAL_PROCESS_BACKEND'):
                require_process_capacity(old,config,path,'a'*64)
            require_process_capacity(old,dict(config,process_source_worlds=False),path,'a'*64)

    def test_process_proof_requires_exact_config_source_workers_and_memory_profile(self):
        with tempfile.TemporaryDirectory()as td:
            path=Path(td)/'config.json';path.write_text('{}')
            config={'process_source_worlds':True,'training_worlds':2,'cuda_memory_fraction':.8}
            proof={'processSourceWorlds':True,'sourceWorkerCount':2,'sourceWorkersCPUOnly':True,
                   'exactObservationActionsHiddenRewardsAndReceipts':True,'configurationSha256':sha_file(path),
                   'sourceManifestSha256':'a'*64,'effectiveCudaMemoryFraction':.8,
                   'workerIdentities':[{'pid':10,'cudaInitialized':False},{'pid':11,'cudaInitialized':False}]}
            require_process_capacity(proof,config,path,'a'*64)
            mutations={'processSourceWorlds':False,'sourceWorkerCount':1,'sourceWorkersCPUOnly':False,
                       'exactObservationActionsHiddenRewardsAndReceipts':False,'configurationSha256':'b'*64,
                       'sourceManifestSha256':'b'*64,'effectiveCudaMemoryFraction':.9,
                       'workerIdentities':[{'pid':10,'cudaInitialized':False}]*2}
            for key,value in mutations.items():
                with self.subTest(key=key),self.assertRaisesRegex(ValueError,'ACTUAL_PROCESS_BACKEND'):
                    require_process_capacity(dict(proof,**{key:value}),config,path,'a'*64)
            for value in(None,[None,None],[{'pid':10,'cudaInitialized':True},{'pid':11,'cudaInitialized':False}]):
                with self.subTest(workerShape=value),self.assertRaises(ValueError):
                    require_process_capacity(dict(proof,workerIdentities=value),config,path,'a'*64)

    def test_entrypoint_collects_actual_spawned_worlds_and_updates_without_gpu(self):
        import torch
        from .process_capacity import run
        from .test_disk_tape import dataset,book
        from .disk_tape import build
        from .snapshot_clock import MODE
        torch.set_num_threads(1)
        with tempfile.TemporaryDirectory()as td:
            root=Path(td)
            data=dataset(root,rows=[book(0),book(1),book(500),book(1000),book(10000),book(120001),book(300000)])
            cache=root/'cache';digest=build(data,cache)
            config={'mode':'inspected_development','device':'cpu','cpu_threads':1,'hidden_size':16,
                    'dataset':str(data),'mechanics':str(Path(__file__).resolve().parents[2]/'research/tartarus-learning-v2/development-mechanics-v2.json'),
                    'replay_cache':{'path':str(cache),'manifest_sha256':digest},
                    'development_train_ms':[1,120001],'development_selection_ms':[120001,300000],
                    'development_episode_ms':1000,'initial_world_history_ms':0,'training_worlds':2,
                    'snapshot_world_clock':MODE,'role_prefix_initial_state':True,
                    'process_source_worlds':True,'compact_rollout_statistics':True,
                    'acting_validation':'consolidated_v1','on_policy_guard':'tensor_versions_v1'}
            path=root/'config.json';path.write_text(json.dumps(config))
            result=run(path,root/'probe',worlds=2,steps=2,start_offset_ms=1000,priming_reads=1,memory_fraction=.2)
            self.assertEqual(result['device'],'cpu');self.assertTrue(result['processSourceWorlds'])
            self.assertEqual(result['sourceWorkerCount'],2);self.assertTrue(result['sourceWorkersCPUOnly'])
            self.assertTrue(result['exactObservationActionsHiddenRewardsAndReceipts'])
            self.assertTrue(result['on_policy_guard_changed_after_optimizer_step'])
            self.assertTrue(any(p['accepted']for p in result['ppo']))
            self.assertFalse(result['fullDayRewardQualification']);self.assertFalse(result['parentOrProductionCheckpointCreated'])
            self.assertEqual(result['configurationSha256'],sha_file(path));self.assertEqual(result['http_requests'],0)
