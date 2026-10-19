from contextlib import ExitStack
import gzip
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
import numpy as np
from moloch.learning_flow.episodes import FlowTape,FEATURE_SCHEMA
from moloch.learning_realism.protocol import Split,DAY
from moloch.learning_realism.test_replay import fixture,reference
from .engine import ApiTape
from .contract import OrderSpec,UNIT
from .snapshot_clock import prepare
from .disk_tape import build,DiskFlowTape,sha


def book(at,ex='x',market='m',ask=450000):
    return {'kind':'book','venue':'SIG','market_id':market,'exchange_id':ex,
        'available_at_ms':at,'received_at_ms':at,'source_at_ms':at,
        'snapshot_origin':'original_official_sig_realtime_frame','original_realtime_received_at_ms':at,
        'original_sha256':'b'*64,'bids':[[400000,100]],'asks':[[ask,100]]}


def print_row(i,available,executed):
    return {'kind':'trade_flow','venue':'SIG','available_at_ms':available,'received_at_ms':available,
        'executed_at_ms':executed,'exchange_id':'x','market_id':'m','tournament_id':'cup',
        'match_id':str(i),'quantity':i+1,'yes_price_micros':410000,'original_sha256':'a'*64}


def dataset(root,rows=None,prints=None):
    rows=rows if rows is not None else [book(0),reference(0),reference(0,'Polymarket'),
        reference(50,'Kalshi','other'),book(100,'y'),book(200),reference(300),book(900),
        book(1000),book(1200),book(2000),book(3000),book(5000)]
    prints=prints if prints is not None else [print_row(1,1100,1000),print_row(2,1300,900),print_row(3,1500,1400)]
    base=root/'base';flow=root/'flow';base.mkdir();flow.mkdir()
    def manifest(folder,value):
        raw=json.dumps(value,sort_keys=True).encode();(folder/'dataset-manifest.json').write_bytes(raw)
        digest=hashlib.sha256(raw).hexdigest();(folder/'COMPLETE.json').write_text(json.dumps({'manifestSha256':digest}));return digest
    obs=gzip.compress(''.join(json.dumps(r)+'\n'for r in rows).encode());(base/'observations.jsonl.gz').write_bytes(obs)
    bsha=manifest(base,{'observationsSha256':hashlib.sha256(obs).hexdigest()})
    trades=gzip.compress(''.join(json.dumps(r)+'\n'for r in prints).encode());(flow/'trade-flow.jsonl.gz').write_bytes(trades)
    manifest(flow,{'featureSchema':FEATURE_SCHEMA,'productionAuthority':False,'baseDataset':str(base),
        'baseManifestSha256':bsha,'tradeRowsSha256':hashlib.sha256(trades).hexdigest()})
    return flow


def prepared(original,*,prefix=False,lifetime=None):
    tape=ApiTape.from_loaded(original,split=Split(0,10*DAY),role='train')
    tape.counterparty_information_bounds=(0,5000)
    prepare(tape,(0,5000),role_prefix_initial_state=prefix)
    tape.world_book_lifetime_ms=lifetime
    return tape


def episode(tape,start=1,end=4000):
    return tape.episode(fixture().mechanics,start,end,initial_cash_micros=1000*UNIT,
                        target_wealth_micros=11000*UNIT,burnin_ms=1)


