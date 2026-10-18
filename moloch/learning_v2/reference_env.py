"""Event-time research simulator; never an account client.

Displayed crossing fills use a conservative counterfactual, not calibrated
passive queue fills. All latency, expiry, source-age and budget settings are
external experiment inputs. Money becomes observable/spendable only when its
modeled fill receipt arrives. Source events precede arrivals at equal clocks.
"""
from dataclasses import dataclass, field
from collections import defaultdict
from .account import Account, Limits, Target, Projection, project, wealth, UNIT, natural
from .clock import BudgetConfig, RateBudget, EventClock
from .liquidity import Book, SharedLiquidity


@dataclass(frozen=True)
class Mechanics:
    budget: BudgetConfig
    limits: Limits
    read_latency_ms: int
    arrival_latency_ms: int
    receipt_latency_ms: int
    maximum_book_age_ms: int
    maximum_reference_age_ms: int
    preflight_reads: int
    valuation_source: str = 'independent_reference'

    def __post_init__(self):
        for name in ['read_latency_ms', 'arrival_latency_ms', 'receipt_latency_ms',
                     'maximum_book_age_ms', 'maximum_reference_age_ms', 'preflight_reads']:
            natural(getattr(self, name), name)
        if min(self.read_latency_ms, self.arrival_latency_ms, self.receipt_latency_ms) <= 0:
            raise ValueError('NONZERO_MODELED_LATENCY_REQUIRED')


@dataclass
class Order:
    id: int
    request: Target
    remaining: int
    reserved_quantity: int
    expires_at: int
    phase: str = 'preflight'
    receipts: list = field(default_factory=list)
    cancel_requested: bool = False
    canceled_quantity: int = 0


@dataclass(frozen=True)
class Reference:
    price_yes: int
    available_at_ms: int
    received_at_ms: int
    source_at_ms: int | None
    original_sha256: str


