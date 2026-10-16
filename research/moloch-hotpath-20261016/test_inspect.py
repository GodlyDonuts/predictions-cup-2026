import importlib.util
import hashlib
import json
from pathlib import Path
import tempfile
import unittest

spec=importlib.util.spec_from_file_location('hotpath',Path(__file__).with_name('inspect.py'))
m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)


def row(seq,previous,event):
    body=json.dumps({'event':event,'previous':previous,'sequence':seq},sort_keys=True,separators=(',',':'))
    value=json.loads(body);value['hash']=hashlib.sha256(body.encode()).hexdigest()
    return json.dumps(value,sort_keys=True,separators=(',',':')).encode()+b'\n'


class TimingTests(unittest.TestCase):
    def test_overlapping_aggregate_receipts_do_not_double_count_wall_time(self):
        rs=[{'endpoint':'/orders/1/fills?limit=200','requestStartedAt':'2026-10-16T00:00:01Z','receivedAt':'2026-10-16T00:00:04Z'},
            {'endpoint':'/markets/2/orderbook?tournamentId=private','requestStartedAt':'2026-10-16T00:00:03Z','receivedAt':'2026-10-16T00:00:05Z'}]
        out=m.receipt_summary(rs,m.clock('2026-10-16T00:00:00Z'),m.clock('2026-10-16T00:00:06Z'))
        self.assertEqual(out['receiptWindowUnionMs'],4000)
        self.assertEqual(out['byEndpointClassMs']['owned_fills']['median'],3000)
        self.assertNotIn('private',json.dumps(out))

    def test_suffix_chain_tamper_and_processing_lag(self):
        e={'type':'book','receivedAt':'2026-10-16T00:00:00Z','processedAt':'2026-10-16T00:00:03Z','book':{'transport':'official_sig_realtime','price':-0.0}}
        a=row(20,'unknown-prefix',e);b=row(21,m.sealed_row(a)['hash'],e)
        out=m.journal_summary(a+b,0,10**15)
        self.assertEqual(out['receivedToProcessingStartMs']['official_sig_realtime']['median'],3000)
        self.assertFalse(out['fullPrefixVerified'])
        with self.assertRaises(ValueError):m.journal_summary(a+b.replace(b'00:00:03',b'00:00:04'),0,10**15)
        with self.assertRaises(ValueError):m.journal_summary(a+row(22,m.sealed_row(a)['hash'],e),0,10**15)

    def test_bound_excludes_partial_rows_and_prefix(self):
        with tempfile.TemporaryDirectory() as d:
            p=Path(d)/'events';p.write_bytes(b'old-prefix\ncomplete-row\n{"torn"')
            raw,b=m.bounded_tail(p,25)
            self.assertEqual(raw,b'complete-row\n');self.assertEqual(b['incompleteTrailingBytesExcluded'],7)
            self.assertEqual(b['sha256'],hashlib.sha256(raw).hexdigest())

    def test_pid_reuse_and_micro_write_detection(self):
        a={'startTicks':1,'monotonic':10,'ticksPerSecond':100,'userTicks':100,'systemTicks':50,'io':{'syscw':5,'wchar':10,'write_bytes':0,'read_bytes':0}}
        b={**a,'monotonic':12,'userTicks':180,'systemTicks':110,'io':{'syscw':1005,'wchar':5010,'write_bytes':8192,'read_bytes':0}}
        delta=m.proc_delta(a,b)
        self.assertEqual(delta['bytesPerWriteSyscall'],5);self.assertEqual(delta['oneCoreCpuPercent'],70)
        with self.assertRaises(ValueError):m.proc_delta(a,{**b,'startTicks':2})

    def test_reversed_and_out_of_window_receipts_not_zero_latency(self):
        rs=[{'endpoint':'/orders','requestStartedAt':'2026-10-16T00:00:05Z','receivedAt':'2026-10-16T00:00:01Z'},
            {'endpoint':'/orders','requestStartedAt':'2026-10-16T01:00:00Z','receivedAt':'2026-10-16T01:00:01Z'}]
        out=m.receipt_summary(rs,m.clock('2026-10-16T00:00:00Z'),m.clock('2026-10-16T00:00:06Z'))
        self.assertEqual(out['invalidClocks'],1);self.assertEqual(out['byEndpointClassMs'],{})

    def test_ack_attempt_timing_keeps_unacknowledged_censored(self):
        a=row(3,'unknown-prefix',{'kind':'attempt','qid':7,'at':'2026-10-16T00:00:01Z'})
        b=row(4,m.sealed_row(a)['hash'],{'kind':'ack','qid':7,'receipt':{'receivedAt':'2026-10-16T00:00:02.500Z'}})
        c=row(5,m.sealed_row(b)['hash'],{'kind':'attempt','qid':8,'at':'2026-10-16T00:00:03Z'})
        out=m.dispatch_summary(a+b+c,0,10**15)
        self.assertEqual(out['attemptToAckReceivedMs']['median'],1500)
        self.assertEqual(out['attemptsWithoutAckInWindow'],1)
        self.assertNotIn('qid',json.dumps(out))


if __name__=='__main__':unittest.main()
