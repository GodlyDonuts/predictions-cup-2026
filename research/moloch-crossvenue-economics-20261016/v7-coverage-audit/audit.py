"""Read-only exact-clock audit. Receipt eligibility is an upper bound, not availability."""
import argparse
from collections import Counter, defaultdict
import datetime as dt
import gzip
import hashlib
import importlib.util
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / 'moloch/external-data'))
import crossvenue_observer as observer


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def sealed(path, expected):
    path = Path(path)
    before = path.stat()
    raw = path.read_bytes()
    if digest(raw) != expected:
        raise ValueError('SOURCE_HASH')
    rows = [json.loads(line) for line in gzip.decompress(raw).splitlines()]
    after = path.stat()
    if (before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns) != (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns) or digest(path.read_bytes()) != expected:
        raise ValueError('SOURCE_CHANGED')
    return rows, {'name': path.name, 'bytes': len(raw), 'sha256': expected}


def summarize(values):
    values = sorted(values)
    return {str(q): values[int((len(values)-1)*q)] if values else None for q in (0, .1, .25, .5, .75, .9, 1)}


def eligibility(trades, decision_ms, export_ms, horizon):
    """Do not infer processed availability from a wire receipt; this is optimistic."""
    received = [r for r in trades if decision_ms-horizon < r['sourceMs'] <= decision_ms and r['receivedMs'] <= decision_ms]
    at_export = [r for r in received if r['receivedMs'] <= export_ms and r['sourceMs'] <= export_ms]
    return received, at_export


def collect(rows):
    trades, kinds, seen = defaultdict(list), Counter(), {}
    for row in rows:
        kinds[row.get('kind', '') + ':' + row.get('type', '')] += 1
        if row.get('kind') != 'websocket' or row.get('type') != 'trade':
            continue
        if digest(row['bodyText'].encode()) != row['bodySha256'] or json.loads(row['bodyText']) != row['body']:
            raise ValueError('TRADE_ORIGINAL_BODY')
        msg = row['body']['msg']
        original = json.dumps(msg, sort_keys=True, separators=(',', ':'))
        prior = seen.get(msg['trade_id'])
        if prior is not None:
            if prior != original:
                raise ValueError('CONFLICTING_TRADE_ID')
            continue
        seen[msg['trade_id']] = original
        if type(msg.get('ts_ms')) is not int:
            raise ValueError('TRADE_EXECUTION_CLOCK')
        received = int(dt.datetime.fromisoformat(row['receivedAt'].replace('Z', '+00:00')).timestamp()*1000)
        trades[msg['market_ticker']].append({'id': msg['trade_id'], 'sourceMs': msg['ts_ms'], 'receivedMs': received, 'bodySha256': row['bodySha256']})
    return trades, kinds


