import gzip,json,tarfile,tempfile,unittest
from pathlib import Path
from unittest.mock import patch
from .compile_prospective_source import compile_capture,sha
from .prospective_source import ProspectiveDecoder
from .test_prospective_source import rest,ws,book,iso
from .test_compile_breadth_index import encoded


def write(path,value):path.write_text(json.dumps(value,sort_keys=True));return sha(path)


class CompileCaptureTests(unittest.TestCase):
    def fixture(self,parent):
        root=parent/'raw';files=root/'originals';files.mkdir(parents=True)
        sub=dict(source='official_sig_realtime',kind='subscribed',marketId='1',receivedAt=iso(1000),requiresRestResync=True)
        event=ws(1400,book(11,1300));metadata=dict(rest(1200),kind='rest_reference')
        previous='';parts=[]
        for i,event in enumerate((sub,event,metadata),1):raw,previous=encoded(event,i,previous);parts.append(raw)
        original=files/'events.jsonl';original.write_bytes(b''.join(parts));canonical=parent/'canonical.jsonl';canonical.write_bytes(original.read_bytes())
        receipts=files/'receipts.jsonl';receipts.write_text(json.dumps(rest(1100))+'\n'+json.dumps(dict(rest(1200),kind='market'))+'\n')
        session=files/'session-manifest.json';session_pin=write(session,dict(tournamentId='cup',pairs=[{'exchangeId':'17','marketId':'1'}],tokenPersistence=False,contestOrders=0,paperOrders=0))
        archive=root/'originals.tar.gz'
        with tarfile.open(archive,'w:gz')as f:
            for p in(original,receipts):f.add(p,arcname=p.name)
        counts={'subscribed':1,'broadcast':1,'rest_reference':1}
        m=dict(format='original_completed_sig_forward_fixture_v1',trainingAdmitted=False,independentTestUsed=False,productionChanged=False,
            sourceStreamManifestSha256=session_pin,archiveSha256=sha(archive),files={p.name:{'sha256':sha(p)}for p in(original,receipts)},
            counts={'events.jsonl':counts,'receipts.jsonl':{'book':1,'market':1}})
        manifest=root/'complete.json';write(manifest,m);audit=parent/'audit.json'
        write(audit,dict(sourceOriginalSha256=sha(original),canonicalTransportSha256=sha(canonical),originalArchiveSha256=sha(archive),
            producerHashPreserved=True,rows=3,terminalProducerHash=previous,counts=counts))
        return root,canonical,audit,session

    def compile(self,parent,args,**kwargs):
        root,canonical,audit,session=args
        return compile_capture(root,sha(root/'complete.json'),canonical,audit,sha(audit),session,parent/'output',
            bounds=kwargs.get('bounds',(900,1600)),role=kwargs.get('role','search'),heldout_start=kwargs.get('cut',2000))

    def test_streamed_metadata_regression_original_scope_and_both_clock_channels(self):
        with tempfile.TemporaryDirectory()as td:
            p=Path(td);v=self.compile(p,self.fixture(p))
            with gzip.open(p/'output/observations.jsonl.gz','rt')as f:rows=[json.loads(line)for line in f]
            self.assertEqual([r['available_at_ms']for r in rows],[1000,1100,1400])
            self.assertEqual([r['kind']for r in rows],['gap','book','book']);self.assertEqual(rows[1]['source_at_ms'],900)
            self.assertEqual(v['originalTransportRows'],3);self.assertEqual(v['exchanges'],1)
            self.assertFalse(v['trainingAdmitted']);self.assertFalse(v['independentTestEligible'])
            with gzip.open(p/'output/observations.jsonl.gz','rb')as f:
                import hashlib
                self.assertEqual(hashlib.sha256(f.read()).hexdigest(),v['observationBodySha256'])

    def test_test_role_or_crossing_reserved_cut_is_rejected_before_output(self):
        for options in({'role':'test'},{'cut':1500}):
            with tempfile.TemporaryDirectory()as td:
                p=Path(td)
                with self.assertRaisesRegex(ValueError,'PRE_TEST'):self.compile(p,self.fixture(p),**options)
                self.assertFalse((p/'output').exists())

    def test_bad_producer_hash_is_rejected_even_when_canonical_digest_redeclared(self):
        with tempfile.TemporaryDirectory()as td:
            p=Path(td);args=self.fixture(p);_,canonical,audit,_=args
            canonical.write_bytes(canonical.read_bytes().replace(b'"quantity":7',b'"quantity":8',1))
            v=json.loads(audit.read_bytes());v['canonicalTransportSha256']=sha(canonical);write(audit,v)
            with self.assertRaisesRegex(ValueError,'PRODUCER_HASH_MISMATCH'):self.compile(p,args)
            self.assertFalse((p/'output/complete.json').exists())

    def test_changed_original_and_out_of_calendar_record_do_not_complete(self):
        for changed in(False,True):
            with tempfile.TemporaryDirectory()as td:
                p=Path(td);args=self.fixture(p)
                if changed:
                    with(args[0]/'originals/receipts.jsonl').open('ab')as f:f.write(b'changed')
                with self.assertRaises(ValueError):self.compile(p,args,bounds=(900,1400))
                self.assertFalse((p/'output/complete.json').exists())

    def test_contract_not_in_pinned_capture_session_is_rejected(self):
        with tempfile.TemporaryDirectory()as td:
            p=Path(td);args=self.fixture(p);root,_,_,session=args;v=json.loads(session.read_bytes());v['pairs'][0]['exchangeId']='99';pin=write(session,v)
            v=json.loads((root/'complete.json').read_bytes());v['sourceStreamManifestSha256']=pin;write(root/'complete.json',v)
            with self.assertRaisesRegex(ValueError,'BOOK_EXCHANGE_OUTSIDE_CAPTURE_SESSION'):self.compile(p,args)
            self.assertFalse((p/'output/complete.json').exists())

    def test_source_metadata_change_during_decode_prevents_completion(self):
        with tempfile.TemporaryDirectory()as td:
            p=Path(td);args=self.fixture(p);audit=args[2];original=ProspectiveDecoder.rest_book
            def mutate(decoder,event,pin):
                with audit.open('ab')as f:f.write(b' ')
                return original(decoder,event,pin)
            with patch.object(ProspectiveDecoder,'rest_book',mutate):
                with self.assertRaisesRegex(ValueError,'ORIGINAL_CAPTURE_INPUT_CHANGED'):self.compile(p,args)
            self.assertFalse((p/'output/complete.json').exists())
