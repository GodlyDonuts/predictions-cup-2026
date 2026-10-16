import os
os.environ.setdefault('OPENBLAS_NUM_THREADS','1')
import argparse,copy,json,unittest
from pathlib import Path
import numpy as np
import model

class AtlasTests(unittest.TestCase):
 @classmethod
 def setUpClass(cls):
  cls.training=json.loads(TRAINING.read_bytes());cls.rows=cls.training['rows'];cls.fitted=model.fit(cls.rows);cls.current=json.loads(CURRENT.read_bytes());cls.validation=model.validate(cls.rows)
 def test_actual_original_cohorts(self):
  self.assertEqual((self.fitted['races'],self.fitted['pollsterRaceObservations']),(416,1291));self.assertEqual(len({r['raceId'] for r in self.rows if r['office']=='senate'}),185);self.assertEqual(sum(r['office']=='senate' for r in self.rows),491);self.assertEqual(sum(r['office']=='governor' for r in self.rows),800)
 def test_held_cycle_is_absent_and_chronological_cutoffs_are_strict(self):
  for f in self.validation['folds']:self.assertNotIn(f['heldOutYear'],f['trainingYears'])
  for p in self.validation['chronologicalRows']:self.assertLess(p['trainedThroughYear'],p['year'])
  # Held-out labels are not a posterior feature: changing all its outcomes
  # cannot change a model fit on the other original cycles or its predictions.
  year=2022;train=[r for r in self.rows if r['year']!=year];test=[r for r in self.rows if r['year']==year];bad=copy.deepcopy(test)
  for r in bad:r['actualMargin']=-r['actualMargin'];r['democraticWon']=not r['democraticWon']
  m=model.fit(train);_,a,ca=model.posterior(m,test);_,b,cb=model.posterior(m,bad);np.testing.assert_array_equal(a,b);np.testing.assert_array_equal(ca,cb)
 def test_actual_joint_covariance_is_psd_and_not_independent(self):
  _,mean,cov=model.posterior(self.fitted,self.current['rows']);self.assertGreater(np.min(np.linalg.eigvalsh(cov)),0);self.assertTrue(np.any(np.abs(cov-np.diag(np.diag(cov)))>1));draw=model.draw_joint(mean,cov,65536,1701);emp=np.cov(draw.T);np.testing.assert_allclose(emp,cov,rtol=.25,atol=1.5)
 def test_shared_uncertainty_does_not_vanish_with_huge_poll_samples(self):
  rows=copy.deepcopy(self.current['rows'])
  for r in rows:r['sampleSize']=10**12
  rs,_,cov=model.posterior(self.fitted,rows)
  self.assertTrue(all(v>self.fitted['variance']['national']*.4 for v in np.diag(cov)));self.assertTrue(any(p['pollsterId'] not in self.fitted['house'] for p in rows))
 def test_actual_house_partial_pooling_retains_uncertainty(self):
  self.assertTrue(self.fitted['house']);self.assertTrue(all(0<v<=self.fitted['houseVariance'] for v in self.fitted['houseUncertainty'].values()));self.assertGreater(self.fitted['varianceEvidence']['nationalCycles'],0);self.assertGreater(self.fitted['varianceEvidence']['statePairs'],0)
 def test_field_time_and_duplicate_house_are_validated(self):
  bad=copy.deepcopy(self.current['rows']);bad[0]['fieldAtMs']=bad[0]['electionAtMs']+1
  with self.assertRaises(ValueError):model.posterior(self.fitted,bad)
  bad=copy.deepcopy(self.current['rows']);extra=dict(bad[0],id='different-question-same-house');bad.append(extra)
  with self.assertRaises(ValueError):model.posterior(self.fitted,bad)
 def artifact(self,current=None,seed=1701):
  c=self.current if current is None else current
  return model.artifact(self.fitted,c['rows'],self.training['provenance'],self.validation,c['observedAtMs'],c['expiresAtMs'],4096,seed)
 def test_artifact_joint_weights_fingerprint_and_integer_wire(self):
  a=self.artifact();b=self.artifact();self.assertEqual(a,b);digest=a.pop('modelSha256');self.assertEqual(digest,model.sha(a));self.assertEqual(sum(x['weightPpm'] for x in a['scenarios']),1000000);self.assertGreater(len(a['scenarios']),2)
  self.assertEqual(a['trainingCutoffMs'],max(p['electionAtMs'] for p in self.rows));self.assertEqual(a['qualification']['kind'],'research');self.assertEqual(a['objective'],'ordinary_terminal_race_outcome')
  def inspect(x):
   self.assertNotIsInstance(x,float)
   if isinstance(x,dict):
    for v in x.values():inspect(v)
   if isinstance(x,list):
    for v in x:inspect(v)
  inspect(a)
 def test_current_future_unavailable_clock_never_becomes_observed(self):
  for missing in [True,False]:
   bad=copy.deepcopy(self.current);bad['rows'][0]['availableAtMs']=None if missing else bad['observedAtMs']+1
   with self.assertRaises(ValueError):self.artifact(bad)
 def test_training_cutoff_cannot_include_the_current_election(self):
  bad=copy.deepcopy(self.current)
  for r in bad['rows']:r['year']=2022
  with self.assertRaises(ValueError):self.artifact(bad)
 def test_t4_probability_matches_actual_joint_sampler(self):
  draws=model.draw_joint(np.array([1.0]),np.array([[4.0]]),131072,1701);emp=float(np.mean(draws[:,0]>0));theory=model.t4cdf(1/np.sqrt(2));self.assertAlmostEqual(emp,theory,delta=.005);self.assertEqual(model.t4cdf(0),.5);self.assertAlmostEqual(model.t4cdf(1)+model.t4cdf(-1),1)
 def test_no_claim_of_baseline_superiority_or_refund_probability(self):
  interval=self.validation['pairedBrierCycleBootstrap']['percentile95Interval'];self.assertLess(interval[0],0);self.assertGreater(interval[1],0)
  self.assertFalse(self.artifact()['qualification']['exceptionalStateProbabilitiesIdentified']);self.assertFalse(self.artifact()['qualification']['executionAuthority'])

if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('--training',type=Path,required=True);p.add_argument('--current',type=Path,required=True);a,rest=p.parse_known_args();TRAINING=a.training;CURRENT=a.current;unittest.main(argv=['test_model.py']+rest)
