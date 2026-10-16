"""Original-shaped inputs; no network, active cohort or outcome dataset."""
import base64
import gzip
import hashlib
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import crossvenue_observer as o
from test_crossvenue_observer import row, external


class Response(io.BytesIO):
    status = 200


def line(event, sequence=1, previous=''):
    unsigned = o.compact({'event': event, 'previous': previous, 'sequence': sequence})
    sha = hashlib.sha256(unsigned.encode()).hexdigest()
    return o.compact({'event': event, 'hash': sha, 'previous': previous, 'sequence': sequence}).encode()+b'\n'


class InputTraceTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory(); self.addCleanup(self.temp.cleanup)
        self.root=Path(self.temp.name); self.mapping=self.root/'universe.json'
        self.mapping.write_bytes(b'{"markets":[],"resolutionEquivalenceVerified":false}\n')
        self.output=self.root/'cohort'; self.output.mkdir()
        self.ticks=iter(range(50000,100000))
        self.disk=patch.object(o.shutil,'disk_usage',return_value=type('Disk',(),{'free':40*1024**3})())
        self.disk.start(); self.addCleanup(self.disk.stop)
        self.trace=o.InputTrace(self.output,self.mapping,{'samplingMode':'book'}, {'historyPrefixVerified':False}, clock=lambda:next(self.ticks))
        self.addCleanup(self.trace.close)

    def records(self):
        return [json.loads(x) for x in gzip.decompress((self.output/'input-trace-v1.jsonl.gz').read_bytes()).splitlines()]

    def finish(self, complete=True):
        return self.trace.seal(complete,'test_stop',{'historyPrefixVerified':False})

    def test_original_cache_bytes_no_sample_poll_and_exact_sampler_argument(self):
        raw=b'{ "source":"kalshi_official_v2", "markets":[],"originalFloat":1.2300 }\n'
        calls=[]
        class Study:
            def external_update(_,value,now): calls.append((value,now))
        o.trace_cache_poll(Study(),'/v1/quotes',self.trace,lambda request,timeout:Response(raw))
        rows=self.records(); events=[r['event'] for r in rows]
        body=next(e for e in events if e['kind']=='poll_body')
        self.assertEqual(base64.b64decode(body['bodyBytesBase64']),raw)
        self.assertEqual(body['bodySha256'],hashlib.sha256(raw).hexdigest())
        begin=next(e for e in events if e['kind']=='apply_begin')
        done=next(e for e in events if e['kind']=='apply_complete')
        self.assertEqual(calls[0][1],begin['samplerNowMs'])
        self.assertGreater(done['completedAtMs'],begin['samplerNowMs'])
        self.assertEqual(done['emittedRowsBefore'],done['emittedRowsAfter'])
        self.assertEqual(events[-1]['kind'],'poll_complete')
        seal=self.finish(); self.assertTrue(seal['complete']); self.assertFalse(seal['wholeWitnessVerified'])
        previous=''
        for i,r in enumerate(rows,1):
            self.assertEqual((r['sequence'],r['previous']),(i,previous))
            self.assertEqual(r['hash'],hashlib.sha256(o.compact({k:v for k,v in r.items() if k!='hash'}).encode()).hexdigest()); previous=r['hash']
        self.assertEqual(previous,seal['traceHead'])
        for f in seal['files']:self.assertEqual(hashlib.sha256(Path(f['path']).read_bytes()).hexdigest(),f['sha256'])

    def test_failed_poll_is_recorded_without_error_text_or_fabricated_body(self):
        def fail(*args,**kwargs):raise OSError('private diagnostic must not be retained')
        with self.assertRaises(OSError):o.trace_cache_poll(None,'/v1/ws/quotes',self.trace,fail)
        events=[r['event'] for r in self.records()]
        self.assertEqual(events[-1]['kind'],'poll_failed'); self.assertEqual(events[-1]['failureKind'],'transport')
        self.assertFalse(any(e['kind']=='poll_body' for e in events)); self.assertNotIn('private diagnostic',o.compact(events))
        self.assertTrue(self.finish()['complete']) # Complete capture includes this genuine failed poll.

    def test_malformed_original_body_retained_but_unfinished_apply_cannot_seal_complete(self):
        raw=b'{malformed original'
        with self.assertRaises(json.JSONDecodeError):o.trace_cache_poll(None,'/v1/quotes',self.trace,lambda *a,**k:Response(raw))
        body=next(r['event'] for r in self.records() if r['event']['kind']=='poll_body')
        self.assertEqual(base64.b64decode(body['bodyBytesBase64']),raw)
        self.assertFalse(self.finish()['complete'])

    def test_sig_raw_bytes_offsets_exact_argument_and_reset_type(self):
        source=self.root/'breadth.jsonl'; original=line({'type':'stream_reset','float':1.25})
        source.write_bytes(original)
        tail=o.Tail(source,from_start=True); self.addCleanup(tail.close)
        called=[]
        for value in tail.read():self.trace.consume_sig(tail,value,lambda v,n:called.append((v,n)),1234)
        event=next(r['event'] for r in self.records() if r['event']['kind']=='apply_begin')
        self.assertEqual(event['samplerNowMs'],1234); self.assertEqual(called[0][1],1234)
        ref=event['input']; self.assertEqual((ref['offset'],ref['bytes']),(0,len(original)))
        self.assertEqual(ref['rawLineSha256'],hashlib.sha256(original).hexdigest()); self.assertEqual(ref['originalEventType'],'stream_reset')
        self.assertFalse(ref['bookmark']['historyPrefixVerified']); self.assertEqual(source.read_bytes(),original)

    def test_initial_partial_line_is_skipped_without_claiming_prefix(self):
        source=self.root/'partial.jsonl'; source.write_bytes(b'old unfinished')
        tail=o.Tail(source); self.addCleanup(tail.close)
        first=tail.bookmark(); self.assertTrue(first['skipPartialLine']); self.assertFalse(first['historyPrefixVerified'])
        original=line({'type':'gap'},7,'older-chain')
        with source.open('ab') as f:f.write(b' completion\n'+original)
        values=list(tail.read()); self.assertEqual(len(values),1)
        self.assertEqual(tail.last_row_offset,len(b'old unfinished completion\n'))
        self.assertEqual(tail.last_row_bytes,original); self.assertEqual(values[0]['sequence'],7)

    def test_apply_failure_is_not_completed_and_cannot_be_a_healthy_seal(self):
        def fail(value,now):raise ValueError('source invalid')
        with self.assertRaises(ValueError):self.trace.apply('sig_row',{},fail,{},42)
        self.assertEqual(self.records()[-1]['event']['kind'],'apply_failed')
        self.assertFalse(self.finish()['complete'])

    def test_samples_preserve_unwrapped_default_bytes_and_counts(self):
        study=o.Study(self.trace.emit,horizons=(1000,3000)); expected=[]; default=o.Study(expected.append,horizons=(1000,3000))
        for s in (study,default):
            s.external_update(external(9000),10000); s.accept(row(10000),10000)
            s.external_update(external(11000),11000); s.accept(row(11000,2,bid='0.430000',ask='0.450000'),11000)
        raw=gzip.decompress((self.output/'trace-samples-v1.jsonl.gz').read_bytes())
        self.assertEqual(raw,('\n'.join(map(o.compact,expected))+'\n').encode())
        self.assertEqual(hashlib.sha256(raw).hexdigest(),'475a5499e3705ade2da67942c853b94ccd488be18c3d53211b84f99bd4b428cb')
        self.assertEqual(self.finish()['outputRows'],3)

    def test_cap_checks_compressed_combined_files_before_write_no_deletion(self):
        self.trace.emit({'kind':'sample','at':100})
        before={p.name:p.read_bytes() for p in self.output.glob('*.gz')}
        self.trace.MAXIMUM_BYTES=self.trace.bytes+self.trace.SEAL_RESERVE+1
        with self.assertRaisesRegex(o.InputTraceError,'TRACE_COMPRESSED_LIMIT'):self.trace.record({'kind':'next'})
        self.assertEqual(before,{p.name:p.read_bytes() for p in self.output.glob('*.gz')})
        self.assertFalse(self.finish()['complete'])

    def test_low_free_space_is_fatal_not_swallowed_as_cache_io(self):
        before=self.trace.bytes
        with patch.object(o.shutil,'disk_usage',return_value=type('Disk',(),{'free':0})()):
            with self.assertRaises(o.InputTraceError) as caught:self.trace.record({'kind':'next'})
        self.assertNotIsInstance(caught.exception,OSError); self.assertEqual(self.trace.bytes,before)
        self.assertFalse(self.finish()['complete'])

    def test_fsync_failure_poisoned_no_false_success_and_no_retry(self):
        with patch.object(o.os,'fsync',side_effect=OSError('disk')):
            with self.assertRaisesRegex(o.InputTraceError,'TRACE_DURABILITY_FAILURE'):self.trace.record({'kind':'next'})
        with self.assertRaises(o.InputTraceError):self.trace.emit({'kind':'sample'})
        self.assertFalse(self.finish()['complete'])

    def test_new_cohort_and_regular_original_mapping_required(self):
        with self.assertRaises(FileExistsError):o.InputTrace(self.output,self.mapping,{'samplingMode':'book'}, {})
        with self.assertRaises(o.InputTraceError):o.trace_file_reference(self.root)
        self.assertTrue((self.output/'input-trace-v1.jsonl.gz').exists())

    def test_real_trade_trigger_input_is_bound_to_original_body_and_no_future_label(self):
        from test_trade_triggered_observer import envelope
        # A separate explicit new trade cohort, never changing an existing run.
        output=self.root/'trade-cohort'; output.mkdir()
        trace=o.InputTrace(output,self.mapping,{'samplingMode':'trade'}, {},clock=lambda:40010)
        self.addCleanup(trace.close)
        study=o.Study(trace.emit,sampling_mode='trade'); study.accept(row(39950),39950)
        original=o.compact(envelope()).encode()
        o.trace_cache_poll(study,'/v1/ws/quotes',trace,lambda *a,**k:Response(original))
        samples=[json.loads(x) for x in gzip.decompress((output/'trace-samples-v1.jsonl.gz').read_bytes()).splitlines()]
        self.assertEqual(len(samples),1); self.assertEqual(samples[0]['kind'],'decision_features')
        self.assertEqual(samples[0]['decisionMs'],40010)
        self.assertEqual(samples[0]['trigger']['observerFirstAvailableMs'],40010)
        self.assertFalse(any(s['kind']=='horizon_label' for s in samples))
        rows=[json.loads(x) for x in gzip.decompress((output/'input-trace-v1.jsonl.gz').read_bytes()).splitlines()]
        body=next(r['event'] for r in rows if r['event']['kind']=='poll_body')
        self.assertEqual(base64.b64decode(body['bodyBytesBase64']),original)
        complete=next(r['event'] for r in rows if r['event']['kind']=='apply_complete')
        self.assertEqual((complete['emittedRowsBefore'],complete['emittedRowsAfter']),(0,1))
        self.assertTrue(trace.seal(True,'test',{})['complete'])

    def test_mapping_changed_and_same_size_trace_tamper_never_seal_complete(self):
        self.mapping.write_bytes(b'changed-original-mapping')
        self.assertFalse(self.finish()['complete'])
        # Hashes are computed over original compressed bytes, not parsed JSON.
        out=self.root/'tampered'; out.mkdir()
        trace=o.InputTrace(out,self.mapping,{'samplingMode':'book'}, {})
        self.addCleanup(trace.close)
        path=out/'input-trace-v1.jsonl.gz'; raw=bytearray(path.read_bytes()); raw[-1]^=1
        with path.open('r+b') as f:f.write(raw)
        seal=trace.seal(True,'test',{})
        self.assertFalse(seal['complete']); self.assertFalse(seal['storedBytesMatchWrittenMembers'])

    def test_partial_write_poisoned_no_complete_even_if_valid_prefix_precedes_it(self):
        write=o.os.write
        count=[0]
        def partial(fd,value):
            count[0]+=1
            if count[0]==1:return write(fd,value[:7])
            raise OSError('disk full')
        with patch.object(o.os,'write',side_effect=partial):
            with self.assertRaises(o.InputTraceError):self.trace.record({'kind':'uncommitted'})
        seal=self.finish()
        self.assertFalse(seal['complete']); self.assertFalse(seal['storedBytesMatchWrittenMembers'])
        self.assertEqual(seal['traceSequence'],1)

    def test_actual_main_traced_cohort_keeps_route_order_consumption_and_terminal_seal(self):
        import sys
        import types
        journal=self.root/'main-source.jsonl'; journal.write_bytes(b'')
        target=self.root/'main-cohort'; calls=[]
        def fetch(request,timeout):
            calls.append(request.full_url)
            if len(calls)==1:
                with journal.open('ab') as f:f.write(line(row(10000)['event']))
            return Response(o.compact(external(9000)).encode())
        argv=['observer','--journal',str(journal),'--output',str(target),'--duration-seconds','10',
              '--input-trace','--trace-mapping',str(self.mapping)]
        fake=types.SimpleNamespace(LOCK_EX=1,LOCK_NB=2,flock=lambda *a:None)
        monotonic=[2]
        with patch.dict(sys.modules,{'fcntl':fake}), patch.object(sys,'argv',argv), patch.object(o.urllib.request,'urlopen',fetch), \
             patch.object(o.time,'monotonic',side_effect=lambda:monotonic[0]), patch.object(o.time,'time',return_value=10), \
             patch.object(o.time,'sleep',side_effect=lambda _:monotonic.__setitem__(0,20)), patch.object(o.signal,'signal'):
            o.main()
        self.assertEqual(calls,['http://127.0.0.1:17841/v1/quotes','http://127.0.0.1:17841/v1/ws/quotes'])
        seal=json.loads((target/'input-trace-seal.json').read_text())
        self.assertTrue(seal['complete']); self.assertEqual(seal['finalBookmark']['lastSequence'],1)
        records=[json.loads(x)['event'] for x in gzip.decompress((target/'input-trace-v1.jsonl.gz').read_bytes()).splitlines()]
        applies=[r for r in records if r['kind']=='apply_begin']
        self.assertEqual([r['inputKind'] for r in applies],['cache_body','cache_body','sig_row'])
        self.assertEqual([r['samplerNowMs'] for r in applies],[10000]*3)
        self.assertEqual(json.loads((target/'status.json').read_text())['state'],'stopped')

    def test_frozen_protocol_matches_source_limits_and_non_authority(self):
        path=Path(__file__).resolve().parents[2]/'research/moloch-trade-trigger-20261016/witness/input-trace-protocol-v1.json'
        protocol=json.loads(path.read_text(encoding='utf-8'))
        self.assertEqual(protocol['capture'],o.INPUT_TRACE_PROTOCOL)
        self.assertEqual(protocol['samplingPolicySha256'],o.TRADE_SAMPLING_POLICY_SHA256)
        self.assertFalse(protocol['wholeWitnessVerifiedByCaptureAlone'])


if __name__=='__main__':unittest.main()
