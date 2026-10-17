"""Captured-depth research views from a verified Task175 packet; never authority.

`from_packet` preserves original clocks and returns alternative buy views plus
separate rotation sell curves. `select_buy_actions` selects at most one buy side
per exchange for the existing buy-only frontier. Selling and buying the same
original level, or simultaneous opposite-side/netting execution, is unsupported.
No sale curve credits acquisition cash. Fees and registry semantics are supplied
explicitly as scoped evidence; their hashes do not authenticate an exchange.
"""
from copy import deepcopy
import hashlib
import json
import re

try:
    from . import worker_packet as W
    from . import native_snapshot as N
except ImportError:
    import worker_packet as W
    import native_snapshot as N

UNIT = 1_000_000
MAX_BOOKS = 512
MAX_LEVELS = 64
MAX_QUANTITY = 10**9  # Existing target_frontier's per-level bound, never clipped.
_ID = re.compile(r"[A-Za-z0-9][A-Za-z0-9_.:-]{0,127}\Z")
_PRICE = re.compile(r"(?:0|[1-9][0-9]*)(?:\.[0-9]{1,6})?\Z")
_COMMON = {"schema_version", "evidence_class", "as_of_ms", "valid_until_ms",
           "source_sha256", "unmodeled_state_statement", "bank_manifest_hash"}


class DepthError(ValueError):
    pass


def need(ok, code):
    if not ok:
        raise DepthError(code)


def integer(value, code, low=0, high=2**63-1):
    need(type(value) is int and low <= value <= high, code)
    return value


def ident(value):
    need(type(value) is str and _ID.fullmatch(value), "IDENTITY_REQUIRED")
    return value


def digest(value):
    # This is a DERIVED normalized JSON witness, not an official receipt hash.
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(",", ":"),
                                    ensure_ascii=False, allow_nan=False).encode()).hexdigest()


def evidence(value, extra, manifest, now, code):
    need(type(value) is dict and set(value) == _COMMON | {extra}, code + "_SHAPE")
    need(type(value["schema_version"]) is int and value["schema_version"] == 1,
         code + "_VERSION")
    need(value["bank_manifest_hash"] == manifest and type(value["source_sha256"]) is str
         and N._HEX.fullmatch(value["source_sha256"]),
         code + "_BOUNDARY")
    need(type(value["evidence_class"]) is str and value["evidence_class"]
         and type(value["unmodeled_state_statement"]) is str and value["unmodeled_state_statement"],
         code + "_DESCRIPTION")
    start = integer(value["as_of_ms"], code + "_CLOCK")
    until = integer(value["valid_until_ms"], code + "_CLOCK")
    need(start <= now < until, code + "_EXPIRED_OR_FUTURE")


def levels(raw, bid):
    need(type(raw) is list and 1 <= len(raw) <= MAX_LEVELS, "BOOK_DEPTH_BOUND_OR_EMPTY")
    result = []
    for row in raw:
        need(type(row) is dict and set(row) == {"price", "quantity"}, "BOOK_LEVEL_SHAPE")
        p = row["price"]
        need(type(p) is str and len(p) <= 8 and _PRICE.fullmatch(p), "BOOK_PRICE_DECIMAL")
        whole, dot, fraction = p.partition(".")
        price = int(whole) * UNIT + int((fraction if dot else "").ljust(6, "0") or "0")
        need(0 < price < UNIT, "BOOK_PRICE_RANGE")
        n = integer(row["quantity"], "BOOK_QUANTITY", 1, MAX_QUANTITY)
        if result:
            need(result[-1][0] > price if bid else result[-1][0] < price,
                 "BOOK_DUPLICATE_OR_UNSORTED_LEVEL")
        result.append((price, n))
    return result


def book_fields(book, scope, contract, capture, now, maximum_age):
    need(type(book) is dict, "BOOK_SHAPE")
    need(book.get("invalid", False) is False, "BOOK_INVALID")
    ex, market = contract["exchange_id"], contract["market_id"]
    need(book.get("tournamentId") == scope["tournamentId"]
         and book.get("exchangeId") == ex and book.get("marketId") == market
         and scope["exchanges"].get(ex) == market, "BOOK_SCOPE")
    seq = book.get("sequence")
    need(type(seq) is str and re.fullmatch(r"[0-9]{1,20}", seq)
         and int(seq) < 2**64, "BOOK_SEQUENCE")
    try:
        clocks = {k: N._ns(book[k]) for k in ("sourceAt", "requestStartedAt", "receivedAt")}
    except (KeyError, N.NativeSnapshotError):
        raise DepthError("BOOK_CLOCK") from None
    source, request, received = (clocks[k] for k in ("sourceAt", "requestStartedAt", "receivedAt"))
    need(0 <= request <= received <= capture*1_000_000 <= now*1_000_000
         and 0 <= source <= capture*1_000_000, "BOOK_FUTURE_OR_NONCAUSAL")
    # Conservative original source AND request ages; a new capture is no renewal.
    deadline = (min(source, request) + maximum_age*1_000_000) // 1_000_000
    need(now < deadline, "BOOK_STALE")
    bids, asks = levels(book.get("bids"), True), levels(book.get("asks"), False)
    need(bids[0][0] < asks[0][0], "BOOK_CROSSED")
    core = {k: book[k] for k in ("tournamentId", "marketId", "exchangeId", "sequence",
                                "sourceAt", "requestStartedAt", "receivedAt", "bids", "asks")}
    return bids, asks, core, (received+999_999)//1_000_000, deadline


