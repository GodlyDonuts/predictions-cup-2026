"""Research-only FIFO lifecycle observer over pinned original native/dispatch logs.

No Session, account client, fitted model or execution authority. Restart by replay.
CLI manifest schema1: sealed:true,protocolSha256,asOfUtc,bankManifest:{path,bytes,
sha256},dispatch:{path,bytes,sha256},events:{path,bytes,sha256}; paths are relative.
The original bank manifest must be a sealed live manifest. Full original journals
start at sequence1/previous zero. No suffix is silently treated as an empty wallet.
"""
from collections import Counter, defaultdict
from decimal import Decimal
from fractions import Fraction
import argparse
import copy
import gzip
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[2]
PARSER_PATH = ROOT/'research/moloch-owned-fill-markouts-20261015/analyze.py'
spec = importlib.util.spec_from_file_location('lifecycle_raw_parser', PARSER_PATH)
RAW = importlib.util.module_from_spec(spec); sys.modules[spec.name] = RAW; spec.loader.exec_module(RAW)
PROTOCOL_SHA = '04e5dd3ccd678921e34531b8b141cf1e57b33bf340b6162ff990248e1f087235'
UNIT, NS = 1000000, 1000000
AGE, SLACK, BOOK_AGE = 60000*NS, 5000*NS, 10000*NS
MAX_BYTES, MAX_ROWS, MAX_ROW, MAX_OWN = 2*1024**3, 1000000, 16*1024**2, 100000


def need(ok, why):
    if not ok: raise ValueError('LIFECYCLE_'+why)


def h(b): return hashlib.sha256(b).hexdigest()
def ident(x): return RAW.identifier(x)
def clock(x): return RAW.instant(x)
def number(x): return RAW.integer(x, True)


def exact(x):
    x = Fraction(x)
    return {'microNumerator': x.numerator, 'microDenominator': x.denominator}


def fee(x):
    if x is None: return None
    v = RAW.units(x); need(v >= 0, 'NEGATIVE_FEE'); return Fraction(v)


def price(x):
    v = Decimal(str(x)); need(v.is_finite(), 'PRICE')
    tick = (v/Decimal('.005')).quantize(Decimal(1))*Decimal('.005')
    need(0 < tick < 1 and abs(tick-v) <= Decimal('.000000000001'), 'PRICE_TICK')
    return RAW.units(tick)


def dispatch_metadata(events):
    orders = {}
    for e in events:
        qid = ident(e['qid']); kind = e['kind']
        if kind == 'intent':
            need(qid not in orders and isinstance(e.get('quote'), dict), 'DUPLICATE_OR_MISSING_INTENT')
            q = e['quote']; need(ident(q['id']) == qid and q['side'] in ('YES', 'NO') and q['action'] in ('buy', 'sell'), 'INTENT')
            ordinary = q['action'] == 'buy' and all(e.get(k) is None for k in ('fundamental', 'package', 'packageId')) and all(q.get(k) is None for k in ('reviewedFundamentalEntry', 'reviewedInventoryExit', 'executionStyle', 'packageId'))
            orders[qid] = {'quote': copy.deepcopy(q), 'ordinary': ordinary, 'attempt': None, 'postAck': None,
                           'closed': False, 'closedProof': None, 'recovered': False, 'sourceIntent': copy.deepcopy(e)}
        elif kind == 'attempt':
            need(qid in orders and orders[qid]['attempt'] is None, 'ATTEMPT')
            orders[qid]['attempt'] = clock(e['at'])
        elif kind in ('ack', 'recovered_ack'):
            need(qid in orders and orders[qid]['attempt'] is not None and orders[qid]['postAck'] is None and not orders[qid]['recovered'], 'ACK_WITHOUT_ATTEMPT')
            if kind == 'recovered_ack': orders[qid]['recovered'] = True
            else:
                r = e['receipt']; need(r['endpoint'] == '/orders' and r['status'] == 200, 'ORIGINAL_POST_ACK')
                start, end = clock(r['requestStartedAt']), clock(r['receivedAt'])
                need(orders[qid]['attempt'] <= start <= end, 'POST_ACK_CLOCK')
                orders[qid]['postAck'] = {'actual': ident(r['data']['orderId']), 'received': end}
        elif kind == 'closed':
            need(qid in orders and not orders[qid]['closed'] and (orders[qid]['postAck'] is not None or orders[qid]['recovered']), 'CLOSED_WITHOUT_ACK')
            orders[qid]['closed'] = True
            orders[qid]['closedProof'] = copy.deepcopy(e)
        # Error/cancel/source/abort records are provenance, never zero-fill proof.
    need(len(orders) <= MAX_OWN, 'ORDER_BOUND')
    return orders


