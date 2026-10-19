"""Index observable wakeups without scanning the hidden source-event heap.

The original clock remains authoritative for event order, payloads and advance.
The extra heap contains only timestamps/sequence IDs and is never a feature.
There is no new decision throttle, coalescing, horizon or event truncation.
"""
import heapq
from moloch.learning_v2.clock import EventClock

OBSERVABLE=frozenset(('feed_receipt','api_receipt','api_account_receipt','api_expiry','api_read_receipt',
    'derived_book_receipt','derived_token_receipt','derived_token_expiry','derived_baseline_expiry',
    'derived_account_receipt'))


class WakeClock(EventClock):
    def __init__(self,start_ms):
        super().__init__(start_ms);self._observable=[]

    def schedule(self,at_ms,priority,kind,payload):
        super().schedule(at_ms,priority,kind,payload)
        if kind in OBSERVABLE:heapq.heappush(self._observable,(at_ms,self._sequence))

    def next_observable(self):
        while self._observable and self._observable[0][0]<=self.now:heapq.heappop(self._observable)
        return self._observable[0][0]if self._observable else None

    def advance(self,until_ms,consume):
        count=super().advance(until_ms,consume)
        self.next_observable()  # Bound stale index storage even without WAIT.
        return count
