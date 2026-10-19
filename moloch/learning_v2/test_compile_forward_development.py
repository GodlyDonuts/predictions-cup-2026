import base64
import gzip
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from .compile_forward_development import native_rows,pin_prefix,gzip_rows,compile_development
from .test_native_capture import encoded,sealed,h


class ForwardPrefixTests(unittest.TestCase):
    def test_complete_three_venue_fixture_emits_verified_fit_and_search_only(self):
        from .reference_adapters import millis
        with tempfile.TemporaryDirectory()as d:
            p=Path(d);k=p/'kalshi';poly=p/'poly';(k/'journal').mkdir(parents=True);(poly/'journal').mkdir(parents=True)
            times=['2000-01-01T00:00:30Z','2000-01-01T00:02:30Z'];identity={'assetId':'token','conditionId':'condition','outcome':'Yes',
                'sigReferences':[{'sigMarketId':'1','sigSide':'YES','relationship':'same_party_conditional'}]}
            ku=json.dumps({'markets':[{'ticker':'k','sigMappings':[{'sigMarketId':'1','side':'YES'}]}]}).encode()
            pu=json.dumps({'outcomes':[identity]}).encode();(k/'universe.json').write_bytes(ku);(p/'poly-universe').write_bytes(pu)
            head=sealed({'sequence':20,'previous':'a'*64,'manifestHash':'b'*64})
            anchor={'kind':'native_anchor','sequence':20,'previous':'a'*64,'start_offset':100,'file':'events.jsonl','identity':[1,2],
                'head_raw_base64':base64.b64encode(head).decode(),'head_raw_sha256':h(head),'producer_pid':1,'producer_start_ticks':'t'}
            envelopes=[(anchor,'2000-01-01T00:00:00Z')];previous='a'*64;offset=100;kr=[];pr=[]
            for i,at in enumerate(times):
                e={'type':'book','receivedAt':at,'processedAt':at,'book':{'receivedAt':at,'sourceAt':at,'marketId':'1','exchangeId':'x','sequence':i+1,
                    'bids':[{'price':'0.4','quantity':10}],'asks':[{'price':'0.6','quantity':10}]}}
                original=sealed({'event':e,'sequence':21+i,'previous':previous})+b'\n';previous=json.loads(original)['hash']
                envelopes.append(({'kind':'native_row','file':'events.jsonl','identity':[1,2],'start_offset':offset,'end_offset':offset+len(original),
                    'original_sequence':21+i,'original_hash':previous,'raw_sha256':h(original),'raw_base64':base64.b64encode(original).decode()},at));offset+=len(original)
                def receipt(body):
                    text=json.dumps(body);return {'status':200,'requestStartedAt':at,'receivedAt':at,'bodyText':text,'bodySha256':h(text.encode()),'body':body}
                kr.append(json.dumps({'kind':'book','ticker':'k',**receipt({'orderbook_fp':{'yes_dollars':[['0.4','5']],'no_dollars':[['0.4','8']]}})}).encode()+b'\n')
                pr.append(json.dumps({'identity':identity,'receipt':receipt({'asset_id':'token','market':'condition','timestamp':str(millis(at)),
                    'bids':[{'price':'0.4','size':'5'}],'asks':[{'price':'0.6','size':'8'}]})}).encode()+b'\n')
            previous='0'*64;transport=[]
            for i,(e,at)in enumerate(envelopes):
                raw=sealed({'sequence':i+1,'previous':previous,'observed_at_ms':millis(at),'observed_monotonic_ns':i,'event':e})+b'\n'
                previous=json.loads(raw)['hash'];transport.append(raw)
            (p/'native').write_bytes(b''.join(transport));(k/'journal/20000101T00-fixture.ndjson.gz').write_bytes(gzip.compress(b''.join(kr)))
            (poly/'journal/books-fixture.ndjson.gz').write_bytes(gzip.compress(b''.join(pr)))
            protocol={'calendarRolesUtc':{'fit':['2000-01-01T00:00:00Z','2000-01-01T00:01:00Z'],
                'search':['2000-01-01T00:02:00Z','2000-01-01T00:03:00Z'],'heldout':['2000-01-01T00:04:00Z','2000-01-01T00:05:00Z']},
                'nativeProducer':{'pid':1,'startTicks':'t'},'minimumFitObservedSigExchanges':1,'minimumFitMappedReferenceMarketsPerExternalVenue':1}
            raw=json.dumps(protocol).encode();(p/'protocol').write_bytes(raw);pin={'protocolSha256':h(raw),'kalshiUniverseSha256':h(ku),
                'polymarketUniverseSha256':h(pu),'polymarketUniversePath':str(p/'poly-universe'),'maximumBoundaryOrObservationGapMs':60000}
            (p/'pin').write_text(json.dumps(pin));manifest=compile_development(p/'native',k,[poly],p/'protocol',p/'pin',p/'output')
            complete=json.loads((p/'output/COMPLETE.json').read_text());self.assertEqual(complete['manifestSha256'],h((p/'output/dataset-manifest.json').read_bytes()))
            with gzip.open(p/'output/observations.jsonl.gz','rt')as f:rows=[json.loads(line)for line in f]
            self.assertEqual(len(rows),6);self.assertEqual({r['venue']for r in rows},{'SIG','Kalshi','Polymarket'})
            self.assertEqual({r['calendar_role']for r in rows},{'fit','search'});self.assertFalse(manifest['heldoutFeaturesIncluded']);self.assertEqual(manifest['accountRequests'],0)
            self.assertEqual((p/'output/originals/kalshi-universe.original.json').read_bytes(),ku)

    def test_exact_prefix_is_copied_and_closed_gzip_truncation_is_not_accepted(self):
        with tempfile.TemporaryDirectory()as d:
            p=Path(d);src=p/'source';src.write_bytes(b'original\n');pin=pin_prefix(src,p/'copy')
            self.assertEqual(pin['sha256'],hashlib.sha256(b'original\n').hexdigest());self.assertEqual((p/'copy').read_bytes(),src.read_bytes())
            g=p/'partial.gz';g.write_bytes(gzip.compress(b'{"x":1}\n')[:-4])
            self.assertEqual(list(gzip_rows(g,True)),[b'{"x":1}\n'])
            with self.assertRaises(EOFError):list(gzip_rows(g,False))

    def test_native_chain_is_verified_but_heldout_prices_are_not_decoded(self):
        head=sealed({'sequence':20,'previous':'a'*64,'manifestHash':'b'*64})
        anchor={'kind':'native_anchor','sequence':20,'previous':'a'*64,'start_offset':100,'file':'events.jsonl','identity':[1,2],
                'head_raw_base64':base64.b64encode(head).decode(),'head_raw_sha256':h(head)}
        envelopes=[anchor];offset=100;previous='a'*64
        for i,(at,price)in enumerate([('2026-10-19T04:15:01Z','0.4'),('2026-10-19T05:25:01Z','DO_NOT_INTERPRET_HELDOUT_PRICE')]):
            event={'type':'book','receivedAt':at,'processedAt':at,'book':{'receivedAt':at,'sourceAt':at,'marketId':'1','exchangeId':'x','sequence':i+1,
                    'bids':[{'price':price,'quantity':10}],'asks':[{'price':'0.6','quantity':10}]}}
            raw=sealed({'sequence':21+i,'previous':previous,'event':event})+b'\n';value=json.loads(raw)
            envelopes.append({'kind':'native_row','file':'events.jsonl','identity':[1,2],'start_offset':offset,'end_offset':offset+len(raw),
                'original_sequence':21+i,'original_hash':value['hash'],'raw_sha256':h(raw),'raw_base64':base64.b64encode(raw).decode()})
            offset+=len(raw);previous=value['hash']
        rows=[];previous='0'*64
        from .reference_adapters import millis
        for i,e in enumerate(envelopes):
            at=millis('2026-10-19T04:15:01Z'if i<2 else'2026-10-19T05:25:01Z')
            raw=sealed({'sequence':i+1,'previous':previous,'observed_at_ms':at,'observed_monotonic_ns':i,'event':e})+b'\n'
            previous=json.loads(raw)['hash'];rows.append(raw)
        roles={'fit':(millis('2026-10-19T04:15:00Z'),millis('2026-10-19T05:00:00Z')),
               'search':(millis('2026-10-19T05:05:00Z'),millis('2026-10-19T05:20:00Z')),
               'heldout':(millis('2026-10-19T05:25:00Z'),millis('2026-10-19T05:55:00Z'))}
        with tempfile.TemporaryDirectory()as d:
            p=Path(d)/'capture';p.write_bytes(b''.join(rows));observations=[];proof=native_rows(p,roles,observations.append)
            self.assertEqual(len(observations),1);self.assertEqual(observations[0]['bids'],[[400000,10]])
            self.assertEqual(proof['nativeRowsVerified'],2)
            mutated=json.loads(rows[0]);mutated['sequence']=2;p.write_text(json.dumps(mutated)+'\n')
            with self.assertRaisesRegex(ValueError,'NATIVE_RECEIVER_CHAIN'):native_rows(p,roles,lambda _:None)

    def test_future_cohort_cannot_be_compiled_early(self):
        with tempfile.TemporaryDirectory()as d:
            p=Path(d);protocol={'calendarRolesUtc':{'fit':['2099-01-01T00:00:00Z','2099-01-01T01:00:00Z'],
                'search':['2099-01-01T01:05:00Z','2099-01-01T01:20:00Z']}};raw=json.dumps(protocol).encode();(p/'protocol').write_bytes(raw)
            (p/'pin').write_text(json.dumps({'protocolSha256':h(raw)}))
            with self.assertRaisesRegex(ValueError,'SEARCH_CAPTURE_NOT_COMPLETE_YET'):
                compile_development(p/'absent',p/'absent',[],p/'protocol',p/'pin',p/'output')
            self.assertFalse((p/'output').exists())

    def test_retrospective_producer_clock_preserves_laptop_receipts_and_excludes_heldout_prices(self):
        from .reference_adapters import millis
        head=sealed({'sequence':20,'previous':'a'*64,'manifestHash':'b'*64})
        anchor={'kind':'native_anchor','sequence':20,'previous':'a'*64,'start_offset':100,'file':'events.jsonl','identity':[1,2],
            'head_raw_base64':base64.b64encode(head).decode(),'head_raw_sha256':h(head)}
        events=[(anchor,'2000-01-01T00:01:00Z')];offset=100;previous='a'*64
        for i,(processed,received,price)in enumerate([
            ('2000-01-01T00:00:15Z','2000-01-01T00:01:15Z','0.4'),
            ('2000-01-01T00:00:30Z','2000-01-01T00:01:30Z','DO_NOT_INTERPRET_HELDOUT')]):
            e={'type':'book','receivedAt':processed,'processedAt':processed,'book':{'receivedAt':processed,'sourceAt':processed,
                'marketId':'1','exchangeId':'x','sequence':i+1,'bids':[{'price':price,'quantity':10}],
                'asks':[{'price':'0.6','quantity':10}]}}
            original=sealed({'sequence':21+i,'previous':previous,'event':e})+b'\n';previous=json.loads(original)['hash']
            events.append(({'kind':'native_row','file':'events.jsonl','identity':[1,2],'start_offset':offset,
                'end_offset':offset+len(original),'original_sequence':21+i,'original_hash':previous,
                'raw_sha256':h(original),'raw_base64':base64.b64encode(original).decode()},received));offset+=len(original)
        previous='0'*64;rows=[]
        for i,(event,at)in enumerate(events):
            raw=sealed({'sequence':i+1,'previous':previous,'observed_at_ms':millis(at),'event':event})+b'\n'
            previous=json.loads(raw)['hash'];rows.append(raw)
        roles={k:tuple(map(millis,v))for k,v in {'fit':['2000-01-01T00:00:10Z','2000-01-01T00:00:20Z'],
            'search':['2000-01-01T00:00:40Z','2000-01-01T00:00:50Z'],
            'heldout':['2000-01-01T00:00:25Z','2000-01-01T00:00:35Z']}.items()}
        with tempfile.TemporaryDirectory()as d:
            p=Path(d)/'capture';p.write_bytes(b''.join(rows));observations=[]
            native_rows(p,roles,observations.append)
            self.assertEqual(observations,[])
            report=native_rows(p,roles,observations.append,'producer_processed')
            self.assertEqual(len(observations),1)
            self.assertEqual(observations[0]['available_at_ms'],millis('2000-01-01T00:00:15Z'))
            self.assertEqual(observations[0]['receiver_observed_at_ms'],millis('2000-01-01T00:01:15Z'))
            self.assertIn('nativeAvailabilityClock',report)

    def test_producer_clock_requires_explicit_retrospective_protocol(self):
        with tempfile.TemporaryDirectory()as d:
            p=Path(d);protocol={'nativeAvailabilityClock':'producer_processed'};raw=json.dumps(protocol).encode()
            (p/'protocol').write_bytes(raw);(p/'pin').write_text(json.dumps({'protocolSha256':h(raw)}))
            with self.assertRaisesRegex(ValueError,'RETROSPECTIVE_PRODUCER_CLOCK_PROTOCOL'):
                compile_development(p/'absent',p/'absent',[],p/'protocol',p/'pin',p/'output')


if __name__=='__main__':unittest.main()