class Observer:
    def __init__(self, policy, original_dispatch):
        need(policy.get('inventoryAgeClock') == 'execution' and policy.get('maximumInventoryAgeMs') == 60000 and policy.get('bookAgeMs') == 10000 and policy.get('inventoryExitDepthBps') == 5000, 'POLICY_MISMATCH')
        self.metadata = copy.deepcopy(original_dispatch)
        self.orders, self.actual, self.fills, self.positions = {}, {}, {}, defaultdict(list)
        self.episodes, self.current, self.coverage = [], {}, {}
        self.last, self.failed, self.counts, self.adopted = None, None, Counter(), False
        self.scope = {}
        self.metadata_by_exchange, self.orders_by_exchange = defaultdict(dict), defaultdict(set)
        for qid, m in self.metadata.items():
            q = m['quote']; ex, market = ident(q['exchangeId']), ident(q['marketId'])
            need(ex not in self.scope or self.scope[ex] == market, 'ORIGINAL_SCOPE_CONFLICT'); self.scope[ex] = market
            self.metadata_by_exchange[ex][qid] = m

    def accept(self, e):
        need(self.failed is None, 'POISONED_OBSERVER')
        try: self._accept(e)
        except Exception as err:
            self.failed = type(err).__name__+':'+str(err)
            raise

    def _episode(self, ex, side, observed):
        key = ex, side
        if key not in self.current:
            p = {'id': len(self.episodes)+1, 'exchange': ex, 'side': side, 'openedObservedNs': observed,
                 'closedObservedNs': None, 'mark': None, 'markState': 'pending', 'deadlineNs': None,
                 'deadlineHistory': [], 'realizedGross': Fraction(0), 'realizedNet': Fraction(0),
                 'realizedFeesKnown': True, 'closedShares': 0, 'actualCloses': [], 'mixed': False}
            self.current[key] = p; self.episodes.append(p)
        return self.current[key]

    def _refresh(self, observed):
        for key, p in list(self.current.items()):
            lots = self.positions[key]
            if not lots:
                p['closedObservedNs'] = observed
                if p['markState'] == 'pending': p['markState'] = 'closed_before_age_mark'
                del self.current[key]; continue
            eligible = [l for l in lots if l['eligible']]
            p['mixed'] |= any(not l['eligible'] for l in lots)
            deadline = min((l['executed'] for l in eligible), default=None)
            deadline = deadline+AGE if deadline is not None else None
            if deadline != p['deadlineNs']:
                p['deadlineNs'] = deadline; p['deadlineHistory'].append({'observedNs': observed, 'deadlineNs': deadline})
            if p['markState'] == 'pending' and deadline is not None and observed > deadline+SLACK+BOOK_AGE:
                p['markState'] = 'no_timely_horizon_book'

    def _consume(self, key, n, unit_proceeds, closing_fee, e, observed):
        lots = self.positions[key]
        need(sum(l['remaining'] for l in lots) >= n, 'FIFO_UNOWNED_CLOSE')
        left = n
        while left:
            l = lots[0]; take = min(left, l['remaining']); left -= take; l['remaining'] -= take
            if l['eligible']:
                p = self.episodes[l['episode']-1]
                gross = Fraction((unit_proceeds-l['price'])*take)
                known = closing_fee is not None and l['feePerShare'] is not None
                net = gross-(closing_fee+l['feePerShare'])*take if known else None
                p['realizedGross'] += gross; p['closedShares'] += take
                if net is not None: p['realizedNet'] += net
                else: p['realizedFeesKnown'] = False
                p['actualCloses'].append({'fillId': str(e['fillId']), 'quantity': take, 'executedNs': clock(e['executedAt']),
                                         'observedNs': observed, 'grossPnl': exact(gross), 'netPnl': exact(net) if net is not None else None,
                                         'kind': 'sell' if e['action'] == 'sell' else 'opposite_buy_netting'})
            if not l['remaining']: lots.pop(0)

    def _ack(self, e):
        qid, actual = ident(e['orderId']), ident(e['actualOrderId'])
        fields = {'market': ident(e['marketId']), 'exchange': ident(e['exchangeId']), 'side': e['side'], 'action': e['action'], 'limit': price(e['price']), 'admitted': number(e['quantity'])}
        need(fields['exchange'] not in self.scope or self.scope[fields['exchange']] == fields['market'], 'ACK_MARKET_SCOPE')
        self.scope[fields['exchange']] = fields['market']
        need(fields['side'] in ('YES', 'NO') and fields['action'] in ('buy', 'sell'), 'ACK_SCOPE')
        if qid in self.orders:
            need(self.orders[qid]['actual'] == actual and all(self.orders[qid][k] == v for k, v in fields.items()), 'ACK_REBIND')
            self.counts['duplicateAcks'] += 1; return
        need(actual not in self.actual, 'ACTUAL_ORDER_REBIND')
        meta = self.metadata.get(qid)
        if meta:
            q = meta['quote']; need(ident(q['marketId']) == fields['market'] and ident(q['exchangeId']) == fields['exchange'] and q['side'] == fields['side'] and q['action'] == fields['action'] and price(q['price']) == fields['limit'] and fields['admitted'] <= number(q['quantity']), 'ACK_INTENT_MISMATCH')
            if meta['postAck'] is not None: need(meta['postAck']['actual'] == actual, 'POST_ACTUAL_BINDING')
        self.orders[qid] = {**fields, 'actual': actual, 'filled': 0, 'terminal': False, 'terminalObservedNs': None, 'closureMatched': False, 'meta': meta}
        self.orders_by_exchange[fields['exchange']].add(qid)
        self.actual[actual] = qid; need(len(self.orders) <= MAX_OWN, 'ORDER_BOUND')

    def _fill(self, e, observed):
        qid, fid, actual = ident(e['orderId']), ident(e['fillId']), ident(e['actualOrderId'])
        n, p, charge, executed = number(e['quantity']), price(e['price']), fee(e.get('fee')), clock(e['executedAt'])
        facts = (qid, actual, ident(e['marketId']), ident(e['exchangeId']), e['side'], e['action'], n, p, charge, executed)
        if fid in self.fills:
            need(self.fills[fid]['facts'] == facts, 'FILL_ID_CONFLICT')
            self.counts['duplicateFills'] += 1; return
        need(qid in self.orders, 'FILL_WITHOUT_NATIVE_ACK')
        q = self.orders[qid]
        need(not q['terminal'] and actual == q['actual'] and facts[2:6] == (q['market'], q['exchange'], q['side'], q['action']) and q['filled']+n <= q['admitted'], 'FILL_BINDING')
        need(executed <= clock(e['receivedAt']) <= observed and (p <= q['limit'] if q['action'] == 'buy' else p >= q['limit']), 'FILL_CLOCK_OR_LIMIT')
        meta = q['meta']
        if meta: need(meta['attempt'] is not None and meta['attempt'] <= executed and int(meta['quote']['createdAt'])*NS <= executed, 'FILL_BEFORE_ATTEMPT')
        eligible = bool(meta and meta['ordinary'] and meta['postAck'] and meta['postAck']['received'] < executed)
        self.fills[fid] = {'facts': facts, 'firstObservedNs': observed, 'receiptHash': e['receiptHash'], 'ordinaryEligible': eligible}
        need(len(self.fills) <= MAX_OWN, 'FILL_BOUND'); q['filled'] += n
        for ep in self.episodes:
            if ep['exchange'] == q['exchange'] and ep['mark'] is not None and executed <= ep['mark']['sourceNs']:
                ep['markState'] = 'late_fill_after_frozen_label'; ep['mark']['invalidated'] = True
        per_fee = charge/n if charge is not None else None
        key = q['exchange'], q['side']
        if q['action'] == 'sell':
            self._consume(key, n, p, per_fee, e, observed)
        else:
            opposite = q['exchange'], 'NO' if q['side'] == 'YES' else 'YES'
            net = min(n, sum(l['remaining'] for l in self.positions[opposite]))
            if net: self._consume(opposite, net, UNIT-p, per_fee, e, observed)
            if n > net:
                ep = self._episode(*key, observed)
                self.positions[key].append({'fillId': fid, 'qid': qid, 'remaining': n-net, 'price': p,
                                            'feePerShare': per_fee, 'executed': executed, 'observed': observed,
                                            'eligible': eligible, 'episode': ep['id']})
                ep['mixed'] |= not eligible
                if eligible and observed > executed+AGE and ep['markState'] == 'pending': ep['markState'] = 'fill_observed_after_age_deadline'
        self._refresh(observed)
        need(sum(len(v) for v in self.positions.values()) <= MAX_OWN, 'LOT_BOUND')

    def _mark(self, e, observed):
        b = e['book']; ex, market = ident(b['exchangeId']), ident(b['marketId'])
        need(ex not in self.scope or self.scope[ex] == market, 'BOOK_MARKET_SCOPE')
        ref = e.get('sourceRef', {})
        need(isinstance(ref.get('id'), str) and ref['id'] and type(ref.get('sequence')) is int and ref['sequence'] > 0 and re.fullmatch('[0-9a-f]{64}', ref.get('hash', '')), 'BOOK_SOURCE_REFERENCE')
        source, received = clock(b['sourceAt']), clock(b['receivedAt'])
        request = clock(b['requestStartedAt'])
        need(received == clock(e['receivedAt']), 'BOOK_RECEIPT_MISMATCH')
        pending = [(key, ep) for key, ep in self.current.items() if key[0] == ex and ep['markState'] == 'pending']
        if b.get('invalid') is True:
            self.coverage.pop(ex, None)
            for _, ep in pending: ep['markState'] = 'invalid_book_during_exposure'
            return
        # No full-history/order scan for a book that cannot supply a label.
        if not any(ep['deadlineNs'] is not None and source >= ep['deadlineNs'] and received >= ep['deadlineNs'] for _, ep in pending): return
        levels = {}
        invalid = b.get('invalid') is True
        for name, descending in (('bids', True), ('asks', False)):
            rows = b[name]; need(isinstance(rows, list) and len(rows) <= 10000, 'DEPTH_BOUND')
            parsed = []
            for l in rows:
                p, n = price(l['price']), number(l['quantity'])
                need(not parsed or (p < parsed[-1][0] if descending else p > parsed[-1][0]), 'DEPTH_ORDER')
                parsed.append([p, n])
            levels[name] = parsed
        invalid |= not levels['bids'] or not levels['asks']
        if not invalid: invalid |= levels['bids'][0][0] >= levels['asks'][0][0]
        if invalid:
            for key, ep in self.current.items():
                if key[0] == ex and ep['markState'] == 'pending': ep['markState'] = 'invalid_book_during_exposure'
        # Known own/unknown original attempts are conservative depth shadows.
        own = []
        filled_at_source_by_qid = Counter()
        for f in self.fills.values():
            if f['facts'][3] == ex and f['facts'][-1] <= source: filled_at_source_by_qid[f['facts'][0]] += f['facts'][6]
        for qid, m in self.metadata_by_exchange[ex].items():
            q = m['quote']
            if m['attempt'] is None or m['attempt'] > observed or ident(q['exchangeId']) != ex: continue
            known = self.orders.get(qid)
            if known and known['terminal'] and known['closureMatched'] and known['terminalObservedNs'] <= source: continue
            filled_at_source = filled_at_source_by_qid[qid]
            remaining = known['admitted']-filled_at_source if known else number(q['quantity'])
            own.append((q['side'], q['action'], price(q['price']), remaining))
        for qid in self.orders_by_exchange[ex]:
            q = self.orders[qid]
            if qid not in self.metadata and q['exchange'] == ex and not (q['terminal'] and q['closureMatched'] and q['terminalObservedNs'] <= source):
                filled_at_source = filled_at_source_by_qid[qid]
                own.append((q['side'], q['action'], q['limit'], q['admitted']-filled_at_source))
        for side, action, p, n in own:
            yes_bid = (action == 'buy') != (side == 'NO')
            target, pp = ('bids' if yes_bid else 'asks'), p if side == 'YES' else UNIT-p
            for level in levels[target]:
                if level[0] == pp: level[1] = max(0, level[1]-n)
        levels = {k: [l for l in v if l[1]] for k, v in levels.items()}
        for key, ep in self.current.items():
            if key[0] != ex or ep['markState'] != 'pending' or ep['deadlineNs'] is None: continue
            deadline = ep['deadlineNs']
            if source < deadline or received < deadline: continue
            lots = self.positions[key]
            oldest = min(l['executed'] for l in lots if l['eligible'])
            state = None
            if ep['mixed']: state = 'mixed_nonordinary_inventory'
            elif source > deadline+SLACK or received > deadline+SLACK: state = 'first_book_after_slack'
            elif max(source, received, request) > observed or source > received or request > received or max(observed-source, observed-received, observed-request) > BOOK_AGE: state = 'stale_or_future_horizon_book'
            elif any(f['facts'][3] == ex and f['facts'][-1] > source for f in self.fills.values()): state = 'book_predates_known_exposure_change'
            elif not (ex in self.coverage and self.coverage[ex][0] <= oldest and self.coverage[ex][1] >= source): state = 'coverage_unavailable'
            raw_levels = levels['bids'] if key[1] == 'YES' else [[UNIT-p, n] for p, n in levels['asks']]
            n = sum(l['remaining'] for l in lots); remaining, proceeds = n, 0
            for p, capacity in raw_levels:
                take = min(remaining, capacity); proceeds += p*take; remaining -= take
                if not remaining: break
            if state is None and remaining: state = 'insufficient_full_external_depth'
            basis = sum(l['price']*l['remaining'] for l in lots)
            fees_known = all(l['feePerShare'] is not None for l in lots)
            entry_fee = sum((l['feePerShare']*l['remaining'] for l in lots), Fraction(0)) if fees_known else None
            ep['markState'] = state or 'full_depth_gross_mark'
            ep['mark'] = {'sourceNs': source, 'receivedNs': received, 'observedNs': observed, 'oldestLotExecutedNs': oldest,
                          'deadlineNs': deadline, 'quantity': n, 'coveredQuantity': n-remaining, 'partialDisplayedProceedsMicros': proceeds,
                          'nativeExitSliceCapacityShares': raw_levels[0][1]*5000//10000 if raw_levels else 0,
                          'fullDisplayedProceedsMicros': proceeds if not remaining else None,
                          'remainingPrincipalMicros': basis, 'entryFees': exact(entry_fee) if entry_fee is not None else None,
                          'grossMarkLessPrincipalMicros': proceeds-basis if not remaining else None,
                          'markLessEntryFees': exact(Fraction(proceeds-basis)-entry_fee) if not remaining and entry_fee is not None else None,
                          'futureExitFeesKnown': False, 'netHypotheticalProfit': None, 'bookEventSha256': e.get('sourceRef', {}).get('hash'),
                          'invalidated': state is not None}

    def _accept(self, e):
        observed = clock(e.get('processedAt', e['receivedAt']))
        need(clock(e['receivedAt']) <= observed, 'SOURCE_RECEIPT_AFTER_PROCESSING')
        need(self.last is None or observed >= self.last, 'ACCEPTED_CLOCK_REGRESSION'); self.last = observed
        kind = e['type']
        if kind.startswith('actual_') and kind in ('actual_order_ack', 'actual_fill', 'actual_order_terminal', 'actual_position_adoption'):
            need(e['source'] == 'official_sig_api_v1' and re.fullmatch('[0-9a-f]{64}', e['receiptHash']), 'ACTUAL_SOURCE')
        if kind == 'actual_order_ack': self._ack(e)
        elif kind == 'actual_fill': self._fill(e, observed)
        elif kind == 'actual_order_terminal':
            q = self.orders.get(ident(e['orderId'])); need(q is not None and q['actual'] == ident(e['actualOrderId']), 'TERMINAL_BINDING')
            need((ident(e['marketId']), ident(e['exchangeId']), e['side'], e['action']) == (q['market'], q['exchange'], q['side'], q['action']), 'TERMINAL_SCOPE')
            need(e['fillsComplete'] is True and type(e['filledQuantity']) is int and e['filledQuantity'] == q['filled'] and e['status'] in ('filled', 'expired', 'canceled') and (e['status'] != 'filled' or q['filled'] == q['admitted']), 'INCOMPLETE_TERMINAL')
            q['terminal'] = True
            if q['terminalObservedNs'] is None: q['terminalObservedNs'] = observed
            proof = q['meta']['closedProof'] if q['meta'] else None
            if proof and proof.get('orderReceiptHash') is not None and proof.get('fillReceiptHash') is not None:
                need(proof['orderReceiptHash'] == e.get('orderReceiptHash') and proof['fillReceiptHash'] == e['receiptHash'], 'ORIGINAL_TERMINAL_RECEIPT_MISMATCH')
                q['closureMatched'] = True
        elif kind == 'actual_position_adoption':
            need(not self.adopted and e['complete'] is True, 'ADOPTION_BOUNDARY')
            self.adopted = True
            seen, lot_ids = set(), set()
            for row in e['positions']:
                ex = ident(row['exchangeId']); signed = row['quantity']; need(type(signed) is int, 'ADOPTION_SIDE')
                market = ident(row['marketId']); need(ex not in seen and (ex not in self.scope or self.scope[ex] == market), 'ADOPTION_DUPLICATE_SCOPE'); seen.add(ex); self.scope[ex] = market
                side = 'NO' if signed < 0 else 'YES'
                existing = self.positions[(ex, side)]; need(not self.positions[(ex, 'YES' if side == 'NO' else 'NO')], 'ADOPTION_OPPOSITE_POSITION')
                incoming = []
                total = 0
                for l in row['lots']:
                    n = number(l['quantity']); total += n; need(l['side'] == side, 'ADOPTION_LOT_SIDE')
                    lot_id, entry = ident(l['lotId']), RAW.units(l['entryPrice'])
                    need(lot_id not in lot_ids and 0 <= entry <= UNIT, 'ADOPTION_LOT_ID_OR_PRICE'); lot_ids.add(lot_id)
                    incoming.append({'fillId': 'inherited:'+lot_id, 'qid': None, 'remaining': n,
                        'price': entry, 'feePerShare': None, 'executed': None, 'observed': observed, 'eligible': False, 'episode': None})
                need(total == abs(signed), 'ADOPTION_QUANTITY')
                if existing:
                    need([(l['remaining'], l['price']) for l in existing] == [(l['remaining'], l['price']) for l in incoming], 'ADOPTION_EXISTING_FIFO_MISMATCH')
                else: self.positions[(ex, side)] = incoming
            need(all(ex in seen for (ex, _), lots in self.positions.items() if lots), 'ADOPTION_MISSING_OWNED_SCOPE')
            need(sum(len(v) for v in self.positions.values()) <= MAX_OWN, 'LOT_BOUND')
        elif kind == 'coverage':
            if e.get('complete') is True:
                start, end = clock(e['from']), clock(e['to']); need(start < end <= observed, 'COVERAGE_CLOCK')
                ex = ident(e['exchangeId']); self.coverage[ex] = (start, end)
        elif kind in ('gap', 'stream_reset'):
            affected = [key for key in self.current if e.get('exchangeId') is None or key[0] == str(e['exchangeId'])]
            for key in affected:
                p = self.current[key]
                if p['markState'] == 'pending': p['markState'] = 'gap_during_exposure'
            if e.get('exchangeId') is None: self.coverage.clear()
            else: self.coverage.pop(str(e['exchangeId']), None)
        elif kind == 'book': self._mark(e, observed)
        elif kind in ('actual_collateral_settlement', 'probabilistic_portfolio_reserve_v1', 'live_package_reserve'):
            raise ValueError('LIFECYCLE_UNSUPPORTED_INVENTORY_OR_PACKAGE_BOUNDARY')
        self._refresh(observed)
        self.counts['acceptedSourceEvents'] += 1

    def summary(self):
        need(self.failed is None, 'POISONED_OBSERVER')
        attempts = Counter()
        for qid, m in self.metadata.items():
            if not m['ordinary'] or m['attempt'] is None: continue
            q = self.orders.get(qid)
            if q is None: state = 'unknown_no_native_ack'
            elif q['terminal'] and q['closureMatched'] and q['filled'] == 0: state = 'verified_terminal_zero_fill'
            elif q['terminal'] and q['closureMatched']: state = 'terminal_full_fill' if q['filled'] == q['admitted'] else 'terminal_partial_fill'
            elif q['filled']: state = 'filled_with_terminal_evidence_pending'
            else: state = 'acknowledged_open_or_terminal_unproved'
            attempts[state] += 1
        episodes = []
        for p in self.episodes:
            x = {k: v for k, v in p.items() if k not in ('realizedGross', 'realizedNet')}
            x['actualRealizedGrossPnl'] = exact(p['realizedGross'])
            x['actualRealizedNetPnl'] = exact(p['realizedNet']) if p['realizedFeesKnown'] else None
            episodes.append(x)
        valid = [p for p in self.episodes if p['markState'] == 'full_depth_gross_mark']
        shares = sum(p['mark']['quantity'] for p in valid)
        gross_marks = sum(p['mark']['grossMarkLessPrincipalMicros'] for p in valid)
        gross_actual = sum((p['realizedGross'] for p in self.episodes), Fraction(0))
        net_actual = sum((p['realizedNet'] for p in self.episodes), Fraction(0)) if all(p['realizedFeesKnown'] for p in self.episodes) else None
        return {'status': 'UNQUALIFIED_RESEARCH', 'attemptOutcomes': dict(attempts), 'counts': dict(self.counts),
                'episodes': episodes, 'markAvailability': dict(Counter(p['markState'] for p in self.episodes)),
                'descriptiveTotals': {'validEpisodeMarks': len(valid), 'markedShares': shares, 'markedGrossLessPrincipal': exact(gross_marks),
                    'markedGrossPerShare': exact(Fraction(gross_marks, shares)) if shares else None, 'hypotheticalNetAfterExitFees': None,
                    'actualOrdinaryRealizedGross': exact(gross_actual), 'actualOrdinaryRealizedNet': exact(net_actual) if net_actual is not None else None,
                    'marksAddedToActualRealized': False, 'independentSampleCountEstablished': False},
                'ordinaryExecutionToObservationNs': [f['firstObservedNs']-f['facts'][-1] for f in self.fills.values() if f['ordinaryEligible']],
                'remainingPositions': {f'{ex}:{side}': [{'quantity': l['remaining'], 'fillId': l['fillId'], 'executedNs': l['executed'], 'firstObservedNs': l['observed'], 'ordinaryEligible': l['eligible']} for l in lots] for (ex, side), lots in self.positions.items() if lots},
                'fillProbabilityEstimated': False, 'modelApproval': False, 'executionAuthority': False}


