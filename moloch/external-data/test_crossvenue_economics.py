import copy
from decimal import Decimal
import gzip
import hashlib
import json
import math
from pathlib import Path
import tempfile
import unittest
from crossvenue_economics import (HORIZONS, canonical, digest, load, examples, feature_vector, fit, report, walk_forward)
from crossvenue_observer import Study, stamp, features, levels
BASE=1792144000000

def records(n=400, step=150000):
    out=[]
    for i in range(n):
        at=BASE+i*step
        sign=1 if i%2 else -1
        sig=features(levels([{'price':'0.490000','quantity':10}],False),levels([{'price':'0.510000','quantity':10}],True))
        ext={**sig,'midpoint':0.52 if sign>0 else 0.48,'imbalance':sign*0.2}
        j=hashlib.sha256(('native'+str(i)).encode()).hexdigest();e=hashlib.sha256(('external'+str(i)).encode()).hexdigest()
        sid=hashlib.sha256(f'1:101:{j}:{e}:{at}'.encode()).hexdigest()
        out.append({'kind':'decision_features','sampleId':sid,'decisionMs':at,'decisionAt':stamp(at),'marketId':'1','exchangeId':'101','sig':{'journalHash':j,'sourceAt':stamp(at-1),'receivedAt':stamp(at),'features':sig},'external':{'receivedMs':at,'availableMs':at,'bodySha256':e,'mappingKind':'direct_party_reference','features':ext},'externalLogitChanges':{str(h):sign*0.1 for h in (1000,5000,30000)}})
        future=features(levels([{'price':'0.530000' if sign>0 else '0.450000','quantity':10}],False),levels([{'price':'0.550000' if sign>0 else '0.470000','quantity':10}],True))
        for h in HORIZONS:
            out.append({'kind':'horizon_label','sampleId':sid,'horizonMs':h,'sourceAt':stamp(at+h),'receivedAt':stamp(at+h),'observedAt':stamp(at+h+10),'sigJournalHash':hashlib.sha256(('label'+str(i)+str(h)).encode()).hexdigest(),'features':future,'yesOneShareTakerRoundTrip':str(Decimal(future['bid'])-Decimal(sig['ask'])),'noOneShareTakerRoundTrip':str(Decimal(sig['bid'])-Decimal(future['ask'])),'counterfactual':True,'actualFill':False,'feesAssumed':'0'})
    return out

class EconomicsTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();self.addCleanup(self.tmp.cleanup);self.root=Path(self.tmp.name)
    def write(self,rows,name='original.jsonl.gz'):
        p=self.root/name
        with gzip.open(p,'wt',encoding='utf-8') as f:
            for r in rows:f.write(canonical(r)+'\n')
        return p
    def test_new_optional_lag_anchors_are_source_local_causal_and_exact(self):
        row=records(1)[0];at=row['decisionMs'];row['external'].update(source='kalshi_official_v2',ticker='RACE')
        row['externalLagAnchors']={}
        for lag in (1000,5000,30000):
            row['externalLagAnchors'][str(lag)]={'bodySha256':'c'*64,'receivedMs':at-lag,'availableMs':at-lag,
                'source':'kalshi_official_v2','ticker':'RACE','mappingKind':'direct_party_reference','midpoint':.5}
            current=row['external']['features']['midpoint'];row['externalLogitChanges'][str(lag)]=math.log(current/(1-current))
        self.assertIsNotNone(feature_vector(row))
        for mode in ('future','old','source','delta','missing','hash'):
            bad=copy.deepcopy(row);a=bad['externalLagAnchors']['30000']
            if mode=='future':a['availableMs']=at+1
            elif mode=='old':a['receivedMs']=at-50001;a['availableMs']=at-50001
            elif mode=='source':a['source']='kalshi_official_ws_v2'
            elif mode=='delta':bad['externalLogitChanges']['30000']+=.000001
            elif mode=='missing':bad['externalLagAnchors']['30000']=None
            else:a['bodySha256']='not-a-hash'
            with self.assertRaisesRegex(ValueError,'LAG_ANCHOR'):feature_vector(bad)
    def test_real_observer_output_schema_is_joined_and_missing_history_stays_missing(self):
        out=[];s=Study(out.append,horizons=(1000,))
        s.external_update({'markets':[{'ticker':'RACE','sigMarketIds':['1'],'mappingKind':'direct_party_reference','receivedAt':stamp(BASE),'bodySha256':'a'*64,'normalized':{'yesBids':[{'price':'0.500000','quantity':'10'}],'yesAsks':[{'price':'0.520000','quantity':'10'}]}}]},BASE)
        def row(at,seq):return{'hash':str(seq)*64,'sequence':seq,'event':{'type':'book','book':{'marketId':'1','exchangeId':'101','sequence':str(seq),'sourceAt':stamp(at),'receivedAt':stamp(at),'bids':[{'price':'0.490000','quantity':10}],'asks':[{'price':'0.510000','quantity':10}]}}}
        s.accept(row(BASE,1),BASE);s.accept(row(BASE+1000,2),BASE+1000)
        r=report([self.write(out)],BASE+1000)
        self.assertGreater(r['horizons']['1000']['data']['missingFeature'],0)
        self.assertTrue(r['horizons']['1000']['abstain'])
    def test_explicit_legacy_producer_format_preserves_original_ids_and_requires_exact_source_hash(self):
        rows=records(2)
        for i in (0,6):
            r=rows[i];old=r['sampleId'];r['sampleId']=hashlib.sha256(f"{r['sig']['journalHash']}:{r['external']['bodySha256']}:{r['decisionMs']}".encode()).hexdigest()
            for l in rows[i+1:i+6]:l['sampleId']=r['sampleId']
        p=self.write(rows);producer=self.root/'actual-producer.py';producer.write_text('original retained producer source')
        h=hashlib.sha256(producer.read_bytes()).hexdigest()
        with self.assertRaisesRegex(ValueError,'SAMPLE_BINDING'):report([p],BASE+500000)
        result=report([p],BASE+500000,sample_id_format='legacy_unscoped_v1',producer_source=producer,producer_sha256=h)
        self.assertTrue(result['producerSourceVerified']);self.assertEqual(result['producerSource']['sha256'],h)
        self.assertEqual(result['policy']['sampleIdFormat'],'legacy_unscoped_v1')
        with self.assertRaisesRegex(ValueError,'PRODUCER_SOURCE_HASH'):report([p],BASE+500000,sample_id_format='legacy_unscoped_v1',producer_source=producer,producer_sha256='0'*64)
    def test_actual_exact_ask_to_future_bid_can_be_negative_even_midpoint_unchanged(self):
        rows=records(1)
        for l in rows[1:]:l['features']=copy.deepcopy(rows[0]['sig']['features']);l['yesOneShareTakerRoundTrip']='-0.020000';l['noOneShareTakerRoundTrip']='-0.020000'
        d,l,_,_=load([self.write(rows)],BASE+130000)
        r,_=examples(d,l,1000,BASE+130000,3000,Decimal('0.001'),Decimal('0.005'),'direct_party_reference')
        self.assertEqual(r[0]['yesGross'],'-0.020000');self.assertAlmostEqual(r[0]['yes'],-0.026)
    def test_purged_walk_forward_positive_research_reports_baseline_without_approval(self):
        r=report([self.write(records())],BASE+400*150000)
        self.assertFalse(r['modelApproval']);self.assertFalse(r['profitEstablished']);self.assertEqual(r['status'],'UNQUALIFIED_RESEARCH')
        for h in HORIZONS:
            x=r['horizons'][str(h)];self.assertEqual(x['outOfSampleClusters'],300);self.assertEqual(x['selectedClusters'],300)
            self.assertGreater(x['selectedNetCrossingReturnSum'],0);self.assertLess(x['modelMseNetReturn'],x['interceptBaselineMseNetReturn']);self.assertFalse(x['abstain'])
    def test_fewer_than100_heldout_selected_clusters_cannot_qualify(self):
        r=report([self.write(records(60))],BASE+60*150000)
        self.assertTrue(all(x['abstain'] for x in r['horizons'].values()))
        self.assertEqual(r['horizons']['1000']['reason'],'INSUFFICIENT_INDEPENDENT_CLUSTERS')
        with self.assertRaisesRegex(ValueError,'MINIMUM_CLUSTERS'):report([self.write([],name='empty.gz')],BASE,minimum_clusters=99)
    def test_market_clusters_remove_overlapping_exchange_or_print_samples(self):
        d,l,_,_=load([self.write(records(300,step=1000))],BASE+500000)
        rows,c=examples(d,l,120000,BASE+500000,3000,Decimal(0),Decimal(0),'direct_party_reference')
        self.assertEqual(len(rows),3);self.assertEqual(c['overlappingClusterRows'],297)
    def test_missing_features_are_dropped_not_imputed_or_replaced_with_zero(self):
        rows=records(2);rows[0]['externalLogitChanges']['30000']=None
        d,l,_,_=load([self.write(rows)],BASE+500000)
        x,c=examples(d,l,1000,BASE+500000,3000,Decimal(0),Decimal(0),'direct_party_reference')
        self.assertEqual(len(x),1);self.assertEqual(c['missingFeature'],1)
    def test_feature_availability_after_decision_and_source_mismatch_reject(self):
        for field in ['future','hash']:
            rows=records(1)
            if field=='future':rows[0]['external']['availableMs']=BASE+1
            else:rows[0]['sampleId']='b'*64
            with self.assertRaisesRegex(ValueError,'FEATURE_LOOKAHEAD|SAMPLE_BINDING'):report([self.write(rows,name=field+'.gz')],BASE+500000)
    def test_asof_cutoff_excludes_future_records_and_records_source_bundle_and_policy_hash(self):
        p=self.write(records(2));r=report([p],BASE+1000)
        self.assertGreater(r['input']['excludedAfterCutoff'],0);self.assertEqual(r['sources'][0]['sha256'],hashlib.sha256(p.read_bytes()).hexdigest())
        self.assertEqual(r['sourceBundleSha256'],digest(r['sources']));self.assertNotEqual(r['cutoffPolicySha256'],report([p],BASE+2000)['cutoffPolicySha256'])
    def test_conflicting_duplicate_sample_or_label_rejects_identical_duplicate_deduplicates(self):
        rows=records(1);p=self.write(rows+[copy.deepcopy(rows[0])]);_,_,_,c=load([p],BASE+500000);self.assertEqual(c['identicalDuplicates'],1)
        changed=copy.deepcopy(rows[1]);changed['yesOneShareTakerRoundTrip']='0.999'
        with self.assertRaisesRegex(ValueError,'CONFLICTING_DUPLICATE'):load([self.write(rows+[changed],name='conflict.gz')],BASE+500000)
    def test_late_or_early_original_label_and_tampered_return_are_rejected(self):
        for mode in ['early','late','return']:
            rows=records(1)
            if mode=='early':rows[1]['sourceAt']=stamp(BASE+999)
            elif mode=='late':rows[1]['receivedAt']=stamp(BASE+4001);rows[1]['observedAt']=stamp(BASE+4001)
            else:rows[1]['yesOneShareTakerRoundTrip']='0.99'
            with self.assertRaisesRegex(ValueError,'LABEL_LOOKAHEAD_OR_WINDOW|LABEL_RETURN_CONFLICT'):report([self.write(rows,name=mode+'.gz')],BASE+500000)
    def test_no_one_share_liquidation_capacity_produces_missing_not_fictitious_pnl(self):
        rows=records(1);rows[1]['features']['bidQuantity']='0.5';rows[1]['yesOneShareTakerRoundTrip']=None
        r=report([self.write(rows)],BASE+500000);self.assertEqual(r['horizons']['1000']['data']['missingOneShareCapacity'],1);self.assertEqual(r['horizons']['1000']['outOfSampleClusters'],0)
    def test_training_purge_uses_full_label_window_and_actual_observation_availability(self):
        d,l,_,_=load([self.write(records(100,step=1000))],BASE+500000)
        x,_=examples(d,l,1000,BASE+500000,3000,Decimal(0),Decimal(0),'direct_party_reference')
        result=walk_forward(x)
        for f in result['folds']:
            expected=[r for r in x if r['end']<f['testStartMs'] and r['available']<=f['testStartMs']]
            self.assertEqual(f['trainingClusters'],len(expected))
    def test_input_bounds_duplicate_json_keys_and_truncated_gzip_fail_closed(self):
        p=self.write(records(1))
        with self.assertRaisesRegex(ValueError,'INPUT_BOUNDS'):load([p],BASE+500000,max_rows=1)
        bad=self.root/'bad.jsonl';bad.write_text('{"kind":"x","kind":"y"}\n')
        with self.assertRaisesRegex(ValueError,'DUPLICATE_JSON_KEY'):load([bad],BASE)
        truncated=self.root/'truncated.gz';truncated.write_bytes(p.read_bytes()[:-8])
        with self.assertRaises((EOFError,OSError)):load([truncated],BASE+500000)
    def test_training_standardization_is_frozen_without_test_features_or_labels(self):
        d,l,_,_=load([self.write(records(20))],BASE+4000000)
        x,_=examples(d,l,1000,BASE+4000000,3000,Decimal(0),Decimal(0),'direct_party_reference')
        model=fit(x[:16],'yes');before=canonical(model);x[-1]['x']=[1e9]*7;x[-1]['yes']=1e9
        self.assertEqual(canonical(fit(x[:16],'yes')),before)

if __name__=='__main__':unittest.main()
