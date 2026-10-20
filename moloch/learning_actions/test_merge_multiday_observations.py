import gzip,json,tempfile,unittest
from collections import Counter
from pathlib import Path
from . import test_compile_breadth_index as sig_fixture
from . import test_compile_external_archive as external_fixture
from .compile_breadth_index import compile_index
from .compile_external_archive import compile_archive
from .merge_multiday_observations import merge
from .breadth_frame_index import sha
from .audit_breadth_inventory import clock
from .disk_tape import build


class MergeMultidayTests(unittest.TestCase):
    def setup_source(self,r):
        s=r/'sig';s.mkdir();raw,pin=sig_fixture.CompileIndexedFramesTests().source(s,duplicate_print=True)
        compile_index(raw,s/'index',pin,s/'normalized',tournament='cup',train_end=clock('2026-10-17T00:00:00Z'))
        e=r/'external';e.mkdir();f=external_fixture.ExternalArchiveTests();entries,*_=f.setup_source(e)
        (e/'index.jsonl').write_text(''.join(json.dumps(v)+'\n'for v in entries))
        compile_archive(e,e/'index.jsonl',sha(e/'index.jsonl'),e/'ku.json',sha(e/'ku.json'),
            e/'pu.json',sha(e/'pu.json'),e/'output',roles={
                'fit':[clock('2026-10-01T00:00:00Z'),clock('2026-10-17T00:00:00Z')],
                'search':[clock('2026-10-17T00:00:00Z'),clock('2026-10-21T00:00:00Z')]},
            heldout_start=clock('2026-10-21T00:00:00Z'),minimum_free_bytes=1)
        return s/'normalized',e/'output'

    def test_exact_row_bytes_gaps_roles_and_trade_bytes_survive_disk_cache(self):
        with tempfile.TemporaryDirectory()as d:
            r=Path(d);s,e=self.setup_source(r)
            m=merge(s,sha(s/'complete.json'),e,sha(e/'complete.json'),r/'merged',heldout_start=clock('2026-10-21T00:00:00Z'),minimum_free_bytes=1)
            def rows(p):
                with gzip.open(p,'rb')as f:return list(f)
            original=rows(s/'observations.jsonl.gz')+rows(e/'references.jsonl.gz');actual=rows(r/'merged/base/observations.jsonl.gz')
            self.assertEqual(Counter(original),Counter(actual));times=[json.loads(v)['available_at_ms']for v in actual]
            self.assertEqual(times,sorted(times));self.assertEqual(len(actual),7)
            self.assertEqual((r/'merged/trade-flow.jsonl.gz').read_bytes(),(s/'trade-flow.jsonl.gz').read_bytes())
            self.assertEqual(m['sourceCoverage']['fit/Kalshi']['rows'],0);self.assertFalse(m['trainingAdmitted'])
            build(r/'merged',r/'cache');cm=json.loads((r/'cache/manifest.json').read_bytes())
            self.assertEqual(cm['counts']['market'],7);self.assertEqual(cm['counts']['world'],4);self.assertEqual(cm['counts']['flow'],1)

    def test_changed_source_and_reserved_calendar_fail_before_output(self):
        with tempfile.TemporaryDirectory()as d:
            r=Path(d);s,e=self.setup_source(r);sp=sha(s/'complete.json');ep=sha(e/'complete.json')
            with self.assertRaisesRegex(ValueError,'NON_HELDOUT'):
                merge(s,sp,e,ep,r/'merged',heldout_start=clock('2026-10-18T00:00:00Z'),minimum_free_bytes=1)
            self.assertFalse((r/'merged').exists())
            p=e/'references.jsonl.gz';p.write_bytes(p.read_bytes()+b'changed')
            with self.assertRaisesRegex(ValueError,'BYTES_CHANGED'):
                merge(s,sp,e,ep,r/'merged',heldout_start=clock('2026-10-21T00:00:00Z'),minimum_free_bytes=1)
            self.assertFalse((r/'merged').exists())


if __name__=='__main__':unittest.main()
