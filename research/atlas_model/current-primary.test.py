import argparse,copy,importlib.util,json,os,shutil,tempfile,unittest
from pathlib import Path
spec=importlib.util.spec_from_file_location('current_primary',Path(__file__).with_name('current-primary.py'));p=importlib.util.module_from_spec(spec);spec.loader.exec_module(p)
class CurrentPrimaryTests(unittest.TestCase):
 def setUp(self):self.current=p.build(SOURCES)
 def test_four_real_races_samples_and_original_published_clocks(self):
  self.assertEqual(len(self.current['rows']),4)
  rows={r['state']:r for r in self.current['rows']};self.assertEqual([(rows[s]['democratic'],rows[s]['republican'],rows[s]['sampleSize']) for s in ['KS','NC','IA','NY']],[(45,43,750),(54,40,643),(45,47,650),(52,40,900)])
  for r in rows.values():self.assertLess(r['publishedAtMs'],r['availableAtMs']);self.assertIn('date_only',r['fieldClockPrecision'])
 def test_original_receipt_ttl_is_not_refreshed_to_now(self):
  self.assertEqual(self.current['observedAtMs'],p.ms('2026-10-16T05:41:40.656128+00:00'));self.assertEqual(self.current['expiresAtMs'],p.ms('2026-10-16T01:12:46.963162+00:00')+6*3600000);self.assertLess(self.current['observedAtMs'],self.current['expiresAtMs'])
 def fixture(self):
  t=tempfile.TemporaryDirectory();base=Path(t.name)
  for s in p.SPECS:
   for suffix in ['', '.receipt.json']:shutil.copyfile(SOURCES/(s[0]+suffix),base/(s[0]+suffix))
  return t,base
 def test_changed_publisher_bytes_are_not_a_primary_source(self):
  t,base=self.fixture()
  with t:
   f=base/p.SPECS[0][0];f.write_bytes(f.read_bytes()+b'changed')
   with self.assertRaisesRegex(ValueError,'source/receipt/hash changed'):p.build(base)
 def test_same_body_with_retained_receipt_retimestamp_cannot_renew_model(self):
  t,base=self.fixture()
  with t:
   f=base/(p.SPECS[0][0]+'.receipt.json');r=json.loads(f.read_bytes());r['receivedAt']='2026-10-16T06:00:00Z';f.write_text(json.dumps(r))
   with self.assertRaisesRegex(ValueError,'clock renewal is forbidden'):p.build(base)
 def test_missing_actual_original_does_not_fall_back_to_oct5_transcription(self):
  t,base=self.fixture()
  with t:
   (base/p.SPECS[2][0]).unlink()
   with self.assertRaises(ValueError):p.build(base)
 def test_no_double_count_same_survey_or_forecast_as_poll(self):
  rows=self.current['rows'];self.assertTrue(all('CTM' not in r['pollster'] and 'ECU' not in r['pollster'] for r in rows));self.assertEqual(sum(r['state']=='IA' for r in rows),1);self.assertEqual(sum(r['state']=='KS' for r in rows),1);self.assertTrue(any('Q5A' in note for note in self.current['notes']))
if __name__=='__main__':
 a=argparse.ArgumentParser();a.add_argument('--sources',type=Path,required=True);args,rest=a.parse_known_args();SOURCES=args.sources;unittest.main(argv=['current-primary.test.py']+rest)
