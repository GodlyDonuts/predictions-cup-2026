#!/usr/bin/env python3
"""Bounded offline actual ordinary BUY fill-conditioned executable markouts."""
import argparse
from collections import Counter, defaultdict
from decimal import Decimal
import gzip
import hashlib
import importlib.util
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[2]
RAW_PATH = ROOT / 'research/moloch-owned-fill-markouts-20261015/analyze.py'
_spec = importlib.util.spec_from_file_location('owned_markout_raw', RAW_PATH)
RAW = importlib.util.module_from_spec(_spec)
sys.modules[_spec.name] = RAW
_spec.loader.exec_module(RAW)
need, ident, units, clock, money = RAW.check, RAW.identifier, RAW.units, RAW.instant, RAW.money
HORIZONS = (1000, 5000, 30000, 120000)
MS = 1_000_000
SLACK = 5000 * MS
AGE = 10000 * MS
MAX_BYTES = 2 * 1024**3
MAX_ROWS = 1_000_000
MAX_FILLS = 100_000


def pinned(path, expected, expected_bytes=None):
    path = Path(path)
    need(path.is_file() and path.stat().st_size <= MAX_BYTES, 'FILE_BOUND')
    h, size = hashlib.sha256(), 0
    with path.open('rb') as f:
        for chunk in iter(lambda: f.read(1024**2), b''):
            h.update(chunk); size += len(chunk)
    need(h.hexdigest() == expected and (expected_bytes is None or size == expected_bytes), 'ORIGINAL_FILE_PIN')
    return {'sha256': expected, 'bytes': size}


def rows(path, previous, stats=None):
    opener = gzip.open if str(path).endswith('.gz') else open
    count, total, last, digest = 0, 0, previous, hashlib.sha256()
    with opener(path, 'rb') as f:
        while True:
            raw = f.readline(RAW.MAX_ROW_BYTES + 1)
            if not raw:
                break
            count += 1; total += len(raw)
            need(count <= MAX_ROWS and total <= MAX_BYTES and len(raw) <= RAW.MAX_ROW_BYTES, 'JOURNAL_BOUND')
            row = RAW.native_row(raw)
            need(type(row['sequence']) is int and row['sequence'] == count and row['previous'] == last, 'JOURNAL_CHAIN')
            last = row['hash']
            digest.update(raw)
            yield row
    need(count > 0, 'EMPTY_JOURNAL')
    if stats is not None:
        stats.update(decodedBytes=total, decodedSha256=digest.hexdigest(), verifiedRows=count, finalRowHash=last)


def fill_inputs(snapshot):
    report = snapshot['report']
    need(report['executionMode'] == 'live' and report['fillModel'] == 'actualAccountFills', 'ACTUAL_REPORT_REQUIRED')
    need(len(snapshot['fills']) <= MAX_FILLS and len(snapshot['orders']) <= 500_000, 'REPORT_BOUND')
    orders = {}
    for q in snapshot['orders']:
        qid = ident(q['id']); need(qid not in orders, 'DUPLICATE_ORDER'); orders[qid] = q
    excluded = {str(k) for k in report.get('reviewedFundamentalEntryReservations', {})}
    for p in report.get('livePackages', {}).get('records', []):
        excluded.update(ident(leg['qid']) for leg in p['legs'])
    fills, totals, bindings, reverse = {}, Counter(), {}, {}
    duplicates = 0
    cutoff = clock(report['at'])
    for f in snapshot['fills']:
        qid, actual, fid = ident(f['orderId']), ident(f['actualOrderId']), ident(f['sourceId'])
        need(qid in orders and f['counterfactual'] is False, 'ACTUAL_FILL_ORDER')
        q = orders[qid]; ex, market = ident(f['exchangeId']), ident(f['marketId'])
        side, action = f['side'], f['action']
        need(side in ('YES', 'NO') and action in ('buy', 'sell'), 'FILL_SIDE_ACTION')
        need(all(str(q[k]) == str(f[k]) for k in ('exchangeId', 'marketId', 'side', 'action')), 'FILL_SCOPE')
        n, price, fee = RAW.integer(f['quantity'], True), units(f['price']), units(f['fee'])
        need(0 < price < RAW.SCALE and price % 5000 == 0 and fee >= 0, 'FILL_PRICE_FEE')
        need(price <= units(q['price']) if action == 'buy' else price >= units(q['price']), 'FILL_LIMIT')
        executed, observed = clock(f['executedAt']), clock(f['at'])
        need(type(q['createdAt']) is int and q['createdAt'] * MS <= executed <= observed <= cutoff, 'FILL_CLOCK')
        need(isinstance(f.get('receiptHash'), str) and len(f['receiptHash']) == 64 and
             all(c in '0123456789abcdef' for c in f['receiptHash']), 'FILL_RECEIPT_HASH')
        need(qid not in bindings or bindings[qid] == actual, 'ORDER_REBOUND')
        need(actual not in reverse or reverse[actual] == qid, 'ACTUAL_ORDER_REBOUND')
        bindings[qid], reverse[actual] = actual, qid
        fact = (qid, actual, ex, market, side, action, n, price, fee, executed, observed, f['receiptHash'])
        key = actual, fid
        if key in fills:
            need(fills[key]['fact'] == fact, 'CONFLICTING_FILL_DUPLICATE'); duplicates += 1; continue
        totals[qid] += n
        fills[key] = dict(fact=fact, qid=qid, actual=actual, exchange=ex, market=market, side=side,
                          action=action, n=n, price=price, fee=fee, executed=executed,
                          observed=observed, order=q, posts={}, prefill=None, prefill_status='no_causal_prefill_book')
    need(report['fills'] == len(fills), 'REPORT_UNIQUE_FILL_COUNT')
    for qid, q in orders.items():
        need(totals[qid] == q['filled'] and 0 <= q['filled'] <= q['quantity'], 'ORDER_FILLED_TOTAL')
        if q.get('reviewedFundamentalEntry') is not None or q.get('executionStyle') is not None:
            excluded.add(qid)
    return list(fills.values()), excluded, duplicates


