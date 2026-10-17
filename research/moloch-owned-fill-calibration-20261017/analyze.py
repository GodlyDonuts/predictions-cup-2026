#!/usr/bin/env python3
"""Finite, offline ACK/closure and displayed-depth audit. Grants no authority."""
from __future__ import annotations
import argparse
from collections import Counter, defaultdict
from decimal import Decimal
import hashlib
import importlib.util
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[2]
RAW_PATH = ROOT / 'research/moloch-owned-fill-markouts-20261015/analyze.py'
spec = importlib.util.spec_from_file_location('owned_fill_exact_raw', RAW_PATH)
RAW = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = RAW
spec.loader.exec_module(RAW)
need, ident, units, clock, money = RAW.check, RAW.identifier, RAW.units, RAW.instant, RAW.money
MS, SCALE = 1_000_000, 1_000_000
MAX_FILE, MAX_ROWS, MAX_RECEIPTS = 2 * 1024**3, 1_000_000, 10000
HORIZONS = (1000, 5000, 30000, 120000)


def load(raw):
    return json.loads(raw, parse_float=Decimal, object_pairs_hook=RAW.unique)


def digest_file(path):
    h, total = hashlib.sha256(), 0
    with path.open('rb') as f:
        for b in iter(lambda: f.read(1024**2), b''):
            h.update(b); total += len(b)
            need(total <= MAX_FILE, 'FILE_BOUND')
    return h.hexdigest(), total


def pinned_sources(directory, expected):
    directory = Path(directory)
    pins_path = directory / 'SHA256SUMS.json'
    need(digest_file(pins_path)[0] == expected, 'SOURCE_MANIFEST_PIN')
    pins = load(pins_path.read_bytes())
    need(isinstance(pins, dict) and 1 <= len(pins) <= MAX_RECEIPTS + 20, 'SOURCE_COUNT')
    for name, sha in pins.items():
        relative = Path(name)
        need(not relative.is_absolute() and '..' not in relative.parts, 'SOURCE_PATH')
        path = directory / relative
        need(path.is_file() and not path.is_symlink(), 'SOURCE_REGULAR_FILE')
        need(digest_file(path)[0] == sha, 'ORIGINAL_FILE_PIN')
    meta = load((directory / 'input-manifest.json').read_bytes())
    report = load((directory / 'report.json').read_bytes())
    need(report['sequence'] == meta['bankPrefixSequence'] and
         report['report']['at'] == meta['bankAt'] and len(report['fills']) == meta['bankFills'] and
         pins['report.json'] == meta['bankReportRawSha256'], 'FROZEN_REPORT_BOUNDARY')
    need(report['report']['executionMode'] == 'live' and
         report['report']['fillModel'] == 'actualAccountFills', 'ACTUAL_REPORT_REQUIRED')
    return meta, report, pins


def journal(path, subset=False, stats=None):
    previous, count, last_sequence = '0' * 64, 0, 0
    with path.open('rb') as f:
        for raw in f:
            count += 1
            need(count <= MAX_ROWS and len(raw) <= RAW.MAX_ROW_BYTES, 'JOURNAL_BOUND')
            row = RAW.native_row(raw)
            seq = RAW.integer(row['sequence'], True)
            need(seq > last_sequence, 'JOURNAL_SEQUENCE')
            if not subset:
                need(seq == count and row['previous'] == previous, 'DISPATCH_CHAIN')
            elif seq == last_sequence + 1 and last_sequence:
                need(row['previous'] == previous, 'ADJACENT_SUBSET_CHAIN')
            previous, last_sequence = row['hash'], seq
            yield row
    if stats is not None:
        stats.update(rows=count, lastSequence=last_sequence, lastHash=previous,
                     completeChain=not subset)


def canonical_equal(a, b):
    return a == b


def receipt_window(receipt, cutoff):
    need(receipt['status'] == 200, 'RECEIPT_STATUS')
    start, end = clock(receipt['requestStartedAt']), clock(receipt['receivedAt'])
    need(start <= end <= cutoff, 'RECEIPT_CLOCK')
    return start, end


def fill_rows(tape, actual, exchange, tid, side, cutoff):
    receipt_window(tape, cutoff)
    need(tape['endpoint'] == f'/orders/{actual}/fills?limit=200', 'CANONICAL_FILL_ENDPOINT')
    data = tape['data']
    need(ident(data['orderId']) == actual and ident(data['exchangeId']) == exchange and
         data['tournamentId'] == tid and data['coverage']['complete'] is True and
         data['pagination']['hasMore'] is False and data['pagination']['nextCursor'] is None,
         'COMPLETE_SCOPED_TAPE')
    result = {}
    for f in data['data']:
        fid = ident(f['id']); n = f['quantity']
        need(type(n) is int and n != 0 and (n > 0) == (side == 'YES') and
             f['side'].upper() == side and fid not in result, 'FILL_ID_SIDE_QUANTITY')
        price, executed = units(f['price']), clock(f['filledAt'])
        need(0 < price < SCALE and price % 5000 == 0 and executed <= clock(tape['receivedAt']),
             'FILL_PRICE_CLOCK')
        result[fid] = (abs(n), price, executed)
    total = sum(f[0] for f in result.values())
    need(abs(data['totalQuantityFilled']) == total and
         (not total or (data['totalQuantityFilled'] > 0) == (side == 'YES')), 'TAPE_TOTAL')
    return result


