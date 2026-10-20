import hashlib,json,tempfile,unittest
from pathlib import Path
from .audit_breadth_inventory import audit_file,audit,clock


def row(seq,previous,at,*,book=False):
    event={'source':'official_sig_realtime','event':'markets_batch'if book else'stream_reset',
           'receivedAt':at,'payload':{'markets':[{'batch':{'books':[{'exchangeId':'840','asOf':{'at':at}}]}}]}if book else{}}
    original={'event':event,'previous':previous,'sequence':seq}
    body=json.dumps(original,sort_keys=True,separators=(',',':')).encode()
    sha=hashlib.sha256(body).hexdigest()
    return(json.dumps(dict(original,hash=sha),sort_keys=True,separators=(',',':'))+'\n').encode(),sha


class InventoryTests(unittest.TestCase):
    def test_native_producer_empty_root_is_allowed_only_for_sequence_one(self):
        with tempfile.TemporaryDirectory()as d:
            p=Path(d)/'breadth-a.jsonl'
            for seq,status in((1,'verified_whole_file'),(2,'quarantined')):
                raw,_=row(seq,'','2026-10-15T00:00:00Z');p.write_bytes(raw)
                self.assertEqual(audit_file(p,clock('2026-10-21T00:00:00Z'))['status'],status)

    def test_whole_file_hash_and_chain_and_book_headers(self):
        with tempfile.TemporaryDirectory()as d:
            path=Path(d)/'breadth-a.jsonl'
            a,sha=row(1,'0'*64,'2026-10-15T00:00:00Z')
            b,_=row(2,sha,'2026-10-15T00:00:01Z',book=True);path.write_bytes(a+b)
            result=audit_file(path,clock('2026-10-21T00:00:00Z'))
            self.assertEqual(result['status'],'verified_whole_file')
            self.assertEqual(result['sha256'],hashlib.sha256(a+b).hexdigest())
            self.assertEqual(result['exchanges']['840']['rawBookOccurrences'],1)
            self.assertFalse(result['trainingAdmitted'])

    def test_partial_tail_or_broken_link_quarantines_whole_source(self):
        with tempfile.TemporaryDirectory()as d:
            path=Path(d)/'breadth-a.jsonl';a,sha=row(1,'0'*64,'2026-10-15T00:00:00Z')
            b,_=row(2,'f'*64,'2026-10-15T00:00:01Z',book=True)
            for data in(a+b,a+b'{"incomplete":'):
                path.write_bytes(data);r=audit_file(path,clock('2026-10-21T00:00:00Z'))
                self.assertEqual(r['status'],'quarantined');self.assertEqual(r['verifiedPrefixRows'],1)

    def test_cutoff_and_aggregate_exclude_quarantined_counts(self):
        with tempfile.TemporaryDirectory()as d:
            root=Path(d)/'raw';root.mkdir();a,sha=row(1,'0'*64,'2026-10-15T00:00:00Z')
            b,_=row(2,sha,'2026-10-21T00:00:00Z',book=True)
            (root/'breadth-good.jsonl').write_bytes(a);(root/'breadth-test.jsonl').write_bytes(a+b)
            r=audit(root,Path(d)/'audit',clock('2026-10-21T00:00:00Z'))
            self.assertEqual((r['verifiedWholeFiles'],r['quarantinedFiles'],r['verifiedRows']),(1,1,1))
            self.assertEqual(r['exchanges'],{})
            self.assertFalse(r['globalHistoryCompletenessEstablished'])

    def test_modified_payload_rejected_even_with_valid_json(self):
        with tempfile.TemporaryDirectory()as d:
            p=Path(d)/'breadth-a.jsonl';a,_=row(1,'0'*64,'2026-10-15T00:00:00Z')
            p.write_bytes(a.replace(b'stream_reset',b'changed_kind'))
            self.assertEqual(audit_file(p,clock('2026-10-21T00:00:00Z'))['status'],'quarantined')
