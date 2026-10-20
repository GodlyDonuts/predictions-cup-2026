import copy,gzip,hashlib,json,sqlite3,tempfile,unittest
from pathlib import Path
from .compile_external_archive import compile_archive
from .breadth_frame_index import sha
from .audit_breadth_inventory import clock
from moloch.learning_v2 import test_reference_adapters as fixture


class ExternalArchiveTests(unittest.TestCase):
    def setup_source(self,r):
        poly=fixture.ReferenceOrientationTests().poly()
        kalshi={**fixture.receipt({'orderbook_fp':{'yes_dollars':[['0.4','12']],'no_dollars':[['0.5','20']]}}),'ticker':'party'}
        ku={'markets':[{'ticker':'party','sigMappings':[{'sigMarketId':'1','side':'YES'}]}]}
        pu={'outcomes':[poly['identity']]};entries=[]
        def add(source,name,rows,truncate=False,prefix=b''):
            raw=gzip.compress(b''.join(fixture.raw(v)for v in rows),mtime=0)
            if truncate:raw=raw[:-8]
            raw=prefix+raw
            h=hashlib.sha256(raw).hexdigest();(r/(h+'.snapshot')).write_bytes(raw)
            e={'source':source,'filename':name,'source_size':len(raw),'sha256':h,'captured_unix':fixture.AT/1000,
               'archive_relative_path':h+'.snapshot'};entries.append(e);return e
        add('Kalshi','k.gz',[kalshi]);add('forward_polymarket_001','p.gz',[poly])
        (r/'ku.json').write_text(json.dumps(ku));(r/'pu.json').write_text(json.dumps(pu))
        return entries,add,poly,kalshi

    def run_compile(self,r,entries):
        (r/'index.jsonl').write_text(''.join(json.dumps(e)+'\n'for e in entries))
        return compile_archive(r,r/'index.jsonl',sha(r/'index.jsonl'),r/'ku.json',sha(r/'ku.json'),
            r/'pu.json',sha(r/'pu.json'),r/'output',roles={'fit':[clock('2026-10-18T00:00:00Z'),clock('2026-10-19T00:00:00Z')],
                'search':[clock('2026-10-19T00:00:00Z'),clock('2026-10-21T00:00:00Z')]},heldout_start=clock('2026-10-21T00:00:00Z'),minimum_free_bytes=1)

    def test_receipt_timing_orientation_and_exact_duplicate_removal(self):
        with tempfile.TemporaryDirectory()as d:
            r=Path(d);entries,add,poly,_=self.setup_source(r);add('forward_polymarket_002','copy.gz',[poly])
            m=self.run_compile(r,entries)
            with gzip.open(r/'output/references.jsonl.gz','rt')as f:rows=[json.loads(l)for l in f]
            self.assertEqual(len(rows),3);self.assertEqual(m['exactDuplicatesRemoved'],2)
            self.assertTrue(all(v['available_at_ms']==fixture.AT-1000 for v in rows))
            self.assertEqual(sorted(v['bid_yes_micros']for v in rows if v['venue']=='Polymarket'),[400000,580000])
            self.assertFalse(m['trainingAdmitted']);self.assertFalse(m['websocketDeltasReconstructed'])

    def test_incomplete_new_snapshot_rolls_back_every_row_before_fallback(self):
        with tempfile.TemporaryDirectory()as d:
            r=Path(d);entries,add,poly,_=self.setup_source(r)
            later=copy.deepcopy(poly);later['receipt']['receivedAt']='2026-10-19T04:15:02Z'
            original=(r/entries[-1]['archive_relative_path']).read_bytes()
            add('forward_polymarket_001','p.gz',[later],truncate=True,prefix=original)
            m=self.run_compile(r,entries)
            self.assertTrue(any(v['reason']=='incomplete_gzip_or_final_row_no_rows_admitted'for v in m['excludedSnapshots']))
            db=sqlite3.connect(r/'output/references.sqlite')
            try:self.assertEqual(db.execute('SELECT COUNT(*) FROM observations').fetchone()[0],3)
            finally:db.close()

    def test_changed_original_body_fails_and_never_seals_output(self):
        with tempfile.TemporaryDirectory()as d:
            r=Path(d);entries,add,poly,_=self.setup_source(r);poly['receipt']['bodyText']+=' '
            entries.pop();add('forward_polymarket_001','p.gz',[poly])
            with self.assertRaisesRegex(ValueError,'BODY_BYTES_CHANGED'):self.run_compile(r,entries)
            self.assertTrue((r/'output/FAILED.json').exists());self.assertFalse((r/'output/complete.json').exists())

    def test_filename_reuse_cannot_silently_discard_conflicting_history(self):
        with tempfile.TemporaryDirectory()as d:
            r=Path(d);entries,add,poly,_=self.setup_source(r)
            changed=copy.deepcopy(poly);changed['receipt']['receivedAt']='2026-10-19T04:15:02Z'
            add('forward_polymarket_001','p.gz',[changed])
            with self.assertRaisesRegex(ValueError,'PREFIX_CONFLICT'):self.run_compile(r,entries)
            self.assertFalse((r/'output/complete.json').exists())

    def test_heldout_capture_and_path_escape_fail_before_output_creation(self):
        with tempfile.TemporaryDirectory()as d:
            r=Path(d);entries,_,_,_=self.setup_source(r);entries[0]['captured_unix']=clock('2026-10-21T00:00:00Z')/1000
            with self.assertRaisesRegex(ValueError,'HELDOUT_ARCHIVE'):self.run_compile(r,entries)
            self.assertFalse((r/'output').exists())
            entries[0]['captured_unix']=fixture.AT/1000;entries[0]['archive_relative_path']='../outside'
            with self.assertRaisesRegex(ValueError,'OUTSIDE_ROOT'):self.run_compile(r,entries)
            self.assertFalse((r/'output').exists())


if __name__=='__main__':unittest.main()
