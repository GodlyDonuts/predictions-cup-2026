import argparse,json,pathlib,shutil,tempfile,unittest
import refresh
BASE=None
class ActualSourceTests(unittest.TestCase):
 def test_new_actual_originals_scope_named_candidates_and_not_old_snapshot(self):
  c=refresh.build(BASE);self.assertEqual({r['raceId'] for r in c['rows']},{'2026:senate:KS','2026:senate:IA'});self.assertEqual([(r['state'],r['democratic'],r['republican'],r['sampleSize']) for r in c['rows']],[('KS',48,45,2255),('IA',47,46,2245)]);self.assertTrue(all(not s['publicationClockKnown'] for s in c['sources']));self.assertEqual(c['expiresAtMs'],1792159441531)
 def test_original_receipt_clock_tampering_rejected(self):
  with tempfile.TemporaryDirectory() as d:
   p=pathlib.Path(d)
   for s in refresh.SPECS:
    for suffix in ['', '.receipt.json']:shutil.copyfile(BASE/(s[1]+suffix),p/(s[1]+suffix))
   f=p/(refresh.SPECS[0][1]+'.receipt.json');r=json.loads(f.read_text());r['receivedAt']='2026-10-16T20:00:00Z';f.write_text(json.dumps(r))
   with self.assertRaisesRegex(ValueError,'receipt changed'):refresh.build(p)
 def test_changed_original_pdf_rejected(self):
  with tempfile.TemporaryDirectory() as d:
   p=pathlib.Path(d)
   for s in refresh.SPECS:
    for suffix in ['', '.receipt.json']:shutil.copyfile(BASE/(s[1]+suffix),p/(s[1]+suffix))
   with (p/refresh.SPECS[0][1]).open('ab') as f:f.write(b'changed')
   with self.assertRaisesRegex(ValueError,'source or receipt changed'):refresh.build(p)
 def test_export_cannot_overwrite_any_existing_output(self):
  with tempfile.TemporaryDirectory() as d:
   with self.assertRaisesRegex(ValueError,'new private'):refresh.export(BASE,BASE,BASE,BASE,pathlib.Path(d))
if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('--sources',type=pathlib.Path,required=True);a,rest=p.parse_known_args();BASE=a.sources;unittest.main(argv=['test_refresh']+rest)
