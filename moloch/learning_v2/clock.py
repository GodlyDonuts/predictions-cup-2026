"""Virtual time and one shared completion-paced request budget.

Limits are required inputs, never chosen by the learner. The inclusive rolling
window and completion pacing mirror the existing Rust queue. No sleep or HTTP.
"""
from collections import deque
from dataclasses import dataclass
import heapq
from .account import natural


@dataclass(frozen=True)
class BudgetConfig:
    reads_per_minute: int
    read_completion_gap_ms: int
    writes_per_minute: int
    write_completion_gap_ms: int
    source_sha256: str

    def __post_init__(self):
        for name in ['reads_per_minute', 'read_completion_gap_ms', 'writes_per_minute', 'write_completion_gap_ms']:
            if type(getattr(self, name)) is not int or getattr(self, name) <= 0:
                raise ValueError('BUDGET_CONFIG')
        if len(self.source_sha256) != 64 or any(c not in '0123456789abcdef' for c in self.source_sha256):
            raise ValueError('BUDGET_SOURCE_REQUIRED')


class RateBudget:
    def __init__(self, config: BudgetConfig):
        self.config = config
        self.last_clock = 0
        self.next_at = 0
        self.cooldown = 0
        self.reads = deque()
        self.writes = deque()
        self.active: tuple[str, bool] | None = None
        self.request_ids: set[str] = set()
        self.unknown: set[str] = set()
        self.poisoned = False

    def _clock(self, now: int):
        natural(now, 'CLOCK')
        if now < self.last_clock:
            self.poisoned = True
            raise ValueError('CLOCK_REGRESSION')
        self.last_clock = now
        for queue in [self.reads, self.writes]:
            while queue and now - queue[0] > 60_000:
                queue.popleft()

    def ready_at(self, now: int, write: bool) -> int:
        self._clock(now)
        if self.poisoned:
            raise ValueError('MANUAL_REVIEW_REQUIRED')
        if self.active is not None:
            raise ValueError('SERIAL_QUEUE_BUSY')
        if write and self.unknown:
            raise ValueError('UNKNOWN_ACCEPTANCE_RECONCILE_FIRST')
        queue = self.writes if write else self.reads
        cap = self.config.writes_per_minute if write else self.config.reads_per_minute
        rolling = queue[0] + 60_001 if len(queue) >= cap else 0
        return max(now, self.next_at, self.cooldown, rolling)

    def admit(self, request_id: str, now: int, write: bool) -> bool:
        if not request_id:
            raise ValueError('REQUEST_ID_REQUIRED')
        at = self.ready_at(now, write)
        if now < at:
            return False
        if request_id in self.request_ids:
            raise ValueError('KNOWN_REQUEST_CANNOT_BE_REPLACED')
        self.request_ids.add(request_id)
        (self.writes if write else self.reads).append(now)
        self.next_at = now + (self.config.write_completion_gap_ms if write else self.config.read_completion_gap_ms)
        self.active = (request_id, write)
        return True

    def complete(self, request_id: str, now: int, *, uncertain=False, status=200,
                 retry_after_ms=0, request_in_flight=False):
        self._clock(now)
        natural(retry_after_ms, 'RETRY_AFTER')
        if self.active is None or self.active[0] != request_id:
            raise ValueError('REQUEST_MISMATCH')
        write = self.active[1]
        if uncertain and write:
            self.unknown.add(request_id)
        if status in [429, 503]:
            self.cooldown = max(self.cooldown, now + max(retry_after_ms, 60_000 if status == 429 else 10_000))
        if request_in_flight:
            self.cooldown = max(self.cooldown, now + 90_000)
        self.next_at = max(self.next_at, self.cooldown,
                           now + (self.config.write_completion_gap_ms if write else self.config.read_completion_gap_ms))
        self.active = None


class EventClock:
    """Immutable exogenous events and modeled arrivals use explicit tie priority.

    A policy sees only events already popped by advance(). Future event storage
    belongs to the environment and must never be an observation feature.
    """
    def __init__(self, start_ms: int):
        self.now = natural(start_ms, 'START_CLOCK')
        self._events = []
        self._sequence = 0

    def schedule(self, at_ms: int, priority: int, kind: str, payload):
        natural(at_ms, 'EVENT_CLOCK')
        if at_ms < self.now or type(priority) is not int:
            raise ValueError('PAST_EVENT_OR_PRIORITY')
        self._sequence += 1
        heapq.heappush(self._events, (at_ms, priority, self._sequence, kind, payload))

    def advance(self, until_ms: int, consume):
        natural(until_ms, 'ADVANCE_CLOCK')
        if until_ms < self.now:
            raise ValueError('CLOCK_REGRESSION')
        count = 0
        while self._events and self._events[0][0] <= until_ms:
            at, priority, sequence, kind, payload = heapq.heappop(self._events)
            self.now = at
            consume(at, kind, payload)
            count += 1
            if count > 1_000_000:
                raise ValueError('ZERO_TIME_OR_OVERSIZED_EVENT_LOOP')
        self.now = until_ms
        return count
