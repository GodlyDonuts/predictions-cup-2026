"""Offline Task175 packet verification; never live SDK/server or trading authority.

verify_packet requires owner-supplied external byte pins. Original journal and
receipt membership is checked independently; SDK quiet/OS lease and historical
no-network archive verification remain explicitly producer-held assertions.
The returned classification only explains native reservations: it releases none.
"""
from pathlib import Path
from copy import deepcopy
import hashlib
import json
import os
import re
import stat
from types import MappingProxyType
try:
    from . import native_snapshot as N
except ImportError:
    import native_snapshot as N

MAX_FILE = 64 * 1024 * 1024
MAX_TOTAL = 128 * 1024 * 1024
ZERO = "0" * 64
_LABEL = re.compile(r"[A-Za-z0-9_-]{1,96}\Z")
_TOKEN = object()

class PacketError(ValueError):
    pass

def need(ok, code):
    if not ok:
        raise PacketError(code)

def sha(raw):
    return hashlib.sha256(raw).hexdigest()

def integer(n):
    return type(n) is int and 0 <= n < 2**64

def decode(raw):
    try:
        return N._DECODER.decode(raw.decode("utf-8"))
    except (ValueError, UnicodeError) as e:
        raise PacketError("PACKET_JSON") from e

def tokens(raw):
    """Original top-level value tokens, including arbitrary-precision numbers."""
    text = raw.decode("utf-8").strip()
    need(text.startswith("{") and text.endswith("}"), "OBJECT_REQUIRED")
    at, out = 1, {}
    while at < len(text)-1:
        while text[at].isspace(): at += 1
        key, end = N._DECODER.raw_decode(text, at)
        need(type(key) is str and key not in out, "OBJECT_DUPLICATE")
        at = end
        while text[at].isspace(): at += 1
        need(text[at] == ":", "OBJECT_COLON")
        at += 1
        while text[at].isspace(): at += 1
        start = at
        _, at = N._DECODER.raw_decode(text, at)
        out[key] = text[start:at].encode()
        while at < len(text) and text[at].isspace(): at += 1
        if at == len(text)-1: break
        need(text[at] == ",", "OBJECT_COMMA")
        at += 1
    need(at == len(text)-1, "OBJECT_END")
    return out

def typed_receipt_sha(raw):
    t = tokens(raw)
    fields = ("endpoint", "requestStartedAt", "receivedAt", "status", "data")
    need(set(t) == set(fields), "RECEIPT_FIELDS")
    return sha(b"{" + b",".join(('"'+k+'":').encode()+t[k] for k in fields) + b"}")

def _identity(path):
    for p in (path, *path.parents):
        need(not p.is_symlink(), "PACKET_SYMLINK")
    s = path.stat()
    need(stat.S_ISREG(s.st_mode) and s.st_nlink == 1, "PACKET_REGULAR_SINGLE_LINK")
    need(s.st_size <= MAX_FILE, "PACKET_FILE_BOUND")
    return (s.st_dev, s.st_ino, s.st_size, s.st_mtime_ns, s.st_ctime_ns)

def _read(path):
    before = _identity(path)
    with path.open("rb") as f:
        raw = f.read(MAX_FILE+1)
    need(len(raw) <= MAX_FILE and before == _identity(path), "PACKET_CHANGED")
    return raw, before

def _pin(raw, pin):
    need(type(pin) is dict and set(pin) == {"sha256", "bytes", "descriptor"}, "MEMBER_PIN")
    need(pin["sha256"] == sha(raw) and type(pin["bytes"]) is int and pin["bytes"] == len(raw), "MEMBER_HASH_OR_SIZE")
    d = pin["descriptor"]
    need(type(d) is dict and set(d) == {"device", "inode", "bytes"} and all(integer(x) for x in d.values()) and d["bytes"] == len(raw), "ORIGINAL_DESCRIPTOR")
    # These are the ORIGINAL producer inode descriptors, not detached-copy inodes.

