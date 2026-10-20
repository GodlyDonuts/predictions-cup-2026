import hashlib,json,sqlite3,tempfile,unittest
from pathlib import Path
from .audit_breadth_inventory import audit,clock
from .breadth_frame_index import build
from .test_audit_breadth_inventory import row


class FrameIndexTests(unittest.TestCase):
    def setup_source(self,root):
        source=root/'raw';source.mkdir();a,sha=row(1,'','2026-10-15T00:00:00Z')
        b,_=row(2,sha,'2026-10-15T00:00:01Z',book=True)
        (source/'breadth-a.jsonl').write_bytes(a+b);(source/'breadth-copy.jsonl').write_bytes(a+b)
        audit(source,root/'inventory',clock('2026-10-21T00:00:00Z'));p=root/'inventory/complete.json'
        return source,p,hashlib.sha256(p.read_bytes()).hexdigest(),(a,b)

    def test_original_byte_offsets_and_duplicate_copies(self):
        with tempfile.TemporaryDirectory()as d:
            root=Path(d);source,p,sha,rows=self.setup_source(root);r=build(source,p,sha,root/'index')
            self.assertEqual((r['sourceRows'],r['uniqueFrames'],r['identicalEnvelopeCopiesRemoved']),(4,2,2))
            db=sqlite3.connect(root/'index/frames.sqlite')
            try:
                for i,(name,offset,length)in enumerate(db.execute('SELECT s.name,f.offset,f.bytes FROM frames f JOIN files s ON s.i=f.file ORDER BY f.at')):
                    with(source/name).open('rb')as f:f.seek(offset);self.assertEqual(f.read(length),rows[i])
            finally:db.close()
            self.assertFalse(r['rawPayloadsDecoded']);self.assertFalse(r['trainingAdmitted'])

    def test_changed_source_cannot_publish_complete(self):
        with tempfile.TemporaryDirectory()as d:
            root=Path(d);source,p,sha,_=self.setup_source(root)
            q=source/'breadth-a.jsonl';q.write_bytes(q.read_bytes().replace(b'stream_reset',b'changed_kind'))
            with self.assertRaisesRegex(ValueError,'ORIGINAL_CHANGED'):build(source,p,sha,root/'index')
            self.assertFalse((root/'index/complete.json').exists())

    def test_changed_inventory_hash_is_rejected_before_output(self):
        with tempfile.TemporaryDirectory()as d:
            root=Path(d);source,p,sha,_=self.setup_source(root);p.write_bytes(p.read_bytes()+b' ')
            with self.assertRaisesRegex(ValueError,'PINNED'):build(source,p,sha,root/'index')
            self.assertFalse((root/'index').exists())