class ReferenceEnv:
    def __init__(self, mechanics: Mechanics, start_ms: int, cash_micros: int,
                 lots: dict | None = None):
        self.mechanics = mechanics
        self.clock = EventClock(start_ms)
        self.budget = RateBudget(mechanics.budget)
        self.cash = natural(cash_micros, 'INITIAL_CASH')
        # Each retained lot is [integer quantity, original price in millionths].
        self.lots = {}
        for key, rows in (lots or {}).items():
            self.lots[key] = []
            for n, p in rows:
                if type(n) is not int or n <= 0 or type(p) is not int or not 0 <= p <= UNIT:
                    raise ValueError('ORIGINAL_LOT')
                self.lots[key].append([n, p])
        self.books: dict[str, SharedLiquidity] = {}
        self.references: dict[tuple[str, str], Reference] = {}
        self.orders: dict[int, Order] = {}
        self.next_order = 0
        self.realized = 0
        self.fees = 0
        self.fill_events = []
        self.decisions = []
        self.execution_events = []
        self.busy_until = start_ms

    @property
    def now(self):
        return self.clock.now

    def add_book(self, book: Book):
        self.clock.schedule(book.available_at_ms, 10, 'book', book)

    def add_reference(self, exchange: str, price_yes: int, available_at_ms: int, original_sha256: str,
                      *, received_at_ms: int | None = None, source_at_ms: int | None = None,
                      source_kind: str = 'independent_reference'):
        if type(price_yes) is not int or not 0 < price_yes < UNIT or len(original_sha256) != 64:
            raise ValueError('REFERENCE_PRICE_OR_PIN')
        received_at_ms = available_at_ms if received_at_ms is None else received_at_ms
        if not 0 <= received_at_ms <= available_at_ms or (source_at_ms is not None and not 0 <= source_at_ms <= available_at_ms):
            raise ValueError('REFERENCE_INFORMATION_CLOCK')
        ref = Reference(price_yes, available_at_ms, received_at_ms, source_at_ms, original_sha256)
        self.clock.schedule(available_at_ms, 10, 'reference', (exchange, source_kind, ref))

    def add_reference_gap(self, exchange: str, available_at_ms: int, source_kind: str):
        self.clock.schedule(available_at_ms, 10, 'reference_gap', (exchange, source_kind))

    def account(self) -> Account:
        quantities = {key:sum(n for n, _ in rows) for key, rows in self.lots.items() if rows}
        market_cost = defaultdict(int)
        for (ex, side), rows in self.lots.items():
            market_cost[ex] += sum(n*p for n, p in rows)
        pending_buys = defaultdict(int); pending_sells = defaultdict(int); pending_market = defaultdict(int)
        reserved = 0
        for o in self.orders.values():
            key = (o.request.exchange, o.request.side)
            if o.request.action == 'BUY':
                pending_buys[key] += o.reserved_quantity
                cost = o.reserved_quantity * (o.request.limit_price + self.mechanics.limits.fee_per_share)
                reserved += cost; pending_market[key[0]] += cost
            else:
                pending_sells[key] += o.reserved_quantity
        return Account(self.cash, reserved, 0, 0, sum(market_cost.values()), reserved,
                       quantities, dict(market_cost), dict(pending_buys), dict(pending_sells), dict(pending_market))

    def marks(self):
        result = {}
        for (ex, source_kind), ref in self.references.items():
            if source_kind != self.mechanics.valuation_source: continue
            if 0 <= self.now-ref.received_at_ms <= self.mechanics.maximum_reference_age_ms and (ref.source_at_ms is None or 0 <= self.now-ref.source_at_ms <= self.mechanics.maximum_reference_age_ms):
                result[(ex, 'YES')] = ref.price_yes
                result[(ex, 'NO')] = UNIT-ref.price_yes
        return result

    def marked_wealth(self):
        return wealth(self.account(), self.marks())

    def advance(self, at_ms: int):
        return self.clock.advance(at_ms, self._event)

    def submit(self, request: Target, patience_ms: int) -> Projection:
        natural(patience_ms, 'PATIENCE')
        if self.budget.active is not None or any(o.phase == 'preflight' for o in self.orders.values()):
            return Projection(0, 0, 0, ('SERIAL_EXECUTION_BUSY',))
        if self.budget.unknown or self.budget.poisoned:
            return Projection(0, 0, 0, ('UNRESOLVED_EXECUTION',))
        book = self.books.get(request.exchange)
        if book is None or not book.visible(self.now, self.mechanics.maximum_book_age_ms):
            return Projection(0, 0, 0, ('BOOK_UNAVAILABLE',))
        projection = project(self.account(), self.marks(), request, self.mechanics.limits)
        self.decisions.append((self.now, request, projection))
        if projection.admitted_quantity:
            self.next_order += 1
            order = Order(self.next_order, request, projection.admitted_quantity,
                          projection.admitted_quantity, self.now+patience_ms)
            self.orders[order.id] = order
            self.clock.schedule(self.now, 20, 'preflight', (order.id, self.mechanics.preflight_reads))
        return projection

    def cancel(self, qid: int) -> bool:
        order = self.orders.get(qid)
        if order is None or order.phase != 'resting' or order.cancel_requested or self.budget.active is not None:
            return False
        order.cancel_requested = True
        self.clock.schedule(self.now, 20, 'cancel_admit', qid)
        return True

    def _read(self, qid: int, left: int):
        if self._busy('preflight', (qid, left)): return
        at = self.budget.ready_at(self.now, False)
        if at > self.now:
            self.clock.schedule(at, 20, 'preflight', (qid, left)); return
        rid = f'{qid}:read:{left}'
        assert self.budget.admit(rid, self.now, False)
        self.busy_until = self.now+self.mechanics.read_latency_ms
        self.clock.schedule(self.now+self.mechanics.read_latency_ms, 30, 'read_receipt', (qid, left, rid))

    def _busy(self, kind, value):
        if self.budget.active is None: return False
        self.clock.schedule(max(self.now+1, self.busy_until), 40, kind, value)
        return True

    def _match(self, order: Order):
        if not order.remaining or order.phase not in ['resting', 'cancel_pending']:
            return
        q = self.books.get(order.request.exchange)
        if q is None:
            return
        fills = q.cross(order.request.side, order.request.action, order.request.limit_price,
                        order.remaining, self.now, self.mechanics.maximum_book_age_ms)
        n = sum(n for _, n in fills)
        if not n:
            return
        order.remaining -= n
        # Do not reveal the filled position or sale money to the actor yet.
        self.clock.schedule(self.now+self.mechanics.receipt_latency_ms, 30, 'fill_receipt', (order.id, self.now, fills))

    def _fill_receipt(self, qid: int, filled_at: int, fills):
        o = self.orders[qid]; r = o.request; key = (r.exchange, r.side)
        for price, n in fills:
            fee = n*self.mechanics.limits.fee_per_share
            if r.action == 'BUY':
                self.cash -= n*price+fee
                self.realized -= fee
                self.lots.setdefault(key, []).append([n, price])
                basis = n*price
                opposite = (r.exchange,'NO' if r.side=='YES' else 'YES')
                pairs = min(sum(q for q,p in self.lots.get(key,[])),sum(q for q,p in self.lots.get(opposite,[])))
                if pairs:
                    paired_basis = self._take(key,pairs)+self._take(opposite,pairs)
                    self.cash += pairs*UNIT
                    self.realized += pairs*UNIT-paired_basis
            else:
                basis = self._take(key,n)
                self.cash += n*price-fee
                self.realized += n*price-basis-fee
            self.fees += fee; o.reserved_quantity -= n
            self.fill_events.append({'order':qid, 'filled_at_ms':filled_at, 'known_at_ms':self.now,
                                     'side':r.side, 'action':r.action, 'quantity':n, 'price':price,
                                     'original_fifo_basis':basis, 'fee':fee})
        if not o.reserved_quantity:
            self.orders.pop(qid)
        self.account()  # exact funding/encumbrance invariants after every receipt

    def _take(self, key, quantity):
        left = quantity; cost = 0; rows = self.lots[key]
        while left:
            take = min(left,rows[0][0]); cost += take*rows[0][1]
            rows[0][0] -= take; left -= take
            if not rows[0][0]: rows.pop(0)
        return cost

    def _event(self, at, kind, value):
        if kind == 'book':
            if value.exchange in self.books: self.books[value.exchange].update(value)
            else: self.books[value.exchange] = SharedLiquidity(value)
            for order in list(self.orders.values()):
                if order.request.exchange == value.exchange: self._match(order)
        elif kind == 'reference':
            ex, source_kind, ref = value; self.references[(ex, source_kind)] = ref
        elif kind == 'reference_gap':
            self.references.pop(value, None)
        elif kind == 'preflight':
            qid, left = value
            if qid not in self.orders: return
            if at>=self.orders[qid].expires_at:
                self.orders.pop(qid)
                self.execution_events.append({'order':qid,'at':at,'kind':'not_sent','reason':'PATIENCE_EXPIRED_BEFORE_SEND'})
                return
            if left: self._read(qid, left)
            else: self.clock.schedule(at, 20, 'write_admit', qid)
        elif kind == 'read_receipt':
            qid, left, rid = value; self.budget.complete(rid, at)
            self.clock.schedule(at, 20, 'preflight', (qid, left-1))
        elif kind == 'write_admit':
            if value not in self.orders: return
            if at>=self.orders[value].expires_at:
                self.orders.pop(value)
                self.execution_events.append({'order':value,'at':at,'kind':'not_sent','reason':'PATIENCE_EXPIRED_BEFORE_SEND'})
                return
            if self._busy(kind, value): return
            o = self.orders[value]; ready = self.budget.ready_at(at, True)
            if ready > at:
                self.clock.schedule(ready, 20, kind, value); return
            book = self.books.get(o.request.exchange)
            if book is None or not book.visible(at, self.mechanics.maximum_book_age_ms):
                self.orders.pop(value)
                self.execution_events.append({'order':value, 'at':at, 'kind':'not_sent', 'reason':'BOOK_UNAVAILABLE'})
                return
            rid = f'{value}:submit'; assert self.budget.admit(rid, at, True)
            o.phase = 'submitting'
            self.busy_until = at+self.mechanics.arrival_latency_ms+self.mechanics.receipt_latency_ms
            self.clock.schedule(at+self.mechanics.arrival_latency_ms, 20, 'arrival', value)
            self.clock.schedule(at+self.mechanics.arrival_latency_ms+self.mechanics.receipt_latency_ms, 30, 'write_receipt', rid)
        elif kind == 'arrival':
            o = self.orders[value]; o.phase = 'resting'; self._match(o)
            # Patience is a local cancel deadline. Already-admitted requests
            # can still arrive/fill before their charged cancellation succeeds.
            self.clock.schedule(max(at, o.expires_at), 40, 'expiry', value)
        elif kind == 'write_receipt':
            self.budget.complete(value, at)
        elif kind == 'fill_receipt':
            self._fill_receipt(*value)
        elif kind == 'expiry':
            # Patience creates a real charged cancel. It is not a free exchange
            # expiry or a local release of still-live order obligations.
            o = self.orders.get(value)
            if o and not o.cancel_requested:
                o.cancel_requested = True
                self.clock.schedule(at, 20, 'cancel_admit', value)
        elif kind == 'cancel_admit':
            if value not in self.orders: return
            if self._busy(kind, value): return
            ready = self.budget.ready_at(at, True)
            if ready > at:
                self.clock.schedule(ready, 20, kind, value); return
            rid = f'{value}:cancel'; assert self.budget.admit(rid, at, True)
            self.busy_until = at+self.mechanics.arrival_latency_ms+self.mechanics.receipt_latency_ms
            self.clock.schedule(at+self.mechanics.arrival_latency_ms, 40, 'cancel_effective', value)
            self.clock.schedule(at+self.mechanics.arrival_latency_ms+self.mechanics.receipt_latency_ms, 30, 'write_receipt', rid)
            self.clock.schedule(at+self.mechanics.arrival_latency_ms+self.mechanics.receipt_latency_ms, 30, 'cancel_receipt', value)
        elif kind == 'cancel_effective':
            o = self.orders.get(value)
            if o:
                o.canceled_quantity = o.remaining; o.remaining = 0; o.phase = 'canceled_pending_receipt'
        elif kind == 'cancel_receipt':
            o = self.orders.get(value)
            if o:
                o.reserved_quantity -= o.canceled_quantity; o.canceled_quantity = 0
                if not o.reserved_quantity: self.orders.pop(value)
        else:
            raise ValueError('UNKNOWN_EVENT_KIND')