def terminal_pair(order, tape, actual, q, tid, admitted, cutoff):
    _, order_end = receipt_window(order, cutoff)
    tape_start, tape_end = receipt_window(tape, cutoff)
    need(order['endpoint'] == f'/orders/{actual}' and tape_start >= order_end,
         'TERMINAL_PAIR_ORDER')
    d = order['data']
    need(ident(d['id']) == actual and ident(d['exchangeId']) == ident(q['exchangeId']) and
         d['tournamentId'] == tid and d['side'].upper() == q['side'] and
         d['action'].lower() == q['action'] and units(d['priceLimit']) == units(q['price']) and
         d['open'] is False and d['quantity'] == admitted, 'TERMINAL_ORDER_BINDING')
    fills = fill_rows(tape, actual, ident(q['exchangeId']), tid, q['side'], cutoff)
    total = sum(x[0] for x in fills.values())
    need(d['quantityFilled'] == total and 0 <= total <= admitted, 'TERMINAL_TOTAL')
    return fills, tape_end


def lineage(directory, meta, snapshot):
    cutoff = clock(meta['bankAt'])
    native = {ident(q['id']): q for q in snapshot['orders']}
    need(len(native) == len(snapshot['orders']), 'NATIVE_ORDER_DUPLICATE')
    intents, attempts, acks, closed, aborts, all_rows = {}, {}, {}, {}, set(), {}
    chain = {}
    for row in journal(directory / 'dispatch.ndjson', stats=chain):
        e = row['event']; qid = ident(e['qid']); kind = e['kind']
        all_rows.setdefault(qid, {})[kind] = row['sequence']
        target = {'intent': intents, 'attempt': attempts, 'ack': acks,
                  'recovered_ack': acks, 'closed': closed}.get(kind)
        if target is not None:
            need(qid not in target, 'DUPLICATE_LIFECYCLE_EVENT')
            target[qid] = e
        if kind == 'aborted_before_admission':
            aborts.add(qid)
    need(chain['lastSequence'] == meta['dispatchVerifiedThroughSequence'] and
         chain['lastHash'] == meta['dispatchVerifiedThroughHash'], 'DISPATCH_BOUNDARY')
    actual_fills, totals = {}, Counter()
    for f in snapshot['fills']:
        qid, actual, fid = ident(f['orderId']), ident(f['actualOrderId']), ident(f['sourceId'])
        q = native[qid]
        need(f['counterfactual'] is False and all(str(f[k]) == str(q[k]) for
             k in ('exchangeId', 'marketId', 'side', 'action')), 'NATIVE_ACTUAL_FILL_SCOPE')
        fact = (f['quantity'], units(f['price']), clock(f['executedAt']))
        need(type(f['quantity']) is int and f['quantity'] > 0 and units(f['fee']) >= 0,
             'NATIVE_FILL_QUANTITY_FEE')
        need((actual, fid) not in actual_fills, 'NATIVE_FILL_DUPLICATE')
        actual_fills[actual, fid] = (qid, fact, f)
        totals[qid] += f['quantity']
    for qid, q in native.items():
        need(totals[qid] == q['filled'], 'NATIVE_FILLED_TOTAL')
    receipts = {}
    def original(sha):
        if sha in receipts:
            return receipts[sha]
        path = directory / 'receipts' / (sha + '.json')
        if not path.exists():
            return None
        need(sha in meta['receipts'] and digest_file(path)[0] == meta['receipts'][sha]['rawSha256'],
             'ORIGINAL_RECEIPT_PIN')
        receipts[sha] = load(path.read_bytes())
        return receipts[sha]
    records, exclusions = [], Counter()
    for qid, ack in acks.items():
        if qid not in native:
            exclusions['ack_qid_outside_native_prefix'] += 1; continue
        q = native[qid]; i = intents[qid]; attempt = attempts[qid]
        r = ack.get('receipt', ack.get('order'))
        if clock(r['receivedAt']) > cutoff:
            exclusions['ack_after_native_cutoff'] += 1; continue
        start, end = receipt_window(r, cutoff)
        need(clock(i['at']) <= clock(attempt['at']) <= start and
             all(i['quote'][k] == q[k] for k in ('id', 'exchangeId', 'marketId', 'side', 'action', 'price')),
             'ORIGINAL_INTENT_ACK_ORDER')
        body = i['spec']['body']
        need(i['spec']['method'] == 'POST' and i['spec']['endpoint'] == '/orders' and
             canonical_equal(load(i['spec']['bodyJson']), body) and
             ident(body['exchangeId']) == ident(q['exchangeId']) and body['side'].upper() == q['side'] and
             body['action'].lower() == q['action'] and units(body['price']) == units(q['price']) and
             0 < body['quantity'] <= i['quote']['quantity'], 'ORIGINAL_FROZEN_REQUEST')
        recovered = ack['kind'] == 'recovered_ack'
        d = r['data']; actual = ident(d['id'] if recovered else d['orderId'])
        admitted = d['quantity']
        need(0 < admitted <= body['quantity'] and ident(d['exchangeId']) == ident(q['exchangeId']) and
             d['side'].upper() == q['side'] and d['action'].lower() == q['action'] and
             units(d['priceLimit'] if recovered else d['price']) == units(q['price']), 'ACK_EXACT_BINDING')
        reviewed = i.get('fundamental') is not None
        package = any(i.get(k) is not None for k in ('package', 'packageId')) or q.get('packageId') is not None
        mode = q.get('reviewedFundamentalEntry', {}).get('executionMode', 'original_intent_mode_unavailable')
        group = ('package_or_unclassified' if package else
                 'reviewed_buy:' + mode if reviewed and q['action'] == 'buy' else
                 'reviewed_sell' if reviewed else
                 'ordinary_buy:opposite_coverage_unavailable' if q['action'] == 'buy' else
                 'ordinary_sell:' + q.get('inventoryExitReason', 'reason_unavailable'))
        rec = dict(qid=qid, q=q, group=group, requested=body['quantity'], admitted=admitted,
                   original_quote_quantity=i['quote']['quantity'], tournament=body['tournamentId'],
                   actual=actual, ack_start=None if recovered else start, ack_end=end,
                   recovered=recovered, status='unresolved', fill_total=totals[qid],
                   terminal_available=None, closed_sequence=None, pre=None, pre_status='no_causal_book')
        c = closed.get(qid)
        if c is not None:
            rec['closed_receipt_refs'] = c['orderReceiptHash'], c['fillReceiptHash']
            order, tape = original(c['orderReceiptHash']), original(c['fillReceiptHash'])
            if order is None or tape is None:
                # Original recovered receipts remain immutable inline dispatch inputs.
                if recovered and ack['event']['receiptHash'] == c['orderReceiptHash'] and ack['event']['fillReceiptHash'] == c['fillReceiptHash']:
                    order, tape = ack['order'], ack['fills']; rec['inline_recovered_receipts'] = True
                else:
                    rec['status'] = 'missing_original_closure_receipt'
            if order is not None and tape is not None:
                if max(clock(order['receivedAt']), clock(tape['receivedAt'])) > cutoff:
                    rec['status'] = 'closure_after_native_cutoff'
                else:
                    fs, available = terminal_pair(order, tape, actual, q, body['tournamentId'], admitted, cutoff)
                    need(q['retired'] is True and sum(x[0] for x in fs.values()) == totals[qid], 'NATIVE_TERMINAL_TOTAL')
                    for fid, fact in fs.items():
                        need((actual, fid) in actual_fills and actual_fills[actual, fid][:2] == (qid, fact),
                             'ORIGINAL_TAPE_NATIVE_FILL_MEMBERSHIP')
                    rec.update(status='confirmed_full' if totals[qid] == admitted else
                               'confirmed_partial' if totals[qid] else 'confirmed_zero',
                               terminal_available=available, closed_sequence=all_rows[qid]['closed'])
        records.append(rec)
    need(len(records) == snapshot['contestOrders'], 'PLACED_NATIVE_COUNT_PARITY')
    fill_records = []
    by_qid = {r['qid']: r for r in records}
    for (_, _), (qid, _, f) in actual_fills.items():
        need(qid in by_qid, 'FILLED_WITHOUT_ORIGINAL_ACK')
        fill_records.append(dict(qid=qid, order=by_qid[qid], n=f['quantity'], price=units(f['price']),
                                 fee=units(f['fee']), executed=clock(f['executedAt']),
                                 observed=clock(f['at']), native=f, posts={}))
    return records, fill_records, dict(exclusions), chain, all_rows


