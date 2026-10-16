import copy
from decimal import Decimal
import hashlib
from pathlib import Path
import unittest
import analyze as a
from test_crossvenue_economics import records, BASE
from test_crossvenue_observer import public_flow, websocket


def cache(at, direction=1):
    study=a.o.Study(lambda _:None);e=websocket(at);e['markets'][0]['tradeFlow']=public_flow(at,direction)
    study.external_update(e,at)
    return study._flow_for_decision('1',at)


class FlowPolicyTests(unittest.TestCase):
    def decision(self):
        r=records(1)[0];r['external']['ticker']='RACE-26-D';r['externalTradeFlow']=cache(r['decisionMs'])
        return r

    def test_actual_observer_factory_supplies_fixed_bounded_fractional_features(self):
        r=self.decision();values,reasons=a.flow_vector(r)
        self.assertEqual(reasons,[None]*3)
        self.assertEqual(values,[float(Decimal('1.25')/Decimal('2.25'))]*3)
        self.assertFalse(r['externalTradeFlow']['executionAuthority'])

    def test_missing_unsigned_and_empty_window_are_missing_not_zero_imputation(self):
        r=self.decision();r['externalTradeFlow']=None
        self.assertEqual(a.flow_vector(r),([None]*3,['missingSnapshot']*3))
        r=self.decision();r['externalTradeFlow']=cache(r['decisionMs'],None)
        self.assertEqual(a.flow_vector(r)[0],[None]*3)
        self.assertEqual(a.flow_vector(r)[1],['unknownDirection']*3)

    def test_tampered_signed_input_hash_clock_or_complete_flag_cannot_be_used(self):
        for mode in ('value','hash','availability','partial','scope'):
            r=self.decision();f=r['externalTradeFlow']
            if mode=='value':f['inputs']['1000']['observedSignedQuantity']='999'
            elif mode=='hash':f['sourceSnapshotSha256']='a'*64
            elif mode=='availability':f['availableMs']=r['decisionMs']+1
            elif mode=='partial':f['inputs']['1000']['completeTradeTape']=True
            else:r['external']['ticker']='WRONG'
            with self.assertRaises(ValueError,msg=mode):a.flow_vector(r)

    def test_generic_ridge_original_seven_columns_exactly_match_prior_solver(self):
        raw=records(80);d={r['sampleId']:r for r in raw if r['kind']=='decision_features'};l={(r['sampleId'],r['horizonMs']):r for r in raw if r['kind']=='horizon_label'}
        rows,_=a.e.examples(d,l,1000,BASE+100000000,3000,Decimal(0),Decimal('.005'),'direct_party_reference')
        old=a.e.fit(rows,'yes');new=a.fit([{**r,'x':r['x']+[.2,.3,.4]} for r in rows],'yes',tuple(range(7)))
        for key in ('means','scales','coefficients'):self.assertEqual(old[key],new[key])

    def test_every_comparator_uses_same_purged_complete_case_cohort(self):
        raw=records(120);d={r['sampleId']:r for r in raw if r['kind']=='decision_features'};l={(r['sampleId'],r['horizonMs']):r for r in raw if r['kind']=='horizon_label'}
        rows,_=a.e.examples(d,l,1000,BASE+100000000,3000,Decimal(0),Decimal('.005'),'direct_party_reference')
        result=a.compare([{**r,'x':r['x']+[.2,.3,.4]} for r in rows],d)
        self.assertEqual(set(x['outOfSampleClusters'] for x in result['models'].values()),{90})
        self.assertEqual(result['outOfSampleSampleIdsSha256'],a.e.walk_forward(rows)['outOfSampleSampleIdsSha256'])
        self.assertTrue(all(x['abstain'] and not x['executionAuthority'] for x in result['models'].values()))

    def test_fixed_policy_has_no_fit_search_or_authority_and_no_short_cohort_fit(self):
        self.assertEqual(a.POLICY['trainingMinimumClusters'],16)
        self.assertEqual(a.POLICY['minimumSelectedClusters'],100)
        self.assertFalse(a.POLICY['fitOrThresholdSearch'])
        self.assertFalse(a.POLICY['missingImputation'])
        r=a.compare([],{});self.assertTrue(all(x['mse'] is None and x['selectedClusters']==0 for x in r['models'].values()))


if __name__=='__main__':unittest.main()
