import json,tempfile,unittest
from pathlib import Path
from .history_readiness import Extent,audit,clock_inventory,sealed_manifest,registry_snapshot
from .selection import frozen
from moloch.learning_realism.protocol import DAY


class HistoryReadinessTests(unittest.TestCase):
    def extents(self,first=80*DAY,last=100*DAY):return[Extent(v,first,last,'fixture','a'*64)for v in('SIG','Kalshi','Polymarket')]

    def test_full_outer_extents_and_empty_registry_cannot_prove_coverage_or_independence(self):
        r=audit(self.extents(),[],cutoff_ms=100*DAY,competition_start_ms=80*DAY)
        self.assertEqual(r['requested_roles_ms']['evolution'],[92*DAY,96*DAY])
        self.assertEqual(r['requested_roles_ms']['independent_test'],[96*DAY,100*DAY])
        self.assertEqual(r['verdict'],'requires_source_row_and_independence_verification')
        self.assertFalse(r['full_history_ready_proven']);self.assertFalse(r['competition_start_verified'])

    def test_short_late_sources_and_exposed_test_are_disqualifying(self):
        records=[{'kind':'train','start_ms':97*DAY,'end_ms':99*DAY},
                 {'kind':'selection','start_ms':98*DAY,'end_ms':100*DAY}]
        r=audit(self.extents(98*DAY,100*DAY),records,cutoff_ms=100*DAY)
        self.assertEqual(r['verdict'],'ineligible_at_requested_cutoff')
        self.assertEqual(r['test_exposure']['union_duration_ms'],3*DAY)
        self.assertEqual(r['test_exposure']['overlapping_records'],2)
        self.assertEqual(len(r['disqualifying_metadata_findings']),4)

    def test_half_open_boundaries_and_missing_registry_do_not_fake_clean_test(self):
        r=audit(self.extents(),[{'kind':'train','start_ms':95*DAY,'end_ms':96*DAY}],cutoff_ms=100*DAY)
        self.assertFalse(r['test_exposure']['already_recorded_exposure'])
        r=audit(self.extents(),None,cutoff_ms=100*DAY)
        self.assertFalse(r['test_exposure']['local_registry_available'])
        self.assertFalse(r['test_exposure']['absence_of_unrecorded_exposure_established'])

    def test_bad_clocks_or_hashes_are_rejected(self):
        for args in(('SIG',True,4,'fixture','a'*64),('SIG',5,4,'fixture','a'*64),('OTHER',1,2,'fixture','a'*64)):
            with self.assertRaises(ValueError):Extent(*args)
        with self.assertRaises(ValueError):audit([],[],cutoff_ms=8*DAY)

    def test_input_price_like_fields_are_never_exported(self):
        with tempfile.TemporaryDirectory()as d:
            p=Path(d)/'inventory.json';p.write_text(json.dumps({'datasets':[{'dataset':'fixture','manifest_sha256':'a'*64,
                'prices':'DO_NOT_EXPORT','venues':{'SIG':{'first_ms':1,'last_ms':2,'private_label':'DO_NOT_EXPORT'}}}]}))
            e,_=clock_inventory(p);r=audit(e,[],cutoff_ms=100*DAY)
            self.assertNotIn('DO_NOT_EXPORT',json.dumps(r));self.assertFalse(r['raw_observation_rows_opened'])

    def test_metadata_seal_is_checked_and_not_treated_as_observation_verification(self):
        with tempfile.TemporaryDirectory()as d:
            p=Path(d)/'dataset-manifest.json';sha=frozen(p,{'sourceCoverage':{'fit/SIG':{
                'validRows':2,'firstAtMs':1,'lastAtMs':9,'completeObservedCoverage':True}}})
            frozen(p.parent/'COMPLETE.json',{'manifestSha256':sha})
            rows,_=sealed_manifest(p);self.assertEqual(rows[0].last_ms,9)
            p.write_text('{}')
            with self.assertRaisesRegex(ValueError,'SEAL_MISMATCH'):sealed_manifest(p)

    def test_registry_audit_is_read_only_and_includes_prior_test_use(self):
        with tempfile.TemporaryDirectory()as d:
            p=Path(d)/'entry.json';frozen(p,{'kind':'test','start_ms':96*DAY,'end_ms':97*DAY,'source_sha256':'a'*64})
            before=p.read_bytes();rows,meta=registry_snapshot(d)
            self.assertEqual(p.read_bytes(),before);self.assertEqual(meta['records'],1)
            r=audit(self.extents(),rows,cutoff_ms=100*DAY)
            self.assertTrue(r['test_exposure']['already_recorded_exposure'])
            self.assertEqual(len(list(Path(d).iterdir())),1)
            self.assertIsNone(registry_snapshot(Path(d)/'missing')[0])
