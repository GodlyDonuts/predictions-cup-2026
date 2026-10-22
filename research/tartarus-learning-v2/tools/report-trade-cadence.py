"""Measure official fill cadence from the existing bank; no API connection."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path


def milliseconds(value):
    return int(datetime.fromisoformat(value.replace('Z', '+00:00')).timestamp() * 1000)


def report(raw, now, minutes):
    assert 1 <= minutes <= 1440
    source = json.loads(raw)
    end = now // 60000 * 60000
    start = end - minutes * 60000
    buckets = {t: set() for t in range(start, end, 60000)}
    ids = set()
    last = None
    receipt_count = 0
    for fill in source['fills']:
        if fill.get('counterfactual') is not False:
            continue
        assert fill.get('actualOrderId') and fill.get('sourceId')
        identity = str(fill['actualOrderId']), str(fill['sourceId'])
        assert identity not in ids, 'Duplicate actual fill identity'
        ids.add(identity)
        at = milliseconds(fill['executedAt'])
        assert at <= now, 'Future actual fill clock'
        last = max(last or at, at)
        bucket = at // 60000 * 60000
        if bucket in buckets:
            buckets[bucket].add(str(fill['actualOrderId']))
            receipt_count += 1
    active = sum(bool(v) for v in buckets.values())
    return dict(observedUtc=datetime.fromtimestamp(now / 1000, timezone.utc).isoformat(),
        originalReportSha256=hashlib.sha256(raw).hexdigest(),
        completedMinutes=minutes, minutesWithActualFills=active,
        emptyMinutes=minutes-active, coverage=active/minutes,
        actualFillReceiptsInWindow=receipt_count,
        distinctFilledOrdersInWindow=len(set().union(*buckets.values())),
        lastActualExecutionUtc=None if last is None else datetime.fromtimestamp(last/1000, timezone.utc).isoformat(),
        secondsSinceLastActualExecution=None if last is None else (now-last)/1000,
        oneFilledTradeEveryCompletedMinute=active == minutes,
        countsOrderAcknowledgmentsAsTrades=False, accountClientsCreated=0,
        minuteCounts=[dict(startUtc=datetime.fromtimestamp(k/1000, timezone.utc).isoformat(),
                           distinctOrdersWithActualFills=len(v)) for k, v in buckets.items()])


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--report', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--minutes', type=int, default=60)
    a = p.parse_args()
    result = report(a.report.read_bytes(), int(datetime.now(timezone.utc).timestamp()*1000), a.minutes)
    with a.output.open('x') as stream:
        stream.write(json.dumps(result, indent=2)+'\n')
    print(json.dumps({k: v for k, v in result.items() if k != 'minuteCounts'}))