def levels(raw, descending):
    need(isinstance(raw, list) and len(raw) <= 10000 and bool(raw), 'DEPTH_EMPTY_OR_BOUND')
    result = []
    for x in raw:
        p = Decimal(str(x['price'])); need(p.is_finite(), 'BOOK_NONFINITE')
        tick = (p / Decimal('.005')).quantize(Decimal(1)) * Decimal('.005')
        need(abs(p - tick) <= Decimal('1e-12'), 'BOOK_OFF_TICK')
        price, n = units(tick), RAW.integer(x['quantity'], True)
        need(0 < price < SCALE and (not result or (price < result[-1][0] if descending else price > result[-1][0])), 'BOOK_DEPTH_ORDER')
        result.append((price, n))
    return result


def depth_value(bids, asks, side, action, quantity):
    depth = bids if side == 'YES' and action == 'sell' else asks if side == 'YES' else (
        [(SCALE-p, n) for p, n in asks] if action == 'sell' else [(SCALE-p, n) for p, n in bids])
    remaining, value = quantity, 0
    for price, n in depth:
        take = min(remaining, n); value += price * take; remaining -= take
        if not remaining:
            return value
    return None


def external_depth(bids, asks, key, orders, fills, at):
    """Subtract known still-open own liquidity, never infer cancel ACK terminality.

    Unknown later fills can make this conservative, not a fictitious external
    fill. An observed own quantity larger than a displayed level is unavailable.
    Only original information available before `at` is used.
    """
    own = Counter()
    for o in orders:
        q = o['q']
        if (ident(q['marketId']), ident(q['exchangeId'])) != key or o.get('native_ack_available', at) >= at:
            continue
        if o.get('terminal_available') is not None and o['terminal_available'] < at:
            continue
        filled = sum(f['n'] for f in fills if f['qid'] == o['qid'] and f['observed'] < at)
        remaining = o['admitted']-filled
        need(remaining >= 0, 'KNOWN_REMAINING_QUANTITY')
        side = 'bid' if (q['side']=='YES') == (q['action']=='buy') else 'ask'
        price = units(q['price']) if q['side']=='YES' else SCALE-units(q['price'])
        own[side,price] += remaining
    result, deducted = [], 0
    for name, depth in [('bid',bids),('ask',asks)]:
        out=[]
        for price,n in depth:
            remove=own[name,price]
            if remove > n:
                return [], [], 'own_depth_quantity_uncertain', deducted
            deducted += remove
            if n > remove: out.append((price,n-remove))
        result.append(out)
    return result[0], result[1], 'valid', deducted