def _journal(raw, descriptor, package=False):
    need(type(descriptor) is dict and descriptor.get("schemaVersion") == 1 and descriptor.get("bytes") == len(raw), "JOURNAL_DESCRIPTOR")
    need(all(integer(descriptor.get(k)) for k in ("device", "inode", "bytes", "sequence")), "JOURNAL_DESCRIPTOR")
    if package: need(descriptor.get("poisoned") is False, "PACKAGE_POISONED")
    previous, sequence, rows = ZERO, 0, []
    for line in raw.splitlines(keepends=True):
        need(len(line) <= 16*1024*1024, "JOURNAL_ROW_BOUND")
        row = N._sealed(line)
        fields = {"hash", "event", "sequence", "previous"} | ({"schemaVersion"} if package else set())
        need(set(row) == fields and row["sequence"] == sequence+1 and row["previous"] == previous, "JOURNAL_CHAIN")
        if package: need(row["schemaVersion"] == 1, "PACKAGE_SCHEMA")
        need(type(row["event"]) is dict, "JOURNAL_EVENT")
        rows.append((row["event"], tokens(line)["event"]))
        sequence, previous = row["sequence"], row["hash"]
    need(descriptor["sequence"] == sequence and descriptor.get("previous") == previous, "JOURNAL_HEAD")
    return rows

class _Frozen:
    __slots__ = ()
    def __setattr__(self, name, value):
        raise AttributeError("verified evidence is immutable")
    def __delattr__(self, name):
        raise AttributeError("verified evidence is immutable")


def _head_bytes(head):
    return json.dumps(head, sort_keys=True, separators=(",", ":"), allow_nan=False).encode()


class DraftClassification(_Frozen):
    """Factory-only research evidence, not a Python-memory or trading boundary."""
    __slots__ = ("_binding", "_token", "_metadata_sha")
    def __init__(self, binding, metadata_sha, token):
        need(token is _TOKEN, "CLASSIFICATION_FACTORY_REQUIRED")
        # Never retain a mutable head shared with any caller projection.
        immutable = (*binding[:2], _head_bytes(binding[2]), *binding[3:])
        object.__setattr__(self, "_binding", immutable)
        object.__setattr__(self, "_token", token)
        object.__setattr__(self, "_metadata_sha", metadata_sha)
    def validate(self, report_bytes, manifest, head, ids, cash, reserved, observed):
        need(self._token is _TOKEN and self._binding == (sha(report_bytes), manifest, _head_bytes(head), tuple(ids), cash, reserved, observed), "CLASSIFICATION_BOUNDARY")
    def evidence(self):
        return {"metadata_sha256":self._metadata_sha,"draft_ids":list(self._binding[3]),
                "source_observation_ms":self._binding[6],
                "producer_held_sdk_and_lease_assertion":True,
                "sdk_or_os_state_independently_verified":False,"execution_authorized":False}

class VerifiedPacket(_Frozen):
    __slots__ = ("_report_bytes", "_head_bytes", "_proof", "_provenance", "_metadata_bytes")
    def __init__(self, report_bytes, head, proof, provenance, metadata_bytes, token):
        need(token is _TOKEN, "PACKET_FACTORY_REQUIRED")
        object.__setattr__(self, "_report_bytes", report_bytes)
        object.__setattr__(self, "_head_bytes", _head_bytes(head))
        object.__setattr__(self, "_proof", proof)
        object.__setattr__(self, "_provenance", MappingProxyType(deepcopy(provenance)))
        object.__setattr__(self, "_metadata_bytes", metadata_bytes)
    @property
    def report_bytes(self): return self._report_bytes
    @property
    def head(self): return decode(self._head_bytes)
    @property
    def draft_classification(self): return self._proof
    @property
    def provenance(self): return MappingProxyType(deepcopy(dict(self._provenance)))
    @property
    def metadata(self):
        """Defensive projection of the ORIGINAL externally pinned metadata."""
        return decode(self._metadata_bytes)
    @property
    def native_books(self):
        """Captured native books; no renewed clocks or server authentication."""
        return self.metadata["nativeBooks"]
    @property
    def metadata_sha256(self): return self._provenance["metadata_sha256"]

