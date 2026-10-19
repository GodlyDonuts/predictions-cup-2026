"""Scoped owned-order pages for a new simulated account.

Filters/page limits/48h unfilled-cancellation archival follow the saved SIG
OpenAPI. Descending creation/id keyset order and synchronous projection are
explicit replay assumptions. Cursors freeze filters, not a multi-page account
snapshot: a later page reflects later fills/cancels and cannot prove atomicity.
"""
from copy import deepcopy
from dataclasses import dataclass
import hashlib

STATUSES=('open','closed','expired','all')
ARCHIVE_MS=48*60*60*1000


@dataclass
class Record:
    order:object
    filled:int=0
    open:bool=True
    terminal_reason:str|None=None
    closed_at_ms:int|None=None


class OrderHistory:
    def __init__(self,scope):
        if type(scope)is not str or not scope:raise ValueError('EXPLICIT_PAPER_ORDER_SCOPE')
        self.scope=scope;self.records={};self.cursors={};self.sequence=0;self.last_at_ms=0;self.digest='0'*64

    def event(self,at,value):
        if type(at)is not int or at<self.last_at_ms:raise ValueError('MONOTONIC_ORDER_HISTORY_CLOCK')
        self.last_at_ms=at;self.digest=hashlib.sha256((self.digest+repr((at,value))).encode()).hexdigest()

    def placed(self,order,at):
        if order.id in self.records:raise ValueError('UNIQUE_PLACED_ORDER_REQUIRED')
        self.event(at,('placed',order));self.records[order.id]=Record(deepcopy(order))

    def filled(self,order,quantity,at):
        row=self.records[order.id]
        if not row.open or type(quantity)is not int or quantity<=0:raise ValueError('OPEN_ORDER_FILL_HISTORY_REQUIRED')
        self.event(at,('fill',order.id,quantity,order.remaining))
        row.filled+=quantity;row.order=deepcopy(order)
        if not order.remaining:self.closed(order.id,at,'Filled')

    def closed(self,order_id,at,reason=None):
        row=self.records[order_id]
        if not row.open:return  # A later scheduled expiry cannot relabel a cancellation/fill.
        self.event(at,('closed',order_id,reason));row.open=False;row.terminal_reason=reason;row.closed_at_ms=at

    def resized(self,order,before,at):
        row=self.records[order.id]
        if not row.open or row.order.remaining!=before or not 0<=order.remaining<before:
            raise ValueError('OPEN_DECREASING_ORDER_RESIZE_REQUIRED')
        self.event(at,('resize',order.id,before,order.remaining));row.order=deepcopy(order)
        if not order.remaining:self.closed(order.id,at)  # No invented engine reason code.

    def page(self,at,*,status='open',limit=50,exchange=None,market=None,mapping=None,cursor=None,revision=0):
        if type(at)is not int or at<self.last_at_ms:raise ValueError('ORDER_QUERY_CLOCK')
        if status not in STATUSES or type(limit)is not int or not 1<=limit<=200:raise ValueError('ORDER_PAGE_STATUS_OR_LIMIT')
        if (exchange is not None and(type(exchange)is not str or not exchange))or(market is not None and(type(market)is not str or not market)):
            raise ValueError('ORDER_QUERY_SCOPE')
        scope=(status,exchange,market);boundary=None;requested_cursor=cursor
        if cursor is not None:
            if cursor not in self.cursors:raise ValueError('UNKNOWN_ORDER_CURSOR')
            original_scope,opened,boundary,query,first_revision=self.cursors[cursor]
            if original_scope!=scope or opened>at:raise ValueError('ORDER_CURSOR_SCOPE_OR_CLOCK')
        else:
            opened=at;first_revision=revision;self.sequence+=1
            query=hashlib.sha256(repr((self.scope,'query',self.sequence,at,scope)).encode()).hexdigest()
        rows=[];archived=False;mapping=mapping or{}
        for q,row in self.records.items():
            o=row.order
            if o.placed_at_ms>at:continue
            if exchange is not None and o.spec.exchange!=exchange:continue
            if market is not None and mapping.get(o.spec.exchange)!=market:continue
            expired=(row.terminal_reason=='Expired'or(row.open and o.spec.expiration_ms is not None and o.spec.expiration_ms<=at))
            if status=='open'and(not row.open or expired):continue
            if status=='closed'and row.open:continue
            if status=='expired'and not expired:continue
            if (not row.open and not row.filled and row.terminal_reason!='Filled'and
                row.closed_at_ms is not None and at-row.closed_at_ms>=ARCHIVE_MS):archived=True;continue
            key=(o.placed_at_ms,q)
            if boundary is not None and key>=boundary:continue
            rows.append((key,deepcopy(row)))
        rows.sort(key=lambda pair:pair[0],reverse=True);selected=rows[:limit];has_more=len(rows)>limit;token=None
        if has_more:
            self.sequence+=1;token=hashlib.sha256(repr((self.scope,'cursor',self.sequence,query)).encode()).hexdigest()
            self.cursors[token]=(scope,opened,selected[-1][0],query,first_revision)
        self.event(at,('page',scope,limit,requested_cursor,token,query,revision))
        return {'data':[row for _,row in selected],'pagination':{'limit':limit,'hasMore':has_more,'nextCursor':token},
            'coverage':{'complete':not archived,'projectedThroughSequence':None},
            # These local replay fields are not exposed as engine sequence or
            # future order-count information to the actor.
            '_query':query,'_requested_cursor':requested_cursor,'_opened':opened,'_revision':revision,
            '_first_revision':first_revision,'_scope':scope,
            'assumption':'live_creation_id_keyset_synchronous_projection_not_atomic_snapshot'}
