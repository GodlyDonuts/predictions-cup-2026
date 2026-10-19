import copy,hashlib,json,gzip,base64,tempfile,unittest
from pathlib import Path
from .breadth_source import Decoder,frames,ORIGIN,wire_amount
from .snapshot_clock import source_clock


def frame(at='2026-10-19T09:00:02Z',revision=10,previous=9):
    return {'source':'official_sig_realtime','receivedAt':at,'event':'markets_batch','payload':{'markets':[
        {'marketId':1,'batch':{'delivery':{'model':'best-effort-authoritative-resync','revision':revision,'previousRevision':previous},
        'books':[{'exchangeId':2,'asOf':{'at':at,'sequence':3},'bids':[{'price':'.4','quantity':20}],
            'asks':[{'price':'.5','quantity':20}],'nextExpiryAt':None}],
        'trades':[],'bookDirty':[],'marketSettled':[]}}]}}


AT=1792400402000


class BreadthSourceTests(unittest.TestCase):
    def test_wire_float_tick_noise_has_explicit_tolerance_not_arbitrary_rounding(self):
        self.assertEqual(wire_amount(.41000000000000003),410000)
        with self.assertRaisesRegex(ValueError,'TICK_PRICE'):wire_amount(.41001)

    def test_raw_snapshot_not_filtered_by_old_native_readiness(self):
        rows,prints=Decoder('cup').decode(frame(),'a'*64,AT,'fit')
        self.assertEqual(len(rows),1);self.assertFalse(prints);self.assertEqual(rows[0]['bids'],[[400000,20]])
        self.assertEqual(rows[0]['snapshot_origin'],ORIGIN);self.assertEqual(source_clock(rows[0]),AT)
        self.assertNotIn('native_processed_at_ms',rows[0])

    def test_one_sided_book_is_preserved_without_invented_depth(self):
        e=frame();e['payload']['markets'][0]['batch']['books'][0]['bids']=[]
        rows,_=Decoder('cup').decode(e,'a'*64,AT,'fit')
        self.assertEqual(rows[0]['bids'],[]);self.assertEqual(rows[0]['asks'],[[500000,20]])

    def test_delivery_hole_fences_missing_state_but_keeps_observed_full_snapshot(self):
        d=Decoder('cup');d.decode(frame(),'a'*64,AT,'fit')
        rows,_=d.decode(frame(revision=13,previous=12),'b'*64,AT,'fit')
        self.assertEqual([r['kind']for r in rows],['gap','book'])
        self.assertEqual(rows[0]['market_id'],'1')

    def test_same_source_conflict_and_future_book_fail(self):
        d=Decoder('cup');d.decode(frame(),'a'*64,AT,'fit');e=frame(revision=11,previous=10)
        e['payload']['markets'][0]['batch']['books'][0]['bids'][0]['quantity']=21
        with self.assertRaisesRegex(ValueError,'CONFLICT'):d.decode(e,'b'*64,AT,'fit')
        e=frame();e['payload']['markets'][0]['batch']['books'][0]['asOf']['at']='2026-10-19T09:00:03Z'
        with self.assertRaisesRegex(ValueError,'CLOCK'):Decoder('cup').decode(e,'a'*64,AT,'fit')

    def test_duplicate_delivery_does_not_replay_snapshots(self):
        d=Decoder('cup');d.decode(frame(),'a'*64,AT,'fit')
        self.assertEqual(d.decode(frame(),'a'*64,AT,'fit'),([],[]))

    def test_verified_engine_prints_and_stream_reset(self):
        e=frame();e['payload']['markets'][0]['batch']['trades']=[dict(exchangeId=2,marketId=1,tournamentId='cup',
            id='abc',sequence=4,executedAt='2026-10-19T09:00:01Z',quantity=5,price='.45')]
        rows,prints=Decoder('cup').decode(e,'a'*64,AT,'fit')
        self.assertEqual(prints[0]['yes_price_micros'],450000);self.assertFalse(prints[0]['aggressor_side_known'])
        e.update(event='stream_reset',payload={'reason':'fixture'})
        rows,_=Decoder('cup').decode(e,'b'*64,AT,'fit');self.assertIsNone(rows[0]['exchange_id'])

    def test_only_pinned_calendar_slices_with_valid_original_hash_are_read(self):
        with tempfile.TemporaryDirectory()as td:
            p=Path(td);body=dict(event=frame(),previous='',sequence=1)
            digest=hashlib.sha256(json.dumps(body,sort_keys=True,separators=(',',':')).encode()).hexdigest()
            raw=(json.dumps({**body,'hash':digest},sort_keys=True,separators=(',',':'))+'\n').encode()
            envelope=dict(kind='original_breadth_row',file='source.jsonl',role='fit',raw_base64=base64.b64encode(raw).decode(),
                raw_sha256=hashlib.sha256(raw).hexdigest(),start_offset=0,end_offset=len(raw))
            (p/'original-slices.jsonl.gz').write_bytes(gzip.compress((json.dumps(envelope)+'\n').encode()))
            archive=hashlib.sha256((p/'original-slices.jsonl.gz').read_bytes()).hexdigest()
            manifest=dict(roles_ms={'fit':[AT,AT+1000]},counts={'fit':1},archive_sha256=archive,
                originals=[dict(file='source.jsonl',bytes=len(raw),first_sequence=1,last_sequence=1)])
            mraw=json.dumps(manifest).encode();(p/'source-manifest.json').write_bytes(mraw);msha=hashlib.sha256(mraw).hexdigest()
            self.assertEqual(len(list(frames(p,archive,msha))),1)
            manifest['roles_ms']['fit']=[AT+1,AT+1000];mraw=json.dumps(manifest).encode();(p/'source-manifest.json').write_bytes(mraw)
            with self.assertRaisesRegex(ValueError,'AUTHORIZED_CLOCK'):
                list(frames(p,archive,hashlib.sha256(mraw).hexdigest()))

    def test_realtime_clock_cannot_be_moved_to_fake_laptop_receipt(self):
        rows,_=Decoder('cup').decode(frame(),'a'*64,AT,'fit');rows[0]['available_at_ms']+=1
        with self.assertRaisesRegex(ValueError,'REALTIME_SNAPSHOT_CLOCK'):source_clock(rows[0])