def causal_features(o, orders, fills):
    if o['recovered']:
        return dict(status='recovered_ack_original_post_unavailable')
    if o['pre_status'] != 'available':
        return dict(status=o['pre_status'])
    b, q = o['pre'], o['q']; key=ident(q['marketId']),ident(q['exchangeId'])
    bids,asks,state,removed=external_depth(b['bids'],b['asks'],key,orders,fills,o['ack_start'])
    if state!='valid' or not bids or not asks:
        return dict(status=state if state!='valid' else 'no_external_two_sided_depth')
    bid,ask=(bids[0][0],asks[0][0]) if q['side']=='YES' else (SCALE-asks[0][0],SCALE-bids[0][0])
    price=units(q['price'])
    placement=('marketable_at_original_book' if price>=ask else 'inside_spread' if price>bid else
               'join_bid' if price==bid else 'behind_bid') if q['action']=='buy' else 'sell_not_entry_feature'
    return dict(status='available', originalBookNativeSequence=b['sequence'],
        sourceAgeMs=(o['ack_start']-b['source'])//MS,
        receivedAgeMs=(o['ack_start']-b['received'])//MS,
        processedAgeMs=(o['ack_start']-b['processed'])//MS,
        bookFirstAvailableAgeMs=(o['ack_start']-b['first_available'])//MS,
        sideBid=money(bid),sideAsk=money(ask),spreadTicks=(ask-bid)//5000,
        unchangedLimit=money(price),distanceFromSideBidTicks=(bid-price)//5000,
        placement=placement,physicalQuantity=o['requested'],knownOwnSharesRemoved=removed,
        knownOppositeOwnedQuantity=o.get('known_opposite_quantity'),unreservedOppositeCoverageProven=False)


