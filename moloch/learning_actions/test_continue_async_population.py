import copy
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock, patch

from . import continue_async_population as subject


class ContinuationTests(unittest.TestCase):
    def test_history_migration_retains_parents_and_requires_bound_v2_source(self):
        from .managed_data_service import INTENT_PROFILE,HISTORY_PROFILE
        template,plan=self.fixture()
        template['managed_data_service']={'profile':INTENT_PROFILE,'initially_enabled':True,'finance_period_ms':30000}
        template['historical_information']={'format':'three_venue_history_information_v1'}
        before=copy.deepcopy(template)
        config,_=subject.continuation_configuration(template,plan,managed_profile=HISTORY_PROFILE)
        self.assertEqual(template,before)
        self.assertEqual(config['managed_data_service'],{**before['managed_data_service'],'profile':HISTORY_PROFILE})
        self.assertTrue(config['migrate_managed_data_profile'])
        self.assertEqual([p['sha256']for p in config['development_initial_checkpoints']],['0'*64,'1'*64])
        for invalid in [None,{}]:
            template['historical_information']=invalid
            with self.assertRaisesRegex(ValueError,'BOUND_HISTORY_MIGRATION'):
                subject.continuation_configuration(template,plan,managed_profile=HISTORY_PROFILE)
        with self.assertRaisesRegex(ValueError,'BOUND_HISTORY_MIGRATION'):
            subject.continuation_configuration(before,plan,managed_profile='arbitrary')

    def fixture(self):
        template = dict(mode='inspected_development', device='cuda', members=2,
                        compute_chunk_decisions=8, inherited_wait_logit_shifts=[[6,0],None],
                        founder_family_logit_shifts=[None,None], test_finalist_once=False,
                        development_train_ms=[1,2], development_selection_ms=[2,3])
        plan = dict(cycle=0, sourceExperimentSha256='a'*64, parents=[
            dict(member=i, path=f'parent{i}.pt', sha256=str(i)*64, origin='checked_evolved_child',
                 learning_rate=1e-5, entropy_coefficient=.001) for i in range(2)])
        return template, plan

    def test_evolved_parents_are_not_mutated_again_or_bounds_changed(self):
        template, plan = self.fixture(); before = copy.deepcopy(template)
        config, removed = subject.continuation_configuration(template, plan)
        self.assertEqual(template, before)
        self.assertNotIn('inherited_wait_logit_shifts', config)
        self.assertEqual(removed['inherited_wait_logit_shifts'], [[6,0],None])
        self.assertEqual([x['sha256'] for x in config['development_initial_checkpoints']],
                         [x['sha256'] for x in plan['parents']])
        for key in ['development_train_ms','development_selection_ms']:
            self.assertEqual(config[key], template[key])
        self.assertFalse(config['test_finalist_once'])

    def test_non_development_template_is_rejected(self):
        template, plan = self.fixture(); template['mode'] = 'full_history'
        with self.assertRaises(ValueError): subject.continuation_configuration(template, plan)

    def test_corrective_weights_are_not_reported_as_a_completed_generation(self):
        from .partial_population import CLASSIFICATION
        template,plan=self.fixture()
        plan.update(classification=CLASSIFICATION,completedGeneration=False,
                    activeSnapshotUpdates=11020,reason='missing historical feed corrected')
        for parent in plan['parents']:parent['origin']='checked_unfinished_member_accepted_weights'
        config,_=subject.continuation_configuration(template,plan)
        self.assertNotIn('transferred_completed_cycle',config)
        self.assertEqual(config['transferred_partial_snapshot_updates'],11020)
        self.assertEqual(config['development_selection_ms'],template['development_selection_ms'])
        self.assertEqual([p['sha256']for p in config['development_initial_checkpoints']],['0'*64,'1'*64])

    def test_changed_partial_plan_never_stops_trainer(self):
        self.run_wait_case(cancel=False,changed_partial=True)

    def test_cancelled_wait_never_stops_trainer(self):
        self.run_wait_case(cancel=True)

    def test_unexpected_trainer_exit_never_starts_replacement(self):
        self.run_wait_case(cancel=False)

    def run_wait_case(self, *, cancel,changed_partial=False):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory); old=root/'old';old.mkdir();job=root/'handoff';job.mkdir()
            template,_=self.fixture()
            (old/'experiment.json').write_text(json.dumps(dict(config=template,dataset_manifest_sha256='dataset')))
            (old/'compute-calibration.json').write_text('{}')
            spec=dict(sourceRoot=str(root/'source'),sourceCommit='commit',sourceManifestSha256='sha',
                      oldRun=str(old),newRun=str(root/'new'),datasetManifestSha256='dataset',
                      calibrationSha256='sha',template=template,oldPid=1,oldConfig='old-config',oldCreation=2,cycle=0)
            if changed_partial:spec['preservedPartialPopulation']={'path':str(job/'original-plan.json'),'sha256':'changed'}
            path=job/'spec.json';path.write_text(json.dumps(spec))
            if cancel:(job/'cancel').touch()
            handle=Mock(pid=1);handle.alive.return_value=False
            with patch.object(subject,'verify_source',return_value={'commit':'commit'}), \
                 patch.object(subject,'sha_file',return_value='sha'), \
                 patch.object(subject,'TrainingProcess',return_value=handle), \
                 patch.object(subject,'boundary',return_value=None), \
                 patch.object(subject.subprocess,'Popen') as start:
                if cancel:subject.run(path)
                else:
                    with self.assertRaisesRegex(ValueError,'PRESERVED_PARTIAL_PLAN_CHANGED'if changed_partial else'EXISTING_TRAINER_EXITED'):
                        subject.run(path)
                handle.terminate_for_handoff.assert_not_called();start.assert_not_called()
                handle.close.assert_called_once()


if __name__ == '__main__': unittest.main()
