"""Causal, bounded reporting-tape pages for offline API replay.

Default/max page sizes and half-open time filters follow the cached official
GET /exchanges/{id}/trades schema. Snapshot cursors and newest-first tie ordering
are explicit replay assumptions, not calibrated reporting-projection behavior.
Only source prints already available when a query opens enter that query.
"""
from copy import deepcopy
import hashlib
import bisect


def seed_role_history(tape, env, episode_opened_at):
    """Recorded past remains queryable, but never crosses a chronological role.

    Seeding reporting history neither fills an order nor publishes an actor
    observation. The caller must still issue and pay for each requested page.
    """
    lower, upper = getattr(tape, 'counterparty_information_bounds', tape.split.bounds(tape.role))
    if not lower <= episode_opened_at < upper:
        raise ValueError('TRADE_HISTORY_EPISODE_MUST_STAY_IN_ROLE')
    if hasattr(tape,'disk_store'):
        env.trade_history.backend=(tape.disk_store,(lower,upper))
        env.trade_history.backend_row_through=bisect.bisect_left(tape.flow_times,episode_opened_at)-1
        return 0
    a = bisect.bisect_left(tape.flow_times, lower)
    b = bisect.bisect_left(tape.flow_times, episode_opened_at)
    for row in tape.flow_rows[a:b]:
        env.trade_history.accept(row['available_at_ms'], row)
    return b - a


class TradeHistory:
    def __init__(self):
        self.rows = {}
        self.seen = {}
        self.cursors = {}
        self.sequence = 0
        self.backend = None
        self.backend_row_through = None

    def accept(self, at, row):
        if row['available_at_ms'] != at or not 0 <= row['executed_at_ms'] <= row['received_at_ms'] <= at:
            raise ValueError('TRADE_HISTORY_SOURCE_CLOCK')
        if self.backend is not None:
            table,index=row['_disk_record']
            if table!='flow':raise ValueError('DISK_TRADE_RECORD_REQUIRED')
            self.backend_row_through=index
            return  # Original immutable facts were validated by the cache builder.
        identity = (row['tournament_id'], row['exchange_id'], row['match_id'])
        facts = (row['market_id'], row['executed_at_ms'], row['quantity'], row['yes_price_micros'])
        if identity in self.seen:
            if self.seen[identity] != facts:
                raise ValueError('ORIGINAL_MATCH_FACTS_CHANGED')
            return
        self.seen[identity] = facts
        self.rows.setdefault(row['exchange_id'], []).append(deepcopy(row))

    def page(self, exchange, at, *, limit=50, cursor=None, from_ms=0, to_ms=None):
        if type(limit) is not int or not 1 <= limit <= 200:
            raise ValueError('TRADE_PAGE_LIMIT_1_TO_200')
        if cursor is not None:
            if cursor not in self.cursors:
                raise ValueError('UNKNOWN_TRADE_CURSOR')
            scoped, opened, lower, upper, rows, offset = self.cursors[cursor]
            if scoped != exchange or opened > at:
                raise ValueError('TRADE_CURSOR_SCOPE_OR_CLOCK')
        else:
            lower, upper = from_ms, at if to_ms is None else to_ms
            if type(lower) is not int or type(upper) is not int or not 0 <= lower <= upper:
                raise ValueError('TRADE_QUERY_TIME_RANGE')
            opened = at
            rows = self.backend_row_through if self.backend is not None else tuple(sorted((r for r in self.rows.get(exchange, ())
                if r['available_at_ms'] <= at and lower <= r['executed_at_ms'] < upper),
                key=lambda r: (-r['executed_at_ms'], r['match_id'])))
            offset = 0
        if rows is None or type(rows)is int:
            store,bounds=self.backend
            fetched=store.history_page(bounds,exchange,opened,lower,upper,offset,limit+1,through=rows)
            selected=fetched[:limit];more=len(fetched)>limit
        else:
            selected = rows[offset:offset + limit]
            more = offset+len(selected)<len(rows)
        next_offset = offset + len(selected)
        token = None
        if more:
            # No hidden future fact or total tape length is encoded in a token.
            self.sequence += 1
            token = hashlib.sha256(f'{exchange}:{opened}:{self.sequence}'.encode()).hexdigest()
            self.cursors[token] = (exchange, opened, lower, upper, rows, next_offset)
        return {'exchange_id': exchange, 'opened_at_ms': opened, 'from_ms': lower,
                'to_ms': upper, 'data': deepcopy(list(selected)),
                'pagination': {'limit': limit, 'hasMore': more, 'nextCursor': token},
                'coverage': {'complete': False, 'reason': 'recorded_role_only_not_full_reporting_projection'},
                'ordering': 'modeled_newest_execution_first_match_id_tie_break'}