def book_observations(rows, orders, fills, cutoff, policy):
    by_scope_orders, by_scope_fills = defaultdict(list), defaultdict(list)
    for o in orders:
        if o['ack_start'] is not None:
            by_scope_orders[ident(o['q']['marketId']), ident(o['q']['exchangeId'])].append(o)
    for f in fills:
        q = f['order']['q']; by_scope_fills[ident(q['marketId']), ident(q['exchangeId'])].append(f)
    cursors, stats = {}, Counter()
    for row in rows:
        e = row['event']; need(e['type'] == 'book', 'BOOK_SUBSET_TYPE')
        b = e['book']; key = ident(b['marketId']), ident(b['exchangeId'])
        source, received, request, processed = (clock(b['sourceAt']), clock(b['receivedAt']),
                                                clock(b['requestStartedAt']), clock(e['processedAt']))
        need(received == clock(e['receivedAt']), 'BOOK_RECEIPT_IDENTITY')
        if processed > cutoff:
            stats['after_cutoff'] += 1; continue
        version = RAW.integer(int(b['sequence'])), source
        signature = json.dumps([b.get('bids'), b.get('asks')], sort_keys=True, separators=(',', ':'), default=str)
        old = cursors.get(key); state = 'valid'
        ref=e.get('sourceRef')
        if not (isinstance(ref,dict) and isinstance(ref.get('id'),str) and ref['id'] and
                type(ref.get('sequence')) is int and ref['sequence']>0 and
                isinstance(ref.get('hash'),str) and len(ref['hash'])==64 and
                all(c in '0123456789abcdef' for c in ref['hash'])):
            state='missing_recorded_source_ref'
        if policy.get('tournamentId') is not None and b.get('tournamentId') != policy['tournamentId']:
            state='wrong_tournament_scope'
        first_available = processed
        if old:
            if version == old['version']:
                if signature != old['signature']:
                    state = 'conflicting_same_version'
                first_available = old['first_available']
            elif version[0] < old['version'][0] or source < old['version'][1]:
                state = 'version_rollback'
        if b.get('invalid') is True:
            state = 'explicit_invalid_or_gap'
        if not (source <= received <= processed and request <= received):
            state = 'invalid_clock'
        observed = source if b.get('transport')=='official_sig_realtime' else min(source,request)
        if processed - observed > policy['maximumSourceAgeMs'] * MS:
            state = 'stale_at_processing'
        if state == 'valid':
            try:
                bids, asks = levels(b['bids'], True), levels(b['asks'], False)
                need(bids[0][0] < asks[0][0], 'BOOK_CROSSED')
            except (RAW.Rejected, KeyError, ValueError, TypeError):
                bids, asks, state = [], [], 'invalid_depth'
        else:
            bids, asks = [], []
        if state != 'version_rollback':
            cursors[key] = dict(version=version, signature=signature, first_available=first_available)
        stats[state] += 1
        witness = dict(sequence=row['sequence'], hash=row['hash'], source=source, received=received,
                       processed=processed, first_available=first_available, observed=observed,
                       state=state, bids=bids, asks=asks)
        for o in by_scope_orders.get(key, ()):
            at = o['ack_start']
            if processed < at and (o['pre'] is None or row['sequence'] > o['pre']['sequence']):
                o['pre'] = witness
                o['pre_status'] = state if state != 'valid' else ('stale_at_decision' if at-observed > policy['maximumSourceAgeMs']*MS else 'available')
        for f in by_scope_fills.get(key, ()):
            for h in HORIZONS:
                if h in f['posts']:
                    continue
                target = f['executed'] + h * MS
                if target <= source <= target + policy['postSourceAndReceiptWindowMs'] * MS:
                    status = state
                    if received > target + policy['postSourceAndReceiptWindowMs'] * MS:
                        status = 'late_receipt'
                    if first_available < f['executed']:
                        status = 'available_before_execution_clock_conflict'
                    value = None
                    if status == 'valid':
                        action = 'sell' if f['order']['q']['action'] == 'buy' else 'buy'
                        ebids,easks,status,_=external_depth(bids,asks,key,orders,fills,processed)
                        if status=='valid':
                            value = depth_value(ebids, easks, f['order']['q']['side'], action, f['n'])
                            if value is None:
                                status = 'shallow_depth'
                    gain = None if value is None or status != 'valid' else (
                        value-f['price']*f['n']-f['fee'] if f['order']['q']['action'] == 'buy' else f['price']*f['n']-value-f['fee'])
                    f['posts'][h] = dict(status=status, gain=gain, witness=witness,
                                         before_fill_discovery=processed < f['observed'])
    for f in fills:
        for h in HORIZONS:
            if h not in f['posts']:
                target = f['executed'] + h*MS
                f['posts'][h] = dict(status='right_censored' if cutoff < target + policy['postSourceAndReceiptWindowMs']*MS else 'no_original_book_in_window', gain=None)
    return dict(stats)


def fraction(a, b):
    return None if not b else format(Decimal(a)/Decimal(b), '.6f')


