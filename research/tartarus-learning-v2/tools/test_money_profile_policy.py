import unittest,json
from copy import deepcopy
from hashlib import sha256
from money_profile_policy import CURRENT,HISTORICAL,select,expected_ids,verify_summary
class PolicyTests(unittest.TestCase):
 def fixture(self):
  ids=list(CURRENT+HISTORICAL)
  v={'model':{'scenarios':[{'clock':123}], 'probabilityModels':[{'id':i,'weightsPpm':[1000000]} for i in ids]},'profiles':[{'id':i,'marginalOverrides':[{'observedAtMs':123,'maximumAgeMs':45}]} for i in ids], 'issuedAtMs':123, 'inputCutoffMs':120, 'marginals':[{'observedAtMs':119}], 'historicalReviewEnvelopeSources':[{'bodyText':'retained original opinion'}]}
  return v,json.dumps(v).encode()
 def test_current_prices_retain_full_stress_and_original_clocks(self):
  v,raw=self.fixture();before=deepcopy(v);r=select(v,raw,'current-prices');self.assertEqual(v,before);self.assertEqual([m['id'] for m in r['model']['probabilityModels']],list(CURRENT));self.assertEqual([p['id'] for p in r['profiles']],list(CURRENT));self.assertEqual(r['model']['scenarios'],v['model']['scenarios'])
  for k in ['issuedAtMs','inputCutoffMs','marginals','historicalReviewEnvelopeSources']:self.assertEqual(r[k],v[k])
  self.assertEqual(r['decisionProfileSelection']['parentModelSha256'],sha256(raw).hexdigest());self.assertEqual(r['decisionProfileSelection']['retainedStressOnlyIds'],list(HISTORICAL));self.assertFalse(r['decisionProfileSelection']['sourceClocksRenewed'])
 def test_all_declared_keeps_all_original_models(self):
  v,raw=self.fixture();r=select(v,raw,'all-declared');self.assertEqual(r['model'],v['model']);self.assertEqual(r['profiles'],v['profiles'])
 def test_bad_parent_unknown_missing_duplicate_or_unreviewed_policy_refuse(self):
  v,raw=self.fixture()
  with self.assertRaises(ValueError):select(v,b'{}','current-prices')
  with self.assertRaises(ValueError):select(v,raw,'best-performing')
  for damage in ['missing','duplicate','unknown','profiles']:
   a=deepcopy(v)
   if damage=='missing':a['model']['probabilityModels'].pop()
   elif damage=='duplicate':a['model']['probabilityModels'][0]=deepcopy(a['model']['probabilityModels'][1])
   elif damage=='unknown':a['model']['probabilityModels'][0]['id']='optimistic'
   else:a['profiles'].reverse()
   with self.assertRaises(ValueError):select(a,json.dumps(a).encode(),'current-prices')
 def test_review_summary_and_actual_campaign_must_match_selected_policy(self):
  ids=expected_ids('current-prices');s={'decisionProfilePolicy':'current-prices','decisionProfileIds':ids,'probabilityModels':3};c={'probabilityModels':[{'id':i} for i in ids]};verify_summary(s,c,'current-prices')
  for field,value in [('decisionProfilePolicy','all-declared'),('decisionProfileIds',list(reversed(ids))),('probabilityModels',5)]:
   bad=deepcopy(s);bad[field]=value
   with self.assertRaises(ValueError):verify_summary(bad,c,'current-prices')
  c['probabilityModels'][0]['id']='other'
  with self.assertRaises(ValueError):verify_summary(s,c,'current-prices')
if __name__=='__main__':unittest.main()