def attribution(dispatch, fills):
    """Original journal intent and POST ACK; recovered ACKs are not passive proof."""
    intents, acks = {}, {}
    actuals = {f['qid']: f['actual'] for f in fills}
    for row in dispatch:
        e = row['event']
        if e.get('kind') == 'intent':
            qid = ident(e['qid'])
            need(qid not in intents, 'DUPLICATE_INTENT')
            intents[qid] = e
        if e.get('kind') == 'ack' and str(e.get('qid')) in actuals:
            qid, r = ident(e['qid']), e['receipt']
            need(r['endpoint'] == '/orders' and r['status'] == 200 and
                 ident(r['data']['orderId']) == actuals[qid], 'ORIGINAL_ACK_BINDING')
            start, end = clock(r['requestStartedAt']), clock(r['receivedAt'])
            need(start <= end and qid not in acks, 'ORIGINAL_ACK_CLOCK_OR_DUPLICATE')
            acks[qid] = end
    return intents, acks


def liquidation(bids, asks, side, n):
    levels = bids if side == 'YES' else [(RAW.SCALE - p, q) for p, q in asks]
    remaining, proceeds, marginal = n, 0, None
    for p, q in levels:
        take = min(q, remaining); proceeds += p * take; remaining -= take; marginal = p
        if not remaining:
            break
    return {'coveredShares': n - remaining, 'proceedsUnits': proceeds,
            'marginalPriceUnits': marginal, 'fullQuantity': remaining == 0}


def levels(raw, descending):
    result = []
    need(isinstance(raw, list) and len(raw) <= 10000, 'DEPTH_BOUND')
    for r in raw:
        raw = Decimal(str(r['price']))
        need(raw.is_finite() and 0 < raw < 1, 'BOOK_PRICE_RANGE')
        canonical = (raw / Decimal('0.005')).quantize(Decimal(1)) * Decimal('0.005')
        need(raw.is_finite() and abs(raw - canonical) <= Decimal('0.000000000001'), 'OFFICIAL_TICK_REPRESENTATION')
        p, n = units(canonical), RAW.integer(r['quantity'], True)
        need(0 < p < RAW.SCALE and p % 5000 == 0 and
             (not result or (p < result[-1][0] if descending else p > result[-1][0])), 'DEPTH_INVALID')
        result.append((p, n))
    need(bool(result), 'DEPTH_EMPTY')
    return result