def pinned(base, spec):
    p = base/spec['path']; need(not p.is_symlink(), 'SYMLINK_INPUT'); p = p.resolve()
    need(p.is_file() and p.stat().st_size == spec['bytes'] <= MAX_BYTES, 'INPUT_FILE')
    before = p.stat(); digest = hashlib.sha256()
    with p.open('rb') as f:
        while chunk := f.read(1024**2): digest.update(chunk)
    need(digest.hexdigest() == spec['sha256'] and p.stat().st_mtime_ns == before.st_mtime_ns, 'INPUT_PIN')
    return p, (before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns)


def journal(path, signature):
    previous, size, count = '0'*64, 0, 0
    with gzip.open(path, 'rb') if path.suffix == '.gz' else path.open('rb') as f:
        while raw := f.readline(MAX_ROW+1):
            size += len(raw); count += 1
            need(len(raw) <= MAX_ROW and size <= MAX_BYTES and count <= MAX_ROWS, 'JOURNAL_BOUND')
            row = RAW.native_row(raw)
            need(row['sequence'] == count and row['previous'] == previous, 'ORIGINAL_CHAIN')
            previous = row['hash']; yield row['event']
    s = path.stat(); need((s.st_dev, s.st_ino, s.st_size, s.st_mtime_ns) == signature, 'INPUT_CHANGED')