def native_membership(directory, expected, meta, records, fills):
    directory = Path(directory)
    manifest = load((directory/'membership-manifest.json').read_bytes())
    need(manifest['rawSubsetSha256'] == expected and digest_file(directory/'native-membership.jsonl')[0] == expected and
         manifest['sourcePrefixSha256'] == meta['sourcePrefixSha256'] and
         manifest['sourcePrefixSequence'] == meta['bankPrefixSequence'], 'MEMBERSHIP_PREFIX_PIN')
    acknowledgments, terminals, financial, counts = defaultdict(list), {}, [], Counter()
    native_fills = {}
    for row in journal(directory/'native-membership.jsonl', subset=True):
        need(row['sequence'] <= meta['bankPrefixSequence'], 'MEMBERSHIP_AFTER_CUTOFF')
        e = row['event']; kind = e['type']; counts[kind] += 1
        if kind == 'actual_order_ack':
            acknowledgments[ident(e['orderId'])].append(e)
        elif kind == 'actual_order_terminal':
            qid = ident(e['orderId']); need(qid not in terminals, 'DUPLICATE_NATIVE_TERMINAL')
            terminals[qid] = e
        elif kind == 'actual_position_adoption':
            financial.append(e)
        elif kind == 'actual_fill':
            key = ident(e['actualOrderId']), ident(e['fillId'])
            # A later complete tape may repeat the identical actual fill with a
            # different receipt/processing clock. Economic identity must not change.
            fact = tuple(str(e[k]) for k in ('orderId','exchangeId','marketId','side','action','quantity','price','fee','executedAt'))
            if key in native_fills:
                need(native_fills[key][0] == fact, 'CHANGED_NATIVE_FILL_DUPLICATE')
            else:
                native_fills[key] = fact, e
                financial.append(e)
    need(dict(counts) == manifest['types'] and len(native_fills) == len(fills), 'MEMBERSHIP_COUNTS')
    for o in records:
        q = o['q']; matches = [e for e in acknowledgments[o['qid']] if
            ident(e['actualOrderId']) == o['actual'] and e['quantity'] == o['admitted'] and
            all(str(e[k]) == str(q[k]) for k in ('exchangeId','marketId','side','action','price')) and
            clock(e['receivedAt']) == o['ack_end']]
        need(bool(matches), 'ORIGINAL_ACK_NATIVE_MEMBERSHIP')
        o['native_ack_available'] = min(clock(e['processedAt']) for e in matches)
        if o['status'].startswith('confirmed_'):
            t = terminals.get(o['qid'])
            need(t is not None and ident(t['actualOrderId']) == o['actual'] and t['fillsComplete'] is True and
                 t['filledQuantity'] == o['fill_total'] and
                 all(str(t[k]) == str(q[k]) for k in ('exchangeId','marketId','side','action')) and
                 clock(t['receivedAt']) == o['terminal_available'] and
                 (t['orderReceiptHash'], t['receiptHash']) == o['closed_receipt_refs'],
                 'ORIGINAL_TERMINAL_NATIVE_MEMBERSHIP')
            o['terminal_available'] = max(o['terminal_available'], clock(t['processedAt']))
    for f in fills:
        original = f['native']; key = ident(original['actualOrderId']), ident(original['sourceId'])
        need(key in native_fills, 'REPORT_FILL_MEMBERSHIP')
        e = native_fills[key][1]
        need(all(str(e[k]) == str(original[k]) for k in
                 ('exchangeId','marketId','side','action','quantity','price','fee','executedAt','receiptHash')) and
             ident(e['orderId']) == f['qid'] and clock(e['processedAt']) == f['observed'], 'REPORT_FILL_EXACT')
    # Native commit-order ownership, not execution-time sorting. The feature is
    # known-owned quantity, not proof of unreserved available hedge coverage.
    eligible = sorted((o for o in records if o['ack_start'] is not None), key=lambda o:o['ack_start'])
    inventory, idx = Counter(), 0
    for e in financial:
        need(clock(e['processedAt']) <= clock(meta['bankAt']), 'FINANCIAL_AFTER_CUTOFF')
    need(all(clock(a['processedAt']) <= clock(b['processedAt']) for a,b in zip(financial, financial[1:])),
         'FINANCIAL_OBSERVATION_CLOCK_ROLLBACK')
    for o in eligible:
        while idx < len(financial) and clock(financial[idx]['processedAt']) < o['ack_start']:
            e = financial[idx]; idx += 1
            if e['type'] == 'actual_position_adoption':
                for p in e['positions']:
                    for lot in p['lots']:
                        inventory[ident(p['exchangeId']), lot['side']] += lot['quantity']
            else:
                ex, side, n = ident(e['exchangeId']), e['side'], e['quantity']
                other = 'NO' if side == 'YES' else 'YES'
                if e['action'] == 'sell':
                    need(inventory[ex,side] >= n, 'CAUSAL_SELL_INVENTORY')
                    inventory[ex,side] -= n
                else:
                    net = min(inventory[ex,other], n)
                    inventory[ex,other] -= net; inventory[ex,side] += n-net
        ex, side = ident(o['q']['exchangeId']), o['q']['side']
        o['known_opposite_quantity'] = inventory[ex, 'NO' if side == 'YES' else 'YES']
        if o['group'].startswith('ordinary_buy:'):
            o['group'] = 'ordinary_buy:' + ('known_opposite_zero' if not o['known_opposite_quantity'] else 'opposite_owned_coverage_unavailable')
    return dict(rawSubsetSha256=expected, manifestSha256=digest_file(directory/'membership-manifest.json')[0],
                originalRowSealsVerified=True, counts=dict(counts), uniqueActualFills=len(native_fills),
                duplicateActualFillRows=counts['actual_fill']-len(native_fills),
                fullNativeChainIndependentlyReplayed=False)