def from_packet(packet, *, expected_metadata_sha256, registry, fee_evidence,
                decision_ms, maximum_book_age_ms, exchange_ids=None):
    """Return dated alternatives/rejections, not a fresh quote or funded order.

    Registry is the existing native_snapshot registry schema. Fee evidence uses
    the same common fields and `fees: [{exchange_id, side: YES|NO,
    action: BUY|SELL, fee_per_share_micros}]`. All four fees are explicit for an
    accepted exchange. Native source/request/receipt clocks are never rewritten.
    """
    need(type(packet) is W.VerifiedPacket, "VERIFIED_PACKET_REQUIRED")
    need(type(expected_metadata_sha256) is str and N._HEX.fullmatch(expected_metadata_sha256)
         and packet.provenance["metadata_sha256"] == expected_metadata_sha256,
         "EXPECTED_METADATA_SHA256")
    metadata = packet.metadata
    books = packet.native_books
    now = integer(decision_ms, "DECISION_CLOCK")
    age = integer(maximum_book_age_ms, "DECLARED_BOOK_AGE_REQUIRED", 1, 86_400_000)
    capture = integer(metadata["capturedAtMs"], "CAPTURE_CLOCK")
    need(capture <= now, "CAPTURE_FUTURE")
    manifest, scope = metadata["bankManifestHash"], metadata["scope"]
    need(type(scope) is dict and type(scope.get("exchanges")) is dict
         and 0 < len(scope["exchanges"]) <= 4096, "PACKET_SCOPE")
    evidence(registry, "contracts", manifest, now, "REGISTRY")
    evidence(fee_evidence, "fees", manifest, now, "FEES")
    need(type(registry["contracts"]) is list and 0 < len(registry["contracts"]) <= MAX_BOOKS,
         "REGISTRY_BOUND")
    contracts, cids = {}, set()
    for row in registry["contracts"]:
        need(type(row) is dict and set(row) == {"contract_id", "exchange_id", "market_id", "semantic_source_sha256"},
             "REGISTRY_CONTRACT_SHAPE")
        cid, ex, market = (ident(row[k]) for k in ("contract_id", "exchange_id", "market_id"))
        need(ex not in contracts and cid not in cids and scope["exchanges"].get(ex) == market
             and type(row["semantic_source_sha256"]) is str
             and N._HEX.fullmatch(row["semantic_source_sha256"]), "REGISTRY_SCOPE_OR_DUPLICATE")
        contracts[ex] = row
        cids.add(cid)
    chosen = sorted(contracts) if exchange_ids is None else exchange_ids
    need(type(chosen) is list and 0 < len(chosen) <= MAX_BOOKS
         and all(type(x) is str and x in contracts for x in chosen)
         and len(set(chosen)) == len(chosen), "EXCHANGE_SELECTION")
    need(type(fee_evidence["fees"]) is list and len(fee_evidence["fees"]) <= 4*MAX_BOOKS,
         "FEE_BOUND")
    fees = {}
    for row in fee_evidence["fees"]:
        need(type(row) is dict and set(row) == {"exchange_id", "side", "action", "fee_per_share_micros"},
             "FEE_SHAPE")
        key = (row["exchange_id"], row["side"], row["action"])
        need(type(key[0]) is str and key[0] in contracts and key[1] in ("YES", "NO") and key[2] in ("BUY", "SELL")
             and key not in fees, "FEE_IDENTITY_OR_DUPLICATE")
        fees[key] = integer(row["fee_per_share_micros"], "FEE_VALUE", 0, UNIT)
    need(type(books) is list and len(books) <= MAX_BOOKS, "BOOK_COLLECTION_BOUND")
    indexed, rejections = {}, []
    for book in books:
        ex = book.get("exchangeId") if type(book) is dict else None
        if type(ex) is not str or ex not in contracts:
            rejections.append({"exchange_id": ex if type(ex) is str else None,
                               "market_id": None, "reason": "BOOK_UNBOUND"})
        else:
            indexed.setdefault(ex, []).append(book)
    buys, sells, resources, accepted = [], [], [], []
    for ex in chosen:
        contract = contracts[ex]
        try:
            candidates = indexed.get(ex, [])
            need(len(candidates) == 1, "BOOK_UNAVAILABLE" if not candidates else "BOOK_DUPLICATE_OR_CONFLICT")
            bids, asks, core, available, deadline = book_fields(candidates[0], scope, contract, capture, now, age)
            need(all((ex, side, action) in fees for side in ("YES", "NO") for action in ("BUY", "SELL")),
                 "EXPLICIT_SIDE_FEES_UNAVAILABLE")
            deadline = min(deadline, registry["valid_until_ms"], fee_evidence["valid_until_ms"])
            # Evidence must itself be available before any prospective decision.
            available = max(available, registry["as_of_ms"], fee_evidence["as_of_ms"], capture)
            need(available <= now < deadline, "DEPTH_WINDOW_UNAVAILABLE")
            book_sha = digest(core)
            evidence_sha = digest({"metadata_sha256": expected_metadata_sha256,
                                   "normalized_book_json_sha256": book_sha,
                                   "registry_sha256": registry["source_sha256"],
                                   "fee_evidence_sha256": fee_evidence["source_sha256"]})
            resource_sets = {}
            for native_side, rows in (("bids", bids), ("asks", asks)):
                ids = []
                for i, (price, n) in enumerate(rows):
                    rid = digest({"metadata_sha256": expected_metadata_sha256, "exchange_id": ex,
                                  "native_side": native_side, "level_index": i})
                    ids.append(rid)
                    resources.append({"id": rid, "exchange_id": ex, "native_side": native_side,
                                      "level_index": i, "yes_price_micros": price, "quantity": n})
                resource_sets[native_side] = ids
            for side, action, native_side, rows in (("YES", "BUY", "asks", asks), ("NO", "BUY", "bids", bids),
                                                     ("YES", "SELL", "bids", bids), ("NO", "SELL", "asks", asks)):
                fee, quantity, cash, curve = fees[(ex, side, action)], 0, 0, []
                for i, (yes_price, n) in enumerate(rows):
                    price = yes_price if side == "YES" else UNIT-yes_price
                    quantity += n
                    cash += n * (price+fee if action == "BUY" else price-fee)
                    curve.append({"quantity": n, "unit_cost_micros" if action == "BUY" else "unit_proceeds_micros": price,
                                  "resource_id": resource_sets[native_side][i], "prefix_quantity": quantity,
                                  "prefix_gross_cost_micros" if action == "BUY" else "prefix_net_proceeds_micros": cash})
                view = {"id": f"{action.lower()}:{ex}:{side}", "contract_id": contract["contract_id"],
                        "exchange_id": ex, "market_id": contract["market_id"], "side": side,
                        "action": action, "levels": curve, "fee_per_share_micros": fee,
                        "source_sha256": evidence_sha, "available_at_ms": available, "valid_until_ms": deadline,
                        "metadata_sha256": expected_metadata_sha256, "normalized_book_json_sha256": book_sha,
                        "official_receipt_sha256": None, "semantic_source_sha256": contract["semantic_source_sha256"],
                        "original_book": {k: core[k] for k in ("sequence", "sourceAt", "requestStartedAt", "receivedAt")},
                        "capture_at_ms": capture, "captured_depth_only": True, "execution_authorized": False}
                (buys if action == "BUY" else sells).append(view)
            accepted.append({"exchange_id": ex, "market_id": contract["market_id"],
                             "contract_id": contract["contract_id"], "normalized_book_json_sha256": book_sha})
        except DepthError as exc:
            rejections.append({"exchange_id": ex, "market_id": contract["market_id"], "reason": str(exc)})
    return {"schema_version": 1, "status": "partial" if accepted and rejections else "available" if accepted else "unavailable",
            "metadata_sha256": expected_metadata_sha256, "bank_manifest_hash": manifest,
            "native_head": deepcopy(metadata["nativeHead"]), "decision_ms": now,
            "maximum_book_age_ms": age, "accepted_markets": accepted, "rejections": rejections,
            "buy_action_views": buys, "rotation_sell_curves": sells, "liquidity_resources": resources,
            "shared_liquidity_rule": "YES BUY and NO SELL share asks; NO BUY and YES SELL share bids. Consume each resource at most once.",
            "simultaneous_buy_sell_supported": False, "opposite_pair_simultaneous_use_supported": False,
            "sale_proceeds_are_available_cash": False, "captured_depth_only": True,
            "forecast_or_profit_estimate": None, "execution_authorized": False, "calibrated_probability_claimed": False}


def select_buy_actions(result, side_by_exchange):
    """Choose one alternative per exchange; return the existing frontier's list.

    Does not select a forecast, net a held opposite side, or credit a sale. The
    frontier must still recheck actual gross cash and its complete account.
    """
    need(type(result) is dict and result.get("execution_authorized") is False
         and type(side_by_exchange) is dict and 0 < len(side_by_exchange) <= 64, "BUY_SELECTION")
    views = {(v["exchange_id"], v["side"]): v for v in result["buy_action_views"]}
    need(all(side in ("YES", "NO") and (ex, side) in views for ex, side in side_by_exchange.items()),
         "BUY_SELECTION_UNAVAILABLE_OR_SIDE_ALIAS")
    chosen = [deepcopy(views[(ex, side)]) for ex, side in sorted(side_by_exchange.items())]
    need(sum(len(x["levels"]) for x in chosen) <= 256, "FRONTIER_DEPTH_MATRIX_BOUND")
    return chosen
