import copy
import gzip
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest
import verify as v

spec=importlib.util.spec_from_file_location('fixture_analysis',Path(__file__).resolve().parents[1]/'test_analyze.py')
f=importlib.util.module_from_spec(spec);spec.loader.exec_module(f)


def event(at,version,bid='.43',ask='.45',source=None,revision=None):
    return {'source':'official_sig_realtime','event':'markets_batch','receivedAt':f.stamp(at),'payload':{'markets':[{'marketId':'1','batch':{
        'delivery':{'model':'best-effort-authoritative-resync','revision':revision or version,'previousRevision':(revision or version)-1},
        'trades':[],'bookDirty':[],'marketSettled':[],
        'books':[{'exchangeId':101,'asOf':{'sequence':version,'at':f.stamp(source if source is not None else at)},'nextExpiryAt':None,
                  'bids':[{'price':float(bid),'quantity':10}],'asks':[{'price':float(ask),'quantity':10}]}]}}]}}


def chain(events):
    rows=[]; previous=''
    for i,e in enumerate(events,1):
        row={'event':e,'sequence':i,'previous':previous};row['hash']=v.sha(v.a.compact(row).encode());previous=row['hash'];rows.append(row)
    return rows


class Fixture:
    def __init__(self):
        self.tmp=tempfile.TemporaryDirectory();self.root=Path(self.tmp.name)
        self.sample=f.sample(complete=False)
        r=self.sample['trigger']['record']
        body={'type':'trade','sid':r['sid'],'seq':r['sequence'],'msg':{'market_ticker':r['ticker'],'trade_id':r['tradeId'],'ts_ms':r['sourceTsMs'],
              'count_fp':r['quantity'],'yes_price_dollars':r['yesPrice'],'no_price_dollars':r['noPrice'],'taker_side':'yes','taker_outcome_side':'yes','taker_book_side':'bid','is_block_trade':False}}
        text=v.a.compact(body)+'\n';h=v.sha(text.encode())
        self.collector=[{'kind':'websocket','source':'kalshi_official_ws_v2','type':'trade','sid':r['sid'],'sequence':r['sequence'],
                         'receivedAt':f.stamp(r['receivedMs']),'bodyText':text,'bodySha256':h,'body':body}]
        flow=self.sample['externalTradeFlow']
        for w in flow['sourceSnapshot']['windows'].values():
            for record in w['evidence']:record['bodySha256']=h
            w['evidenceSha256']=f.evidence_sha(w['evidence'])
        flow['sourceSnapshotSha256']=v.a.digest(flow['sourceSnapshot']);self.sample['trigger']['sourceSnapshotSha256']=flow['sourceSnapshotSha256']
        self.sample['trigger']['record']=copy.deepcopy(flow['sourceSnapshot']['windows']['1000']['evidence'][0])
        self.segments=[chain([event(99950,1,'.4','.42'),event(101000,2)])]
        self.labels=[];self.bind()

    def bind(self, label_segment=0, label_index=1):
        original=self.segments[0][0];self.sample['sig']['journalHash']=original['hash'];self.sample['sig']['journalSequence']=original['sequence'];self.sample['sig']['sourceAt']=original['event']['payload']['markets'][0]['batch']['books'][0]['asOf']['at'];self.sample['sig']['receivedAt']=original['event']['receivedAt'];f.rebind(self.sample)
        original=self.segments[label_segment][label_index]
        book=original['event']['payload']['markets'][0]['batch']['books'][0]
        label=f.label(self.sample);label.update(sigJournalHash=original['hash'],sourceAt=book['asOf']['at'],receivedAt=original['event']['receivedAt'],observedAt=f.stamp(v.o.millis(original['event']['receivedAt'])+1))
        label['features']=v.sig_segment(('\n'.join(v.a.compact(r)for r in self.segments[label_segment])+'\n').encode(),False,str(label_segment))[-1]['features']
        label['logitMidpointChange']=v.a.logit(label['features']['midpoint'])-v.a.logit(self.sample['sig']['features']['midpoint'])
        label['yesOneShareTakerRoundTrip']=str(v.a.decimal(label['features']['bid'])-v.a.decimal(self.sample['sig']['features']['ask']))
        label['noOneShareTakerRoundTrip']=str(v.a.decimal(self.sample['sig']['features']['bid'])-v.a.decimal(label['features']['ask']))
        self.labels=[label]

    def write(self, trace=None):
        def file(name,rows):
            p=self.root/name;p.write_bytes(gzip.compress((''.join(v.a.compact(r)+'\n' for r in rows)).encode(),mtime=0))
            return {'path':p.name,'bytes':p.stat().st_size,'sha256':v.sha(p.read_bytes())}
        m={'schemaVersion':1,'sealed':True,'cohortId':'synthetic-original-membership','asOfUtc':f.stamp(200000),
           'sampleFiles':[file('samples.gz',[self.sample]+self.labels)],
           'sigSegments':[{'segmentId':str(i),'file':file(f'sig-{i}.gz',rows)}for i,rows in enumerate(self.segments)],
           'collectorFiles':[file('collector.gz',self.collector)],'observerInputTrace':trace}
        p=self.root/'manifest.json';p.write_text(v.a.compact(m),encoding='utf-8')
        return p,v.sha(p.read_bytes())

    def verify(self,trace=None):return v.verify(*self.write(trace))


