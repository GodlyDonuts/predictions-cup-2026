"""Read-only conversion audit over an exact retained dispatch prefix and reports.

Counts native order IDs with original official fill receipts, never partial fills
as independent orders. An admission/attempt is not proof that HTTP was sent.
"""
import argparse
from collections import Counter
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
from statistics import median


def clock(value):
    return int(datetime.fromisoformat(value.replace('Z', '+00:00')).timestamp()*1000)


def digest(path):
    with path.open('rb') as f:
        return hashlib.file_digest(f, 'sha256').hexdigest()


def verify_prefix(before, after):
    with before.open('rb') as a, after.open('rb') as b:
        while chunk := a.read(1024*1024):
            assert b.read(len(chunk)) == chunk, 'ORIGINAL_DISPATCH_PREFIX_CHANGED'


def measure(values):
    return dict(count=len(values), minimumMs=min(values, default=None),
                medianMs=median(values) if values else None,
                maximumMs=max(values, default=None))


def run(a):
    verify_prefix(a.before_dispatch, a.after_dispatch)
    baseline = json.loads(a.before_report.read_bytes())
    report = json.loads(a.after_report.read_bytes())
    assert report['fills'][:len(baseline['fills'])] == baseline['fills'], 'ORIGINAL_FILLS_CHANGED'
    by_kind, intents, acknowledgments = {}, {}, {}
    counts = Counter()
    with a.after_dispatch.open('rb') as f:
        f.seek(a.before_dispatch.stat().st_size)
        for line in f:
            assert line.endswith(b'\n'), 'PARTIAL_DISPATCH_EVENT'
            event = json.loads(line)['event']
            kind, qid = event['kind'], event['qid']
            counts[kind] += 1
            by_kind.setdefault(kind, set()).add(qid)
            if kind == 'intent':
                assert qid not in intents, 'DUPLICATE_NEW_INTENT'
                intents[qid] = event
            if kind in ('ack', 'recovered_ack'):
                assert qid not in acknowledgments, 'DUPLICATE_NEW_ACK'
                acknowledgments[qid] = event
    first_fills = {}
    identities = set()
    for fill in report['fills'][len(baseline['fills']):]:
        assert fill.get('counterfactual') is False
        assert fill.get('actualOrderId') and fill.get('sourceId')
        identity = str(fill['actualOrderId']), str(fill['sourceId'])
        assert identity not in identities, 'DUPLICATE_OFFICIAL_FILL_IDENTITY'
        identities.add(identity)
        qid = fill['orderId']
        first_fills[qid] = min(first_fills.get(qid, clock(fill['executedAt'])), clock(fill['executedAt']))
    http_times, preparation, fill_delays, request_starts = [], [], [], []
    origins = Counter()
    for qid, event in intents.items():
        q = event['quote']
        origin = ('reviewed_monetary_or_rank' if event.get('rankCampaign') else
                  'fundamental' if event.get('fundamental') else
                  'reviewed_inventory_exit' if q.get('reviewedInventoryExit') else 'ordinary')
        origins[origin] += 1
        ack = acknowledgments.get(qid)
        if ack is None:
            continue
        receipt = ack['receipt']
        assert receipt['endpoint'] == '/orders'
        start, received = clock(receipt['requestStartedAt']), clock(receipt['receivedAt'])
        duration, prepared = received-start, start-clock(event['at'])
        assert duration >= 0 and prepared >= 0, 'RECEIPT_CLOCK_ORDER'
        http_times.append(duration)
        preparation.append(prepared)
        request_starts.append(start)
        if qid in first_fills:
            # Exchange execution may precede client receipt of acknowledgment.
            fill_delays.append(first_fills[qid]-start)
    request_starts.sort()
    gaps = [b-a for a,b in zip(request_starts, request_starts[1:])]
    iso = lambda ms: datetime.fromtimestamp(ms/1000, timezone.utc).isoformat()
    result = dict(observedUtc=datetime.now(timezone.utc).isoformat(),
        beforeDispatchSha256=digest(a.before_dispatch), afterDispatchSha256=digest(a.after_dispatch),
        beforeReportSha256=digest(a.before_report), afterReportSha256=digest(a.after_report),
        originalDispatchPrefixPreserved=True, originalFillRowsPreserved=True,
        newEventCounts=dict(counts), distinctNativeIdsByEventKind={k:len(v) for k,v in by_kind.items()},
        distinctNewIntents=len(intents), distinctNewAcknowledgments=len(acknowledgments),
        distinctOrdersWithIncrementalOfficialFills=len(first_fills),
        filledIds=sorted(first_fills), newIntentOrigins=dict(origins),
        newAcknowledgmentsWithIncrementalFills=len(set(acknowledgments)&set(first_fills)),
        incrementalFillIdsWithoutNewAck=sorted(set(first_fills)-set(acknowledgments)),
        intentToOfficialRequestStart=measure(preparation), officialPostRequestToReceipt=measure(http_times),
        interAcknowledgedPostRequestStart=measure(gaps),
        firstAcknowledgedPostRequestStartUtc=iso(request_starts[0]) if request_starts else None,
        lastAcknowledgedPostRequestStartUtc=iso(request_starts[-1]) if request_starts else None,
        officialRequestStartToFirstExecution=measure(fill_delays),
        actualFillReceipts=len(identities), countsPartialFillsAsSeparateOrders=False,
        admissionOrAttemptProvesHttpSent=False, acknowledgmentsProveFills=False,
        accountRequestsByAudit=0,
        limitations=['Intent-to-request excludes all selection and pre-intent read preparation.',
                     'Retired never-attempted drafts do not prove an eligible lost opportunity.',
                     'HTTP receipt duration excludes queue admission and does not measure full cycle latency.'])
    with a.output.open('x', encoding='utf-8') as f:
        json.dump(result, f, indent=2)
        f.write('\n')
    return result


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    for name in ('before-dispatch', 'after-dispatch', 'before-report', 'after-report', 'output'):
        p.add_argument('--'+name, type=Path, required=True)
    print(json.dumps(run(p.parse_args()), indent=2))