def analyze(path, expected):
    path = Path(path).resolve(); need(path.stat().st_size <= MAX_ROW, 'MANIFEST_BOUND'); raw = path.read_bytes(); need(h(raw) == expected, 'MANIFEST_PIN')
    m = json.loads(raw, object_pairs_hook=RAW.unique); need(m['schemaVersion'] == 1 and m['sealed'] is True and m['protocolSha256'] == PROTOCOL_SHA, 'SEALED_PROTOCOL_REQUIRED')
    protocol = Path(__file__).with_name('protocol.json').read_bytes(); need(h(protocol.replace(b'\r\n', b'\n')) == PROTOCOL_SHA, 'PROTOCOL_CHANGED')
    inputs = {k: pinned(path.parent, m[k]) for k in ('bankManifest', 'dispatch', 'events')}
    need(inputs['bankManifest'][0].stat().st_size <= MAX_ROW, 'BANK_MANIFEST_BOUND')
    bank = inputs['bankManifest'][0].read_bytes(); need(h(bank) == m['bankManifest']['sha256'], 'BANK_MANIFEST_CHANGED')
    bank = RAW.native_sealed_object(bank if bank.endswith(b'\n') else bank+b'\n')
    need(bank['mode'] == 'rust_live_session' and bank['seed']['kind'] == 'actual_account_allocation', 'ORIGINAL_LIVE_SEED')
    dispatch = dispatch_metadata(journal(*inputs['dispatch']))
    observer = Observer(bank['policy'], dispatch); cutoff = clock(m['asOfUtc'])
    for event in journal(*inputs['events']):
        need(clock(event.get('processedAt', event['receivedAt'])) <= cutoff, 'EVENT_AFTER_CUTOFF')
        observer.accept(event)
    result = observer.summary()
    return {**result, 'protocolSha256': PROTOCOL_SHA, 'sourceCodeSha256': h(Path(__file__).read_bytes()),
            'parserSha256': h(PARSER_PATH.read_bytes()), 'manifestSha256': expected, 'sourceManifest': m,
            'nativeFinancialReplayIndependentlyPerformed': False, 'underlyingOfficialReceiptsIndependentlyFetched': False,
            'restartMethod': 'replay_exact_original_prefixes', 'actualProfitEstablishedByThisStudy': False}


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__); p.add_argument('--manifest', required=True); p.add_argument('--manifest-sha256', required=True); p.add_argument('--output', required=True)
    args = p.parse_args(); result = analyze(args.manifest, args.manifest_sha256)
    with Path(args.output).open('x', encoding='utf-8') as f: json.dump(result, f, sort_keys=True, separators=(',', ':'), allow_nan=False); f.write('\n')
