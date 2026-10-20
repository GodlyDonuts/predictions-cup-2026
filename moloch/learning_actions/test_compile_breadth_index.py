import gzip,hashlib,json,tempfile,unittest
from pathlib import Path
from .audit_breadth_inventory import audit,clock
from .breadth_frame_index import build
from .compile_breadth_index import compile_index
from .test_breadth_source import frame


def encoded(event,seq,previous):
    row={'event':event,'sequence':seq,'previous':previous}
    body=json.dumps(row,sort_keys=True,separators=(',',':')).encode()
    sha=hashlib.sha256(body).hexdigest()
    return(json.dumps(dict(row,hash=sha),sort_keys=True,separators=(',',':'))+'\n').encode(),sha


class CompileIndexedFramesTests(unittest.TestCase):
    def source(self,root,*,duplicate_print=False,changed_print=False):
        raw=root/'raw';raw.mkdir()
        event={'source':'official_sig_realtime','receivedAt':'2026-10-15T08:59:59Z','event':'stream_reset','payload':{}}
        first=frame('2026-10-15T09:00:00Z');second=frame('2026-10-17T09:00:00Z')
        if duplicate_print:
            trade=dict(exchangeId=2,marketId=1,tournamentId='cup',id='match',sequence=4,
                executedAt='2026-10-15T08:59:59Z',quantity=5,price='.45')
            first['payload']['markets'][0]['batch']['trades']=[trade]
            second['payload']['markets'][0]['batch']['trades']=[dict(trade,quantity=6 if changed_print else 5)]
        a,h=encoded(event,1,'');b,_=encoded(first,2,h)
        c,_=encoded(second,1,'')
        (raw/'breadth-a.jsonl').write_bytes(a+b);(raw/'breadth-b.jsonl').write_bytes(c)
        audit(raw,root/'audit',clock('2026-10-21T00:00:00Z'))
        p=root/'audit/complete.json';build(raw,p,hashlib.sha256(p.read_bytes()).hexdigest(),root/'index')
        return raw,hashlib.sha256((root/'index/complete.json').read_bytes()).hexdigest()

    def test_streamed_roles_gap_fence_and_exact_depth(self):
        with tempfile.TemporaryDirectory()as d:
            root=Path(d);raw,sha=self.source(root)
            result=compile_index(raw,root/'index',sha,root/'output',tournament='cup',train_end=clock('2026-10-17T00:00:00Z'))
            self.assertEqual(result['frames'],3)
            self.assertEqual(result['counts']['derived_chain_boundaries'],1)
            self.assertEqual(result['counts']['fit/book'],1);self.assertEqual(result['counts']['search/book'],1)
            with gzip.open(root/'output/observations.jsonl.gz','rt')as f:rows=[json.loads(l)for l in f]
            self.assertEqual([r['kind']for r in rows],['gap','book','gap','book'])
            self.assertEqual(rows[1]['bids'],[[400000,20]])
            self.assertFalse(result['trainingAdmitted']);self.assertFalse(result['externalObservationsIncluded'])

    def test_changed_original_fails_without_complete_artifact(self):
        with tempfile.TemporaryDirectory()as d:
            root=Path(d);raw,sha=self.source(root);p=raw/'breadth-a.jsonl'
            p.write_bytes(p.read_bytes().replace(b'stream_reset',b'changed_kind'))
            with self.assertRaises(ValueError):compile_index(raw,root/'index',sha,root/'output',tournament='cup',train_end=clock('2026-10-17T00:00:00Z'))
            self.assertTrue((root/'output/FAILED.json').exists());self.assertFalse((root/'output/complete.json').exists())

    def test_outside_role_cut_is_rejected(self):
        with tempfile.TemporaryDirectory()as d:
            root=Path(d);raw,sha=self.source(root)
            with self.assertRaisesRegex(ValueError,'CHRONOLOGICAL'):
                compile_index(raw,root/'index',sha,root/'output',tournament='cup',train_end=clock('2026-10-22T00:00:00Z'))
            self.assertFalse((root/'output').exists())

    def test_disk_reserve_is_checked_before_creating_output(self):
        with tempfile.TemporaryDirectory()as d:
            root=Path(d);raw,sha=self.source(root)
            with self.assertRaisesRegex(ValueError,'DISK_HEADROOM'):
                compile_index(raw,root/'index',sha,root/'output',tournament='cup',
                    train_end=clock('2026-10-17T00:00:00Z'),minimum_free_bytes=2**80)
            self.assertFalse((root/'output').exists())

    def test_disk_fact_deduplication_does_not_replay_recovered_old_print(self):
        with tempfile.TemporaryDirectory()as d:
            root=Path(d);raw,sha=self.source(root,duplicate_print=True)
            r=compile_index(raw,root/'index',sha,root/'output',tournament='cup',train_end=clock('2026-10-17T00:00:00Z'))
            self.assertEqual(r['counts']['fit/trade'],1);self.assertEqual(r['counts']['duplicate_engine_prints'],1)
            with gzip.open(root/'output/trade-flow.jsonl.gz','rt')as f:records=[json.loads(x)for x in f]
            self.assertEqual(len(records),1);self.assertEqual(records[0]['quantity'],5)

    def test_conflicting_duplicate_print_fails_closed(self):
        with tempfile.TemporaryDirectory()as d:
            root=Path(d);raw,sha=self.source(root,duplicate_print=True,changed_print=True)
            with self.assertRaisesRegex(ValueError,'MATCH_FACTS_CHANGED'):
                compile_index(raw,root/'index',sha,root/'output',tournament='cup',train_end=clock('2026-10-17T00:00:00Z'))
            self.assertFalse((root/'output/complete.json').exists())
