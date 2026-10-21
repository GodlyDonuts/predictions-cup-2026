import io,json,shutil,tarfile,tempfile,unittest
from pathlib import Path
from unittest.mock import patch
from .compile_prospective_source import sha
from .ingest_pretest_captures import ingest,FILES,FORMAT
from .test_compile_prospective_source import CompileCaptureTests
from .test_prospective_source import iso
from .selection import DataUse


class IngestCaptureTests(unittest.TestCase):
    def fixture(self,root,*,extra=None):
        original,_,_,session=CompileCaptureTests().fixture(root)
        package=root/'package';package.mkdir();source=root/'capture-source';source.mkdir()
        contents={'result.json':json.dumps({'exitCode':0,'financialExecution':False,'finishedUtc':iso(1599)}).encode(),
                  'capture/complete.json':json.dumps({'financialExecution':False,'trainingAdmitted':False,'until':iso(1598)}).encode(),
                  'capture/stream/manifest.json':session.read_bytes(),'capture/stream/stats.json':b'{}',
                  'capture/stream/events.jsonl':(original/'originals/events.jsonl').read_bytes(),
                  'capture/receipts.jsonl':(original/'originals/receipts.jsonl').read_bytes()}
        records={}
        with tarfile.open(package/'segments.tar.gz','w:gz')as f:
            for name,value in contents.items():
                p=source/name;p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes(value)
                f.add(p,arcname='segment-00000/'+name,recursive=False)
                records[name]={'bytes':len(value),'sha256':sha(p)}
            if extra:
                member=tarfile.TarInfo(extra);member.size=1;f.addfile(member,io.BytesIO(b'x'))
        manifest={'format':FORMAT,'accountRequests':0,'productionChanged':False,'independentTestUsed':False,
                  'archiveBytes':(package/'segments.tar.gz').stat().st_size,'archiveSha256':sha(package/'segments.tar.gz'),
                  'segments':[{'segment':0,'until':iso(1598),'finishedUtc':iso(1599),'files':records}]}
        return package,manifest

    def seal(self,package,manifest):
        (package/'manifest.json').write_text(json.dumps(manifest))
        pin=sha(package/'manifest.json');(package/'COMPLETE.json').write_text(json.dumps({'manifestSha256':pin}));return pin

    def run_ingest(self,root,package,pin,**kwargs):
        return ingest(package,pin,root/'output',bounds=(900,1600),heldout_start=2000,
                      data_use=DataUse(root/'registry'),**kwargs)

    @unittest.skipUnless(shutil.which('node'),'Node required for original producer audit')
    def test_original_node_producer_and_semantic_pipeline(self):
        with tempfile.TemporaryDirectory()as td:
            r=Path(td);package,m=self.fixture(r);v=self.run_ingest(r,package,self.seal(package,m))
            self.assertEqual(len(v['segments']),1);self.assertEqual(v['segments'][0]['exchanges'],1)
            self.assertTrue(v['segments'][0]['allInputsRehashedAfterRead']);self.assertFalse(v['trainingAdmitted'])
            self.assertFalse(v['independentTestUsed']);self.assertEqual(v['accountRequests'],0)

    def test_crossing_capture_rejected_before_archive_read(self):
        with tempfile.TemporaryDirectory()as td:
            r=Path(td);package,m=self.fixture(r);m['segments'][0]['finishedUtc']=iso(2000);pin=self.seal(package,m)
            (package/'segments.tar.gz').unlink()
            with self.assertRaisesRegex(ValueError,'WHOLE_CAPTURE_MUST_PRECEDE_TEST'):self.run_ingest(r,package,pin)
            self.assertFalse((r/'output').exists())

    def test_shared_calendar_blocks_before_archive_read(self):
        with tempfile.TemporaryDirectory()as td:
            r=Path(td);package,m=self.fixture(r);pin=self.seal(package,m)
            (package/'segments.tar.gz').unlink()
            with patch.object(DataUse,'record',side_effect=ValueError('PROTECTED_CALENDAR_OVERLAP')):
                with self.assertRaisesRegex(ValueError,'PROTECTED_CALENDAR'):self.run_ingest(r,package,pin)
            self.assertFalse((r/'output').exists())

    def test_unexpected_or_traversal_archive_member_is_rejected(self):
        for extra in ('../escaped','segment-00000/extra.json'):
            with tempfile.TemporaryDirectory()as td:
                r=Path(td);package,m=self.fixture(r,extra=extra);pin=self.seal(package,m)
                with self.assertRaisesRegex(ValueError,'EXACT_REGULAR_CAPTURE_MEMBER'):self.run_ingest(r,package,pin)
                self.assertFalse((r/'escaped').exists())

    def test_source_hash_change_rejected_before_decode(self):
        with tempfile.TemporaryDirectory()as td:
            r=Path(td);package,m=self.fixture(r);m['segments'][0]['files']['capture/receipts.jsonl']['sha256']='f'*64
            with self.assertRaisesRegex(ValueError,'CAPTURE_MEMBER_HASH_CHANGED'):
                self.run_ingest(r,package,self.seal(package,m))


if __name__=='__main__':unittest.main()
