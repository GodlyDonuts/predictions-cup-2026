"""Polling delivery over causal recorded history, with SIG-only execution.

The exchange world keeps original event clocks. Actor caches receive snapshots
only after charged requests finish. Repeated reads never refresh source age or
replenish liquidity. External venues have no order submission interface.
"""
from collections import defaultdict, deque
from copy import deepcopy
from dataclasses import asdict
import hashlib
import json
from pathlib import Path
from moloch.learning_flow.episodes import FlowEpisode, FlowTape
from moloch.learning_v2.account import Projection
from .protocol import Delivery, Split, digest


class PollingEpisode(FlowEpisode):
    def __init__(self, *args, delivery=None, **kwargs):
        super().__init__(*args, **kwargs)
        self.delivery = delivery or Delivery()
        self.actor_books = {}
        self.latest = {}; self.keys = defaultdict(list); self.cursor = defaultdict(int)
        self.external_admissions = defaultdict(deque)
        self.external_next = defaultdict(int)
        self.feed_requests = deque(maxlen=4096); self.feed_receipts = deque(maxlen=4096)
        self.feed_request_counts = defaultdict(int); self.feed_receipt_counts = defaultdict(int)
        self.feed_sequence = 0
        self.pending_prints = defaultdict(list)
        self._started = False

    def start_delivery(self):
        if self._started: raise ValueError('DELIVERY_ALREADY_STARTED')
        self._started = True
        for venue in ('SIG', 'Kalshi', 'Polymarket'):
            self.clock.schedule(self.now, 25, 'feed_poll', venue)

    def _remember(self, venue, key, row):
        if key not in self.latest:
            self.keys[venue].append(key)
        self.latest[key] = row

    def _poll(self, at, venue):
        if at > self.end: return
        if not self.keys[venue]:
            self.clock.schedule(at+100, 25, 'feed_poll', venue); return
        if venue == 'SIG':
            if self.budget.active is not None:
                self.clock.schedule(max(at+1, self.busy_until), 25, 'feed_poll', venue); return
            ready = self.budget.ready_at(at, False)
            if ready > at:
                self.clock.schedule(ready, 25, 'feed_poll', venue); return
            latency = self.mechanics.read_latency_ms
        else:
            admissions = self.external_admissions[venue]
            while admissions and at-admissions[0] >= 1000: admissions.popleft()
            cap = self.delivery.kalshi_reads_per_second if venue == 'Kalshi' else self.delivery.polymarket_reads_per_second
            ready = max(at, self.external_next[venue], admissions[0]+1000 if len(admissions) >= cap else at)
            if ready > at:
                self.clock.schedule(ready, 25, 'feed_poll', venue); return
            admissions.append(at)
            self.external_next[venue] = at+(1000+cap-1)//cap
            latency = self.delivery.external_latency_ms
        keys = self.keys[venue]; key = keys[self.cursor[venue] % len(keys)]
        self.cursor[venue] += 1; self.feed_sequence += 1
        rid = f'feed:{venue}:{self.feed_sequence}'
        if venue == 'SIG':
            assert self.budget.admit(rid, at, False)
            self.busy_until = at+latency
        row = self.latest[key]
        if key[0] == 'trade_flow':
            payload = self.pending_prints.pop(key, [])
        elif key[0] == 'book':
            payload = deepcopy(self.books.get(row['exchange_id']))
        else:
            payload = dict(row)
        self.feed_requests.append({'venue': venue, 'at_ms': at, 'key': key, 'request_id': rid})
        self.feed_request_counts[venue] += 1
        self.clock.schedule(at+latency, 30, 'feed_receipt', (venue, rid, key, payload))
        # External requests may overlap; SIG remains serialized with execution.
        self.clock.schedule(at+1 if venue == 'SIG' else self.external_next[venue], 25, 'feed_poll', venue)

    def _event(self, at, kind, value):
        if kind == 'feed_poll':
            self._poll(at, value); return
        if kind == 'feed_receipt':
            venue, rid, key, payload = value
            if venue == 'SIG': self.budget.complete(rid, at)
            self.feed_receipts.append({'venue': venue, 'at_ms': at, 'key': key})
            self.feed_receipt_counts[venue] += 1
            if key[0] == 'book':
                if payload is not None: self.actor_books[key[1]] = payload
            elif key[0] == 'trade_flow':
                for original in payload:
                    row = dict(original)
                    row['original_available_at_ms'] = row['available_at_ms']
                    row['available_at_ms'] = at  # delivery time, never execution time
                    self.flow.accept(at, row)
            else:
                # Preserve original availability/receipt/source timestamps.
                super()._event(at, 'tape', payload)
            return
        if kind == 'trade_flow':
            key = ('trade_flow', value['exchange_id'])
            self._remember('SIG', key, value); self.pending_prints[key].append(value); return
        if kind == 'tape':
            row = value
            if row['kind'] in ('reference', 'reference_gap'):
                venue = row['venue']
                if venue not in ('Kalshi', 'Polymarket'): raise ValueError('SUPPORTED_REFERENCE_VENUE')
                self._remember(venue, ('reference', venue, row['market_id'], row.get('source', venue)), row)
                return
            super()._event(at, kind, value)  # hidden SIG exchange world
            if row['kind'] == 'book': self._remember('SIG', ('book', row['exchange_id']), row)
            return
        super()._event(at, kind, value)

    def observe(self):
        world = self.books
        try:
            self.books = self.actor_books
            out = super().observe()
        finally:
            self.books = world
        out['delivery_profile_sha256'] = digest(asdict(self.delivery))
        return out

    def act(self, action):
        # Price offsets must be derived from the actor's delivered book.
        world = self.books
        try:
            self.books = self.actor_books
            return super().act(action)
        finally:
            self.books = world

    def submit(self, request, patience_ms):
        observed = self.actor_books.get(request.exchange)
        if observed is None or not observed.visible(self.now, self.mechanics.maximum_book_age_ms):
            return Projection(0, 0, 0, ('ACTOR_BOOK_NOT_DELIVERED_OR_STALE',))
        return super().submit(request, patience_ms)