def analyze(decisions, trades, flow_vector):
    counts, ages, export_delivery, delivery_decision = Counter(), [], [], []
    unique_evidence, cases, ids, tickers = set(), [], set(), set()
    first, last = min(d['decisionMs'] for d in decisions), max(d['decisionMs'] for d in decisions)
    for d in decisions:
        if d['sampleId'] in ids:
            raise ValueError('DUPLICATE_DECISION')
        ids.add(d['sampleId'])
        if d['external']['mappingKind'] != 'direct_party_reference':
            counts['excludedOtherMapping'] += 1
            continue
        counts['directDecisions'] += 1
        at, ticker, cached = d['decisionMs'], d['external']['ticker'], d['externalTradeFlow']
        tickers.add(ticker)
        values, _ = flow_vector(d)  # Original frozen validator, no fitted model.
        counts['allThreeIdentified'] += all(v is not None for v in values)
        if cached is None:
            counts['missingFlowSnapshot'] += 1
            continue
        snap = cached['sourceSnapshot']
        ages.append(at-snap['asOfMs'])
        export_delivery.append(cached['availableMs']-snap['asOfMs'])
        delivery_decision.append(at-cached['availableMs'])
        counts['snapshotAgeAtLeast1000ms'] += at-snap['asOfMs'] >= 1000
        for i, ms in enumerate((1000, 5000, 30000)):
            w = snap['windows'][str(ms)]
            received, exported = eligibility(trades.get(ticker, []), at, snap['asOfMs'], ms)
            retained = {r['tradeId']: r for r in w['evidence']}
            for r in retained.values():
                original = next((t for t in trades.get(ticker, []) if t['id'] == r['tradeId']), None)
                if original is None or (original['sourceMs'], original['receivedMs'], original['bodySha256']) != (r['sourceTsMs'], r['receivedMs'], r['bodySha256']):
                    raise ValueError('FLOW_ORIGINAL_TRADE_BINDING')
            prefix = str(ms) + ':'
            counts[prefix+'receiptEligibleUpperBoundDecisions'] += bool(received)
            counts[prefix+'receiptEligibleAtOriginalExportDecisions'] += bool(exported)
            counts[prefix+'originalSnapshotNonemptyDecisions'] += w['observedTradeCount'] > 0
            counts[prefix+'actualIdentifiedDecisions'] += values[i] is not None
            counts[prefix+'atExportReceiptCandidatesAbsentFromEvidence'] += any(r['id'] not in retained for r in exported)
            if ms == 1000 and received:
                cases.append({'sampleId': d['sampleId'], 'ticker': ticker, 'decisionMs': at, 'sourceExportMs': snap['asOfMs'], 'snapshotAgeMs': at-snap['asOfMs'], 'receiptEligibleTrades': received, 'actuallyIdentified': values[i] is not None})
            if ms == 30000:
                unique_evidence.update(retained)
    during = [r for rs in trades.values() for r in rs if first <= r['receivedMs'] <= last]
    return {'counts': dict(counts), 'decisionIntervalMs': [first, last], 'directDecisionTickers': len(tickers),
            'sourceExportToDecisionMsQuantiles': summarize(ages), 'sourceExportToObserverAvailabilityMsQuantiles': summarize(export_delivery),
            'observerAvailabilityToDecisionMsQuantiles': summarize(delivery_decision), 'uniqueActualRetained30sTradeIds': len(unique_evidence),
            'collectorTradesReceivedDuringDecisionInterval': len(during), 'collectorTradeTickersDuringDecisionInterval': len({t for t, rs in trades.items() if any(first <= r['receivedMs'] <= last for r in rs)}),
            'oneSecondReceiptEligibleCases': cases}


def main():
    p = argparse.ArgumentParser()
    p.add_argument('--observer', required=True); p.add_argument('--observer-sha256', required=True)
    p.add_argument('--collector', required=True); p.add_argument('--collector-sha256', required=True)
    p.add_argument('--output', required=True)
    args = p.parse_args()
    producer = Path(observer.__file__)
    if digest(producer.read_bytes()) != '674264745ced43b053881ad7408a5031a6e389c8652ff950b23bca0e1448b34d':
        raise ValueError('ORIGINAL_V7_PRODUCER_REQUIRED')
    frozen = ROOT / 'research/moloch-crossvenue-economics-20261016/v7-flow/analyze.py'
    spec = importlib.util.spec_from_file_location('frozen_v7', frozen)
    if digest(frozen.read_bytes().replace(b'\r\n', b'\n')) != '12b185a9dd4efd0f6e72583d4274c240256416818a351018a27543525d4bb06c':
        raise ValueError('ORIGINAL_V7_ANALYSIS_REQUIRED')
    original = importlib.util.module_from_spec(spec); spec.loader.exec_module(original)
    decisions, a = sealed(args.observer, args.observer_sha256)
    collector, b = sealed(args.collector, args.collector_sha256)
    trades, kinds = collect(collector)
    report = analyze([r for r in decisions if r.get('kind') == 'decision_features'], trades, original.flow_vector)
    report.update(schemaVersion=1, sources=[a, b], sourceCodeSha256=digest(Path(__file__).read_bytes()), producerSha256=digest(producer.read_bytes()), originalFrozenAnalysisRawSha256=digest(frozen.read_bytes()), originalFrozenAnalysisLfSha256=digest(frozen.read_bytes().replace(b'\r\n', b'\n')), collectorFrameCounts=dict(kinds), collectorUniqueTrades=sum(map(len,trades.values())), collectorTradeTickers=len(trades), executionAuthority=False, modelApproved=False, completeTradeTape=False, finding='UNRESOLVED_SOURCE_EXPORT_CANDIDATES' if any(v for k,v in report['counts'].items() if k.endswith(':atExportReceiptCandidatesAbsentFromEvidence')) else 'NO_MISSING_EXPORTED_TRADE_EVIDENCE_OBSERVED', limitations=['Wire-receipt eligibility is an optimistic upper bound, not proof of processing availability.', 'Public flow and market mapping do not prove SIG resolution equivalence or maker fills.', 'No change to the sealed v7 features, thresholds, clock boundaries or outcomes.'])
    Path(args.output).write_text(json.dumps(report, indent=2, sort_keys=True)+'\n', encoding='utf-8')


if __name__ == '__main__':
    main()
