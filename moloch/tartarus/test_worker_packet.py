"""Synthetic original-shaped Task175 packets. No live-account or SDK assertions."""
from copy import deepcopy
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
import worker_packet as W
import native_snapshot as N
from test_native_snapshot import fixture, seal


def encoded(v):
    return (json.dumps(v,sort_keys=True,separators=(",",":"))+"\n").encode()

class Fixture:
    def __init__(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.d,self.raw,self.args,self.registry,self.liability = fixture(active=True)
        manifest = seal({"seed":{"kind":"actual_account_allocation","evidence":{"runId":"run"}},"policy":{}})
        h = json.loads(manifest)["hash"]
        self.d["manifestHash"] = h
        self.headraw = seal({"manifestHash":h,"sequence":7,"previous":"b"*64})
        self.head = json.loads(self.headraw)
        scope={"tournamentId":"cup","tournamentSlug":"slug","exchanges":{"1":"m1"}}
        self.members = {"manifest-original":manifest,"head-original":self.headraw,
            "control":encoded({"scope":scope,"accountScope":"acct","activation":{"runId":"run"},"bankName":"bank"}),
            "ordinary-dispatch-original":b"","package-dispatch-original":b""}
        self.m={"schemaVersion":1,"status":"available_as_evidence","capturedAtMs":2000,
            "sourceObservationMs":1001,"oldestReceiptRequestMs":1000,"nativeHead":self.head,
            "bankManifestHash":h,"scope":scope,"generationSha256":W.ZERO,"originalFiles":{},"originalReceipts":{},
            "heldBoundary":{"scope":scope,"accountScope":"acct","runId":"run","manifestHash":h,"nativeHead":self.head,
                "leaseDescriptor":{"device":1,"inode":3,"bytes":12},"ownerLeaseSha256":"a"*64,
                "queue":{"globalUnknownCount":0,"hasUnknown":False,"unknown_acceptances":0,
                    "state":{"active":None,"manual_review":False,"scope":"acct","last_clock":1000}},
                "provedNoNetworkOrderIds":[],"originalAckReceiptHashes":[],"unsubmitted":[1]},"neverSubmittedDraftIds":[1],
            "nativeBooks":[{"exchangeId":"1","sourceAt":"1970-01-01T00:00:01.000Z",
                "bids":[{"price":"0.400000","quantity":5}],"asks":[]}],
            "terminalDebtMicros":None,"executionAuthority":False}
        for label,endpoint,data in (
            ("account-original","/account?tournamentId=cup",{"id":"profile","balance":10}),
            ("positions-original","/tournaments/slug/portfolio/positions",{"data":[]}),
            ("open-orders-original","/orders?status=open&limit=200&tournamentId=cup",{"coverage":{"complete":True},"pagination":{"hasMore":False,"nextCursor":None},"data":[]})):
            r={"endpoint":endpoint,"requestStartedAt":"1970-01-01T00:00:01.000Z","receivedAt":"1970-01-01T00:00:01.001Z","status":200,"data":data}
            self.members[label]=encoded(r)
        self.refresh()
    def refresh(self):
        self.members["report-original"]=seal(self.d)
        for label,raw in self.members.items():
            pin={"sha256":W.sha(raw),"bytes":len(raw),"descriptor":{"device":1,"inode":42,"bytes":len(raw)}}
            if label.endswith("-original") and label.split("-original")[0] in ("account","positions","open-orders","collateral"):
                r=json.loads(raw)
                self.m["originalReceipts"][label]={"raw":pin,"receiptSha256":W.typed_receipt_sha(raw),
                    **{k:r[k] for k in ("endpoint","requestStartedAt","receivedAt")},"sourceAgeAtCaptureMs":1000}
            else: self.m["originalFiles"][label]=pin
            (self.root/(label+".json")).write_bytes(raw)
        b=self.m["heldBoundary"]
        b["reportHash"]=W.sha(W.tokens(self.members["report-original"])["report"])
        for label,key,package in (("ordinary-dispatch-original","ordinary",False),("package-dispatch-original","package",True)):
            raw=self.members[label]
            last=json.loads(raw.splitlines()[-1]) if raw else {"sequence":0,"hash":W.ZERO}
            b[key+"Sha256"]=W.sha(raw)
            b[key+"Descriptor"]={"schemaVersion":1,"device":1,"inode":2,"bytes":len(raw),"sequence":last["sequence"],"previous":last["hash"]}
            if package: b[key+"Descriptor"]["poisoned"]=False
        self.meta()
        self.args.update(expected_report_sha256=W.sha(self.members["report-original"]),expected_manifest_sha256=self.d["manifestHash"],expected_head=self.head)
        self.registry["bank_manifest_hash"]=self.d["manifestHash"]
        self.liability.update(report_sha256=self.args["expected_report_sha256"],head=self.head)
    def meta(self):
        raw=encoded(self.m);(self.root/"metadata.json").write_bytes(raw)
        self.pins={"expected_metadata_sha256":W.sha(raw),"expected_control_sha256":W.sha(self.members["control"]),
            "expected_generation_sha256":self.m["generationSha256"],"expected_report_sha256":W.sha(self.members["report-original"]),"expected_head_sha256":W.sha(self.headraw)}
    def chain(self,events,package=False):
        previous=W.ZERO;raw=b""
        for seq,e in enumerate(events,1):
            row={"sequence":seq,"previous":previous,"event":e}
            if package:row["schemaVersion"]=1
            line=seal(row);previous=json.loads(line)["hash"];raw+=line
        self.members["package-dispatch-original" if package else "ordinary-dispatch-original"]=raw
        self.refresh()
    def verify(self): return W.verify_packet(self.root,**self.pins)

class PacketTests(unittest.TestCase):
    def setUp(self): self.f=Fixture();self.addCleanup(self.f.temp.cleanup)
    def test_exact_draft_reservations_not_released_and_whole_account_usable(self):
        f=self.f;p=f.verify()
        self.assertEqual(p.provenance["draft_ids"],(1,))
        r=N.adapt_native_snapshot(p.report_bytes,registry=f.registry,liabilities=f.liability,draft_classification=p.draft_classification,**f.args)
        self.assertEqual(r["status"],"available")
        self.assertEqual((r["account_input"]["cash_micros"],r["account_input"]["reserved_cash_micros"]),(10_000_000,2_000_000))
        self.assertEqual(len(r["account_input"]["holdings"]),23)
        self.assertFalse(p.provenance["sdk_or_os_state_independently_verified"])
    def test_missing_debt_and_registry_remain_unavailable(self):
        p=self.f.verify()
        r=N.adapt_native_snapshot(p.report_bytes,draft_classification=p.draft_classification,**self.f.args)
        self.assertEqual(r["unavailable_reasons"],["REGISTRY_UNAVAILABLE","LIABILITY_EVIDENCE_UNAVAILABLE"])
        self.assertIsNone(r["terminal_debt_micros"])
    def test_default_and_forged_dictionary_cannot_lift_gate(self):
        f=self.f
        self.assertIn("NATIVE_OPEN_ORDER_CLASSIFICATION_UNAVAILABLE",N.adapt_native_snapshot(f.members["report-original"],**f.args)["unavailable_reasons"])
        with self.assertRaises(N.NativeSnapshotError):N.adapt_native_snapshot(f.members["report-original"],draft_classification={},**f.args)
    def test_tampered_member_even_restored_mtime_rejected(self):
        path=self.f.root/"control.json";old=path.stat();raw=path.read_bytes();path.write_bytes(raw.replace(b"acct",b"evil"))
        import os
        os.utime(path,ns=(old.st_atime_ns,old.st_mtime_ns))
        with self.assertRaisesRegex(W.PacketError,"MEMBER_HASH"):self.f.verify()
    def test_external_metadata_pin_not_self_hash(self):
        self.f.m["capturedAtMs"]=3000
        (self.f.root/"metadata.json").write_bytes(encoded(self.f.m))
        with self.assertRaisesRegex(W.PacketError,"EXPECTED_METADATA"):self.f.verify()
    def test_incomplete_draft_set_rejected_even_repinned_metadata(self):
        self.f.m["neverSubmittedDraftIds"]=[];self.f.meta()
        with self.assertRaisesRegex(W.PacketError,"COMPLETE_DRAFT_SET"):self.f.verify()
    def test_intent_only_and_attempted_current_orders_rejected(self):
        for events in ([{"kind":"intent","qid":1}], [{"kind":"intent","qid":1},{"kind":"attempt","qid":1}]):
            self.f.chain(events)
            with self.assertRaises(W.PacketError):self.f.verify()
    def test_ack_cancel_is_not_terminal_release(self):
        receipt=json.loads(self.f.members["account-original"])
        self.f.chain([{"kind":k,"qid":1,**({"receipt":receipt} if k == "ack" else {})} for k in ("intent","attempt","ack","cancel_ack")])
        with self.assertRaisesRegex(W.PacketError,"ACTUAL_NONTERMINAL"):self.f.verify()
    def test_native_fill_cannot_be_classified_never_sent(self):
        self.f.d["fills"]=[{"orderId":1}];self.f.refresh()
        with self.assertRaisesRegex(W.PacketError,"UNCLASSIFIED_CURRENT"):self.f.verify()
    def test_package_draft_membership_cannot_bypass(self):
        self.f.chain([{"kind":"intent","packageId":"p","admission":{"reserve":{"legs":[{"qid":1}]}}}],True)
        with self.assertRaisesRegex(W.PacketError,"UNCLASSIFIED_CURRENT"):self.f.verify()
    def test_unknown_and_missing_quiet_fields_not_zero(self):
        for change in ({"globalUnknownCount":1},{"hasUnknown":True}):
            old=deepcopy(self.f.m);self.f.m["heldBoundary"]["queue"].update(change);self.f.meta()
            with self.assertRaisesRegex(W.PacketError,"QUEUE_NOT_QUIET"):self.f.verify()
            self.f.m=old
        del self.f.m["heldBoundary"]["queue"]["state"]["active"];self.f.meta()
        with self.assertRaisesRegex(W.PacketError,"QUEUE_NOT_QUIET"):self.f.verify()
    def test_chain_head_and_seal_not_just_whole_file_hash(self):
        self.f.chain([{"kind":"not_sent","qid":1}]);self.f.m["heldBoundary"]["ordinaryDescriptor"]["previous"]=W.ZERO;self.f.meta()
        with self.assertRaisesRegex(W.PacketError,"JOURNAL_HEAD"):self.f.verify()
    def test_actual_open_orders_override_never_sent_claim(self):
        r=json.loads(self.f.members["open-orders-original"]);r["data"]["data"]=[{"id":"actual"}]
        self.f.members["open-orders-original"]=encoded(r);self.f.refresh()
        with self.assertRaisesRegex(W.PacketError,"ACTUAL_OPEN_ORDERS"):self.f.verify()
    def test_proof_cannot_replay_on_changed_report_or_reservations(self):
        f=self.f;p=f.verify();f.d["report"]["reservedCash"]="0.000000";f.refresh()
        with self.assertRaisesRegex(W.PacketError,"CLASSIFICATION_BOUNDARY"):N.adapt_native_snapshot(f.members["report-original"],draft_classification=p.draft_classification,**f.args)
    def test_original_observation_clock_cannot_be_renewed_by_adapter(self):
        f=self.f;p=f.verify();args={**f.args,"source_observation_ms":2000}
        with self.assertRaisesRegex(W.PacketError,"CLASSIFICATION_BOUNDARY"):
            N.adapt_native_snapshot(p.report_bytes,draft_classification=p.draft_classification,**args)
    def test_factory_binding_and_packet_attributes_cannot_be_reassigned_or_deleted(self):
        f=self.f;p=f.verify();proof=p.draft_classification
        f.d["report"]["reservedCash"]="0.000000";f.refresh()
        rebound=(f.args["expected_report_sha256"],f.d["manifestHash"],f.head,(1,),10_000_000,0,1001)
        for name,value in (("_binding",rebound),("_token",None),("_metadata_sha","f"*64)):
            with self.assertRaises(AttributeError):setattr(proof,name,value)
            with self.assertRaises(AttributeError):delattr(proof,name)
        for name,value in (("head",{}),("report_bytes",f.members["report-original"]),
                           ("_proof",None),("_metadata_bytes",b"{}"),("provenance",{})):
            with self.assertRaises(AttributeError):setattr(p,name,value)
            with self.assertRaises(AttributeError):delattr(p,name)
        with self.assertRaisesRegex(W.PacketError,"CLASSIFICATION_BOUNDARY"):
            N.adapt_native_snapshot(f.members["report-original"],draft_classification=proof,**f.args)
    def test_mutating_all_projections_cannot_change_pinned_metadata_head_or_proof(self):
        f=self.f;p=f.verify();head=p.head;metadata=p.metadata;books=p.native_books
        head["sequence"]=999
        metadata["nativeHead"]["sequence"]=999
        metadata["nativeBooks"][0]["bids"][0]["price"]="0.990000"
        books[0]["bids"][0]["quantity"]=999
        evidence=p.draft_classification.evidence();evidence["draft_ids"].clear()
        with self.assertRaises(TypeError):p.provenance["metadata_sha256"]="f"*64
        self.assertEqual(p.head,f.head)
        self.assertEqual(p.metadata_sha256,f.pins["expected_metadata_sha256"])
        self.assertEqual(p.native_books,f.m["nativeBooks"])
        self.assertEqual(p.draft_classification.evidence()["draft_ids"],[1])
        self.assertIsInstance(p.draft_classification._binding[2],bytes)
        self.assertEqual(N.adapt_native_snapshot(p.report_bytes,registry=f.registry,liabilities=f.liability,
            draft_classification=p.draft_classification,**f.args)["status"],"available")
        replay=deepcopy(f.head);replay["sequence"]=999
        with self.assertRaisesRegex(W.PacketError,"CLASSIFICATION_BOUNDARY"):
            p.draft_classification.validate(p.report_bytes,f.d["manifestHash"],replay,[1],10_000_000,2_000_000,1001)
    def test_receipt_digest_preserves_original_decimal_tokens_and_field_order(self):
        raw=b'{"data":{"cash":0.1000000000000000000001},"endpoint":"/x","receivedAt":"r","requestStartedAt":"s","status":200}\n'
        expected=b'{"endpoint":"/x","requestStartedAt":"s","receivedAt":"r","status":200,"data":{"cash":0.1000000000000000000001}}'
        self.assertEqual(W.typed_receipt_sha(raw),hashlib.sha256(expected).hexdigest())
    def test_generation_request_preserved_across_binary_only_control_rebase(self):
        f=self.f;request={"runId":"run","bankManifestSha256":f.d["manifestHash"],"controlSha256":"c"*64}
        f.members["generation-request"]=encoded(request);f.m["generationSha256"]=W.sha(f.members["generation-request"]);f.refresh()
        self.assertEqual(f.verify().provenance["generation_sha256"],f.m["generationSha256"])
    def test_historical_proved_abort_is_conditional_not_a_current_draft(self):
        f=self.f
        intent={"kind":"intent","qid":2,"spec":{"method":"POST"}}
        attempt={"kind":"attempt","qid":2,"at":"1970-01-01T00:00:00.500Z","captureAtMs":500}
        error={"kind":"error","qid":2,"at":"1970-01-01T00:00:00.600Z","error":{"code":"LIVE_CONTRACT_OR_BOOK_STALE","status":None,"orderId":None,"mayHaveExecuted":False}}
        abort={"kind":"aborted_before_admission","qid":2,"at":"1970-01-01T00:00:00.700Z",
            "originalIntentSha256":W.sha(encoded(intent).rstrip(b"\n")),
            "originalAttemptSha256":W.sha(encoded(attempt).rstrip(b"\n")),
            "originalErrorSha256":W.sha(encoded(error).rstrip(b"\n")),
            "proof":{"attemptAtMs":500,"originalError":error["error"],"queueScope":"acct"}}
        f.d["orders"].append({"id":2,"retired":True,"quantity":1,"filled":0})
        f.m["heldBoundary"]["provedNoNetworkOrderIds"]=[2]
        f.chain([intent,attempt,error,abort]);p=f.verify()
        self.assertEqual(p.provenance["historical_aborts_producer_verified"],(2,))
        self.assertFalse(p.provenance["historical_abort_sdk_archive_independently_verified"])
        self.assertEqual(p.provenance["draft_ids"],(1,))
        f.d["orders"][1]["retired"]=False;f.m["neverSubmittedDraftIds"]=[1,2];f.m["heldBoundary"]["unsubmitted"]=[1,2];f.refresh()
        with self.assertRaisesRegex(W.PacketError,"ABORT_NATIVE_TERMINAL"):f.verify()
    def test_replayed_native_head_external_pin_refuses(self):
        self.f.pins["expected_head_sha256"]="d"*64
        with self.assertRaisesRegex(W.PacketError,"EXPECTED_ORIGINAL"):self.f.verify()
    def test_truncated_journal_not_repaired(self):
        self.f.chain([{"kind":"not_sent","qid":1}])
        self.f.members["ordinary-dispatch-original"]=self.f.members["ordinary-dispatch-original"][:-1]
        self.f.refresh()
        with self.assertRaises(W.PacketError):self.f.verify()

if __name__ == "__main__":unittest.main()