class DiskTapeTests(unittest.TestCase):
    def pair(self,root,**kw):
        path=dataset(root,**kw);cache=root/'cache';digest=build(path,cache)
        return path,FlowTape(path),DiskFlowTape(cache,digest,path),digest

    def equal(self,a,b):
        x,y=a.observe(),b.observe();self.assertEqual(x['entities'],y['entities'])
        for name in('contracts','global_state','present'):np.testing.assert_array_equal(x[name],y[name])
        for name in x['masks']:np.testing.assert_array_equal(x['masks'][name],y['masks'][name])
        self.assertEqual((a.cash,a.lots,a.api_fills,a.realized,a.now),(b.cash,b.lots,b.api_fills,b.realized,b.now))
        self.assertEqual(a.api_orders,b.api_orders)

    def test_lazy_open_indexed_slices_and_original_rows(self):
        with tempfile.TemporaryDirectory()as td,ExitStack()as resources:
            _,e,d,_=self.pair(Path(td));resources.callback(d.disk_store.close)
            self.assertEqual(d.disk_store.decoded_rows,0)
            self.assertEqual(list(d.times),list(e.times));self.assertEqual(d.exchanges,e.exchanges)
            self.assertEqual([{k:v for k,v in r.items()if k!='_disk_record'}for r in d.rows[2:5]],list(e.rows[2:5]))
            self.assertEqual(d.disk_store.decoded_rows,3)
            self.assertEqual(d.rows[-1]['available_at_ms'],e.rows[-1]['available_at_ms'])

    def test_events_observations_finance_and_paid_history_match(self):
        with tempfile.TemporaryDirectory()as td,ExitStack()as resources:
            _,e,d,_=self.pair(Path(td));resources.callback(d.disk_store.close)
            a,b=episode(prepared(e)),episode(prepared(d));self.equal(a,b)
            for env in(a,b):
                env.dispatch('ORDER',[OrderSpec('x','YES','BUY',5,450000)],idempotency_key='paired')
                env.advance(301);env.request_observation('SIG',('book','x'));env.advance(401)
            self.equal(a,b)
            for env in(a,b):
                env.advance(1200);env.request_observation('SIG',('trade_flow','x'),trade_limit=1);env.advance(1300)
            self.equal(a,b);self.assertEqual(a.known_trade_pages,b.known_trade_pages)
            for env in(a,b):
                env.advance(1600);env.request_observation('SIG',('trade_flow','x'),trade_limit=2,continue_trade_page=False);env.advance(1700)
            self.equal(a,b);self.assertEqual(a.known_trade_pages,b.known_trade_pages)
            self.assertFalse(b.trade_history.rows);self.assertLessEqual(sum(v[3]=='api_source_cursor'for v in b.clock._events),4)

    def test_indexed_prefix_preserves_discovery_order_initial_state_and_expiries(self):
        with tempfile.TemporaryDirectory()as td,ExitStack()as resources:
            _,e,d,_=self.pair(Path(td));resources.callback(d.disk_store.close)
            a,b=episode(prepared(e,prefix=True,lifetime=500),1001),episode(prepared(d,prefix=True,lifetime=500),1001)
            self.equal(a,b);self.assertEqual(a.keys,b.keys)
            self.assertEqual(a.initial_market_state,b.initial_market_state)
            self.assertEqual(a.world_confidence_expiries,b.world_confidence_expiries)
            for env in(a,b):env.request_observation('SIG',('book','x'));env.advance(1101)
            self.equal(a,b)

    def test_source_and_cache_mutation_or_unfinished_cache_rejected(self):
        with tempfile.TemporaryDirectory()as td,ExitStack()as resources:
            root=Path(td);path,e,d,digest=self.pair(root);d.disk_store.close()
            with (root/'cache/replay.sqlite').open('ab')as f:f.write(b'changed')
            with self.assertRaisesRegex(ValueError,'DATABASE_CHANGED'):DiskFlowTape(root/'cache',digest,path)
            with self.assertRaises(FileExistsError):build(path,root/'cache')

    def test_disk_pagination_keeps_original_snapshot_and_role_fences(self):
        from .trade_history import TradeHistory
        with tempfile.TemporaryDirectory()as td,ExitStack()as resources:
            rows=[book(0),book(5000)]
            prints=[print_row(i,at,0)for i,at in enumerate((100,200,300,400))]
            _,_,d,_=self.pair(Path(td),rows=rows,prints=prints);resources.callback(d.disk_store.close)
            h=TradeHistory();h.backend=(d.disk_store,(150,350))
            first=h.page('x',250,limit=1);self.assertEqual([r['match_id']for r in first['data']],['1'])
            self.assertFalse(first['pagination']['hasMore'])
            newest=h.page('x',500,limit=1);self.assertTrue(newest['pagination']['hasMore'])
            nextpage=h.page('x',600,limit=1,cursor=newest['pagination']['nextCursor'])
            self.assertEqual([r['match_id']for r in newest['data']+nextpage['data']],['1','2'])

    def test_cached_coverage_is_identical_without_decoding_prices(self):
        from moloch.learning_realism.prepare import coverage
        with tempfile.TemporaryDirectory()as td,ExitStack()as resources:
            _,e,d,_=self.pair(Path(td));resources.callback(d.disk_store.close)
            split=Split(0,10*DAY)
            self.assertEqual(coverage(e,split),coverage(d,split))
            self.assertEqual(d.disk_store.decoded_rows,0)

    def test_counterparty_ties_late_reporting_and_role_boundary_match(self):
        from .passive import StrictPriorityFlow
        with tempfile.TemporaryDirectory()as td,ExitStack()as resources:
            prints=[print_row(3,1500,1400),print_row(20,5000,1500)]
            _,e,d,_=self.pair(Path(td),prints=prints);resources.callback(d.disk_store.close)
            tapes=[prepared(t,prefix=True)for t in(e,d)]
            for t in tapes:t.counterparty_replay_enabled=True
            a,b=[episode(t,1001)for t in tapes]
            for env in(a,b):
                env.passive=StrictPriorityFlow(env,'cup')
                env.dispatch('ORDER',[OrderSpec('x','YES','BUY',5,420000)],idempotency_key='rest')
                env.advance(2000)
            self.equal(a,b);self.assertEqual(sum(f['quantity']for f in b.api_fills),4)
            self.assertEqual(b.api_orders[1].remaining,1)

    def test_snapshot_double_role_fence_survives_earlier_native_source_clock(self):
        with tempfile.TemporaryDirectory()as td,ExitStack()as resources:
            future=book(4500,ask=900000);future.pop('snapshot_origin');future.pop('original_realtime_received_at_ms')
            future.update(native_event_received_at_ms=100,native_processed_at_ms=4500,received_at_ms=100,source_at_ms=100)
            _,e,d,_=self.pair(Path(td),rows=[book(0),book(1000),future,book(5000)])
            resources.callback(d.disk_store.close)
            a,b=[episode(prepared(t,prefix=True),1001)for t in(e,d)]
            self.equal(a,b);self.assertEqual(b.books['x'].book.asks[0][0],450000)

    def test_unprocessed_same_clock_trade_is_not_in_a_frozen_paid_query(self):
        with tempfile.TemporaryDirectory()as td,ExitStack()as resources:
            _,e,d,_=self.pair(Path(td),prints=[print_row(1,0,0)])
            resources.callback(d.disk_store.close)
            envs=[]
            for t in(e,d):
                p=prepared(t)
                env=p.episode(fixture().mechanics,1,4000,initial_cash_micros=1000*UNIT,
                    target_wealth_micros=11000*UNIT,burnin_ms=1,advance_to_start=False)
                env.request_observation('SIG',('trade_flow','x'));env.advance(100);envs.append(env)
            self.assertEqual(envs[0].known_trade_pages,envs[1].known_trade_pages)
            self.assertFalse(envs[1].known_trade_pages['x']['data'])

    def test_full_depth_stochastic_stream_and_fused_logprobs_match(self):
        import copy
        import torch
        from .full_depth import FullDepthTape,FullDepthPolicy
        from .stream import Stream
        from .training import sequence_forward
        with tempfile.TemporaryDirectory()as td,ExitStack()as resources:
            _,e,d,_=self.pair(Path(td));resources.callback(d.disk_store.close)
            envs=[]
            for original in(e,d):
                t=FullDepthTape.from_loaded(original,split=Split(0,10*DAY),role='train')
                t.counterparty_information_bounds=(0,5000);prepare(t,(0,5000),role_prefix_initial_state=True)
                envs.append(episode(t,1001))
            torch.set_num_threads(1);torch.manual_seed(57);model=FullDepthPolicy(16)
            streams=[];chunks=[]
            for env in envs:
                torch.manual_seed(92);m=copy.deepcopy(model)
                s=Stream(env,m,calibration={'decision_latency_ms':7},record_training=True)
                from .training import collect_chunk
                chunks.append(collect_chunk(s,32));streams.append(s)
            self.equal(*envs);self.assertEqual(streams[0].action_counts,streams[1].action_counts)
            self.assertEqual(streams[0].status_counts,streams[1].status_counts)
            self.assertEqual(streams[0].decisions,streams[1].decisions)
            for chunk in chunks:
                values,_=sequence_forward(chunk.stream.model,chunk.frames,chunk.initial_hidden)
                torch.testing.assert_close(values['logprob'],torch.stack([f['old_logprob']for f in chunk.frames]),atol=1e-6,rtol=1e-5)

    def test_full_history_and_independent_test_checks_cannot_be_bypassed_by_cache(self):
        from dataclasses import asdict
        from moloch.learning_realism.replay import open_replay
        from moloch.learning_realism.protocol import Delivery
        with tempfile.TemporaryDirectory()as td,ExitStack()as resources:
            root=Path(td);path,_,d,digest=self.pair(root);resources.callback(d.disk_store.close)
            mechanics=root/'mechanics.json';mechanics.write_text('{}')
            spec={'full_history_ready':False,'only_execution_venue':'SIG','mechanics_sha256':sha(mechanics),
                'source_sha256':{},'split':asdict(Split(0,10*DAY)),'delivery':asdict(Delivery()),
                'dataset_manifest_sha256':d.manifest_sha256,'maximum_venue_gap_ms':120000,
                'replay_cache_manifest_sha256':digest,'independent_test_eligible':True}
            p=root/'spec.json';p.write_text(json.dumps(spec))
            with self.assertRaisesRegex(ValueError,'FULL_THREE_VENUE_HISTORY_NOT_READY'):
                open_replay(p,path,mechanics,'train',original=d)
            spec['full_history_ready']=True;p.write_text(json.dumps(spec))
            with self.assertRaisesRegex(ValueError,'ACTUAL_THREE_VENUE_COVERAGE_FAILED'):
                open_replay(p,path,mechanics,'train',original=d)
            with self.assertRaisesRegex(ValueError,'ACTUAL_TEST_PROVENANCE'):
                open_replay(p,path,mechanics,'test',original=d,frozen_finalist_sha256='a'*64)

    def test_embedded_source_claims_cannot_be_relabelled_in_cache(self):
        with tempfile.TemporaryDirectory()as td,ExitStack()as resources:
            root=Path(td);path,_,d,_=self.pair(root);d.disk_store.close()
            p=root/'cache/manifest.json';m=json.loads(p.read_text());m['sourceManifest']['productionAuthority']=True
            raw=json.dumps(m).encode();p.write_bytes(raw);digest=hashlib.sha256(raw).hexdigest()
            (root/'cache/COMPLETE.json').write_text(json.dumps({'manifestSha256':digest}))
            with self.assertRaisesRegex(ValueError,'CACHE_IS_NOT_CONFIGURED_SOURCE_DATASET'):
                DiskFlowTape(root/'cache',digest,path)


if __name__=='__main__':unittest.main()
