"""Synthetic inputs only; no actual cohort/outcomes or network."""
import copy
from decimal import Decimal
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

HERE=Path(__file__).resolve().parent
spec=importlib.util.spec_from_file_location('prospective_trade_analysis',HERE/'analyze.py')
a=importlib.util.module_from_spec(spec);spec.loader.exec_module(a)
R=a.READER
sys.path.insert(0,str(HERE.parent/'witness'))
spec=importlib.util.spec_from_file_location('replay_fixture',HERE.parent/'witness/test_trace_replay.py')
w=importlib.util.module_from_spec(spec);spec.loader.exec_module(w)
spec=importlib.util.spec_from_file_location('frozen_math_fixture',HERE.parent/'test_analyze.py')
f=importlib.util.module_from_spec(spec);spec.loader.exec_module(f)
from test_trade_triggered_observer import envelope

class Input:
    def __init__(self,root):
        self.root=root;self.fixture=w.Fixture(root,mode='trade');x=self.fixture
        x.clock=39950;x.sig(39950);x.clock=40010
        for route,body in [('/v1/quotes',w.external(39000)),('/v1/ws/quotes',envelope())]:
            w.O.trace_cache_poll(x.study,route,x.trace,lambda *args,body=body,**kw:w.Response(w.O.compact(body).encode()))
        previous=x.tail.previous;x.clock=41010;x.sig(41010,2,previous,bid='0.430000',ask='0.450000');x.finish()
        result=x.verify();self.proof=root/'replay-result.json';self.proof.write_text(R.compact(result),encoding='utf-8')
        trace=[json.loads(y) for y in w.gzip.decompress(Path(x.manifest['trace']['path']).read_bytes()).splitlines()]
        start=trace[0]['event']
        self.manifest={'schemaVersion':1,'sealed':True,'asOfUtc':f.stamp(50000),'analysisProtocolSha256':R.PROTOCOL_SHA,
            'cohorts':[{'cohortId':'prospectively-named-synthetic','producer':w.pin(Path(w.O.__file__)),
                'samplingPolicySha256':R.POLICY_SHA,'configSha256':start['effectiveSamplerConfigSha256'],
                'mappingSha256':start['mappingFileReference']['sha256'],'files':[w.pin(Path(x.manifest['samples']['path']))],
                'inputReplay':{'manifest':w.pin(x.path),'result':w.pin(self.proof)}}]}
        self.save()
    def save(self):
        self.path=self.root/'analysis-manifest.json';self.path.write_text(R.compact(self.manifest),encoding='utf-8');self.sha=a.sha(self.path.read_bytes())
    def close(self):self.fixture.close()
    def run(self):return a.analyze(self.path,self.sha)

class ProspectiveAnalysisTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.addCleanup(self.temp.cleanup);self.root=Path(self.temp.name)
        self.disk=patch.object(w.O.shutil,'disk_usage',return_value=type('D',(),{'free':40*1024**3})());self.disk.start();self.addCleanup(self.disk.stop)
    def input(self):
        value=Input(self.root);self.addCleanup(value.close);return value
    def test_genuine_sealed_replay_is_bound_and_missing_lags_abstain_without_zero(self):
        x=self.input();r=x.run()
        self.assertTrue(r['samplerInputReplayVerified']);self.assertFalse(r['modelApproval']);self.assertFalse(r['executionAuthority'])
        self.assertEqual(r['input']['canonicalRepresentatives'],1)
        self.assertEqual(r['horizons']['1000']['coverage']['missingTenFeatures'],1)
        self.assertEqual(r['featureVariationCanonicalRepresentatives']['externalLogitDelta1s']['missing'],1)
        self.assertEqual(r['horizons']['1000']['coverage']['commonCompleteClusters'],0)
        self.assertTrue(all(m['abstain'] for h in r['horizons'].values() for m in h['models'].values()))
        self.assertTrue(all(m['selectedEconomicsAvailability']=='unavailable_no_oos' and m['selectedNetCrossingMeanRational'] is None for h in r['horizons'].values() for m in h['models'].values()))
        self.assertFalse(r['originalInputReplay'][0]['producerSkipCounters']['available'])
    def test_stored_positive_replay_flags_alone_cannot_admit(self):
        x=self.input();proof=json.loads(x.proof.read_text());proof['outputRows']+=1;x.proof.write_text(R.compact(proof))
        x.manifest['cohorts'][0]['inputReplay']['result']=w.pin(x.proof);x.save()
        with self.assertRaisesRegex(ValueError,'REPLAY_RESULT_CHANGED'):x.run()
    def test_alternate_samples_cannot_borrow_another_trace_verification(self):
        x=self.input();other=self.root/'other.jsonl.gz';other.write_bytes(w.gzip.compress(b'',mtime=0))
        x.manifest['cohorts'][0]['files']=[w.pin(other)];x.save()
        with self.assertRaisesRegex(ValueError,'SAMPLE_MEMBER_BINDING'):x.run()
    def test_original_config_and_mapping_require_exact_source_binding(self):
        x=self.input()
        for key in ('configSha256','mappingSha256'):
            original=x.manifest['cohorts'][0][key];x.manifest['cohorts'][0][key]='0'*64;x.save()
            with self.assertRaisesRegex(ValueError,'CONFIG_OR_MAPPING_BINDING'):x.run()
            x.manifest['cohorts'][0][key]=original
    def test_absent_witness_and_changed_cutoff_are_explicit_failures(self):
        x=self.input();original=x.manifest['cohorts'][0].pop('inputReplay');x.save()
        with self.assertRaisesRegex(ValueError,'ORIGINAL_INPUT_REPLAY_REQUIRED'):x.run()
        x.manifest['cohorts'][0]['inputReplay']=original;x.manifest['asOfUtc']=f.stamp(41009);x.save()
        with self.assertRaisesRegex(ValueError,'SEAL_AFTER_FROZEN_CUTOFF'):x.run()
    def test_foreign_or_legacy_producer_cannot_borrow_replay(self):
        x=self.input();other=self.root/'foreign.py';other.write_text('raise RuntimeError("never execute")')
        x.manifest['cohorts'][0]['producer']=w.pin(other);x.save()
        with self.assertRaisesRegex(ValueError,'PRODUCER_BINDING'):x.run()
    def test_exact_crossing_costs_and_fractional_depth_are_never_imputed(self):
        sample=f.sample();y=R.label(sample,f.label(sample),1000,200000)
        self.assertEqual((y['yes'],y['no']),(Decimal('.005'),Decimal('-.055')))
        self.assertIsNone(R.label(sample,f.label(sample,quantity='.999999'),1000,200000))
        false=f.label(sample);false['feesAssumed']='0.005'
        with self.assertRaisesRegex(ValueError,'LABEL_AUTHORITY'):R.label(sample,false,1000,200000)
    def test_incomplete_first_alias_and_cluster_consume_window_without_rescue(self):
        first=f.sample(100000,'same',complete=False);alias=f.sample(100000,'same',market='2');later=f.sample(101000,'later')
        inp=f.Input([[first],[alias,f.label(alias),later,f.label(later)]]);self.addCleanup(inp.close)
        _,cut,reps,labels,counts,_=inp.load();rows,coverage=R.examples(reps,labels,1000,cut)
        self.assertEqual(rows,[]);self.assertEqual(counts['duplicateAliases'],1)
        self.assertEqual(coverage['missingTenFeatures'],1);self.assertEqual(coverage['overlappingTickerWindow'],1)
    def test_decimal_equivalent_aliases_share_key_but_real_economic_conflict_quarantines(self):
        first=f.sample();other=copy.deepcopy(first)
        for window in other['externalTradeFlow']['sourceSnapshot']['windows'].values():
            window['evidence'][0]['quantity']='1.25';window['evidenceSha256']=f.evidence_sha(window['evidence'])
        flow=other['externalTradeFlow'];flow['sourceSnapshotSha256']=R.sha(R.compact(flow['sourceSnapshot']).encode())
        other['trigger']['sourceSnapshotSha256']=flow['sourceSnapshotSha256'];other['trigger']['record']=copy.deepcopy(flow['sourceSnapshot']['windows']['1000']['evidence'][0]);f.rebind(other)
        inp=f.Input([[first],[other]]);self.addCleanup(inp.close)
        self.assertEqual(inp.load()[4]['economicConflicts'],0);self.assertEqual(inp.load()[4]['duplicateAliases'],1)
        for window in other['externalTradeFlow']['sourceSnapshot']['windows'].values():
            window['evidence'][0]['quantity']='2.5';window['observedQuantity']='2.5';window['knownSignedQuantity']='2.5'
            window['signedQuantity']='2.5';window['evidenceSha256']=f.evidence_sha(window['evidence'])
        flow=other['externalTradeFlow'];flow['sourceSnapshotSha256']=R.sha(R.compact(flow['sourceSnapshot']).encode())
        other['trigger']['sourceSnapshotSha256']=flow['sourceSnapshotSha256'];other['trigger']['record']=copy.deepcopy(flow['sourceSnapshot']['windows']['1000']['evidence'][0]);f.rebind(other)
        conflict=f.Input([[first],[other]]);self.addCleanup(conflict.close)
        loaded=conflict.load();self.assertEqual(loaded[4]['economicConflicts'],1);self.assertEqual(loaded[2],[])
    def test_shared_calendar_is_frozen_before_completeness_and_all_models_share_oos(self):
        reps=[{'sample':{'decisionMs':at}} for at in [100,200,300,400,500,600,700,800]]
        times=sorted({r['sample']['decisionMs'] for r in reps});self.assertEqual([times[len(times)*i//4] for i in (1,2,3)],[300,500,700])
        rows=self.rows(120);result=R.compare(rows,[300000,600000,900000],1200000)
        self.assertEqual(len({m['outOfSampleIdsSha256'] for m in result['models'].values()}),1)
        self.assertEqual({m['outOfSampleClusters'] for m in result['models'].values()},{90})
    def rows(self,n):
        return [{'id':str(i),'ticker':str(i),'market':'1','exchange':'101','at':i*10000,'end':i*10000+4000,
                 'available':i*10000+1001,'x':[float(i%3)]*10,'yes':Decimal('.01'),'no':Decimal('-.03'),'bbo':Decimal('-.025')} for i in range(n)]
    def test_training_purge_actual_availability_and_training_only_scaling(self):
        rows=self.rows(120);rows[0]['end']=300000;rows[1]['available']=300001
        result=R.compare(rows,[300000,600000,900000],1200000)
        self.assertEqual(result['folds'][0]['trainingClusters'],28)
        before=R.fit(rows[:30],(4,5),'yes');rows[90]['x']=[1e50]*10
        self.assertEqual(before,R.fit(rows[:30],(4,5),'yes'))
    def test_numerical_failure_abstains_all_four_models_no_bbo_fallback(self):
        with patch.object(R,'fit',side_effect=ValueError('TRADE_ANALYSIS_SINGULAR_MODEL')):
            result=R.compare(self.rows(120),[300000,600000,900000],1200000)
        self.assertEqual({m['outOfSampleClusters'] for m in result['models'].values()},{0})
        self.assertTrue(all(x['abstentionReason']=='NUMERICAL_FIT_FAILURE' for x in result['folds']))
    def test_positive_predictions_need_strict_threshold_and_100_both_minima(self):
        rows=[{'id':str(i),'ticker':str(i),'market':'1','at':i*4000,'end':i*4000+4000,'outcomes':[Decimal('.010001'),Decimal('-.02')],
               'predictions':[.02,.02],'intercept':[0.,0.]} for i in range(100)]
        result=R.metrics(rows);self.assertEqual(result['selectedNetCrossingSumExact'],'1.000100')
        self.assertTrue(all(w['side']=='YES' for w in result['selectedWindows']));self.assertTrue(result['coverageThresholdsPass'])
        rows[0]['predictions']=[0.,-1.];self.assertEqual(R.metrics(rows)['selectedClusters'],99);self.assertTrue(R.metrics(rows)['abstain'])
    def test_fixed_source_or_protocol_changes_cannot_load_as_tuning_parameters(self):
        with self.assertRaisesRegex(ValueError,'FIXED_SOURCE_CHANGED'):a.checked_module(HERE/'analyze.py',a.READER_LF_SHA,'bad')
        x=self.input();x.manifest['analysisProtocolSha256']='0'*64;x.save()
        with self.assertRaisesRegex(ValueError,'FROZEN_MANIFEST'):x.run()

if __name__=='__main__':unittest.main()
