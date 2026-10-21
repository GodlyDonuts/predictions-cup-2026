import contextlib
import copy
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import Mock, patch

from . import island_generation_handoff as subject
from .population_handoff import sha_file
from .training_process import TrainingProcess


class IslandGenerationHandoffTests(unittest.TestCase):
    def test_real_completed_islands_keep_all_selected_weights_and_hyperparameters(self):
        from .asynchronous_islands import AsynchronousIslands
        from .test_disk_tape import dataset
        from .disk_tape import build
        from .snapshot_clock import MODE
        with tempfile.TemporaryDirectory()as td:
            root=Path(td);source=dataset(root);cache=root/'cache';manifest=build(source,cache)
            config=dict(mode='inspected_development',device='cpu',cpu_threads=1,seed=81,
                dataset=str(source),mechanics=str(Path(__file__).resolve().parents[2]/
                    'research/tartarus-learning-v2/development-mechanics-v2.json'),hidden_size=16,
                replay_cache=dict(path=str(cache),manifest_sha256=manifest),
                development_train_ms=[1,1001],development_selection_ms=[1001,5000],
                development_episode_ms=200,initial_world_history_ms=0,training_worlds=2,
                compute_chunk_decisions=2,snapshot_world_clock=MODE,
                on_policy_guard='tensor_versions_v1',compact_finished_worlds=True,
                members=4,cycles=2,fresh_founders=4,initial_learning_rates=[1e-5,2e-5,3e-5,4e-5],
                population_search=dict(islands=2,probe_decisions=2,archive_size=4))
            run=root/'parallel'
            with contextlib.redirect_stdout(io.StringIO()):
                runner=AsynchronousIslands(config,run,{'decision_latency_ms':20},migrate=False)
                runner.run()
            global_sha=sha_file(run/'global-experiment.json');calibration=sha_file(run/'shared-actor-calibration.json')
            plan=subject.select_completed_generations(run,global_sha,calibration)
            self.assertEqual(plan['cycles'],[1,1])
            self.assertEqual([p['member']for p in plan['parents']],[0,1,2,3])
            self.assertEqual([(p['island'],p['local_member'])for p in plan['parents']],[(0,0),(1,0),(0,1),(1,1)])
            for parent in plan['parents']:
                original=plan['islandPlans'][parent['island']]['parents'][parent['local_member']]
                self.assertEqual(parent['sha256'],original['sha256'])
                self.assertEqual(parent['learning_rate'],original['learning_rate'])
                self.assertEqual(parent['entropy_coefficient'],original['entropy_coefficient'])
                self.assertEqual(parent['sha256'],sha_file(parent['path']))
            before=copy.deepcopy(config);config['founder_family_logit_shifts']=[None]*4
            changed=subject.next_configuration(config,plan)
            self.assertNotIn('founder_family_logit_shifts',changed)
            self.assertNotIn('transferred_source_experiment_sha256',changed)
            self.assertEqual(changed['transferred_source_island_experiments_sha256'],plan['sourceIslandExperimentsSha256'])
            for key in ('development_train_ms','development_selection_ms','training_worlds','compute_chunk_decisions'):
                self.assertEqual(changed[key],before[key])
            self.assertEqual(changed['training_chunk_transitions'],64)
            self.assertTrue(changed['evaluate_founders_before_training'])
            self.assertFalse(changed['test_finalist_once'])
            self.assertEqual([p['sha256']for p in changed['development_initial_checkpoints']],
                             [p['sha256']for p in plan['parents']])
            # An unfinished second island cannot authorize stopping the whole
            # coordinator, even when the other island has finished later work.
            progress=run/'island-001/progress.jsonl';raw=progress.read_bytes()
            rows=[json.loads(line)for line in raw.splitlines()]
            progress.write_text('\n'.join(json.dumps(x)for x in rows if not(x['event']=='cycle_complete'and x['cycle']==1))+'\n',encoding='utf-8')
            unequal=subject.select_completed_generations(run,global_sha,calibration)
            self.assertEqual(unequal['cycles'],[1,0])
            progress.write_text('\n'.join(json.dumps(x)for x in rows if x['event']!='cycle_complete')+'\n',encoding='utf-8')
            self.assertIsNone(subject.select_completed_generations(run,global_sha,calibration))
            progress.write_bytes(raw)
            member=run/'membership.json';old=member.read_bytes();bad=json.loads(old);bad['islands']=[[0,1],[2,3]]
            member.write_text(json.dumps(bad),encoding='utf-8')
            with self.assertRaisesRegex(ValueError,'MEMBERSHIP'):
                subject.select_completed_generations(run,global_sha,calibration)
            member.write_bytes(old)
            # A committed migration happens after within-island replacement.
            # Use a real local checkpoint with a synthetic migration record to
            # test replay ordering, not migration profitability.
            local=run/'island-000';migrated=local/'cross-island-000001-000.pt'
            migrated.write_bytes(Path(plan['parents'][2]['path']).read_bytes())
            event=dict(event='cross_island_migration',island=0,cycle=1,slot=0,
                checkpoint=dict(path=str(migrated),sha256=sha_file(migrated)),
                productionAuthority=False,completedWorldsOrScoresRewritten=False)
            proof=run/'migration-000-000001.json';proof.write_text(json.dumps(event))
            history=local/'progress.jsonl';original=history.read_bytes();changed_rows=[]
            for row in map(json.loads,original.splitlines()):
                if row['event']=='cycle_complete'and row['cycle']==1:changed_rows.append(event)
                changed_rows.append(row)
            history.write_text('\n'.join(json.dumps(row)for row in changed_rows)+'\n',encoding='utf-8')
            migrated_plan=subject.select_completed_generations(run,global_sha,calibration)
            self.assertEqual(migrated_plan['parents'][0]['path'],str(migrated))
            self.assertEqual(migrated_plan['parents'][0]['origin'],'checked_cross_island_champion')
            altered={**event,'productionAuthority':True};proof.write_text(json.dumps(altered))
            with self.assertRaisesRegex(ValueError,'MIGRATION_CHANGED'):
                subject.select_completed_generations(run,global_sha,calibration)
            history.write_bytes(original)
            cp=Path(plan['parents'][0]['path']);cp.write_bytes(cp.read_bytes()+b'changed')
            with self.assertRaisesRegex(ValueError,'CHECKPOINT_CHANGED'):
                subject.select_completed_generations(run,global_sha,calibration)

    def test_cancel_and_missing_process_never_stop_or_launch(self):
        for cancel in (True,False):
            with self.subTest(cancel=cancel),tempfile.TemporaryDirectory()as td:
                root=Path(td);old=root/'old';old.mkdir();job=root/'job';job.mkdir()
                base={'device':'cuda'};config=job/'old.json';config.write_text(json.dumps(base))
                (old/'global-experiment.json').write_text(json.dumps(base))
                spec=dict(sourceRoot=str(root/'source'),sourceCommit='commit',sourceManifestSha256='sha',
                    oldRun=str(old),newRun=str(root/'new'),oldConfig=str(config),oldConfigSha256='sha',
                    oldPid=1,oldCreation=2,globalConfigurationSha256='sha',calibrationSha256='sha')
                path=job/'spec.json';path.write_text(json.dumps(spec))
                if cancel:(job/'cancel').touch()
                handle=Mock(pid=1);handle.alive.return_value=False
                with patch.object(subject,'verify_source',return_value={'commit':'commit'}),\
                     patch.object(subject,'sha_file',return_value='sha'),\
                     patch.object(subject,'TrainingProcess',return_value=handle),\
                     patch.object(subject.subprocess,'Popen')as start:
                    if cancel:subject.run(path)
                    else:
                        with self.assertRaisesRegex(ValueError,'EXITED_WITHOUT_HANDOFF'):subject.run(path)
                    handle.terminate_for_handoff.assert_not_called();start.assert_not_called()
                    handle.close.assert_called_once()

    @unittest.skipUnless(os.name=='nt','Windows retained process handle')
    def test_actual_island_process_requires_explicit_module_run_and_creation(self):
        with tempfile.TemporaryDirectory()as td:
            root=Path(td);package=root/'moloch/learning_actions';package.mkdir(parents=True)
            (root/'moloch/__init__.py').write_text('');(package/'__init__.py').write_text('')
            (package/'asynchronous_islands.py').write_text("import pathlib,time;pathlib.Path('ready').touch();time.sleep(60)")
            config=root/'config.json';config.write_text('{}');run=root/'run'
            child=subprocess.Popen([sys._base_executable,'-m','moloch.learning_actions.asynchronous_islands',
                '--config',str(config),'--output',str(run)],cwd=root,creationflags=subprocess.CREATE_NO_WINDOW)
            handle=None
            try:
                until=time.monotonic()+10
                while not(root/'ready').exists()and time.monotonic()<until:time.sleep(.05)
                self.assertTrue((root/'ready').exists())
                with self.assertRaisesRegex(ValueError,'NOT_RAW_API_TRAINING'):TrainingProcess(child.pid,run,config)
                handle=TrainingProcess(child.pid,run,config,expected_module='moloch.learning_actions.asynchronous_islands')
                with self.assertRaisesRegex(ValueError,'CREATION_CHANGED'):
                    TrainingProcess(child.pid,run,config,expected_creation=handle.creation+1,
                                    expected_module='moloch.learning_actions.asynchronous_islands')
                self.assertTrue(handle.alive());handle.terminate_for_handoff();child.wait(timeout=10)
                self.assertFalse(handle.alive());self.assertEqual(child.returncode,42)
            finally:
                if handle is not None:handle.close()
                if child.poll()is None:child.terminate();child.wait(timeout=10)
        with self.assertRaisesRegex(ValueError,'SIMULATED_TRAINING_MODULE'):
            TrainingProcess(1,'run','config',expected_module='moloch.live_runtime')


if __name__=='__main__':unittest.main()
