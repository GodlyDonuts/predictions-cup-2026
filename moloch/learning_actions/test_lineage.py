import hashlib,json,tempfile,unittest
from pathlib import Path
from .lineage import development_parent


class DevelopmentLineageTests(unittest.TestCase):
    def test_cross_dataset_requires_opt_in_and_original_development_source(self):
        with tempfile.TemporaryDirectory()as d:
            p=Path(d);source={'config':{'mode':'inspected_development'},'dataset_manifest_sha256':'a'*64,
                'full_history_experiment':False,'independent_test_used':False,'production_authority':False,
                'actual_train_bounds_ms':[10,20],'actual_evolution_bounds_ms':[30,40]}
            raw=json.dumps(source).encode();(p/'experiment.json').write_bytes(raw)
            bundle={'dataset_manifest_sha256':'a'*64,'split_specification_sha256':hashlib.sha256(raw).hexdigest()}
            with self.assertRaisesRegex(ValueError,'DATASET_CHANGED'):
                development_parent(bundle,p/'parent.pt',new_dataset_sha256='b'*64)
            result=development_parent(bundle,p/'parent.pt',new_dataset_sha256='b'*64,allow_cross_dataset=True)
            self.assertEqual(result['prior_calendar_bounds_ms']['evolution'],[30,40])
            self.assertFalse(result['independent_test_qualification']);self.assertTrue(result['optimizer_moments_reset'])
            same=development_parent(bundle,p/'parent.pt',new_dataset_sha256='a'*64)
            self.assertTrue(same['same_dataset'])
            (p/'experiment.json').write_bytes(raw+b' ')
            with self.assertRaisesRegex(ValueError,'SOURCE_CHANGED'):
                development_parent(bundle,p/'parent.pt',new_dataset_sha256='b'*64,allow_cross_dataset=True)

    def test_production_or_test_parents_cannot_be_relabelled(self):
        with tempfile.TemporaryDirectory()as d:
            p=Path(d)
            for flag in ('full_history_experiment','independent_test_used','production_authority'):
                source={'config':{'mode':'inspected_development'},'dataset_manifest_sha256':'a'*64,
                    'full_history_experiment':False,'independent_test_used':False,'production_authority':False}
                source[flag]=True;raw=json.dumps(source).encode();(p/'experiment.json').write_bytes(raw)
                bundle={'dataset_manifest_sha256':'a'*64,'split_specification_sha256':hashlib.sha256(raw).hexdigest()}
                with self.assertRaisesRegex(ValueError,'AUDITED_DEVELOPMENT'):
                    development_parent(bundle,p/'parent.pt',new_dataset_sha256='b'*64,allow_cross_dataset=True)
                with self.assertRaisesRegex(ValueError,'AUDITED_DEVELOPMENT'):
                    development_parent(bundle,p/'parent.pt',new_dataset_sha256='a'*64)