def summarize(orders, fills, protocol):
    grouped = {}
    for group in sorted({o['group'] for o in orders}):
        os = [o for o in orders if o['group'] == group]; fs = [f for f in fills if f['order']['group'] == group]
        statuses = Counter(o['status'] for o in os)
        completed = sum(statuses[k] for k in ('confirmed_zero', 'confirmed_partial', 'confirmed_full'))
        positive = statuses['confirmed_partial'] + statuses['confirmed_full']
        horizons = {}
        for h in HORIZONS:
            labelled = [f for f in fs if f['posts'][h]['status'] == 'valid']
            horizons[str(h)] = dict(fillRows=len(fs), statusCounts=dict(Counter(f['posts'][h]['status'] for f in fs)),
                labelledShares=sum(f['n'] for f in labelled), totalFilledShares=sum(f['n'] for f in fs),
                displayedDepthGainBeforeUnknownExitFees=money(sum(f['posts'][h]['gain'] for f in labelled)),
                labelsAvailableBeforeNativeFillDiscovery=sum(f['posts'][h].get('before_fill_discovery', False) for f in labelled))
        grouped[group] = dict(placedOrders=len(os), closureStatusCounts=dict(statuses),
            completedPositiveFillFraction=fraction(positive, completed),
            completedQuantityWeightedFraction=fraction(sum(o['fill_total'] for o in os if o['status'].startswith('confirmed_')),
                                                       sum(o['admitted'] for o in os if o['status'].startswith('confirmed_'))),
            requestedShares=sum(o['requested'] for o in os), admittedShares=sum(o['admitted'] for o in os),
            actualFilledShares=sum(o['fill_total'] for o in os),
            originalPostFeatureStatusCounts=dict(Counter('recovered_ack_original_post_unavailable' if o['recovered'] else o['pre_status'] for o in os)),
            causalExternalFeatureStatusCounts=dict(Counter(o['features']['status'] for o in os)),
            originalPlacementCounts=dict(Counter(o['features'].get('placement','unavailable') for o in os)),
            executionTiming=dict(fillRowsBeforeOrDuringOriginalPostResponse=sum(not f['order']['recovered'] and f['executed']<=f['order']['ack_end'] for f in fs),
                sharesBeforeOrDuringOriginalPostResponse=sum(f['n'] for f in fs if not f['order']['recovered'] and f['executed']<=f['order']['ack_end']),
                fillRowsStrictlyAfterOriginalPostResponse=sum(not f['order']['recovered'] and f['executed']>f['order']['ack_end'] for f in fs),
                sharesStrictlyAfterOriginalPostResponse=sum(f['n'] for f in fs if not f['order']['recovered'] and f['executed']>f['order']['ack_end'])),
            markouts=horizons)
    completed = [o for o in orders if o['status'].startswith('confirmed_') and o['ack_start'] is not None]
    blocks = sorted({o['ack_start']//(protocol['fitConditions']['blockMs']*MS) for o in completed})
    split = len(blocks)*2//3
    reasons = []
    if len(completed) < protocol['fitConditions']['minimumCompletedOrders']: reasons.append('insufficient_completed_orders')
    if split < protocol['fitConditions']['minimumEarlierTrainingTimeBlocks'] or len(blocks)-split < protocol['fitConditions']['minimumLaterTestTimeBlocks']: reasons.append('insufficient_chronological_blocks')
    if any(f['posts'][h]['status'] != 'valid' for f in fills for h in HORIZONS): reasons.append('economic_label_coverage_not_complete')
    if any(o['status'] not in ('confirmed_zero', 'confirmed_partial', 'confirmed_full') for o in orders): reasons.append('unresolved_or_missing_closure')
    reasons.append('causal_opposite_inventory_and_own_depth_not_fully_reconstructed')
    return grouped, dict(fitted=False, reasonCodes=reasons, completedOriginalPostOrders=len(completed),
                         originalAckTimeBlocks=len(blocks), earlierBlocks=split, laterBlocks=len(blocks)-split,
                         noMissingToZero=True, noLiveQualification=True)


def run(directory, expected, protocol_path, membership_dir, membership_sha):
    directory, protocol_path = Path(directory), Path(protocol_path)
    protocol = load(protocol_path.read_bytes())
    need(tuple(protocol['markoutHorizonsMs']) == HORIZONS and protocol['executionAuthority'] is False, 'FROZEN_PROTOCOL')
    meta, report, pins = pinned_sources(directory, expected)
    orders, fills, exclusions, chain, _ = lineage(directory, meta, report)
    membership = native_membership(membership_dir, membership_sha, meta, orders, fills)
    tournaments={o['tournament'] for o in orders}
    need(len(tournaments)==1, 'MIXED_TOURNAMENT_SCOPE')
    book_stats = {}
    observations = book_observations(journal(directory/'books.jsonl', subset=True, stats=book_stats),
                                    orders, fills, clock(meta['bankAt']), dict(protocol,tournamentId=next(iter(tournaments))))
    for o in orders:
        o['features']=causal_features(o,orders,fills)
    need(book_stats['rows'] == meta['originalBookRows'] and book_stats['lastSequence'] <= meta['bankPrefixSequence'], 'BOOK_SUBSET_BOUNDARY')
    groups, fit = summarize(orders, fills, protocol)
    # Detect source changes while the bounded audit was running.
    need(digest_file(directory/'SHA256SUMS.json')[0] == expected and
         all(digest_file(directory/name)[0]==sha for name,sha in pins.items()) and
         digest_file(Path(membership_dir)/'native-membership.jsonl')[0]==membership_sha,
         'SOURCE_CHANGED_DURING_AUDIT')
    return dict(schemaVersion=1, counterfactual=False, executionAuthority=False, modelActivation=False,
        source=dict(inputManifestSha256=pins['input-manifest.json'], sourceManifestSha256=expected,
                    bankReportRawSha256=pins['report.json'], bankSequence=meta['bankPrefixSequence'], bankAt=meta['bankAt'],
                    originalFullPrefixSha256=meta['sourcePrefixSha256'], originalFullPrefixBytes=meta['sourcePrefixBytes'],
                    fullNativePrefixPresent=False, booksSubsetRawSha256=pins['books.jsonl'], bookRows=book_stats['rows'],
                    bookSubsetIndividualRowSealsVerified=True, fullNativeChainIndependentlyReplayed=False,
                    dispatchRawSha256=pins['dispatch.ndjson'], dispatchChain=chain,
                    missingOriginalReceiptFiles=len(meta['missingReceiptRefs']),
                    nativeMembership=membership,
                    typedReceiptDigestsIndependentlyRecomputed=False),
        sourceCode=dict(auditorGitLfSha256=hashlib.sha256(Path(__file__).read_bytes().replace(b'\r\n',b'\n')).hexdigest(),
                        rawSealHelperGitLfSha256=hashlib.sha256(RAW_PATH.read_bytes().replace(b'\r\n',b'\n')).hexdigest(),
                        protocolSha256=digest_file(protocol_path)[0]),
        placedOrders=len(orders), actualUniqueFillRows=len(fills), excludedDispatch=dict(exclusions),
        bookObservationCounts=observations, groups=groups, fitDecision=fit,
        orderDiagnostics=[dict(nativeQid=int(o['qid']),group=o['group'],closureStatus=o['status'],
            requestedShares=o['requested'],admittedShares=o['admitted'],actualFilledShares=o['fill_total'],
            originalNativeQuoteShares=o['original_quote_quantity'], finalNativeQuoteShares=o['q']['quantity'],
            admittedFilledFraction=fraction(o['fill_total'],o['admitted']),
            causalFeatures=o['features']) for o in orders],
        limitations=[
            'Original ACK/terminal/fill subset membership is checked against the pinned full-prefix extraction and exact row seals; the absent intervening full native chain is not independently financially replayed.',
            'Underlying original book bodies and raw sourceRef hashes are retained in normalized native events; independent archived HTTP/WS source-body authentication is not performed.',
            'Party/candidate contract semantics are not included in this packet; YES/NO complement is algebraic only, no inferred election mapping.',
            'Known own resting liquidity is subtracted using earlier original ACK/fill/complete terminal evidence; quantity conflict is unavailable. Unreserved opposite inventory is not proved, so no fully-covered hedge classification is inferred.',
            'Closure-conditioned empirical fractions are descriptive under the historical cancellation policy, not passive eventual-fill probabilities.',
            'Historical controls/releases are mixed; no per-entry current production guard or stationary execution-model qualification is inferred.',
            'Displayed-depth markouts are not hypothetical realized fills/profit; future exit fees are unknown and short fundamental marks do not value terminal holdings.'
        ])


def main():
    p = argparse.ArgumentParser()
    p.add_argument('--input', required=True, type=Path)
    p.add_argument('--source-manifest-sha256', required=True)
    p.add_argument('--native-membership', required=True, type=Path)
    p.add_argument('--native-membership-sha256', required=True)
    p.add_argument('--protocol', type=Path, default=Path(__file__).with_name('protocol.json'))
    p.add_argument('--output', required=True, type=Path)
    a = p.parse_args()
    result = run(a.input, a.source_manifest_sha256, a.protocol, a.native_membership, a.native_membership_sha256)
    encoded = (json.dumps(result, sort_keys=True, indent=2) + '\n').encode()
    with a.output.open('xb') as f:
        f.write(encoded)
    print(json.dumps(dict(outputSha256=hashlib.sha256(encoded).hexdigest(), placedOrders=result['placedOrders'],
                          actualUniqueFillRows=result['actualUniqueFillRows'], fitted=result['fitDecision']['fitted'])))


if __name__ == '__main__':
    main()