def _verify_packet(directory, *, expected_metadata_sha256, expected_control_sha256,
                  expected_generation_sha256, expected_report_sha256, expected_head_sha256):
    """Verify a detached original packet against FIVE external owner pins.

    A successful result is conditional on the pinned producer's original held
    lease/SDK checks. No freshness window, terminal debt, or registry is invented.
    """
    for pin in (expected_metadata_sha256, expected_control_sha256, expected_generation_sha256, expected_report_sha256, expected_head_sha256):
        need(type(pin) is str and N._HEX.fullmatch(pin), "EXTERNAL_PIN_REQUIRED")
    root = Path(directory).absolute()
    rawmeta, metaid = _read(root / "metadata.json")
    need(sha(rawmeta) == expected_metadata_sha256, "EXPECTED_METADATA")
    m = decode(rawmeta)
    need(m.get("schemaVersion") == 1 and m.get("status") == "available_as_evidence", "PACKET_SCHEMA")
    need(integer(m.get("capturedAtMs")), "CAPTURE_CLOCK")
    capture = m["capturedAtMs"]
    files, receipts = m.get("originalFiles"), m.get("originalReceipts")
    need(type(files) is dict and type(receipts) is dict and 0 < len(files) <= 38 and len(receipts) in (3,4), "MEMBER_BOUND")
    required = {"control", "manifest-original", "head-original", "report-original", "ordinary-dispatch-original", "package-dispatch-original"}
    need(required <= files.keys() and {"account-original", "positions-original", "open-orders-original"} <= receipts.keys() and not files.keys() & receipts.keys(), "REQUIRED_MEMBERS")
    need(set(receipts) <= {"account-original", "positions-original", "open-orders-original", "collateral-original"}, "RECEIPT_LABEL")
    originals, identities, total = {}, {}, 0
    for label, pin in list(files.items()) + [(k,v["raw"]) for k,v in receipts.items()]:
        need(type(label) is str and _LABEL.fullmatch(label) and label != "metadata", "MEMBER_LABEL")
        raw, identity = _read(root/(label+".json"))
        _pin(raw, pin)
        total += len(raw)
        need(total <= MAX_TOTAL, "PACKET_TOTAL_BOUND")
        originals[label], identities[label] = raw, identity
    need(sha(originals["control"]) == expected_control_sha256 and sha(originals["report-original"]) == expected_report_sha256 and sha(originals["head-original"]) == expected_head_sha256, "EXPECTED_ORIGINAL")
    manifest, head, report = (N._sealed(originals[k]) for k in ("manifest-original", "head-original", "report-original"))
    h = manifest["hash"]
    N._head(head,h)
    need(m["nativeHead"] == head and m["bankManifestHash"] == h and report["manifestHash"] == h and report["sequence"] == head["sequence"] and report["previous"] == head["previous"], "NATIVE_BOUNDARY")
    control, b = decode(originals["control"]), m["heldBoundary"]
    scope = m["scope"]
    need(control["scope"] == scope == b["scope"] and control["accountScope"] == b["accountScope"], "SCOPE")
    need(manifest["seed"]["kind"] == "actual_account_allocation" and manifest["seed"]["evidence"]["runId"] == control["activation"]["runId"] == b["runId"], "SEED_OWNER")
    need(b["nativeHead"] == head and b["manifestHash"] == h and b["ordinarySha256"] == sha(originals["ordinary-dispatch-original"]) and b["packageSha256"] == sha(originals["package-dispatch-original"]), "HELD_BOUNDARY")
    need(sha(tokens(originals["report-original"])["report"]) == b["reportHash"], "HELD_REPORT_HASH")
    need(m["generationSha256"] == expected_generation_sha256, "GENERATION_PIN")
    if expected_generation_sha256 != ZERO:
        need("generation-request" in originals and sha(originals["generation-request"]) == expected_generation_sha256, "GENERATION_REQUEST")
        request = decode(originals["generation-request"])
        need(request["runId"] == b["runId"] and request["bankManifestSha256"] == h, "GENERATION_SCOPE")
    planning = report["report"].get("managedInventoryPlanning")
    if planning is not None: need(planning["generationSha256"] == expected_generation_sha256, "PLANNING_GENERATION")
    need(N._ns(report["report"]["at"]) <= capture*1_000_000, "NATIVE_SNAPSHOT_AFTER_CAPTURE")
    q = b["queue"]
    s = q["state"]
    need(type(q.get("globalUnknownCount")) is int and q["globalUnknownCount"] == 0 and q.get("hasUnknown") is False and type(q.get("unknown_acceptances")) is int and q["unknown_acceptances"] == 0 and "active" in s and s["active"] is None and s.get("manual_review") is False and s.get("scope") == b["accountScope"] and integer(s.get("last_clock")) and s["last_clock"] <= capture, "PRODUCER_QUEUE_NOT_QUIET")
    need(type(b.get("leaseDescriptor")) is dict and N._HEX.fullmatch(b.get("ownerLeaseSha256", "")), "PRODUCER_LEASE_MISSING")
    starts, ends = [], []
    endpoints = {"account-original":f'/account?tournamentId={scope["tournamentId"]}', "positions-original":f'/tournaments/{scope["tournamentSlug"]}/portfolio/positions', "open-orders-original":f'/orders?status=open&limit=200&tournamentId={scope["tournamentId"]}', "collateral-original":f'/portfolio/collateral?tournamentId={scope["tournamentId"]}'}
    for label,pin in receipts.items():
        raw = originals[label]
        r = decode(raw)
        start, end = (N._ns(r[k])//1_000_000 for k in ("requestStartedAt", "receivedAt"))
        need(r["status"] == 200 and r["endpoint"] == endpoints[label] and 0 <= start <= end <= capture, "RECEIPT_SCOPE_OR_CLOCK")
        need(pin["receiptSha256"] == typed_receipt_sha(raw) and all(pin[k] == r[k] for k in ("endpoint","requestStartedAt","receivedAt")) and pin["sourceAgeAtCaptureMs"] == capture-start, "RECEIPT_BINDING")
        starts.append(start); ends.append(end)
    need(m["sourceObservationMs"] == max(ends) and m["oldestReceiptRequestMs"] == min(starts), "ORIGINAL_RECEIPT_CLOCKS")
    opened = decode(originals["open-orders-original"])["data"]
    need(opened["coverage"]["complete"] is True and opened["pagination"]["hasMore"] is False and "nextCursor" in opened["pagination"] and opened["pagination"]["nextCursor"] is None and opened["data"] == [], "ACTUAL_OPEN_ORDERS_UNAVAILABLE")
    ordinary = _journal(originals["ordinary-dispatch-original"], b["ordinaryDescriptor"])
    package = _journal(originals["package-dispatch-original"], b["packageDescriptor"], True)
    intents, attempts, acks, closed, aborted, errors = {}, {}, set(), set(), set(), {}
    attempt_events, error_events, ack_receipts, cancel_acks = {}, {}, set(), set()
    intent_events, intent_raws, local_guards = {}, {}, set()
    for e,eventraw in ordinary:
        qid, kind = e.get("qid"), e.get("kind")
        need(integer(qid) and qid > 0, "DISPATCH_QID")
        if kind == "intent":
            need(qid not in intents, "DUPLICATE_INTENT"); intents[qid] = sha(eventraw); intent_events[qid] = e; intent_raws[qid] = eventraw
        elif kind == "attempt":
            need(qid in intents and qid not in attempts, "ATTEMPT_ORDER"); attempts[qid] = sha(eventraw)
            attempt_events[qid] = e
        elif kind in ("ack","recovered_ack"):
            need(qid in attempts and qid not in acks and qid not in aborted, "ACK_ORDER"); acks.add(qid)
            if kind == "ack": ack_receipts.add(typed_receipt_sha(tokens(eventraw)["receipt"]))
        elif kind == "closed":
            need(qid in acks and qid not in closed, "CLOSED_ORDER"); closed.add(qid)
        elif kind == "aborted_before_admission":
            need(qid in attempts and qid not in acks and qid not in aborted and e["originalIntentSha256"] == intents[qid] and e["originalAttemptSha256"] == attempts[qid] and e["originalErrorSha256"] == errors.get(qid), "ABORT_CONTEXT")
            original = error_events[qid]
            failure = original["error"]
            attempt_at = N._ns(attempt_events[qid]["at"])//1_000_000
            need(attempt_events[qid].get("captureAtMs", attempt_at) == attempt_at and N._ns(e["at"]) >= N._ns(original["at"]) >= attempt_at*1_000_000, "ABORT_CLOCK")
            need(failure.get("mayHaveExecuted") is False and failure.get("code") in ("PERMIT_EXPIRED_OR_INVALID","LIVE_CONTRACT_OR_BOOK_STALE") and "status" in failure and failure["status"] is None and "orderId" in failure and failure["orderId"] is None and type(e.get("proof")) is dict, "ABORT_ORIGINAL_ERROR")
            proof = e["proof"]
            need(proof.get("attemptAtMs") == attempt_at and proof.get("originalError") == failure and proof.get("queueScope") == b["accountScope"], "ABORT_PROOF_CONTEXT")
            aborted.add(qid)
        elif kind == "local_final_guard_no_http":
            need(qid in attempts and qid not in acks and qid not in aborted and e["originalIntentSha256"] == intents[qid] and e["originalAttemptSha256"] == attempts[qid] and e["originalErrorSha256"] == errors.get(qid), "LOCAL_GUARD_CONTEXT")
            original, attempt = error_events[qid], attempt_events[qid]
            failure, proof = original["error"], e.get("proof")
            attempt_at = N._ns(attempt["at"])//1_000_000
            need(attempt.get("captureAtMs", attempt_at) == attempt_at and N._ns(e["at"]) >= N._ns(original["at"]) >= attempt_at*1_000_000, "LOCAL_GUARD_CLOCK")
            need(type(failure.get("mayHaveExecuted")) is bool and "status" in failure and failure["status"] is None and "orderId" in failure and failure["orderId"] is None and type(proof) is dict, "LOCAL_GUARD_ORIGINAL_ERROR")
            need(proof.get("kind") == "sdk_ordinary_final_guard_no_http_v1" and proof.get("attemptAt") == attempt_at and proof.get("originalError") == failure and proof.get("originalSpec") == intent_events[qid].get("spec") and proof.get("originalPermit") == intent_events[qid].get("permit") and proof.get("queueScope") == b["accountScope"], "LOCAL_GUARD_PROOF_CONTEXT")
            need(proof.get("executionAuthority") is False and proof.get("retryPermitted") is False and N._HEX.fullmatch(proof.get("sdkSourceSha256", "")) and type(proof.get("verifiedSDKHead")) is dict and type(proof.get("binding")) is dict, "LOCAL_GUARD_PROOF_SHAPE")
            proof_tokens = tokens(tokens(eventraw)["proof"])
            context_raw = b"{" + b",".join(json.dumps(k).encode()+b":"+proof_tokens[k] for k in sorted(proof_tokens) if k != "contextSha256") + b"}"
            need(proof.get("contextSha256") == sha(context_raw), "LOCAL_GUARD_PROOF_HASH")
            # This checks original immutable SDK identity, not the private SDK
            # archive/OS state itself. The externally pinned producer asserts
            # opaque reverification AND current held SDK quiet above.
            binding, permit, spec = proof["binding"], intent_events[qid]["permit"], intent_events[qid]["spec"]
            payload_raw = b'{"scope":'+tokens(originals["control"])["scope"]+b',"spec":'+tokens(intent_raws[qid])["spec"]+b'}'
            need(spec.get("method") == "POST" and spec.get("endpoint") == "/orders" and spec["body"].get("tournamentId") == scope["tournamentId"] and spec["body"].get("exchangeId") in scope["exchanges"] and permit.get("owner") == "Codex", "LOCAL_GUARD_ORDINARY_SCOPE")
            need(binding.get("intent") == str(permit["runId"])+":"+str(permit["intentId"]) and binding.get("key") == spec["body"]["idempotencyKey"] and binding.get("payload_hash") == sha(payload_raw), "LOCAL_GUARD_IMMUTABLE_BINDING")
            aborted.add(qid); local_guards.add(qid)
        elif kind == "error":
            if e.get("operation") is None and qid in attempts and qid not in acks:
                need(qid not in aborted, "ERROR_AFTER_ABORT")
                errors[qid], error_events[qid] = sha(eventraw), e
        elif kind == "cancel_ack":
            need(qid in acks and qid not in cancel_acks, "CANCEL_ACK_WITHOUT_ORDER_OR_DUPLICATE")
            cancel_acks.add(qid)
        elif kind not in ("source","not_sent","cancel_attempt","cancel_ack"):
            raise PacketError("DISPATCH_EVENT_KIND")
    need(not (acks-closed) and not (set(attempts)-acks-aborted), "ACTUAL_NONTERMINAL_OR_UNKNOWN")
    need(type(b.get("provedNoNetworkOrderIds")) is list and all(integer(x) for x in b["provedNoNetworkOrderIds"]) and len(set(b["provedNoNetworkOrderIds"])) == len(b["provedNoNetworkOrderIds"]) and set(b["provedNoNetworkOrderIds"]) == aborted, "PRODUCER_ABORT_SET")
    package_qids = set()
    package_phases = {}
    for e,eventraw in package:
        need(e.get("kind") in {"intent","attempt","unknown","ack","never_attempted","definitive_zero","first_call_not_sent","not_sent"}, "PACKAGE_EVENT_KIND")
        if e["kind"] == "intent":
            need(e["packageId"] not in package_phases, "PACKAGE_DUPLICATE_INTENT")
            package_qids.update(x["qid"] for x in e["admission"]["reserve"]["legs"])
        else:
            need(e["packageId"] in package_phases, "PACKAGE_WITHOUT_INTENT")
        package_phases[e["packageId"]] = e["kind"]
        if e["kind"] == "ack":
            # Original typed receipt membership, not independent exchange truth.
            ack_receipts.add(typed_receipt_sha(tokens(eventraw)["receipt"]))
    need(not any(x in ("attempt","unknown") for x in package_phases.values()), "PACKAGE_UNRESOLVED")
    need(b.get("originalAckReceiptHashes") == sorted(ack_receipts), "ORIGINAL_ACK_RECEIPT_SET")
    packages = report["report"].get("livePackages")
    if packages is not None:
        need(packages.get("blocksNewSubmissions") is False, "PACKAGE_LIABILITY")
        for p in packages["records"]:
            need(p["phase"] in ("complete","no_execution"), "PACKAGE_LIABILITY")
            package_qids.update(x["qid"] for x in p["legs"])
    orders = report["orders"]
    need(type(orders) is list and len(orders) <= N.MAX_NATIVE_ORDERS, "ORDER_BOUND")
    ids, allids = [], set()
    fills = {f["orderId"] for f in report["fills"]}
    for order in orders:
        qid = order["id"]
        need(integer(qid) and qid > 0 and qid not in allids and type(order["retired"]) is bool, "ORDER_IDENTITY")
        allids.add(qid)
        if qid in aborted: need(order["retired"] and order["filled"] == 0 and qid not in fills, "ABORT_NATIVE_TERMINAL")
        if not order["retired"]:
            need(order["filled"] == 0 and qid not in intents and qid not in attempts and qid not in acks and qid not in fills and qid not in package_qids, "UNCLASSIFIED_CURRENT_ORDER")
            ids.append(qid)
    need(aborted <= allids and ids == m["neverSubmittedDraftIds"] == b["unsubmitted"], "COMPLETE_DRAFT_SET")
    r = report["report"]
    need(integer(r["unresolvedOrders"]) and r["unresolvedOrders"] <= len(ids) and integer(r["activeOrders"]) and r["activeOrders"] <= len(ids), "NATIVE_ORDER_COUNTS")
    binding = (expected_report_sha256,h,head,tuple(ids),N._micros(r["cash"],"CASH"),N._micros(r["reservedCash"],"RESERVATIONS"),m["sourceObservationMs"])
    # Recheck every detached original after parsing; never compare its inode to VPS origin.
    for label,raw in originals.items():
        current, identity = _read(root/(label+".json"))
        need(identity == identities[label] and current == raw, "PACKET_CHANGED")
    need(_read(root/"metadata.json") == (rawmeta,metaid), "METADATA_CHANGED")
    return VerifiedPacket(originals["report-original"],head,DraftClassification(binding,expected_metadata_sha256,_TOKEN),{
        "metadata_sha256":expected_metadata_sha256,"control_sha256":expected_control_sha256,
        "generation_sha256":expected_generation_sha256,"draft_ids":tuple(ids),
        "raw_dispatch_chains_verified":True,"original_receipt_membership_verified":True,
        "producer_held_sdk_quiet_and_lease_asserted":True,"sdk_or_os_state_independently_verified":False,
        "historical_abort_sdk_archive_independently_verified":False,
        "historical_aborts_producer_verified":tuple(sorted(aborted)),
        "historical_local_final_guards_producer_verified":tuple(sorted(local_guards)),
        "native_binding_and_package_semantics_producer_checked":True,
        "terminal_debt_model_available":False,"execution_authorized":False,
        "source_observation_ms":m["sourceObservationMs"],"captured_at_ms":capture},rawmeta,_TOKEN)


def verify_packet(directory, **expected_pins):
    """See _verify_packet. Malformed or incomplete evidence always refuses."""
    try:
        return _verify_packet(directory, **expected_pins)
    except (KeyError, TypeError, AttributeError, IndexError, OSError,
            N.NativeSnapshotError) as exc:
        raise PacketError("PACKET_MALFORMED_OR_INCOMPLETE: " + str(exc)) from exc
