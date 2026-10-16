"""Synthetic original-shaped sealed captures; never reads an actual cohort."""
import base64
import gzip
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

HERE=Path(__file__).resolve().parent
sys.path.insert(0,str(HERE.parents[2]/'moloch/external-data'))
import trace_replay as v
from test_crossvenue_observer import row,external,websocket
O=v.known_sampler()

class Response(io.BytesIO):status=200

def pin(path):return {'path':str(path.resolve()),'sha256':v.sha(path.read_bytes()),'bytes':path.stat().st_size}

def rawline(event,sequence,previous):
    unsigned=O.compact({'event':event,'sequence':sequence,'previous':previous})
    return (O.compact({'event':event,'hash':v.sha(unsigned.encode()),'sequence':sequence,'previous':previous})+'\n').encode()

class Fixture:
    def __init__(self,root,mode='book',format='native',initial=b'',after_open=b'',sampler=None):
        self.sampler=sampler or O
        S=self.sampler
        self.root=root;self.out=root/'capture';self.out.mkdir()
        self.mapping=root/'universe.json';self.mapping.write_text('{"markets":[],"pricingAuthority":false}',encoding='utf-8')
        self.source=root/'source.jsonl';self.source.write_bytes(initial);self.tail=S.Tail(self.source)
        if after_open:
            with self.source.open('ab') as out:out.write(after_open)
        self.clock=10000
        self.config={'samplingMode':mode,'format':format,'journal':str(self.source),'durationSeconds':10,
                     'horizonsMs':[1000,3000,10000,30000,120000],'sigMaximumAgeMs':3000,'externalMaximumAgeMs':20000,'labelSlackMs':3000}
        with patch.object(O.shutil,'disk_usage',return_value=type('D',(),{'free':40*1024**3})()):
            self.trace=S.InputTrace(self.out,self.mapping,self.config,self.tail.bookmark(),clock=lambda:self.clock)
        self.study=S.Study(self.trace.emit,sampling_mode=mode)
        self.source_paths=[self.source];self.identities={str(self.source):self.tail.identity}
    def polls(self):
        for route,body in [('/v1/quotes',external(self.clock-1000)),('/v1/ws/quotes',websocket(self.clock-1000))]:
            O.trace_cache_poll(self.study,route,self.trace,lambda *a,body=body,**k:Response(O.compact(body).encode()))
    def sig(self,at,seq=1,previous='',book_sequence=None,**kwargs):
        event=row(at,seq,**kwargs)['event']
        if book_sequence is not None:event['book']['sequence']=str(book_sequence)
        with self.source.open('ab') as stream:stream.write(rawline(event,seq,previous))
        for item in self.tail.read():self.trace.consume_sig(self.tail,item,self.study.breadth if self.config['format']=='breadth' else self.study.accept,self.clock)
    def close(self):self.trace.close();self.tail.close()
    def finish(self):
        seal=self.trace.seal(True,'normal_stop',self.tail.bookmark())
        self.manifest={'version':1,'seal':pin(self.out/'input-trace-seal.json'),'trace':pin(self.out/'input-trace-v1.jsonl.gz'),
            'samples':pin(self.out/'trace-samples-v1.jsonl.gz'),'producer':pin(Path(self.sampler.__file__)),'mapping':pin(self.mapping),
            'sigSources':[{**pin(p),'recordedPath':str(p),'recordedDevice':self.identities[str(p)][0],
                           'recordedInode':self.identities[str(p)][1]} for p in self.source_paths]}
        self.save();return seal
    def save(self):
        self.path=self.root/'manifest.json';self.path.write_text(O.compact(self.manifest)+'\n',encoding='utf-8');self.hash=v.sha(self.path.read_bytes())
    def verify(self):return v.verify(self.path,self.hash)
    def mutate_trace(self,fn):
        rows=[json.loads(x) for x in gzip.decompress(Path(self.manifest['trace']['path']).read_bytes()).splitlines()]
        fn(rows)
        previous=''
        for i,r in enumerate(rows,1):
            r['sequence']=i;r['previous']=previous;r['hash']=v.sha(O.compact({k:value for k,value in r.items() if k!='hash'}).encode());previous=r['hash']
        path=Path(self.manifest['trace']['path']);path.write_bytes(gzip.compress(('\n'.join(map(O.compact,rows))+'\n').encode(),mtime=0))
        seal_path=Path(self.manifest['seal']['path']);seal=json.loads(seal_path.read_text())
        seal['traceSequence']=len(rows);seal['traceHead']=previous
        for ref in seal['files']:
            if ref['path'].endswith('input-trace-v1.jsonl.gz'):ref.update(sha256=pin(path)['sha256'],bytes=pin(path)['bytes'])
        seal_path.write_text(O.compact(seal)+'\n',encoding='utf-8')
        self.manifest['trace']=pin(path);self.manifest['seal']=pin(seal_path);self.save()

class ReplayTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.addCleanup(self.temp.cleanup)
        self.root=Path(self.temp.name)
        self.disk=patch.object(O.shutil,'disk_usage',return_value=type('D',(),{'free':40*1024**3})());self.disk.start();self.addCleanup(self.disk.stop)
    def fixture(self,**kwargs):
        f=Fixture(self.root,**kwargs);self.addCleanup(f.close);return f
    def valid(self):
        f=self.fixture();f.polls();f.sig(10000);previous=f.tail.previous
        f.clock=11000;f.polls();f.sig(11000,2,previous,bid='0.430000',ask='0.450000');f.finish();return f
    def test_actual_known_sampler_replays_exact_inputs_first_book_and_all_outputs(self):
        f=self.valid();result=f.verify()
        self.assertTrue(result['samplerInputReplayVerified']);self.assertEqual(result['counts']['sigApplies'],2)
        self.assertEqual(result['outputRows'],3);self.assertTrue(result['firstConsumedBookReplayVerified']);self.assertFalse(result['wholeWitnessVerified']);self.assertFalse(result['eligibleForModelValidation'])
        self.assertEqual(result['collectorProcessingEpochProof'],'unavailable')
    def test_no_sample_inputs_and_failed_poll_are_original_missingness_not_zero(self):
        f=self.fixture()
        def fail(*a,**k):raise OSError('unretained')
        with self.assertRaises(OSError):O.trace_cache_poll(f.study,'/v1/quotes',f.trace,fail)
        f.sig(10000);f.finish();r=f.verify()
        self.assertEqual(r['outputRows'],0);self.assertEqual(r['counts']['failedPolls'],1);self.assertEqual(r['counts']['sigApplies'],1)
    def test_unpinned_or_mutated_trace_is_refused(self):
        f=self.valid()
        with self.assertRaisesRegex(v.Rejected,'INPUT_HASH'):v.verify(f.path,'0'*64)
        path=Path(f.manifest['trace']['path']);path.write_bytes(path.read_bytes()+b'x')
        with self.assertRaisesRegex(v.Rejected,'INPUT_HASH'):f.verify()
    def test_even_resealed_wrong_output_is_refused_by_actual_callback(self):
        f=self.valid();p=Path(f.manifest['samples']['path']);rows=[json.loads(x) for x in gzip.decompress(p.read_bytes()).splitlines()]
        rows[0]['decisionMs']+=1;p.write_bytes(gzip.compress(('\n'.join(map(O.compact,rows))+'\n').encode(),mtime=0))
        seal_path=Path(f.manifest['seal']['path']);seal=json.loads(seal_path.read_text())
        for ref in seal['files']:
            if ref['path'].endswith('trace-samples-v1.jsonl.gz'):ref.update(sha256=pin(p)['sha256'],bytes=pin(p)['bytes'])
        seal_path.write_text(O.compact(seal)+'\n');f.manifest['samples']=pin(p);f.manifest['seal']=pin(seal_path);f.save()
        with self.assertRaisesRegex(v.Rejected,'EMITTED_ROW_MISMATCH'):f.verify()
    def test_resealed_changed_sampler_now_cannot_keep_original_outputs(self):
        f=self.valid()
        def mutate(rows):
            e=next(r['event'] for r in rows if r['event'].get('inputKind')=='sig_row');e['samplerNowMs']+=1;e['recordedAtMs']+=1
        f.mutate_trace(mutate)
        with self.assertRaisesRegex(v.Rejected,'EMITTED_ROW_MISMATCH'):f.verify()
    def test_skipping_first_consumed_horizon_book_for_later_book_is_refused(self):
        f=self.fixture();f.polls();f.sig(10000)
        for now in (11000,12000):
            previous=f.tail.previous;f.clock=now;f.polls();f.sig(now,now//1000-9,previous,bid='0.430000',ask='0.450000')
        f.finish()
        def mutate(rows):
            sigs=[r['event'] for r in rows if r['event'].get('inputKind')=='sig_row']
            sigs[1]['input']=sigs[2]['input']
        f.mutate_trace(mutate)
        with self.assertRaisesRegex(v.Rejected,'SIG_CONSUMPTION_GAP'):f.verify()

    def test_source_offset_or_body_substitution_rejected_even_with_new_chain(self):
        f=self.valid()
        def mutate(rows):next(r['event'] for r in rows if r['event'].get('inputKind')=='sig_row')['input']['offset']+=1
        f.mutate_trace(mutate)
        with self.assertRaisesRegex(v.Rejected,'SIG_CONSUMPTION_GAP'):f.verify()
    def test_missing_raw_sig_source_is_not_a_trusted_boolean(self):
        f=self.valid();f.manifest['sigSources']=[];f.save()
        with self.assertRaisesRegex(v.Rejected,'SIG_BOOKMARK_SOURCE'):f.verify()
    def test_pending_completion_cannot_be_fixed_by_complete_true(self):
        f=self.valid()
        f.mutate_trace(lambda rows:rows.pop())
        with self.assertRaisesRegex(v.Rejected,'PENDING_TRACE_INPUT|SEAL_FRONTIER'):f.verify()
    def test_cache_body_hash_mismatch_and_reversed_routes_refused(self):
        f=self.valid()
        def mutate(rows):next(r['event'] for r in rows if r['event']['kind']=='poll_body')['bodySha256']='0'*64
        f.mutate_trace(mutate)
        with self.assertRaisesRegex(v.Rejected,'CACHE_BODY_HASH'):f.verify()
    def test_decompression_line_bound_and_duplicate_json_keys(self):
        f=self.valid()
        with patch.object(v,'MAX_LINE',100):
            with self.assertRaisesRegex(v.Rejected,'DECOMPRESS_BOUND'):f.verify()
        with self.assertRaisesRegex(v.Rejected,'DUPLICATE_JSON_KEY'):v.loads('{"a":1,"a":2}')
    def test_real_trade_trigger_and_duplicate_poll_replay_do_not_add_future_labels(self):
        from test_trade_triggered_observer import envelope
        f=self.fixture(mode='trade');f.clock=39950;f.sig(39950);f.clock=40010
        for route,body in [('/v1/quotes',external(39000)),('/v1/ws/quotes',envelope())]:
            O.trace_cache_poll(f.study,route,f.trace,lambda *a,body=body,**k:Response(O.compact(body).encode()))
        f.clock=40110
        for route,body in [('/v1/quotes',external(39000)),('/v1/ws/quotes',envelope())]:
            O.trace_cache_poll(f.study,route,f.trace,lambda *a,body=body,**k:Response(O.compact(body).encode()))
        f.finish();result=f.verify()
        self.assertEqual(result['outputRows'],1);self.assertEqual(result['samplingMode'],'trade')
        self.assertFalse(result['firstConsumedBookReplayVerified']);self.assertEqual(result['firstConsumedBookProof'],'unavailable_no_horizon_label')
        self.assertEqual(result['counts']['cacheApplies'],4)

    def test_independent_rotation_with_unread_original_bytes_is_censored(self):
        f=self.fixture(format='breadth')
        event={'source':'official_sig_realtime','event':'stream_reset'}
        with f.source.open('ab') as out:out.write(rawline(event,1,''))
        for item in f.tail.read():f.trace.consume_sig(f.tail,item,f.study.breadth,f.clock)
        with f.source.open('ab') as out:out.write(rawline(event,2,f.tail.previous))
        old=f.tail.bookmark();f.tail.close()
        f.source=self.root/'source-part-0001.jsonl';f.source.write_bytes(b'');f.tail=O.Tail(f.source,from_start=True)
        f.source_paths.append(f.source);f.identities[str(f.source)]=f.tail.identity
        f.trace.record({'kind':'sig_rotation','previousBookmark':old,'nextBookmark':f.tail.bookmark(),
                        'observedAtMs':f.clock,'crossSegmentContinuityVerified':False})
        with f.source.open('ab') as out:out.write(rawline(event,1,''))
        for item in f.tail.read():f.trace.consume_sig(f.tail,item,f.study.breadth,f.clock)
        f.finish();r=f.verify()
        self.assertEqual(r['counts']['rotations'],1);self.assertEqual(len(r['unconsumedSourceObservations']),1)
        self.assertFalse(r['crossSegmentContinuityVerified']);self.assertFalse(r['wholeWitnessVerified'])

    def test_real_append_between_tail_open_and_first_bookmark_preserves_original_cursor(self):
        original=rawline(row(10000)['event'],1,'')
        f=self.fixture(after_open=original);f.polls()
        # The real Tail was opened at EOF0, then a real row arrived before its
        # first bookmark. It must consume that row at0, never jump to new EOF.
        for item in f.tail.read():f.trace.consume_sig(f.tail,item,f.study.accept,f.clock)
        f.finish();r=f.verify()
        self.assertEqual(r['counts']['sigApplies'],1);self.assertEqual(r['outputRows'],1)
        self.assertFalse(r['historyPrefixVerified'])
        trace=[json.loads(x) for x in gzip.decompress(Path(f.manifest['trace']['path']).read_bytes()).splitlines()]
        bookmark=trace[0]['event']['initialBookmark']
        self.assertEqual(bookmark['readOffset'],0);self.assertEqual(bookmark['fileBytesAtObservation'],len(original))
        ref=next(r['event']['input'] for r in trace if r['event'].get('inputKind')=='sig_row')
        self.assertEqual(ref['offset'],0)

    def test_initial_partial_line_skip_matches_original_offset_without_prefix_claim(self):
        f=self.fixture(initial=b'old partial')
        with f.source.open('ab') as out:out.write(b' completion\n')
        f.sig(10000);f.finish();r=f.verify()
        self.assertEqual(r['counts']['sigApplies'],1);self.assertFalse(r['historyPrefixVerified'])

    def test_reversed_route_and_completion_ordinal_rejected_after_reseal(self):
        f=self.valid()
        def mutate(rows):next(r['event'] for r in rows if r['event']['kind']=='poll_begin')['route']='/v1/ws/quotes'
        f.mutate_trace(mutate)
        with self.assertRaisesRegex(v.Rejected,'POLL_ORDER'):f.verify()

    def test_apply_output_range_is_not_a_caller_success_assertion(self):
        f=self.valid()
        def mutate(rows):next(r['event'] for r in rows if r['event']['kind']=='apply_complete')['emittedRowsAfter']+=1
        f.mutate_trace(mutate)
        with self.assertRaisesRegex(v.Rejected,'APPLY_COMPLETION'):f.verify()

    def test_no_manifest_selected_unreviewed_code_can_execute(self):
        f=self.valid();fake=self.root/'evil.py';fake.write_text('raise RuntimeError("must not run")')
        f.manifest['producer']=pin(fake);f.save()
        with self.assertRaisesRegex(v.Rejected,'UNREVIEWED_CAPTURE_PRODUCER'):f.verify()
    def test_changed_mapping_or_incomplete_seal_denied(self):
        f=self.valid();seal_path=Path(f.manifest['seal']['path']);seal=json.loads(seal_path.read_text());seal['complete']=False
        seal_path.write_text(O.compact(seal));f.manifest['seal']=pin(seal_path);f.save()
        with self.assertRaisesRegex(v.Rejected,'INCOMPLETE_SEAL'):f.verify()

    def test_fixed_versions_replay_same_sequence_new_read_differently_without_reinterpreting_old(self):
        results={}
        for name,producer,expected_rows in [('historical',v.HISTORICAL_LF,1),('historical_crlf',v.HISTORICAL_CRLF,1),('new_read_version',v.BOOK_VERSION_LF,3)]:
            root=self.root/name;root.mkdir()
            sampler=v.known_sampler(producer)
            # A capture's producer is an exact pinned INPUT. Replay still loads
            # only its fixed repository module, not this temporary data path.
            captured=root/'captured-producer.py'
            raw=Path(sampler.__file__).read_bytes().replace(b'\r\n',b'\n')
            if producer==v.HISTORICAL_CRLF:raw=raw.replace(b'\n',b'\r\n')
            self.assertEqual(v.sha(raw),producer)
            captured.write_bytes(raw);sampler.__file__=str(captured)
            f=Fixture(root,sampler=sampler);self.addCleanup(f.close)
            f.polls();f.sig(10000);f.clock=11000
            f.sig(11000,2,f.tail.previous,book_sequence=1)
            f.finish();result=f.verify();results[name]=result
            self.assertEqual(result['outputRows'],expected_rows)
            self.assertEqual(result['producerSha256'],producer)
            self.assertFalse(result['wholeWitnessVerified']);self.assertFalse(result['eligibleForModelValidation'])
        self.assertFalse(results['historical']['firstConsumedBookReplayVerified'])
        self.assertTrue(results['new_read_version']['firstConsumedBookReplayVerified'])

    def test_historical_lf_and_known_crlf_select_same_fixed_archived_sampler(self):
        first=v.known_sampler(v.HISTORICAL_LF);second=v.known_sampler(v.HISTORICAL_CRLF)
        self.assertEqual(Path(first.__file__),Path(second.__file__))
        self.assertIn('known-samplers',Path(first.__file__).parts)
        for module in (first,second):
            study=module.Study(lambda _:None,sampling_mode='trade')
            study.accept(row(10000),10000);study.accept(row(11000),11000)
            self.assertEqual(study.counts['sigBooks'],1)

    def test_current_source_cannot_substitute_for_historical_trusted_bytes(self):
        old=Path(O.__file__);current=Path(v.known_sampler(v.BOOK_VERSION_LF).__file__)
        original=Path.read_bytes;replacement=current.read_bytes()
        with patch.object(Path,'read_bytes',lambda p:replacement if p==old else original(p)):
            with self.assertRaisesRegex(v.Rejected,'LOCAL_PRODUCER_CHANGED|UNREVIEWED_LOCAL_PRODUCER'):v.known_sampler(v.HISTORICAL_LF)

    def test_mutated_exact_repository_bytes_refused_for_each_selected_version(self):
        for producer in (v.HISTORICAL_LF,v.BOOK_VERSION_LF):
            target=Path(v.known_sampler(producer).__file__);original=Path.read_bytes
            raw=target.read_bytes();changed=raw.replace(b'no network',b'NO NETWORK',1)
            if changed==raw:changed=b'#'+raw[1:]
            self.assertNotEqual(v.sha(changed),v.sha(raw))
            with patch.object(Path,'read_bytes',lambda p:changed if p==target else original(p)):
                with self.assertRaisesRegex(v.Rejected,'UNREVIEWED_LOCAL_PRODUCER'):v.known_sampler(producer)

    def test_unreviewed_capture_hash_and_manifest_code_path_never_select_a_module(self):
        with self.assertRaisesRegex(v.Rejected,'UNREVIEWED_CAPTURE_PRODUCER'):v.known_sampler('0'*64)
        with self.assertRaisesRegex(v.Rejected,'UNREVIEWED_CAPTURE_PRODUCER'):v.known_sampler(str(self.root/'unreviewed.py'))

if __name__=='__main__':unittest.main()