class ReplayTape(FlowTape):
    """Drop-in rollout tape. One immutable role per instance, including burn-in.

    Existing policy/rollout callers can use .episode without changing mechanics.
    Full-history selection must first pass coverage preparation; short tapes are
    permitted only for explicitly labeled engineering smoke tests.
    """
    episode_type = PollingEpisode

    def __init__(self, directory, *, split: Split, role: str, delivery=None,
                 frozen_finalist_sha256=None):
        super().__init__(directory)
        split.bounds(role)
        self.split, self.role = split, role
        self.delivery = delivery or Delivery()
        self.finalist = frozen_finalist_sha256

    def episode(self, mechanics, start_ms, end_ms, *, initial_cash_micros,
                target_wealth_micros, burnin_ms=60000, advance_to_start=True):
        self.split.require(self.role, start_ms-burnin_ms, end_ms,
                           frozen_finalist_sha256=self.finalist)
        if (mechanics.budget.reads_per_minute, mechanics.budget.read_completion_gap_ms,
            mechanics.budget.writes_per_minute, mechanics.budget.write_completion_gap_ms) != (100, 600, 24, 2500):
            raise ValueError('ORIGINAL_PRODUCTION_SIG_SHARED_BUDGET_REQUIRED')
        env = super().episode(mechanics, start_ms, end_ms,
            initial_cash_micros=initial_cash_micros, target_wealth_micros=target_wealth_micros,
            burnin_ms=burnin_ms, advance_to_start=False)
        env.delivery = self.delivery
        env.start_delivery()
        if advance_to_start: env.advance(start_ms)
        return env


def open_replay(specification, dataset, mechanics_path, role, *, frozen_finalist_sha256=None):
    """Verified entry point for training/evolution/final-test rollout callers.

    Returns (role-bound Tape, original Mechanics). It cannot silently substitute
    the old instantaneous-observation Tape or a different dataset/profile.
    """
    from moloch.learning_v2.run_experiment import load_mechanics
    spec = json.loads(Path(specification).read_text(encoding='utf-8'))
    if role=='test'and spec.get('independent_test_eligible')is not True:
        raise ValueError('INDEPENDENT_TEST_SOURCE_PROVENANCE_NOT_ESTABLISHED')
    if spec['full_history_ready'] is not True or spec['only_execution_venue'] != 'SIG':
        raise ValueError('FULL_THREE_VENUE_HISTORY_NOT_READY')
    if hashlib.sha256(Path(mechanics_path).read_bytes()).hexdigest() != spec['mechanics_sha256']:
        raise ValueError('FROZEN_MECHANICS_CHANGED')
    for name, expected in spec['source_sha256'].items():
        path = Path(__file__).parent/name
        if path.name != name or hashlib.sha256(path.read_bytes()).hexdigest() != expected:
            raise ValueError('FROZEN_REPLAY_SOURCE_CHANGED')
    split = Split(**spec['split'])
    lo, hi = split.bounds(role)
    split.require(role, lo, hi-1, frozen_finalist_sha256=frozen_finalist_sha256)
    tape = ReplayTape(dataset, split=split, role=role, delivery=Delivery(**spec['delivery']),
                      frozen_finalist_sha256=frozen_finalist_sha256)
    if tape.manifest_sha256 != spec['dataset_manifest_sha256']:
        raise ValueError('FROZEN_DATASET_CHANGED')
    if role=='test':
        from .protocol import independent_test_claim
        if not independent_test_claim(tape.manifest,split):
            raise ValueError('ACTUAL_TEST_PROVENANCE_MUST_MATCH_FROZEN_CALENDAR')
    from .prepare import coverage
    if not all(r['coverage_pass'] for r in coverage(tape, split, spec['maximum_venue_gap_ms']).values()):
        raise ValueError('ACTUAL_THREE_VENUE_COVERAGE_FAILED')
    return tape, load_mechanics(mechanics_path)
