"""Index observable wakeups without scanning the hidden source-event heap.

The original clock remains authoritative for event order, payloads and advance.
The extra heap contains only timestamps/sequence IDs and is never a feature.
There is no new decision throttle, coalescing, horizon or event truncation.
"""
import heapq
from moloch.learning_v2.clock import EventClock

OBSERVABLE=frozenset(('feed_receipt','api_receipt','api_account_receipt','api_expiry','api_read_receipt','api_trade_receipt','api_quote_receipt','api_order_page_receipt',
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
        from moloch.learning_v2.account import natural
        natural(until_ms,'ADVANCE_CLOCK')
        if until_ms<self.now:raise ValueError('CLOCK_REGRESSION')
        count=0
        # Multi-day legitimate source streams can exceed a million records in
        # one policy-selected WAIT. This is not a decision or event-count cap.
        while self._events and self._events[0][0]<=until_ms:
            at,priority,sequence,kind,payload=heapq.heappop(self._events)
            self.now=at;consume(at,kind,payload);count+=1
        self.now=until_ms
        self.next_observable()  # Bound stale index storage even without WAIT.
        return count
