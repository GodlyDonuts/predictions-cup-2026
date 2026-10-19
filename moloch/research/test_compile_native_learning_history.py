import hashlib
import io
import json
import unittest
from .compile_native_learning_history import compile_stream,amount,levels


def rows(events):
    raw=b'';previous=''
    for n,e in enumerate(events,1):
        row={'event':e,'previous':previous,'sequence':n}
        row['hash']=hashlib.sha256(json.dumps(row,sort_keys=True,separators=(',',':')).encode()).hexdigest()
        previous=row['hash'];raw+=(json.dumps(row,sort_keys=True,separators=(',',':'))+'\n').encode()
    return raw,{'bytes':len(raw),'sha256':hashlib.sha256(raw).hexdigest(),'terminalSequence':len(events)}


def event(second,kind='book'):
    at=f'2026-10-19T00:00:{second:02d}Z'
    e={'type':kind,'receivedAt':at,'processedAt':at,'marketId':'m','exchangeId':'e'}
    if kind=='book':e['book']={'marketId':'m','exchangeId':'e','receivedAt':at,'sourceAt':at,'sequence':second,
                              'bids':[{'price':'0.450000','quantity':3}],'asks':[{'price':'0.550000','quantity':5}]}
    return e


class NativeHistoryTests(unittest.TestCase):
    def protocol(self):return {'calendarRolesUtc':{'fit':['2026-10-19T00:00:00Z','2026-10-19T00:00:10Z'],
                                                  'search':['2026-10-19T00:00:11Z','2026-10-19T00:00:20Z'],
                                                  'heldOut':['2026-10-19T00:00:21Z','2026-10-19T00:00:30Z']}}

    def test_whole_chain_verified_but_held_out_price_never_interpreted_or_emitted(self):
        hidden=event(22);hidden['book']['bids'][0]['price']='deliberately_not_interpretable'
        raw,pin=rows([event(1),event(12,'gap'),hidden]);out=[]
        report=compile_stream(io.BytesIO(raw),pin,self.protocol(),out.append)
        self.assertEqual(report['calendarRowCounts'],{'fit':1,'search':1,'heldOut':1})
        self.assertEqual([x['kind']for x in out],['book','gap']);self.assertEqual(out[0]['bids'],[[450000,3]])

    def test_changed_original_bytes_truncation_and_wrong_pin_are_fatal(self):
        raw,pin=rows([event(1)])
        for bad in [raw.replace(b'0.450000',b'0.440000'),raw[:-1]]:
            with self.assertRaises(ValueError):compile_stream(io.BytesIO(bad),pin,self.protocol(),lambda _:None)
        with self.assertRaises(ValueError):compile_stream(io.BytesIO(raw),{**pin,'terminalSequence':2},self.protocol(),lambda _:None)

    def test_noncausal_processed_clock_and_overlapping_roles_are_fatal(self):
        e=event(2);e['processedAt']='2026-10-19T00:00:01Z';raw,pin=rows([e])
        with self.assertRaises(ValueError):compile_stream(io.BytesIO(raw),pin,self.protocol(),lambda _:None)
        raw,pin=rows([event(12)]);p=self.protocol();p['calendarRolesUtc']['fit'][1]='2026-10-19T00:00:15Z'
        with self.assertRaises(ValueError):compile_stream(io.BytesIO(raw),pin,p,lambda _:None)

    def test_exact_decimals_and_depth_order(self):
        self.assertEqual(amount('0.123456'),123456)
        for value in ['NaN','0.1234567','-0.1','1.1']:
            with self.assertRaises(ValueError):amount(value)
        with self.assertRaises(ValueError):levels([{'price':'.4','quantity':1},{'price':'.5','quantity':2}],True)


if __name__=='__main__':unittest.main()
