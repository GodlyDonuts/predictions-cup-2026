import gzip,json,tempfile,unittest
from pathlib import Path
from unittest.mock import patch
from .extend_replay_corpus import extend
from .compile_prospective_source import sha,FORMAT as CAPTURE_FORMAT
from .selection import DataUse
from .test_disk_tape import book
from .disk_tape import build,DiskFlowTape
from moloch.learning_flow.episodes import FEATURE_SCHEMA


def write(path,value):
    path.parent.mkdir(parents=True,exist_ok=True);path.write_text(json.dumps(value));return sha(path)


def compressed(path,rows):
    path.parent.mkdir(parents=True,exist_ok=True)
    body=b''.join((json.dumps(r,separators=(',',':'))+'\n').encode()for r in rows)
    path.write_bytes(gzip.compress(body));return sha(path),body


def trade(at,*,identity='first',quantity=10):
    return {'kind':'trade_flow','venue':'SIG','available_at_ms':at,'received_at_ms':at,
            'executed_at_ms':1300 if identity=='first'else at-10,'exchange_id':'x','market_id':'m',
            'tournament_id':'cup','match_id':identity,'quantity':quantity,'yes_price_micros':450000,
            'original_sha256':'a'*64,'aggressor_side_known':False}


class ExtendCorpusTests(unittest.TestCase):
    def fixture(self,root,*,quantity=10):
        parent=root/'parent';base=parent/'base';capture=root/'captures';segment=capture/'normalized/segment-00000'
        rows=[dict(book(1000),calendar_role='fit'),
              {'available_at_ms':1100,'venue':'Kalshi','kind':'reference','market_id':'m','source':'K','calendar_role':'fit'},
              {'available_at_ms':1200,'venue':'Polymarket','kind':'reference','market_id':'m','source':'P','calendar_role':'fit'},
              dict(book(2000),calendar_role='search')]
        obs,body=compressed(base/'observations.jsonl.gz',rows);bpin=write(base/'dataset-manifest.json',{'observationsSha256':obs})
        write(base/'COMPLETE.json',{'manifestSha256':bpin});flow,_=compressed(parent/'trade-flow.jsonl.gz',[trade(1400)])
        p={'featureSchema':FEATURE_SCHEMA,'externalObservationsIncluded':True,'productionAuthority':False,
           'trainingAdmitted':False,'heldoutFeaturesIncluded':False,'calendarRolesMs':{'fit':[1000,1500],'search':[1500,2500]},
           'baseDataset':str(base),'baseManifestSha256':bpin,'tradeRowsSha256':flow,'tradeRows':1,
           'observationCounts':{'fit/SIG/book':1,'fit/Kalshi/reference':1,'fit/Polymarket/reference':1,'search/SIG/book':1},
           'requiredWorldBookLifetimeMs':60000,'sourceCoveragePolicy':'explicit_partial'}
        pp=write(parent/'dataset-manifest.json',p);write(parent/'COMPLETE.json',{'manifestSha256':pp})
        extra=[dict(book(2600),kind='gap',calendar_role='search'),dict(book(2700),calendar_role='search')]
        op,_=compressed(segment/'observations.jsonl.gz',extra)
        tp,_=compressed(segment/'trades.jsonl.gz',[trade(2600,quantity=quantity),trade(2800,identity='second')])
        session=capture/'wrapped/segment-00000/originals/session-manifest.json'
        session_pin=write(session,{'tokenPersistence':False,'contestOrders':0,'paperOrders':0,'tournamentId':'cup'})
        m={'format':CAPTURE_FORMAT,'calendarRole':'search','receiptBoundsMs':[2500,3000],'heldoutStartMs':3500,
           'allInputsRehashedAfterRead':True,'trainingAdmitted':False,'independentTestUsed':False,'productionChanged':False,
           'sessionManifestSha256':session_pin,'observationsSha256':op,'tradeRowsSha256':tp,
           'normalizedCounts':{'WS/gap':1,'WS/book':1}}
        mp=write(segment/'complete.json',m)
        c={'accountRequests':0,'productionChanged':False,'independentTestUsed':False,'trainingAdmitted':False,
           'receiptBoundsMs':[2500,3000],'heldoutStartMs':3500,'segments':[{'segment':0,'normalizedCompleteSha256':mp,**m}]}
        cp=write(capture/'COMPLETE.json',c);return parent,pp,capture,cp,body

    def run_extend(self,root,args,**kwargs):
        return extend(*args[:4],root/'output',heldout_start=3500,minimum_free_bytes=1,
                      data_use=DataUse(root/'registry'),**kwargs)

    def test_original_bytes_references_gaps_and_duplicate_trade_identity_survive(self):
        with tempfile.TemporaryDirectory()as td:
            r=Path(td);args=self.fixture(r);v=self.run_extend(r,args)
            with gzip.open(r/'output/base/observations.jsonl.gz','rb')as f:body=f.read()
            self.assertTrue(body.startswith(args[4]));self.assertEqual(len(body.splitlines()),6)
            self.assertEqual(v['tradeRows'],2);self.assertEqual(v['repeatedTradeReportsSuppressed'],1)
            self.assertEqual(v['sourceRowsConsumed']['extension/observation'],2)
            self.assertFalse(v['trainingAdmitted']);self.assertFalse(v['heldoutFeaturesIncluded'])
            pin=build(r/'output',r/'cache');tape=DiskFlowTape(r/'cache',pin,r/'output')
            try:
                self.assertEqual(len(tape.rows),6);self.assertEqual(len(tape.flow_rows),2)
                self.assertEqual(tape.flow_rows[0]['available_at_ms'],1400)
            finally:tape.disk_store.close()

    def test_conflicting_repeated_match_fails_instead_of_creating_volume(self):
        with tempfile.TemporaryDirectory()as td:
            r=Path(td);args=self.fixture(r,quantity=11)
            with self.assertRaisesRegex(ValueError,'MATCH_FACTS_CHANGED'):self.run_extend(r,args)
            self.assertFalse((r/'output/COMPLETE.json').exists())

    def test_crossing_or_protected_calendar_rejected_before_price_file_read(self):
        with tempfile.TemporaryDirectory()as td:
            r=Path(td);args=list(self.fixture(r));p=args[2]/'COMPLETE.json';c=json.loads(p.read_bytes())
            c['receiptBoundsMs'][1]=4000;args[3]=write(p,c)
            (args[0]/'base/observations.jsonl.gz').unlink()
            with self.assertRaisesRegex(ValueError,'NON_HELDOUT_SOURCE_BOUNDS'):self.run_extend(r,args)
            self.assertFalse((r/'output').exists())
        with tempfile.TemporaryDirectory()as td:
            r=Path(td);args=self.fixture(r);(args[0]/'base/observations.jsonl.gz').unlink()
            with patch.object(DataUse,'record',side_effect=ValueError('PROTECTED_CALENDAR_OVERLAP')):
                with self.assertRaisesRegex(ValueError,'PROTECTED_CALENDAR'):self.run_extend(r,args)
            self.assertFalse((r/'output').exists())

    def test_changed_source_bytes_reject_before_output(self):
        with tempfile.TemporaryDirectory()as td:
            r=Path(td);args=self.fixture(r);p=args[2]/'normalized/segment-00000/trades.jsonl.gz'
            p.write_bytes(p.read_bytes()+b'changed')
            with self.assertRaisesRegex(ValueError,'EXTENSION_SOURCE_CHANGED'):self.run_extend(r,args)
            self.assertFalse((r/'output').exists())


if __name__=='__main__':unittest.main()
