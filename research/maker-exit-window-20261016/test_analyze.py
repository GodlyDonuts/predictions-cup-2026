import copy
from collections import Counter
import importlib.util
import json
from pathlib import Path
import sys
import unittest

def module(name,path):
    spec=importlib.util.spec_from_file_location(name,path);m=importlib.util.module_from_spec(spec);sys.modules[name]=m;spec.loader.exec_module(m);return m
HERE=Path(__file__).parent
A=module("exit_windows",HERE/"analyze.py")
F=module("exit_fixture",HERE.parent/"moloch-actual-economics-20261016/tests/test_analyze.py")

def cohort(n=10,p="0.4",side="YES",observed=10):
    return dict(exchange="21",side=side,n=n,p=A.units(p),origin="ordinary",executed=A.clock(F.stamp(10)),observed=A.clock(F.stamp(observed)),fee=0,first_post_execution=None,first_post_observation=None,labels={},gains={h:None for h in A.HORIZONS},valid_books=Counter())
def book(at=70,bid="0.405",ask="0.45",qty=100,source=None,request=None,transport="official_sig_realtime"):
    raw=dict(exchangeId="21",sourceAt=F.stamp(at if source is None else source),requestStartedAt=F.stamp(at if request is None else request),receivedAt=F.stamp(at),bids=[dict(price=bid,quantity=qty)],asks=[dict(price=ask,quantity=qty)])
    return A.book(raw,F.stamp(at),F.stamp(at),transport,"a"*64)

class ExitWindowTests(unittest.TestCase):
    def test_exact_fifo_clock_trace_keeps_inherited_and_new_partial_close_separate(self):
        data=F.mixed();orders={str(q["id"]):q for q in data["orders"]};cs,rows=A.trace(data["events"],orders)
        self.assertEqual(sum(c["gross"]for c in cs if c["origin"]=="inherited"),1_100_000)
        self.assertEqual(sum(c["gross"]for c in cs if c["origin"]=="ordinary"),200_000)
        self.assertEqual(sum(c["remaining"]for c in cs if c["origin"]=="ordinary"),3)
        self.assertEqual(sum(c["fee"]for c in cs if c["origin"]=="ordinary"),40_000)
        self.assertEqual(A.FIFO.analyze(data)["accounting"]["realized"],"1.200000")

    def test_exact_duplicate_is_ignored_changed_duplicate_rejected(self):
        data=F.mixed();data["events"].append(copy.deepcopy(data["events"][1]));orders={str(q["id"]):q for q in data["orders"]}
        cs,_=A.trace(data["events"],orders);self.assertEqual(sum(c["n"]for c in cs if c["origin"]=="ordinary"),4)
        data["events"][-1]["event"]["price"]="0.505"
        with self.assertRaisesRegex(ValueError,"TRACE_CHANGED_DUPLICATE"):A.trace(data["events"],orders)

    def test_no_side_uses_complement_of_ask_and_full_depth(self):
        b=book(70,ask="0.7",bid="0.65",qty=3);b["asks"].append((750000,7))
        self.assertEqual(A.full_liquidation(b,"NO",10),(2_650_000,250000))
        self.assertIsNone(A.full_liquidation(b,"NO",11))

    def test_missing_depth_is_not_zero_gain(self):
        c=cohort(n=101);A.observe([c],book(qty=100))
        self.assertEqual(c["labels"][60000]["status"],"depth_missing")
        self.assertIsNone(c["labels"][60000]["gain"])

    def test_late_observed_old_source_is_not_a_post_horizon_label(self):
        c=cohort();A.observe([c],book(at=70,source=69))
        self.assertNotIn(60000,c["labels"])
        A.observe([c],book(at=101))
        self.assertNotIn(60000,c["labels"],"first packet more than30s beyond horizon is missing")

    def test_no_exit_before_actual_fill_is_observed(self):
        c=cohort(observed=80);A.observe([c],book(70))
        self.assertEqual(c["labels"][60000]["status"],"fill_not_observed")
        self.assertIsNone(c["labels"][60000]["gain"])
        self.assertTrue(all(v is None for v in c["gains"].values()))

    def test_patient_window_adds_only_later_actual_displayed_opportunity(self):
        c=cohort();A.observe([c],book(70,bid="0.4"));self.assertIsNone(c["gains"][60000])
        A.observe([c],book(100));self.assertIsNone(c["gains"][60000]);self.assertEqual(c["gains"][180000],A.clock(F.stamp(100)))
        self.assertEqual(c["labels"][60000]["gain"],0)

    def test_stream_subscription_age_is_not_fresh_book_age_but_rest_request_age_is(self):
        self.assertEqual(book(70,request=0)["status"],"valid")
        self.assertEqual(book(70,request=0,transport=None)["status"],"stale_rest_request")

    def test_stale_crossed_and_changed_receipt_are_not_labels(self):
        c=cohort();A.observe([c],book(70,source=0));self.assertFalse(c["labels"])
        A.observe([c],book(70,bid="0.5",ask="0.45"));self.assertFalse(c["labels"])
        raw=dict(exchangeId="21",sourceAt=F.stamp(70),receivedAt=F.stamp(69),bids=[],asks=[])
        with self.assertRaisesRegex(ValueError,"BOOK_RECEIPT_MISMATCH"):A.book(raw,F.stamp(70),F.stamp(70),None,"a"*64)

    def test_original_raw_seals_detect_changed_number_lexeme(self):
        value={"event":{"type":"tick","receivedAt":F.stamp()},"previous":"","sequence":1}
        import hashlib
        value["hash"]=hashlib.sha256(json.dumps(value,sort_keys=True,separators=(",",":")).encode()).hexdigest()
        raw=(json.dumps(value,sort_keys=True,separators=(",",":"))+"\n").encode();self.assertEqual(A.RAW.native_row(raw)["sequence"],1)
        with self.assertRaises(ValueError):A.RAW.native_row(raw.replace(b'"sequence":1',b'"sequence":2'))

    def test_genuine_stage_clocks_keep_ack_before_passive_fill_and_missing_distinct(self):
        data=F.mixed();orders={str(q["id"]):q for q in data["orders"]};orders["2"]["createdAt"]=A.clock(F.stamp(75))//A.MS
        cs,rows=A.trace(data["events"],orders);summary=A.clock_summary(cs,rows,{"1":A.clock(F.stamp(5))})
        self.assertEqual(summary["nativeFillIngestionToExitQuoteCreated"]["medianMs"],"65000")
        self.assertEqual(summary["exitQuoteCreatedToActualFillExecution"]["medianMs"],"5000")
        self.assertEqual(summary["originalPostAckBeforeBuyExecutionCohorts"],1)
        self.assertEqual(summary["buyExecutionToOriginalPostAckReceiptSigned"]["medianMs"],"-5000")
        self.assertEqual(A.clock_summary(cs,rows,{})["originalPostAckMissingOrRecoveredCohorts"],1)

    def test_public_breadth_packet_does_not_become_native_book_observation(self):
        c=cohort();public=book();A.observe([c],public);self.assertNotIn("first_native_post_execution",c)
        native=book(71);native["lane"]="native";A.observe([c],native)
        self.assertEqual(c["first_native_post_execution"],A.clock(F.stamp(71)))

if __name__=="__main__":unittest.main()
