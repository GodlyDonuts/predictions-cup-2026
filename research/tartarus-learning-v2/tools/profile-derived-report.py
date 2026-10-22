"""Measure retained report shape only; no API, ledger edits or trade permission."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import time


def compact_bytes(value):
    return len(json.dumps(value, separators=(',', ':'), ensure_ascii=False).encode())


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--report', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    a = p.parse_args()
    raw = a.report.read_bytes()
    started = time.perf_counter()
    source = json.loads(raw)
    parsed = time.perf_counter()-started
    body = source['report']
    largest_orders = sorted((dict(id=q['id'], exchangeId=q['exchangeId'],
        retired=q['retired'], pythonCompactBytes=compact_bytes(q)) for q in source['orders']),
        key=lambda q: -q['pythonCompactBytes'])[:15]
    result = dict(observedUtc=datetime.now(timezone.utc).isoformat(),
        originalReportSha256=hashlib.sha256(raw).hexdigest(),
        originalSealedReportHash=source['hash'], originalBytes=len(raw),
        snapshotAt=body['at'], pythonParseSeconds=parsed,
        topLevelPythonCompactBytes={k: compact_bytes(v) for k,v in source.items()},
        reportFieldsPythonCompactBytes=dict(sorted(((k,compact_bytes(v)) for k,v in body.items()),
            key=lambda x:-x[1])), largestOrders=largest_orders,
        originalOrderCount=len(source['orders']), originalFillCount=len(source['fills']),
        sourceCursorCount=len(source['sourceCursors']), networkRequests=0,
        originalMutated=False, financialAuthority=False,
        limitations=['Field sizes use Python compact JSON, not a Rust serialization benchmark.',
            'Retained snapshot; parse timing depends on shared CPU/cache.',
            'No production latency, throughput, fill or ROI claim.'])
    with a.output.open('x', encoding='utf-8') as f:
        f.write(json.dumps(result,indent=2)+'\n')
    print(json.dumps({k:result[k] for k in ('originalBytes','originalOrderCount','originalFillCount','sourceCursorCount')}))


if __name__ == '__main__':
    main()
