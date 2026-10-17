"""Finite, hash-pinned descriptive acquisition audit. No network or model fit."""
import argparse
import hashlib
import json
from datetime import datetime
from decimal import Decimal
from pathlib import Path

UNIT = Decimal(1_000_000)
UNIT_MS_PER_PRINCIPAL_SECOND = Decimal(1_000_000_000)


def clock(value):
    instant = datetime.fromisoformat(value.replace("Z", "+00:00"))
    if instant.utcoffset() is None:
        raise ValueError("timezone required")
    epoch = datetime.fromisoformat("1970-01-01T00:00:00+00:00")
    delta = instant - epoch
    return delta.days * 86_400_000 + delta.seconds * 1_000 + delta.microseconds // 1_000


def dec(value):
    return format(Decimal(value), "f")


def audit(path, expected_sha):
    if path.stat().st_size > 8 * 1024 * 1024:
        raise ValueError("artifact exceeds 8 MiB bound")
    raw = path.read_bytes()
    sha = hashlib.sha256(raw).hexdigest()
    if sha != expected_sha:
        raise ValueError("source SHA256 mismatch")
    data = json.loads(raw, parse_float=Decimal)
    if data["kind"] != "fundamental_acquisition_only" or data["disabled"] is not None:
        raise ValueError("unsupported or disabled observer")
    rows = data["rows"]
    if not 0 < len(rows) <= 4096:
        raise ValueError("row bound")
    result = []
    seen_qids, seen_fills = set(), set()
    total_cost = total_limit = total_reservation = total_owned = total_delta = Decimal(0)
    modes, exchanges = {}, set()
    for row in rows:
        qid = row["qid"]
        if qid in seen_qids:
            raise ValueError("duplicate qid")
        seen_qids.add(qid)
        evidence = row["originalEvidence"]
        meta = evidence["nativeReviewMetadata"]
        quantity = row["observedFilledQuantity"]
        # Execution-clock comparison is valid here only for fully-held BUY lots.
        if not (row["class"] == "confirmed_full" and row["acquisitionConfirmed"]
                and row["reservationReleased"] and row["terminalStatus"] == "filled"
                and quantity == row["requestedQuantity"] == row["admittedQuantity"]
                == row["nativeReservedQuantity"] == row["ownedRemainingQuantity"]):
            raise ValueError("requires confirmed full, entirely remaining acquisitions")
        fills = row["fillObservations"]
        if not 0 < len(fills) <= 256:
            raise ValueError("fill bound")
        created, attempted, through = (row[k] for k in
                                       ("quoteCreatedAtMs", "attemptAtMs", "observedThroughMs"))
        ack = clock(evidence["ackObservation"]["event"]["receivedAt"])
        if not created <= attempted <= ack <= through:
            raise ValueError("attempt/ACK clock order")
        cost, fees, delta, execution_owned = Decimal(0), Decimal(0), Decimal(0), Decimal(0)
        executions, receipts, commits = [], [], []
        for fill in fills:
            facts, observation = fill["facts"], fill["firstObservation"]
            key = (facts["actualOrderId"], facts["fillId"])
            if key in seen_fills:
                raise ValueError("duplicate actual fill identity")
            seen_fills.add(key)
            if (facts["orderId"] != qid or facts["action"] != "buy"
                    or facts["exchangeId"] != evidence["exchangeId"]
                    or facts["marketId"] != evidence["marketId"]
                    or facts["side"] != evidence["side"]
                    or facts["actualOrderId"] != evidence["ackObservation"]["event"]["actualOrderId"]):
                raise ValueError("fill attribution mismatch")
            executed, received, committed = (clock(facts["executedAt"]),
                                            clock(observation["receivedAt"]),
                                            clock(observation["processedAt"]))
            if not attempted <= executed <= received <= committed <= through:
                raise ValueError("fill clock order")
            if not (isinstance(facts["quantity"], int) and facts["quantity"] > 0
                    and isinstance(facts["priceUnits"], int) and 0 < facts["priceUnits"] <= 1_000_000):
                raise ValueError("fill economics")
            principal = Decimal(facts["quantity"]) * facts["priceUnits"]
            cost += principal
            fees += Decimal(facts["feeUnits"])
            delta += principal * (committed - executed)
            execution_owned += principal * (through - executed)
            executions.append(executed)
            receipts.append(received)
            commits.append(committed)
        if sum(f["facts"]["quantity"] for f in fills) != quantity:
            raise ValueError("fill quantity mismatch")
        limit = Decimal(meta["actionSpec"]["limitPrice"]) * UNIT * quantity
        if cost > limit:
            raise ValueError("fill cost exceeds frozen limit")
        observed_owned = Decimal(row["ownedPrincipalTimeObservedUnitMs"])
        if observed_owned + delta != execution_owned:
            raise ValueError("fully-held principal time reconciliation failed")
        first, last = min(executions), max(executions)
        if row["firstExecutionLatencyMs"] != first - attempted:
            raise ValueError("execution latency summary mismatch")
        if row["firstObservationLatencyMs"] != min(commits) - attempted:
            raise ValueError("observation latency summary mismatch")
        mode = evidence["executionMode"]
        modes[mode] = modes.get(mode, 0) + 1
        exchanges.add(evidence["exchangeId"])
        reservation = Decimal(row["reservationPrincipalTimeUnitMs"])
        result.append({
            "qid": qid, "executionMode": mode, "quantity": quantity,
            "actualPrincipal": dec(cost / UNIT), "actualFees": dec(fees / UNIT),
            "frozenLimitPrincipal": dec(limit / UNIT),
            "limitPriceImprovement": dec((limit - cost) / UNIT),
            "quoteToAttemptMs": attempted - created,
            "attemptToAckReceiptMs": ack - attempted,
            "attemptToFirstExecutionMs": first - attempted,
            "attemptToFullExecutionMs": last - attempted,
            "firstExecutionToItsReceiptMs": min(clock(f["firstObservation"]["receivedAt"]) - first
                for f in fills if clock(f["facts"]["executedAt"]) == first),
            "firstExecutionToItsCommitMs": min(clock(f["firstObservation"]["processedAt"]) - first
                for f in fills if clock(f["facts"]["executedAt"]) == first),
            "maximumFillReceiptToCommitMs": max(clock(f["firstObservation"]["processedAt"])
                - clock(f["firstObservation"]["receivedAt"]) for f in fills),
            "bookReceiptAgeAtAttemptMs": attempted - clock(evidence["book"]["receivedAt"]),
            "reservationPrincipalSeconds": dec(reservation / UNIT_MS_PER_PRINCIPAL_SECOND),
            "ownedObservationPrincipalSeconds": dec(observed_owned / UNIT_MS_PER_PRINCIPAL_SECOND),
            "ownedExecutionPrincipalSeconds": dec(execution_owned / UNIT_MS_PER_PRINCIPAL_SECOND),
            "ownedExecutionMinusObservationPrincipalSeconds": dec(delta / UNIT_MS_PER_PRINCIPAL_SECOND),
            "observedThroughMs": through,
        })
        total_cost += cost
        total_limit += limit
        total_reservation += reservation
        total_owned += observed_owned
        total_delta += delta
    return {
        "version": 1, "sourceSha256": sha,
        "auditorSourceSha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        "cohortSha256": data["cohortSha256"], "selectedRows": len(rows),
        "startedAtMs": data["startedAtMs"],
        "observedThroughMsRange": [min(r["observedThroughMs"] for r in rows), max(r["observedThroughMs"] for r in rows)],
        "preCohortPendingUnscoredCount": len(data["preCohortPendingUnscored"]),
        "excludedOrdinaryAttempts": data["excludedOrdinary"],
        "distinctExchanges": len(exchanges), "fillRows": len(seen_fills),
        "modeCounts": modes, "reportedCompletedZeroPartialFull": data["completedZeroPartialFull"],
        "incompleteSelectedSnapshot": True, "selectionDenominatorAvailable": False,
        "fillProbabilityEstimated": False, "comparativeModeEffectEstimated": False,
        "profitOrModelQualificationClaimed": False, "rawSourceMembershipIndependentlyReplayed": False,
        "reservationClock": data["reservationClock"], "ownedClock": data["ownedCapitalClock"],
        "clockComparisonRestriction": "all audited BUY quantities confirmed full and still entirely held; fees excluded",
        "totals": {
            "quantity": sum(r["quantity"] for r in result),
            "actualPrincipal": dec(total_cost / UNIT),
            "frozenLimitPrincipal": dec(total_limit / UNIT),
            "limitPriceImprovement": dec((total_limit - total_cost) / UNIT),
            "reservationPrincipalSeconds": dec(total_reservation / UNIT_MS_PER_PRINCIPAL_SECOND),
            "ownedObservationPrincipalSeconds": dec(total_owned / UNIT_MS_PER_PRINCIPAL_SECOND),
            "ownedExecutionPrincipalSeconds": dec((total_owned + total_delta) / UNIT_MS_PER_PRINCIPAL_SECOND),
            "ownedExecutionMinusObservationPrincipalSeconds": dec(total_delta / UNIT_MS_PER_PRINCIPAL_SECOND),
        }, "rows": result,
    }


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("artifact", type=Path)
    parser.add_argument("--expected-sha256", required=True)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    result = audit(args.artifact, args.expected_sha256)
    with args.output.open("x", encoding="utf-8", newline="\n") as out:
        json.dump(result, out, indent=2, sort_keys=True)
        out.write("\n")
