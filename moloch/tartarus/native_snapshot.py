"""Read-only sealed-native snapshot adapter for Tartarus Contract A.

This verifies a supplied byte pin and native outer raw-token seal, not account
API membership, full financial replay, resolver semantics, or live authority.
Missing registry/obligation evidence remains unavailable. An old authenticated
snapshot can still yield a clearly dated loose static holdings bound.
"""
from copy import deepcopy
from datetime import datetime, timezone
from decimal import Decimal
import hashlib
import json
import re

try:
    from . import account_scenarios as A
except ImportError:
    import account_scenarios as A

MAX_REPORT_BYTES = 64 * 1024 * 1024
MAX_EXCHANGES = 4096
MAX_NATIVE_ORDERS = 200_000
_HEX = re.compile(r"[0-9a-f]{64}\Z")
_PRICE = re.compile(r"\d+(?:\.\d{1,6})?\Z")
_CLOCK = re.compile(r"(\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d)(?:\.(\d{1,9}))?(Z|[+-]\d\d:\d\d)\Z")
_COMMON = {"schema_version", "evidence_class", "as_of_ms", "valid_until_ms",
           "source_sha256", "unmodeled_state_statement"}


class NativeSnapshotError(ValueError):
    def __init__(self, code):
        self.code = code
        super().__init__(code)


def _need(ok, code):
    if not ok:
        raise NativeSnapshotError(code)


def _int(n, code, minimum=0):
    _need(type(n) is int and minimum <= n <= A.MAX_INTEGER, code)
    return n


def _hex(s, code):
    _need(type(s) is str and _HEX.fullmatch(s) is not None, code)
    return s


def _micros(s, code):
    _need(type(s) is str and len(s) <= 40 and _PRICE.fullmatch(s) is not None, code)
    a, _, b = s.partition(".")
    return _int(int(a) * A.UNIT + int(b.ljust(6, "0") or "0"), code)


def _ns(s):
    _need(type(s) is str, "SNAPSHOT_CLOCK")
    match = _CLOCK.fullmatch(s)
    _need(match is not None, "SNAPSHOT_CLOCK")
    try:
        t = datetime.fromisoformat(match[1] + match[3].replace("Z", "+00:00"))
        delta = t.astimezone(timezone.utc) - datetime(1970, 1, 1, tzinfo=timezone.utc)
        n = (delta.days * 86400 + delta.seconds) * 1_000_000_000 + int((match[2] or "").ljust(9, "0") or "0")
    except (ValueError, OverflowError) as exc:
        raise NativeSnapshotError("SNAPSHOT_CLOCK") from exc
    _need(n >= 0, "SNAPSHOT_CLOCK")
    return n


def _unique(pairs):
    out = {}
    for key, value in pairs:
        _need(key not in out, "DUPLICATE_JSON_KEY")
        out[key] = value
    return out


def _constant(_):
    raise NativeSnapshotError("NONFINITE_JSON")


_DECODER = json.JSONDecoder(parse_float=Decimal, parse_constant=_constant, object_pairs_hook=_unique)


def _sealed(raw):
    """Hash the ORIGINAL outer tokens, avoiding arbitrary-precision reencoding."""
    _need(type(raw) is bytes and 0 < len(raw) <= MAX_REPORT_BYTES, "REPORT_BYTE_BOUND")
    _need(raw.endswith(b"\n") and not raw.endswith(b"\r\n"), "ORIGINAL_REPORT_LF_REQUIRED")
    try:
        text = raw[:-1].decode("utf-8")
        _need(text.startswith("{") and text.endswith("}"), "REPORT_OBJECT_LAYOUT")
        at, values, spans = 1, {}, {}
        while at < len(text) - 1:
            start = at
            key, end = _DECODER.raw_decode(text, at)
            _need(type(key) is str and key not in values and text[end:end+1] == ":", "REPORT_FIELD_LAYOUT")
            value, at = _DECODER.raw_decode(text, end + 1)
            values[key], spans[key] = value, (start, at)
            if at == len(text) - 1:
                break
            _need(text[at:at+1] == ",", "REPORT_FIELD_LAYOUT")
            at += 1
        _need(at == len(text) - 1 and "hash" in values, "REPORT_FIELD_LAYOUT")
        _hex(values["hash"], "REPORT_SEAL_HASH")
        start, end = spans["hash"]
        body = text[:start-1] + text[end:] if start > 1 else text[:start] + text[end+1:]
        _need(hashlib.sha256(body.encode()).hexdigest() == values["hash"], "REPORT_RAW_BODY_SEAL")
        return values
    except (UnicodeDecodeError, json.JSONDecodeError, IndexError) as exc:
        raise NativeSnapshotError("REPORT_JSON") from exc


