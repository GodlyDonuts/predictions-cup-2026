#!/usr/bin/env python3
"""One-pass frozen-journal FIFO/exit-window audit. No networking or trading."""
from __future__ import annotations
import argparse
from collections import Counter, defaultdict, deque
from decimal import Decimal
import hashlib
import importlib.util
import json
from pathlib import Path
import sys


def load(name, relative):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).resolve().parents[1] / relative)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


FIFO = load("exit_window_fifo", "moloch-actual-economics-20261016/analyze.py")
RAW = load("exit_window_raw", "moloch-owned-fill-markouts-20261015/analyze.py")
SCALE, MS = 1_000_000, 1_000_000
HORIZONS = (60_000, 180_000, 300_000, 600_000)
need, clock, units, money = FIFO.need, FIFO.clock, FIFO.units, FIFO.money


def distribution(values):
    values = sorted(values)
    if not values:
        return {"count": 0, "minimumMs": None, "medianMs": None, "p90Ms": None, "maximumMs": None}
    return {"count": len(values), "minimumMs": str(Decimal(values[0]) / MS),
            "medianMs": str(Decimal(values[(len(values)-1)//2]) / MS),
            "p90Ms": str(Decimal(values[(len(values)-1)*9//10]) / MS),
            "maximumMs": str(Decimal(values[-1]) / MS)}


def clock_summary(cohorts, closes, acknowledgments):
    ordinary=[c for c in cohorts if c["origin"]=="ordinary"]
    rows=[r for r in closes if cohorts[r["cohort"]]["origin"]=="ordinary"]
    stage=lambda key:distribution([r["quote_created"]-cohorts[r["cohort"]][key]for r in rows if r["quote_created"]>=cohorts[r["cohort"]][key]])
    ack_known=[c for c in ordinary if c["qid"]in acknowledgments]
    return {"unit":"Unweighted actual BUY-fill cohorts or FIFO-close fragments; not current-service cycle benchmarks.",
            "buyExecutionToFillReceipt":distribution([c["received"]-c["executed"]for c in ordinary]),
            "fillReceiptToNativeIngestion":distribution([c["observed"]-c["received"]for c in ordinary]),
            "nativeFillIngestionToExitQuoteCreated":stage("observed"),
            "quotePredatesThisLotObservationFragments":sum(r["quote_created"]<cohorts[r["cohort"]]["observed"]for r in rows),
            "exitQuoteCreatedToActualFillExecution":distribution([r["executed"]-r["quote_created"]for r in rows]),
            "actualExitExecutionToReceipt":distribution([r["received"]-r["executed"]for r in rows]),
            "exitReceiptToNativeIngestion":distribution([r["observed"]-r["received"]for r in rows]),
            "buyExecutionToOriginalPostAckReceiptSigned":distribution([acknowledgments[c["qid"]]-c["executed"]for c in ack_known]),
            "originalPostAckBeforeBuyExecutionCohorts":sum(acknowledgments[c["qid"]]<c["executed"]for c in ack_known),
            "originalPostAckMissingOrRecoveredCohorts":len(ordinary)-len(ack_known),
            "originalPostAckToNativeFillIngestion":distribution([c["observed"]-acknowledgments[c["qid"]]for c in ack_known]),
            "missingNativeFreshBookAfterExecutionCohorts":sum(c.get("first_native_post_execution")is None for c in ordinary),
            "missingNativeFreshBookAfterFillIngestionCohorts":sum(c.get("first_native_post_observation")is None for c in ordinary),
            "buyExecutionToFirstNativeFreshBook":distribution([c["first_native_post_execution"]-c["executed"]for c in ordinary if c.get("first_native_post_execution")is not None]),
            "nativeFillIngestionToFirstNativeFreshBook":distribution([c["first_native_post_observation"]-c["observed"]for c in ordinary if c.get("first_native_post_observation")is not None])}


def trace(events, orders, forecast=()):
    """Clock attribution only; the existing exact analyzer independently proves financial parity."""
    lots, cohorts, closes, seen = defaultdict(deque), [], [], {}
    forecasts = set(map(str, forecast))
    def add(ex, side, n, p, origin, executed, observed, qid=None, received=None):
        i = len(cohorts)
        cohorts.append(dict(exchange=ex, side=side, n=n, p=p, origin=origin, executed=executed,
                            observed=observed, received=observed if received is None else received, qid=qid, remaining=n, closed=0, gross=0, fee=0,
                            first_post_execution=None, first_post_observation=None,
                            labels={}, gains={h: None for h in HORIZONS}, valid_books=Counter()))
        lots[ex, side].append([i, n])
        return i
    def consume(ex, side, n, p, executed, observed, received, order, mechanism):
        need(sum(x[1] for x in lots[ex, side]) >= n, "TRACE_UNCOVERED_CLOSE")
        while n:
            ci, left = lots[ex, side][0]; c = cohorts[ci]; take = min(left, n)
            need(executed >= c["executed"], "TRACE_NEGATIVE_AGE")
            gain = take * (p-c["p"]); c["remaining"] -= take; c["closed"] += take; c["gross"] += gain
            closes.append(dict(cohort=ci, n=take, gross=gain, executed=executed, observed=observed, received=received,
                               quote_created=int(order.get("createdAt", 0))*MS,
                               reason=order.get("inventoryExitReason", "other"), mechanism=mechanism))
            n -= take; lots[ex, side][0][1] -= take
            if not lots[ex, side][0][1]: lots[ex, side].popleft()
    for wrapper in events:
        e = wrapper["event"]
        if e["type"] == "actual_position_adoption":
            for row in e["positions"]:
                for lot in row["lots"]:
                    add(str(row["exchangeId"]), lot["side"], lot["quantity"], units(lot["entryPrice"]),
                        "inherited", clock(lot["openedAt"]), clock(e["processedAt"]))
            continue
        fact = FIFO.fill_fact(e); key = fact[:2]
        if key in seen:
            need(seen[key] == fact, "TRACE_CHANGED_DUPLICATE"); continue
        seen[key] = fact
        actual, fid, qid, ex, mid, side, action, n, p, fee, executed = fact
        observed = clock(e.get("processedAt", e["receivedAt"])); received=clock(e["receivedAt"]);q = orders[qid]
        need(executed<=received<=observed,"TRACE_RECEIPT_CLOCK")
        if action == "buy":
            ci = add(ex, side, n, p, "forecast" if qid in forecasts else "ordinary", executed, observed, qid,received)
            cohorts[ci]["fee"] = fee
            other = "NO" if side == "YES" else "YES"
            pairs = min(sum(x[1] for x in lots[ex, side]), sum(x[1] for x in lots[ex, other]))
            if pairs:
                consume(ex, side, pairs, p, executed, observed, received, q, "oppositeBuyPair")
                consume(ex, other, pairs, SCALE-p, executed, observed, received, q, "oppositeBuyPair")
        else:
            consume(ex, side, n, p, executed, observed, received, q, "sell")
    return cohorts, closes


def book(raw, received, processed, transport, proof):
    ex = FIFO.ident(raw["exchangeId"])
    source = clock(raw.get("sourceAt", raw.get("asOf", {}).get("at")))
    received, processed = clock(received), clock(processed)
    request = clock(raw["requestStartedAt"]) if "requestStartedAt" in raw else received
    need(source <= received <= processed and request <= received, "BOOK_CLOCK_ORDER")
    if "receivedAt" in raw: need(clock(raw["receivedAt"]) == received, "BOOK_RECEIPT_MISMATCH")
    status = "valid"
    if raw.get("invalid") is True: status = "invalid"
    if max(processed-source, processed-received) > 10_000*MS: status = "stale_source_or_observer"
    if transport != "official_sig_realtime" and processed-request > 10_000*MS: status = "stale_rest_request"
    levels = {}
    for name, descending in (("bids", True), ("asks", False)):
        result = []
        for level in raw.get(name, []):
            p, n = units(level["price"]), FIFO.integer(level["quantity"], True)
            need(0 < p < SCALE and p % 5_000 == 0, "BOOK_TICK")
            need(not result or (p < result[-1][0] if descending else p > result[-1][0]), "BOOK_SORT")
            result.append((p,n))
        levels[name] = result
    if not levels["bids"] or not levels["asks"]: status = "empty"
    elif levels["bids"][0][0] >= levels["asks"][0][0]: status = "crossed"
    return dict(exchange=ex, source=source, received=received, processed=processed,
                status=status, proof=proof, **levels)


def full_liquidation(b, side, n):
    levels = b["bids"] if side == "YES" else [(SCALE-p,q) for p,q in b["asks"]]
    remaining, proceeds, marginal = n, 0, None
    for p,q in levels:
        take = min(q, remaining); proceeds += p*take; remaining -= take; marginal = p
        if not remaining: return proceeds, marginal
    return None


def observe(cohorts, b, *, window_ms=30_000):
    for c in cohorts:
        if c["origin"] != "ordinary" or c["exchange"] != b["exchange"]: continue
        age = b["processed"]-c["executed"]
        if age < 0: continue
        if b["status"] != "valid": continue
        if b["source"] < c["executed"] or b["received"] < c["executed"]: continue
        if c["first_post_execution"] is None or b["processed"] < c["first_post_execution"]:
            c["first_post_execution"] = b["processed"]
        if b["processed"] >= c["observed"] and (c["first_post_observation"] is None or b["processed"] < c["first_post_observation"]):
            c["first_post_observation"] = b["processed"]
        if b.get("lane")=="native":
            if c.get("first_native_post_execution")is None or b["processed"]<c["first_native_post_execution"]:c["first_native_post_execution"]=b["processed"]
            if b["processed"]>=c["observed"]and(c.get("first_native_post_observation")is None or b["processed"]<c["first_native_post_observation"]):c["first_native_post_observation"]=b["processed"]
        result = full_liquidation(b, c["side"], c["n"])
        for h in HORIZONS:
            target = c["executed"]+h*MS
            # A label needs a genuinely subsequent source packet, not a later-observed old book.
            if target <= b["source"] <= b["processed"] <= target+window_ms*MS:
                known = b["processed"] >= c["observed"]
                label = dict(at=b["processed"], source=b["source"], status="fill_not_observed" if not known else "depth_missing" if result is None else "full_depth",
                             gain=None if result is None or not known else result[0]-c["n"]*c["p"]-c["fee"], proof=b["proof"])
                if h not in c["labels"] or label["at"] < c["labels"][h]["at"]: c["labels"][h] = label
            if c["observed"] <= b["processed"] <= target and b["source"] >= c["observed"]:
                c["valid_books"][h] += 1
                # Full original depth must clear the whole acquisition at >= one tick profit per share.
                if result and result[1] >= c["p"]+5_000 and result[0]-c["n"]*c["p"]-c["fee"] > 0:
                    if c["gains"][h] is None or b["processed"] < c["gains"][h]:
                        c["gains"][h]=b["processed"];c.setdefault("gain_evidence",{})[h]={"sourceRowSha256":b["proof"],"lane":b.get("lane","public"),"sourceAtUnixNs":str(b["source"]),"receivedAtUnixNs":str(b["received"]),"marginalBid":money(result[1]),"hypotheticalGrossGain":money(result[0]-c["n"]*c["p"]-c["fee"])}


def chain(path, *, limit=None, previous=""):
    digest, count, size, last = hashlib.sha256(), 0, 0, previous
    with Path(path).open("rb") as f:
        while limit is None or size < limit:
            raw = f.readline(RAW.MAX_ROW_BYTES+1)
            if not raw: break
            need(len(raw) <= RAW.MAX_ROW_BYTES and raw.endswith(b"\n"), "CHAIN_ROW_BOUND")
            need(limit is None or size+len(raw) <= limit, "PREFIX_ROW_BOUNDARY")
            row = RAW.native_row(raw)
            need(row["sequence"] == count+1 and row["previous"] == last, "CHAIN_ORDER")
            digest.update(raw); size += len(raw); count += 1; last = row["hash"]
            yield row, (count,size,last,digest.hexdigest())


def audit(boundary_path, dispatch_path, *, breadth_paths=(), label_window_ms=30_000):
    boundary_raw = Path(boundary_path).read_bytes(); boundary = json.loads(boundary_raw, parse_float=Decimal)
    pin = boundary["snapshot"]; frozen = Path(boundary["offline"])
    report_raw = (frozen/"report.json").read_bytes()
    need(hashlib.sha256(report_raw).hexdigest() == pin["reportSha256"], "REPORT_ORIGINAL_BYTES")
    snapshot = RAW.native_sealed_object(report_raw)
    manifest = RAW.native_sealed_object((frozen/"manifest.json").read_bytes())
    need(snapshot["sequence"] == pin["head"]["sequence"] and snapshot["previous"] == pin["head"]["previous"], "REPORT_HEAD")
    orders = {str(q["id"]):q for q in snapshot["orders"]}
    forecast, dispatch_count, dispatch_hash, dispatch_prefix = set(), 0, "", ""
    original_acks={};actual_ids={str(f["orderId"]):str(f["actualOrderId"])for f in snapshot["fills"]}
    for row, (count,size,last,digest) in chain(dispatch_path, previous="0"*64):
        e = row["event"]
        if e.get("kind") == "intent" and e.get("fundamental") is not None: forecast.add(str(e["qid"]))
        if e.get("kind")=="ack"and str(e["qid"])in actual_ids:
            receipt=e["receipt"];qid=str(e["qid"])
            need(receipt["endpoint"]=="/orders"and str(receipt["data"]["orderId"])==actual_ids[qid],"ORIGINAL_ACK_BINDING")
            need(clock(receipt["requestStartedAt"])<=clock(receipt["receivedAt"]),"ORIGINAL_ACK_CLOCK")
            original_acks[qid]=clock(receipt["receivedAt"])
        dispatch_count, dispatch_hash, dispatch_prefix = count,last,digest
        if count == pin["dispatch"]["sequence"]: break
    need(dispatch_count == pin["dispatch"]["sequence"] and dispatch_hash == pin["dispatch"]["hash"], "DISPATCH_PREFIX")
    # Native unique fills give prospective queries before streaming the single frozen bank pass.
    query_events = [{"event":dict(f, type="actual_fill", source="official_sig_api_v1", fillId=f["sourceId"], processedAt=f["at"], receivedAt=f["at"])} for f in snapshot["fills"]]
    # Actual adopted FIFO is installed when encountered; queries need no inherited lots.
    query_cohorts = []
    for w in query_events:
        e=w["event"]
        if e["action"] == "buy": query_cohorts.append(dict(exchange=str(e["exchangeId"]), side=e["side"], n=e["quantity"], p=units(e["price"]), origin="forecast" if str(e["orderId"])in forecast else"ordinary", executed=clock(e["executedAt"]), observed=clock(e["processedAt"]), qid=str(e["orderId"]), fee=units(e["fee"]), first_post_execution=None, first_post_observation=None, labels={}, gains={h:None for h in HORIZONS}, valid_books=Counter()))
    relevant = {c["exchange"] for c in query_cohorts}; events=[]; types=Counter(); book_status=Counter(); last_at=None; final=None
    for row, state in chain(frozen/"events.jsonl", limit=pin["prefixBytes"]):
        e=row["event"]; types[e["type"]]+=1; at=clock(e.get("processedAt",e["receivedAt"])); last_at=e.get("processedAt",e["receivedAt"]); final=state
        if e["type"] in ("actual_position_adoption","actual_fill"): events.append(row)
        if e["type"] == "book" and str(e["book"]["exchangeId"])in relevant:
            try:
                b=book(e["book"],e["receivedAt"],e.get("processedAt",e["receivedAt"]),e["book"].get("transport"),row["hash"]);b["lane"]="native";book_status[b["status"]]+=1;observe(query_cohorts,b,window_ms=label_window_ms)
            except (FIFO.AnalysisError,KeyError,TypeError):book_status["rejected"]+=1
    need(final is not None and final==(pin["head"]["sequence"],pin["prefixBytes"],pin["head"]["previous"],pin["prefixSha256"]),"EXACT_FROZEN_PREFIX")
    breadth_sources=[]
    for path in breadth_paths:
        final_b=None; revisions={}; accepted=0; status=Counter()
        for row,state in chain(path):
            final_b=state;e=row["event"]
            if e["source"]!="official_sig_realtime":raise FIFO.AnalysisError("BREADTH_SOURCE")
            if clock(e["receivedAt"])>clock(last_at):continue
            if e["event"]=="stream_reset":revisions.clear();continue
            if e["event"]!="markets_batch":continue
            for market in e["payload"]["markets"]:
                batch=market["batch"];d=batch["delivery"];rev=d["revision"];prev=d["previousRevision"];mid=str(market["marketId"])
                need(d["model"]=="best-effort-authoritative-resync"and rev>prev,"BREADTH_DELIVERY")
                prior=revisions.get(mid);revisions[mid]=max(rev,prior or 0)
                if prior is not None and rev<=prior:continue
                # Explicit gap/resync is not continuous coverage; original fresh book snapshots may resume afterwards.
                if batch.get("resyncRequired")or batch.get("marketSettled"):status["gap_or_settled"]+=1;continue
                for raw in batch.get("books",[]):
                    if str(raw["exchangeId"])not in relevant:continue
                    try:
                        b=book(raw,e["receivedAt"],e["receivedAt"],"official_sig_realtime",row["hash"]);status[b["status"]]+=1;observe(query_cohorts,b,window_ms=label_window_ms);accepted+=1
                    except (FIFO.AnalysisError,KeyError,TypeError):status["rejected"]+=1
        need(final_b is not None,"EMPTY_BREADTH")
        breadth_sources.append(dict(fileName=Path(path).name,bytes=final_b[1],sha256=final_b[3],rows=final_b[0],bookStatuses=dict(status),acceptedBooks=accepted))
    input_data={"schemaVersion":1,"observedAt":last_at,"source":{"bankSequence":snapshot["sequence"],"bankPrevious":snapshot["previous"],"reportEnvelopeHash":snapshot["hash"],"journalPrefixSha256":final[3],"journalPrefixBytes":final[1],"everyOriginalRowChainVerified":True},"events":events,"fills":snapshot["fills"],"orders":snapshot["orders"],"expectedReport":snapshot["report"],"forecastQids":sorted(forecast),"startingCash":manifest["policy"]["initialCash"]}
    economics=FIFO.analyze(input_data,inventory_age_ms=60_000)
    cohorts, closes=trace(events,orders,forecast); ordinary=[c for c in cohorts if c["origin"]=="ordinary"]
    for origin, base_origin in (("ordinary","new"),("forecast","forecast"),("inherited","inherited")):
        need(sum(c["gross"]for c in cohorts if c["origin"]==origin)==units(economics["origins"][base_origin]["grossRealized"]),"CLOCK_TRACE_GROSS_PARITY")
        need(sum(c["remaining"]*c["p"]for c in cohorts if c["origin"]==origin)==units(economics["origins"][base_origin]["remainingUnmarkedCost"]),"CLOCK_TRACE_OPEN_COST_PARITY")
    need(len(ordinary)==len([c for c in query_cohorts if c["origin"]=="ordinary"]),"QUERY_COHORT_PARITY")
    for c,q in zip([c for c in cohorts if c["origin"]!="inherited"],query_cohorts):
        need((c["exchange"],c["side"],c["n"],c["p"],c["executed"],c["observed"])==(q["exchange"],q["side"],q["n"],q["p"],q["executed"],q["observed"]),"QUERY_ORDER_PARITY")
        for k in ("first_post_execution","first_post_observation","first_native_post_execution","first_native_post_observation","labels","gains","valid_books","gain_evidence"):c[k]=q.get(k)
    realized_by_reason=defaultdict(lambda:Counter()); actual_rows=[]
    for close in closes:
        c=cohorts[close["cohort"]]; r=realized_by_reason[c["origin"],close["reason"]];r["shares"]+=close["n"];r["gross"]+=close["gross"]
        if c["origin"]=="ordinary":actual_rows.append(close)
    horizons={}
    for h in HORIZONS:
        labeled=[c for c in ordinary if h in c["labels"]];full=[c for c in labeled if c["labels"][h]["gain"]is not None];eligible=[c for c in ordinary if c["gains"][h]is not None]
        horizons[str(h)]={"cohorts":len(ordinary),"freshPostHorizonLabels":len(labeled),"fullDepthLabels":len(full),"depthMissingLabels":sum(c["labels"][h]["status"]=="depth_missing"for c in labeled),"fillNotYetObservedLabels":sum(c["labels"][h]["status"]=="fill_not_observed"for c in labeled),"missingLabels":len(ordinary)-len(labeled),"rightCensoredBySnapshot":sum(c["executed"]+h*MS>clock(last_at)for c in ordinary),"hypotheticalFullDepthGain":money(sum(c["labels"][h]["gain"]for c in full)),"hypotheticalPositiveLabels":sum(c["labels"][h]["gain"]>0 for c in full),"observedOneTickFullDepthOpportunityCohorts":len(eligible),"opportunityAfter60sCohorts":sum(c["gains"][h]>c["executed"]+60_000*MS for c in eligible),"noValidBookInWindowCohorts":sum(not c["valid_books"][h]for c in ordinary),"interpretation":"Counterfactual displayed liquidation only; overlapping acquisition cohorts, not executable portfolio profit or maker fill."}
    def lag(field):return distribution([c[field]-c["executed"]for c in ordinary if c[field]is not None])
    groups=[]
    for ex,side in sorted({(c["exchange"],c["side"])for c in ordinary}):
        cs=[c for c in ordinary if(c["exchange"],c["side"])==(ex,side)];rows=[r for r in actual_rows if cohorts[r["cohort"]]in cs]
        groups.append({"exchangeId":ex,"side":side,"buyFillCohorts":len(cs),"acquiredShares":sum(c["n"]for c in cs),"remainingShares":sum(c["remaining"]for c in cs),"actualClosedShares":sum(r["n"]for r in rows),"actualGrossRealized":money(sum(r["gross"]for r in rows)),"flatClosedShares":sum(r["n"]for r in rows if r["gross"]==0),"maximumAgeClosedShares":sum(r["n"]for r in rows if r["reason"]=="maximumAge"),"actualHolding":distribution([r["executed"]-cohorts[r["cohort"]]["executed"]for r in rows]),"buyObservationLag":distribution([c["observed"]-c["executed"]for c in cs]),"oneTickOpportunityCohortsByHorizon":{str(h):sum(c["gains"][h]is not None for c in cs)for h in HORIZONS}})
    result={"schemaVersion":1,"analysis":"actual_maker_exit_window","cutoff":last_at,"source":{"boundaryFileSha256":hashlib.sha256(boundary_raw).hexdigest(),"bankPrefixSha256":final[3],"bankPrefixBytes":final[1],"bankSequence":final[0],"bankPrevious":final[2],"reportFileSha256":hashlib.sha256(report_raw).hexdigest(),"reportEnvelopeHash":snapshot["hash"],"allOriginalBankRowSealsAndChainVerified":True,"dispatchRows":dispatch_count,"dispatchHead":dispatch_hash,"dispatchPrefixSha256":dispatch_prefix,"breadth":breadth_sources},"actualFifoParity":economics["parity"],"accounting":economics["accounting"],"origins":economics["origins"],"uniqueActualFills":economics["uniqueActualFills"],"duplicateActualFillEvents":economics["duplicateOriginalFillEvents"],"actualByOriginAndExitReason":[{"origin":o,"reason":r,"shares":s["shares"],"grossRealized":money(s["gross"])}for(o,r),s in sorted(realized_by_reason.items())],"ordinaryTimings":{"buyExecutionToObserved":distribution([c["observed"]-c["executed"]for c in ordinary]),"buyExecutionToFirstFreshBook":lag("first_post_execution"),"buyExecutionToFirstFreshBookAfterFillObserved":lag("first_post_observation"),"buyExecutionToExitQuoteCreated":distribution([r["quote_created"]-cohorts[r["cohort"]]["executed"]for r in actual_rows]),"buyExecutionToActualExit":distribution([r["executed"]-cohorts[r["cohort"]]["executed"]for r in actual_rows]),"exitExecutionToObserved":distribution([r["observed"]-r["executed"]for r in actual_rows])},"perExchangeSide":groups,"hypotheticalExitWindows":horizons,"labelWindowMs":label_window_ms,"inventoryAgeThresholdMs":60_000,"nativeRelevantBookStatuses":dict(book_status),"nativeEventTypes":dict(types),"limitations":["No maker fill is inferred from quote touch, public print or displayed depth. Hypothetical exits do not change actual FIFO/PnL.","Public trades have no verified aggressor/own-queue identity; no public print is classified as an own fill.","Labels require original source and receipt clocks, valid depth and source >= horizon; missing/censored labels are not zero gains.","Overlapping fill cohorts and shared depth cannot be summed into feasible portfolio profit.","Selected native books are sparse; only explicitly named frozen breadth files supplement them. The09:20 capture gap and restart gaps prevent continuous opportunity/no-opportunity claims.","Fixed-horizon gains are hypothetical taker liquidation at displayed full depth, not guaranteed submission/ACK/fill outcomes.","Actual gross inherited attribution remains distinct from new trading and from whole-account/dashboard PnL."],"policyChanged":False,"profitabilityEstablished":False}

    result["clockStageDecomposition"]=clock_summary(cohorts,closes,original_acks)
    result["oneTickOpportunityEvidence"]=[{"exchangeId":c["exchange"],"side":c["side"],"acquisitionShares":c["n"],"firstOpportunityAfterExecutionMs":str(Decimal(c["gains"][600000]-c["executed"])/MS),"firstOpportunityAfterNativeFillObservedMs":str(Decimal(c["gains"][600000]-c["observed"])/MS),**c["gain_evidence"][600000],"actualProfitClaimed":False}for c in ordinary if c["gains"][600000]is not None]
    result["analysisSourceSha256"]={"analyzer":hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),"fifo":hashlib.sha256(Path(FIFO.__file__).read_bytes()).hexdigest(),"rawSealVerifier":hashlib.sha256(Path(RAW.__file__).read_bytes()).hexdigest()}
    result["limitations"].append("All-history timing statistics include earlier service stops/restarts and are not current-worker latency benchmarks. ACK minus fill execution is signed because passive ACKs normally precede later fills.")
    return result


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument("--boundary",required=True);p.add_argument("--dispatch",required=True);p.add_argument("--breadth",action="append",default=[]);p.add_argument("--output",required=True);a=p.parse_args()
    need(Path(a.output).resolve()not in {Path(x).resolve()for x in [a.boundary,a.dispatch,*a.breadth]},"OUTPUT_OVERWRITES_SOURCE")
    result=audit(a.boundary,a.dispatch,breadth_paths=a.breadth);Path(a.output).write_text(json.dumps(result,sort_keys=True,indent=2)+"\n",encoding="utf-8")
    print(json.dumps({"uniqueActualFills":result["uniqueActualFills"],"accounting":result["accounting"],"outputSha256":hashlib.sha256(Path(a.output).read_bytes()).hexdigest()}))


if __name__=="__main__":main()