def analyze(snapshot, breadth, dispatch=None, report_sha=None):
    fills, excluded, duplicate_fills = fill_inputs(snapshot)
    intents, acks = attribution(dispatch, fills) if dispatch is not None else ({}, {})
    status = Counter(); selected = []
    for f in fills:
        qid = f['qid']; intent = intents.get(qid)
        why = ('not_buy' if f['action'] != 'buy' else
               'package_or_reviewed' if qid in excluded else
               'missing_original_intent' if intent is None else
               'fundamental_or_package_intent' if any(intent.get(k) is not None for k in ('fundamental', 'package', 'packageId')) else
               'missing_original_post_ack' if qid not in acks else
               'not_proved_rested_after_ack' if acks[qid] >= f['executed'] else 'ordinary_rested_after_ack')
        status[why] += 1
        if why == 'ordinary_rested_after_ack':
            selected.append(f)
    by_scope = defaultdict(list)
    for f in selected:
        by_scope[f['market'], f['exchange']].append(f)
    revisions, book_cursors, observations = {}, {}, Counter()
    cutoff, last_receive = clock(snapshot['report']['at']), None
    for row in breadth:
        e = row['event']
        need(e.get('source') == 'official_sig_realtime', 'BREADTH_SOURCE')
        received = clock(e['receivedAt'])
        need(last_receive is None or received >= last_receive, 'BREADTH_RECEIPT_ROLLBACK')
        last_receive = received
        if received > cutoff:
            observations['after_report_cutoff'] += 1; continue
        if e.get('event') == 'stream_reset':
            revisions.clear(); book_cursors.clear(); observations['stream_reset'] += 1
            for f in selected:
                if received <= f['executed']:
                    f['prefill'], f['prefill_status'] = None, 'prefill_invalidated_by_gap'
            continue
        if e.get('event') != 'markets_batch':
            continue
        for item in e['payload']['markets']:
            market, batch = ident(item['marketId']), item['batch']
            d = batch['delivery']; rev, prev = d['revision'], d['previousRevision']
            need(type(rev) is int and type(prev) is int and rev > prev >= 0 and
                 d['model'] == 'best-effort-authoritative-resync', 'DELIVERY_SCHEMA')
            old = revisions.get(market)
            if old is not None and rev <= old:
                observations['duplicate_or_older_revision'] += 1; continue
            revisions[market] = rev
            if batch.get('resyncRequired') or batch.get('marketSettled') or (old is not None and prev != old):
                observations['gap_resync_or_settled'] += 1
                for (mid, _), fs in by_scope.items():
                    if mid == market:
                        for f in fs:
                            if received <= f['executed']:
                                f['prefill'], f['prefill_status'] = None, 'prefill_invalidated_by_gap'
                for key in list(book_cursors):
                    if key[0] == market:
                        del book_cursors[key]
                continue
            for b in batch.get('books', []):
                key = market, ident(b['exchangeId'])
                if key not in by_scope:
                    continue
                source = clock(b['asOf']['at']); seq = RAW.integer(b['asOf']['sequence'])
                prior = book_cursors.get(key)
                signature = json.dumps(b, sort_keys=True, separators=(',', ':'), default=str)
                if prior and seq == prior[0]:
                    need(signature == prior[1], 'BOOK_CURSOR_CONFLICT')
                    observations['duplicate_book'] += 1; continue
                need(not prior or seq > prior[0], 'BOOK_CURSOR_ROLLBACK')
                book_cursors[key] = seq, signature
                label_status = 'valid'
                try:
                    bids, asks = levels(b['bids'], True), levels(b['asks'], False)
                    need(bids[0][0] < asks[0][0], 'DEPTH_CROSSED')
                except (RAW.Rejected, KeyError, TypeError, ValueError):
                    bids, asks, label_status = [], [], 'invalid_depth'
                if source > received:
                    label_status = 'future_source'
                elif received - source > AGE:
                    label_status = 'stale_source'
                observations[label_status + '_books'] += 1
                for f in by_scope[key]:
                    if received <= f['executed'] and source <= received:
                        f['prefill_status'] = label_status
                        if f['executed'] - source > AGE:
                            f['prefill_status'] = 'stale_at_execution'
                        f['prefill'] = dict(depth=liquidation(bids, asks, f['side'], f['n']),
                            journalHash=row['hash'], source=source, received=received) if f['prefill_status'] == 'valid' else None
                    for h in HORIZONS:
                        target = f['executed'] + h * MS
                        if h in f['posts'] or source < target or received < target:
                            continue
                        if source > target + SLACK:
                            continue
                        effective_status = label_status if label_status != 'valid' else ('late_observation' if received > target + SLACK else 'valid')
                        result = liquidation(bids, asks, f['side'], f['n']) if effective_status == 'valid' else None
                        f['posts'][h] = dict(status=effective_status if result is None else ('full_depth' if result['fullQuantity'] else 'insufficient_depth'),
                          source=source, received=received, journalHash=row['hash'], depth=result,
                          beforeFillDiscovery=received < f['observed'])

    def aggregate(items):
        horizons = {}
        for h in HORIZONS:
            counts, qty, total, principal, entry_fees, covered_only = Counter(), 0, 0, 0, 0, 0
            before_discovery = 0
            movement, movement_n, movement_fills = 0, 0, 0
            negative, zero, positive = 0, 0, 0
            full = []
            for f in items:
                p = f['posts'].get(h)
                if p is None:
                    why = 'right_censored' if cutoff < f['executed'] + (h + 5000) * MS else 'no_original_book_in_horizon_window'
                    counts[why] += 1; continue
                counts[p['status']] += 1
                before_discovery += p['beforeFillDiscovery']
                if p['depth']:
                    covered_only += p['depth']['coveredShares']
                if p['status'] == 'full_depth':
                    full.append(f)
                    qty += f['n']; principal += f['price'] * f['n']; entry_fees += f['fee']
                    value = p['depth']['proceedsUnits'] - f['price'] * f['n'] - f['fee']
                    total += value
                    negative += value < 0; zero += value == 0; positive += value > 0
                    pre = f['prefill']
                    if pre and pre['depth']['fullQuantity']:
                        movement += p['depth']['proceedsUnits'] - pre['depth']['proceedsUnits']
                        movement_n += f['n']; movement_fills += 1
            cluster_end, clusters = {}, 0
            for f in sorted(full, key=lambda x: x['executed']):
                key = f['market'], f['exchange']
                if key not in cluster_end or f['executed'] >= cluster_end[key]:
                    clusters += 1; cluster_end[key] = f['executed'] + (h + 5000) * MS
            horizons[str(h)] = {'coverage': dict(counts), 'fullDepthShares': qty,
              'fullDepthIntentCohorts': len({f['qid'] for f in full}),
              'fullDepthMarketScopes': len({(f['market'], f['exchange']) for f in full}),
              'nonOverlappingMarketTimeClusters': clusters,
              'displayedCoveredSharesIncludingPartial': covered_only, 'actualEntryPrincipal': money(principal),
              'actualEntryFees': money(entry_fees), 'hypotheticalLiquidationMinusEntryAndEntryFees': money(total) if qty else None,
              'quantityWeightedMarkoutPerShare': RAW.mean_ratio(total, qty * RAW.SCALE) if qty else None,
              'fullDepthMarkSigns': {'negative': negative, 'zero': zero, 'positive': positive},
              'causalPrefillMatchedFills': movement_fills, 'causalPrefillMatchedShares': movement_n,
              'futureMinusCausalPrefillLiquidationValue': money(movement) if movement_n else None,
              'quantityWeightedSubsequentBidMovement': RAW.mean_ratio(movement, movement_n * RAW.SCALE) if movement_n else None,
              'labelsBeforeNativeFillDiscovery': before_discovery, 'exitFeesKnown': False}
        return {'actualFillObservations': len(items), 'actualShares': sum(f['n'] for f in items),
                'intentCohorts': len({f['qid'] for f in items}),
                'causalPrefillCoverage': dict(Counter(f['prefill_status'] if f['prefill'] is None else
                   ('full_depth' if f['prefill']['depth']['fullQuantity'] else 'insufficient_depth') for f in items)), 'horizonsMs': horizons}

    groups = []
    for market, ex, side in sorted({(f['market'], f['exchange'], f['side']) for f in selected}):
        fs = [f for f in selected if (f['market'], f['exchange'], f['side']) == (market, ex, side)]
        groups.append(dict(marketId=market, exchangeId=ex, side=side, **aggregate(fs)))
    cohorts = []
    for qid in sorted({f['qid'] for f in selected}, key=int):
        fs = [f for f in selected if f['qid'] == qid]
        evidence = [{'actualExecutionMs': str(Decimal(f['executed']) / MS),
                     'actualFillShares': f['n'], 'actualPrice': money(f['price']), 'actualEntryFee': money(f['fee']),
                     'horizons': {str(h): {'status': p['status'], 'bookJournalHash': p['journalHash'],
                       'originalSourceMs': str(Decimal(p['source']) / MS), 'originalReceiptMs': str(Decimal(p['received']) / MS),
                       'coveredShares': p['depth']['coveredShares'] if p['depth'] else None,
                       'fullQuantity': p['depth']['fullQuantity'] if p['depth'] else False,
                       'hypotheticalProceeds': money(p['depth']['proceedsUnits']) if p['depth'] else None}
                       for h, p in sorted(f['posts'].items())}} for f in fs]
        cohorts.append(dict(intentCohortSha256=hashlib.sha256(((report_sha or '') + ':' + qid).encode()).hexdigest(),
                            marketId=fs[0]['market'], exchangeId=fs[0]['exchange'], side=fs[0]['side'], labelEvidence=evidence, **aggregate(fs)))
    return {'schemaVersion': 1, 'analysis': 'actual_ordinary_rested_buy_postfill_markouts',
      'reportCutoff': snapshot['report']['at'], 'horizonsMs': list(HORIZONS), 'labelSlackMs': 5000, 'maximumSourceAgeMs': 10000,
      'attribution': dict(status), 'duplicateActualFillObservations': duplicate_fills,
      'aggregate': aggregate(selected), 'perMarketSide': groups, 'intentCohorts': cohorts, 'breadthDiagnostics': dict(observations),
      'venueMakerRoleVerified': False, 'financialReplayPerformed': False, 'profitabilityEstablished': False,
      'ownRestingLiquidityRemoved': False, 'selfTradePreventionCanReduceExecutableDepth': True,
      'limitations': ['Only genuine attributed fills are used; no public print/touch is an own fill or queue-position label.',
       'Rested-after-original-ACK is not proof of venue maker classification or unconditional entry alpha.',
       'Breadth receipt is an optimistic source-level availability clock, not native processing or physical preflight.',
       'Missing/stale/gap/depth-limited horizons are unavailable, not zero returns; no favorable replacement of a selected shallow/invalid book.',
       'Displayed full-quantity hypothetical exits can share liquidity across fills/cohorts and are not additive feasible portfolio profit.',
       'Entry fees are actual; hypothetical future exit fees are unknown and not fabricated. No realized exit or remaining inventory mark is changed.',
       'The causal pre-fill book can contain our own resting quote; its depletion can cause a later bid decline without new information or informed flow.',
       'Displayed depth can contain own resting orders at either clock; self-trade prevention can make part of it inaccessible. Values are optimistic displayed-depth diagnostics, not guaranteed own executable proceeds.',
       'Actual BUY quantities may already have netted or sold before a horizon; liquidation is a diagnostic, not a claim those lots remained held.']}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--report', required=True); p.add_argument('--report-sha256', required=True)
    p.add_argument('--breadth', action='append', nargs=3, metavar=('FILE', 'SHA256', 'BYTES'), required=True)
    p.add_argument('--dispatch', nargs=3, metavar=('FILE', 'SHA256', 'BYTES'))
    p.add_argument('--output', required=True)
    a = p.parse_args()
    try:
        inputs = [a.report] + [x[0] for x in a.breadth] + ([a.dispatch[0]] if a.dispatch else [])
        need(Path(a.output).resolve() not in {Path(x).resolve() for x in inputs}, 'OUTPUT_OVERWRITES_INPUT')
        report_pin = pinned(a.report, a.report_sha256)
        need(report_pin['bytes'] <= 64 * 1024**2, 'REPORT_SIZE')
        snapshot = RAW.native_sealed_object(Path(a.report).read_bytes())
        sources = {'report': report_pin, 'breadth': [], 'dispatch': None}
        for file, sha, size in a.breadth:
            sources['breadth'].append(pinned(file, sha, int(size)))
        if a.dispatch:
            sources['dispatch'] = pinned(a.dispatch[0], a.dispatch[1], int(a.dispatch[2]))
        def breadth():
            for i, (file, _, _) in enumerate(a.breadth):
                yield from rows(file, '', sources['breadth'][i])
        result = analyze(snapshot, breadth(), rows(a.dispatch[0], '0' * 64, sources['dispatch']) if a.dispatch else None, a.report_sha256)
        # Hash again after streaming: a mutable input is not a frozen original.
        for file, sha, size in a.breadth + ([a.dispatch] if a.dispatch else []):
            pinned(file, sha, int(size))
        pinned(a.report, a.report_sha256)
        result['source'] = sources
        result['sourceCodeSha256'] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
        result['rawSealParserSha256'] = hashlib.sha256(RAW_PATH.read_bytes()).hexdigest()
        Path(a.output).write_text(json.dumps(result, sort_keys=True, indent=2) + '\n', encoding='utf-8')
        return 0
    except (ValueError, KeyError, TypeError, OSError, OverflowError):
        print('analysis rejected: SOURCE_OR_SCHEMA_OR_BOUND', file=sys.stderr)
        return 2


if __name__ == '__main__':
    raise SystemExit(main())