class OriginalWitnessTests(unittest.TestCase):
    def fixture(self):
        x=Fixture();self.addCleanup(x.tmp.cleanup);return x

    def test_original_bodies_and_first_recorded_book_verified_but_missing_consumption_never_qualifies(self):
        result=self.fixture().verify()
        self.assertEqual(result['sampleMembershipVerified'],1)
        self.assertEqual(result['labelRecordedOrder'][0]['firstRecordedBook'],'verified_first_in_recorded_segment')
        self.assertFalse(result['wholeWitnessVerified']);self.assertFalse(result['eligibleForModelValidation'])
        self.assertIn('missing',result['observerFirstAvailability']);self.assertFalse(result['executionAuthority'])

    def test_self_consistent_sample_cannot_replace_missing_original_trade(self):
        x=self.fixture();x.collector=[]
        with self.assertRaisesRegex(ValueError,'MISSING_TRADE_BODY'):x.verify()

    def test_original_raw_body_hash_required_even_when_manifest_is_rebound(self):
        x=self.fixture();x.collector[0]['bodyText']+=' '
        with self.assertRaisesRegex(ValueError,'TRADE_BODY'):x.verify()

    def test_conflicting_original_trade_identity_rejected_before_sample_selection(self):
        x=self.fixture();other=copy.deepcopy(x.collector[0]);other['body']['msg']['count_fp']='2.500000';other['bodyText']=v.a.compact(other['body']);other['bodySha256']=v.sha(other['bodyText'].encode());x.collector.append(other)
        with self.assertRaisesRegex(ValueError,'TRADE_IDENTITY_CONFLICT'):x.verify()

    def test_missing_chain_prefix_cannot_be_declared_complete_by_manifest(self):
        x=self.fixture();x.segments[0]=x.segments[0][1:]
        with self.assertRaisesRegex(ValueError,'SIG_CHAIN'):x.verify()

    def test_membership_rejects_same_bbo_with_different_original_quantity(self):
        x=self.fixture();r=x.segments[0][0];e=copy.deepcopy(r['event']);e['payload']['markets'][0]['batch']['books'][0]['bids'][0]['quantity']=11
        x.segments[0]=chain([e,x.segments[0][1]['event']]);x.sample['sig']['journalHash']=x.segments[0][0]['hash'];f.rebind(x.sample);x.labels[0]['sampleId']=x.sample['sampleId']
        with self.assertRaisesRegex(ValueError,'SIG_ORIGINAL_MISMATCH'):x.verify()

    def test_later_better_book_cannot_replace_first_original_eligible_book(self):
        x=self.fixture();x.segments=[chain([event(99950,1,'.4','.42'),event(101000,2),event(101100,3,'.44','.46')])];x.bind(label_index=2)
        with self.assertRaisesRegex(ValueError,'FIRST_RECORDED_BOOK_MISMATCH'):x.verify()

    def test_late_received_old_source_is_not_mistaken_for_first_future_book(self):
        x=self.fixture();x.segments=[chain([event(99950,1,'.4','.42'),event(101100,2,source=100900),event(101200,3)])];x.bind(label_index=2)
        self.assertEqual(x.verify()['labelRecordedOrder'][0]['firstRecordedBook'],'verified_first_in_recorded_segment')

    def test_gap_preserves_body_membership_but_withholds_first_consumed_proof(self):
        x=self.fixture();reset={'source':'official_sig_realtime','event':'stream_reset','receivedAt':f.stamp(100500),'payload':{'localBoundary':True}}
        x.segments=[chain([event(99950,1,'.4','.42'),reset,event(101000,2)])];x.bind(label_index=2)
        self.assertEqual(x.verify()['labelRecordedOrder'][0]['firstRecordedBook'],'unavailable_recorded_boundary')
        x.labels[0]['sigJournalHash']='e'*64
        with self.assertRaisesRegex(ValueError,'MISSING_OR_AMBIGUOUS_LABEL_BODY'):x.verify()

    def test_rotated_independent_chains_do_not_invent_cross_segment_continuity(self):
        x=self.fixture();x.segments=[chain([event(99950,1,'.4','.42')]),chain([event(101000,2)])];x.bind(label_segment=1,label_index=0)
        self.assertIn('unavailable',x.verify()['labelRecordedOrder'][0]['firstRecordedBook'])

    def test_same_version_conflicting_depth_is_rejected_not_silently_ignored(self):
        first=event(99950,1,'.4','.42');conflict=event(99950,1,'.4','.42',revision=2);conflict['payload']['markets'][0]['batch']['books'][0]['asks'][0]['quantity']=11
        raw=('\n'.join(v.a.compact(r)for r in chain([first,conflict]))+'\n').encode()
        with self.assertRaisesRegex(ValueError,'SIG_VERSION_CONFLICT'):v.sig_segment(raw,False,'0')

    def test_plain_json_complete_trace_flag_cannot_mint_observer_availability(self):
        with self.assertRaisesRegex(ValueError,'TRACE_SCHEMA_NOT_IMPLEMENTED'):self.fixture().verify({'complete':True,'firstAvailable':100000})


if __name__=='__main__':unittest.main()