def _head(head, manifest):
    _need(type(head) is dict and set(head) == {"hash", "manifestHash", "sequence", "previous"}, "EXPECTED_HEAD_SHAPE")
    _int(head["sequence"], "EXPECTED_HEAD_SEQUENCE")
    _need(head["manifestHash"] == manifest, "EXPECTED_HEAD_MANIFEST")
    _need(head["previous"] == "" and head["sequence"] == 0 or type(head["previous"]) is str and _HEX.fullmatch(head["previous"]) is not None, "EXPECTED_HEAD_PREVIOUS")
    body = {k: v for k, v in head.items() if k != "hash"}
    digest = hashlib.sha256(json.dumps(body, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
    _need(head["hash"] == digest, "EXPECTED_HEAD_SEAL")


def _evidence(value, extra, prefix, decision, reasons):
    if value is None:
        reasons.append(prefix + "_UNAVAILABLE")
        return False
    _need(type(value) is dict and not set(value) - (_COMMON | extra), prefix + "_SHAPE")
    if any(k not in value or value[k] is None for k in _COMMON | extra):
        reasons.append(prefix + "_FIELDS_UNAVAILABLE")
        return False
    _need(type(value["schema_version"]) is int and value["schema_version"] == 1, prefix + "_VERSION")
    _hex(value["source_sha256"], prefix + "_SOURCE_SHA256")
    for field in ("evidence_class", "unmodeled_state_statement"):
        _need(type(value[field]) is str and 0 < len(value[field]) <= 8192 and value[field].strip(), prefix + "_STATEMENT")
    start = _int(value["as_of_ms"], prefix + "_AS_OF")
    end = _int(value["valid_until_ms"], prefix + "_VALID_UNTIL")
    _need(start < end, prefix + "_CLOCK_WINDOW")
    if start > decision:
        reasons.append(prefix + "_FUTURE")
    if decision >= end:
        reasons.append(prefix + "_EXPIRED")
    return True


def adapt_native_snapshot(report_bytes, *, expected_report_sha256, expected_manifest_sha256,
                          expected_head, source_observation_ms, decision_ms, valid_until_ms,
                          registry=None, liabilities=None):
    """Return an exact Contract A input only when all declared evidence fits.

    expected_manifest_sha256 is native manifestHash (not the manifest file's raw
    byte SHA). registry and liabilities are explicitly scoped normalized source
    evidence; their raw receipt/semantic membership is NOT independently proved
    here. This first version refuses native unretired orders pending original
    physical/draft classification, even if a caller says official open=[]:
    a native draft is not itself an official order or a zero liability proof.
    """
    _hex(expected_report_sha256, "EXPECTED_REPORT_SHA256")
    _hex(expected_manifest_sha256, "EXPECTED_MANIFEST_HASH")
    _need(type(report_bytes) is bytes and 0 < len(report_bytes) <= MAX_REPORT_BYTES, "REPORT_BYTE_BOUND")
    _need(hashlib.sha256(report_bytes).hexdigest() == expected_report_sha256, "REPORT_RAW_SHA256")
    _head(expected_head, expected_manifest_sha256)
    d = _sealed(report_bytes)
    _need(set(d) == {"contestOrders", "fills", "hash", "manifestHash", "orders", "previous", "report", "sequence", "sourceCursors"}, "REPORT_ENVELOPE_SHAPE")
    _need(d["manifestHash"] == expected_manifest_sha256 and d["sequence"] == expected_head["sequence"] and d["previous"] == expected_head["previous"], "REPORT_EXPECTED_BOUNDARY")
    _need(type(d["sequence"]) is int and type(d["report"]) is dict, "REPORT_SHAPE")
    decision = _int(decision_ms, "DECISION_CLOCK")
    r = d["report"]
    snapshot_at = r.get("at")
    snapshot_ns = _ns(snapshot_at)
    cash = _micros(r.get("cash"), "NATIVE_CASH")
    reserved = _micros(r.get("reservedCash"), "NATIVE_RESERVED_CASH")
    _need(reserved <= cash, "NATIVE_RESERVATIONS_EXCEED_CASH")
    native_cost = _micros(r.get("ownedCost"), "NATIVE_OWNED_COST")
    positions = r.get("positions")
    _need(type(positions) is dict and len(positions) <= MAX_EXCHANGES, "NATIVE_POSITIONS_BOUND")
    held, clocks, qty, basis, zero_lots, lot_count = [], [], 0, 0, 0, 0
    for exchange, sides in positions.items():
        _need(type(exchange) is str and A._ID.fullmatch(exchange) is not None and type(sides) is dict and set(sides) == {"YES", "NO"}, "NATIVE_POSITION_SCOPE")
        for side in ("YES", "NO"):
            lots = sides[side]
            _need(type(lots) is list and len(lots) <= A.MAX_LOTS, "NATIVE_LOT_BOUND")
            normalized, original_clocks = [], []
            for lot in lots:
                _need(type(lot) is dict and {"quantity", "price"} <= set(lot) <= {"quantity", "price", "executedAt"}, "NATIVE_LOT_SHAPE")
                q = _int(lot["quantity"], "NATIVE_LOT_QUANTITY")
                unit = _micros(lot["price"], "NATIVE_LOT_PRICE")
                _need(unit <= A.UNIT, "NATIVE_LOT_PRICE_EXCEEDS_PAYOUT")
                execution = lot.get("executedAt")
                if execution is not None:
                    _need(_ns(execution) <= snapshot_ns, "NATIVE_LOT_EXECUTION_FUTURE")
                if q == 0:
                    zero_lots += 1
                    continue
                lot_count += 1
                _need(lot_count <= A.MAX_LOTS, "NATIVE_TOTAL_LOT_BOUND")
                normalized.append({"quantity": q, "unit_cost_micros": unit})
                original_clocks.append(execution)
            if normalized:
                quantity = _int(sum(x["quantity"] for x in normalized), "NATIVE_HOLDING_QUANTITY_RANGE")
                cost = _int(sum(x["quantity"] * x["unit_cost_micros"] for x in normalized), "NATIVE_HOLDING_COST_RANGE")
                held.append({"exchange_id": exchange, "side": side, "quantity": quantity, "lots": normalized})
                clocks.append({"exchange_id": exchange, "side": side, "executed_at_original": original_clocks})
                qty = _int(qty + quantity, "NATIVE_TOTAL_QUANTITY_RANGE")
                basis = _int(basis + cost, "NATIVE_TOTAL_BASIS_RANGE")
    _need(basis == native_cost, "NATIVE_REMAINING_BASIS_MISMATCH")
    _need(len(held) <= A.MAX_HOLDINGS, "NATIVE_HOLDING_BOUND")
    orders = d["orders"]
    _need(type(orders) is list and len(orders) <= MAX_NATIVE_ORDERS, "NATIVE_ORDER_BOUND")
    unretired = 0
    ids = set()
    for order in orders:
        _need(type(order) is dict, "NATIVE_ORDER_SHAPE")
        qid = _int(order.get("id"), "NATIVE_ORDER_ID", 1)
        _need(qid not in ids and type(order.get("retired")) is bool, "NATIVE_ORDER_IDENTITY")
        ids.add(qid)
        filled = _int(order.get("filled"), "NATIVE_ORDER_FILLED")
        _need(filled <= _int(order.get("quantity"), "NATIVE_ORDER_QUANTITY", 1), "NATIVE_ORDER_FILLED")
        unretired += not order["retired"]
    active = _int(r.get("activeOrders"), "NATIVE_ACTIVE_ORDER_COUNT")
    unresolved = _int(r.get("unresolvedOrders"), "NATIVE_UNRESOLVED_ORDER_COUNT")
    _need(active <= unretired, "NATIVE_ACTIVE_ORDER_COUNT")
    reasons = []
    if source_observation_ms is None:
        reasons.append("SOURCE_OBSERVATION_UNAVAILABLE")
    else:
        observed = _int(source_observation_ms, "SOURCE_OBSERVATION_CLOCK")
        if observed * 1_000_000 < snapshot_ns:
            reasons.append("SOURCE_OBSERVATION_PRECEDES_SNAPSHOT")
        if observed > decision:
            reasons.append("SOURCE_OBSERVATION_FUTURE")
    if snapshot_ns > decision * 1_000_000:
        reasons.append("SNAPSHOT_FUTURE")
    if valid_until_ms is None:
        reasons.append("SOURCE_VALIDITY_UNAVAILABLE")
    else:
        deadline = _int(valid_until_ms, "SOURCE_VALIDITY_CLOCK")
        if source_observation_ms is not None:
            _need(deadline > observed, "SOURCE_VALIDITY_WINDOW")
        if decision >= deadline:
            reasons.append("SNAPSHOT_EXPIRED")
    contracts, by_exchange = [], {}
    if _evidence(registry, {"bank_manifest_hash", "contracts"}, "REGISTRY", decision, reasons):
        _need(registry["bank_manifest_hash"] == expected_manifest_sha256, "REGISTRY_MANIFEST_MISMATCH")
        rows = registry["contracts"]
        _need(type(rows) is list and 0 < len(rows) <= A.MAX_CONTRACTS, "REGISTRY_CONTRACT_BOUND")
        contract_ids = set()
        for row in rows:
            fields = {"contract_id", "exchange_id", "market_id", "semantic_source_sha256"}
            _need(type(row) is dict and not set(row) - fields, "REGISTRY_CONTRACT_SHAPE")
            if set(row) != fields or any(row[k] is None for k in fields):
                reasons.append("EXACT_REGISTRY_IDENTITY_UNAVAILABLE")
                continue
            for key in ("contract_id", "exchange_id", "market_id"):
                _need(type(row[key]) is str and A._ID.fullmatch(row[key]) is not None, "REGISTRY_CONTRACT_IDENTITY")
            _hex(row["semantic_source_sha256"], "REGISTRY_SEMANTIC_SOURCE_SHA256")
            _need(row["exchange_id"] not in by_exchange and row["contract_id"] not in contract_ids, "REGISTRY_IDENTITY_CONFLICT")
            by_exchange[row["exchange_id"]] = row
            contract_ids.add(row["contract_id"])
            contracts.append({k: row[k] for k in ("contract_id", "exchange_id", "semantic_source_sha256")})
        if any(h["exchange_id"] not in by_exchange for h in held):
            reasons.append("EXACT_HELD_CONTRACT_REGISTRY_UNAVAILABLE")
    debt = None
    if _evidence(liabilities, {"report_sha256", "head", "terminal_debt_micros", "pending_obligations", "physical_open_orders", "unknown_acceptance", "unresolved_operations"}, "LIABILITY_EVIDENCE", decision, reasons):
        _need(liabilities["report_sha256"] == expected_report_sha256 and liabilities["head"] == expected_head, "LIABILITY_SNAPSHOT_BINDING")
        if liabilities["as_of_ms"] * 1_000_000 < snapshot_ns:
            reasons.append("LIABILITY_OBSERVATION_PRECEDES_SNAPSHOT")
        debt = _int(liabilities["terminal_debt_micros"], "TERMINAL_DEBT_AMOUNT")
        _need(type(liabilities["unknown_acceptance"]) is bool, "LIABILITY_UNKNOWN_FLAG")
        if liabilities["unknown_acceptance"]:
            reasons.append("UNKNOWN_ACCEPTANCE_UNAVAILABLE")
        for key in ("pending_obligations", "physical_open_orders", "unresolved_operations"):
            _need(type(liabilities[key]) is list and len(liabilities[key]) <= 10_000, "LIABILITY_COLLECTION_BOUND")
            if liabilities[key]:
                reasons.append(key.upper() + "_UNAVAILABLE")
    if unretired or unresolved:
        reasons.append("NATIVE_OPEN_ORDER_CLASSIFICATION_UNAVAILABLE")
    static = {"held_sides": len(held), "absolute_shares": qty, "remaining_fifo_principal_micros": basis,
              "cash_micros": cash, "reserved_cash_micros": reserved,
              "cash_plus_all_sides_pay_one_ceiling_micros": _int(cash + qty * A.UNIT, "STATIC_BOUND_RANGE"),
              "native_active_orders": active, "native_unretired_orders": unretired,
              "native_unresolved_orders": unresolved, "explicit_zero_lots_skipped": zero_lots,
              "scope": "Dated loose static bound only; future execution, terminal debt and future fees excluded. Both sides paying one is not a joint outcome certificate."}
    account = None
    if not reasons:
        deadline = min(valid_until_ms, registry["valid_until_ms"], liabilities["valid_until_ms"])
        combined = {"report_sha256": expected_report_sha256, "head": expected_head,
                    "source_observation_ms": source_observation_ms, "valid_until_ms": valid_until_ms,
                    "registry": registry, "liabilities": liabilities}
        combined_sha = hashlib.sha256(json.dumps(combined, sort_keys=True, separators=(",", ":"), allow_nan=False).encode()).hexdigest()
        account = {"schema_version": 1, "evidence_class": registry["evidence_class"] if registry["evidence_class"] == liabilities["evidence_class"] else "mixed",
                   "as_of_ms": max(source_observation_ms, registry["as_of_ms"], liabilities["as_of_ms"]),
                   "valid_until_ms": deadline, "source_sha256": combined_sha,
                   "cash_micros": cash, "reserved_cash_micros": reserved, "terminal_debt_micros": debt,
                   "pending_obligations": [], "holdings": [{"contract_id": by_exchange[h["exchange_id"]]["contract_id"], **h, "source_sha256": expected_report_sha256} for h in held],
                   "unmodeled_state_statement": "Sealed dated native snapshot with separately declared scoped registry/liability evidence; not a live account, independent source-membership proof, or exhaustive scenario model. Full source statements and pins remain in adapter provenance."}
    return {"schema_version": 1, "status": "unavailable" if reasons else "available",
            "unavailable_reasons": list(dict.fromkeys(reasons)), "account_input": account,
            "contracts": contracts, "static_held_bounds": static,
            "snapshot": {"sequence": d["sequence"], "at_original": snapshot_at, "at_ns": str(snapshot_ns),
                         "source_observation_ms": source_observation_ms, "valid_until_ms": valid_until_ms,
                         "report_raw_sha256": expected_report_sha256, "report_seal_sha256": d["hash"],
                         "manifest_hash": expected_manifest_sha256, "head": deepcopy(expected_head)},
            "source_evidence": {"registry": deepcopy(registry), "liabilities": deepcopy(liabilities)},
            "original_lot_clocks": clocks, "evidence_class": "sealed_dated_native_snapshot_research_only",
            "raw_report_pin_and_seal_verified": True, "financial_prefix_replay_performed": False,
            "official_receipt_membership_independently_verified": False,
            "registry_semantics_independently_verified": False,
            "terminal_debt_micros": debt, "decision_ms": decision,
            "execution_authorized": False, "calibrated_probability_claimed": False}
