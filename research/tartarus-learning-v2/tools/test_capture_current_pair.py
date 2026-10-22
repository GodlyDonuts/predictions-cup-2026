import importlib.util,json,sys,tempfile,unittest
from pathlib import Path
from datetime import datetime,timezone,timedelta
sys.path.insert(0,str(Path(__file__).resolve().parents[3]))
from moloch.tartarus.test_current_portfolio import fixture,seal
spec=importlib.util.spec_from_file_location('capture_pair',Path(__file__).with_name('capture-current-rank-inputs.py'));capture=importlib.util.module_from_spec(spec);spec.loader.exec_module(capture)
class CapturePairTests(unittest.TestCase):
 def setUp(self):
  self.temp=tempfile.TemporaryDirectory();self.root=Path(self.temp.name);self.bank=self.root/'bank';self.bank.mkdir();self.r,self.v,self.registry=fixture();self.at=datetime(2026,10,18,11,0,2,tzinfo=timezone.utc);self.write()
 def tearDown(self):self.temp.cleanup()
 def write(self):
  (self.root/'tartarus-current-native-state.json').write_text(json.dumps(self.v));(self.bank/'report.json').write_bytes(seal(self.r))
 def test_advancing_writer_head_does_not_invalidate_recent_exact_retained_pair(self):
  newer=seal({'manifestHash':'a'*64,'sequence':8,'previous':'c'*64});(self.bank/'head.json').write_bytes(newer);v,r,c=capture.read_research_pair(self.root,self.bank,self.registry,self.at);self.assertIsNotNone(c['financial']);self.assertEqual(c['nativeSequence'],7);self.assertFalse(c['executionAuthority']);self.assertEqual(c['financial']['cash'],'7.000000');self.assertEqual((self.bank/'head.json').read_bytes(),newer)
 def test_mismatched_pair_and_financial_substitution_still_refuse(self):
  self.r['sequence']=8;self.write()
  with self.assertRaisesRegex(ValueError,'MATCHED_NATIVE_BOUNDARY_REQUIRED'):capture.read_research_pair(self.root,self.bank,self.registry,self.at)
  self.r['sequence']=7;self.v['account']['cash']='8.000000';self.write()
  with self.assertRaisesRegex(ValueError,'CURRENT_FINANCIAL_BINDING'):capture.read_research_pair(self.root,self.bank,self.registry,self.at)
 def test_stale_or_future_views_cannot_be_reissued_as_current(self):
  for at in [self.at+timedelta(seconds=90),self.at-timedelta(seconds=2)]:
   with self.assertRaisesRegex(ValueError,'CURRENT_RESEARCH_VIEW_AGE'):capture.read_research_pair(self.root,self.bank,self.registry,at)
 def test_tampered_report_seal_is_not_a_transient_clock_miss(self):
  p=self.bank/'report.json';p.write_bytes(p.read_bytes().replace(b'7.000000',b'8.000000'))
  with self.assertRaises(ValueError):capture.read_research_pair(self.root,self.bank,self.registry,self.at)
if __name__=='__main__':unittest.main()
