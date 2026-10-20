"""Raw order execution adapter, with explicit uncalibrated replay assumptions.

Uses original world liquidity, FIFO, delayed receipts and one shared SIG budget.
There are no target-fraction, per-market exposure, fresh-market or portfolio caps.
Automatic ALL buying power requires a caller-supplied verified collateral oracle;
no statistical forecast or externally marked portfolio can stand in for one.
"""
from copy import deepcopy
from dataclasses import dataclass,replace
from collections import deque
import math
import hashlib
import numpy as np
from moloch.learning_realism.fast_replay import PreparedEpisode,PreparedTape
from moloch.learning_v2.clock import RateBudget
from .contract import ApiBudget,OrderSpec,UNIT,TICK


@dataclass
class Resting:
    id:int
    spec:OrderSpec
    remaining:int
    placed_at_ms:int=0


class ApiEpisode(PreparedEpisode):
    from .observation_protocol import CURRENT as observation_protocol
    def __init__(self,*args,**kwargs):
        super().__init__(*args,**kwargs)
        from .wake_clock import WakeClock
        if self.clock._events:raise ValueError('RAW_API_INITIAL_CLOCK_MUST_BE_EMPTY')
        self.clock=WakeClock(self.clock.now)
        self.budget=RateBudget(ApiBudget())
        self.api_orders={};self.api_id=0;self.api_fills=[];self.responses=[]
        self.known_orders={};self.known_cash=self.cash;self.known_lots={}
        self.known_advance=0;self.observation_revision=0;self.api_sequence=0
        self.idempotency={};self.builder=[];self.collateral_oracle=None
        self.order_history=None
        self.constraint_validator=None;self.ledger_basis_known=True
        self.relationships={}
        self.realtime=None
        self.passive=None
        self.private_fill_push=True  # Legacy profile; new stream profile opts out.
        self.world_book_lifetime_ms=None;self.world_confidence_expiries=0
        self.global_state_encoding='raw'
        self.scheduled_reads=set();self.pending_reads=deque()
        self.expiry_scheduled=set()
        self.external_ready={'Kalshi':0,'Polymarket':0}
        self.finance_revision=0;self.known_finance_revision=-1
        from .account_reads import FIELDS
        self.known_resource_revisions={k:-1 for k in FIELDS}
        self.finance_observation_count=0
        self.known_order_revisions={};self.known_acknowledgments=[];self.known_private_fills=[]
        from .trade_history import TradeHistory
        self.trade_history=TradeHistory();self.known_trade_pages={}
        for key in (('account',),('collateral',),('owned_orders',),('positions',)):
            self.keys['SIG'].append(key)
        # Endpoint identity is public metadata, not a free signal that a hidden
        # print just occurred. Empty and active exchanges have the same targets.
        self.keys['SIG'].extend(('trade_flow',ex)for ex in self.exchanges[1:])

    def start_delivery(self):
        # Fully policy-controlled reads, no compulsory round-robin collector.
        if self._started:raise ValueError('DELIVERY_ALREADY_STARTED')
        self._started=True

    def advance_capacity(self):
        if self.collateral_oracle is None:return 0
        value=self.collateral_oracle(self.lots)
        possible=sum(n for rows in self.lots.values()for n,p in rows)*UNIT
        if type(value)is not int or not 0<=value<=possible:raise ValueError('VERIFIED_GUARANTEED_FLOOR_REQUIRED')
        return value

    def buying_power(self):return max(0,self.cash+self.advance_capacity())

    def held(self,ex,side):return sum(n for n,p in self.lots.get((ex,side),[]))

    def _funding_changed(self):
        if getattr(self,'resting_buy_funding',None)is not None:
            from .resting_power import reconcile
            reconcile(self)

    def _buy_commitment(self,exchange):
        if getattr(self,'resting_buy_funding',None)is not None:
            from .resting_power import commitment
            return commitment(self,exchange)
        return sum(o.remaining*(o.spec.price or UNIT)for o in self.api_orders.values()
            if o.spec.exchange==exchange and o.spec.action=='BUY')

    def _prepare_resting(self,order):
        if getattr(self,'resting_buy_funding',None)is not None:
            from .resting_power import prepare
            prepare(self,order)

    def _canonical(self,spec):
        if spec.action=='SELL' and self.held(spec.exchange,spec.side)<spec.quantity:
            return replace(spec,side='NO' if spec.side=='YES' else 'YES',action='BUY',
                           price=None if spec.market else UNIT-spec.price)
        return spec

    def _apply_fill(self,order,price,quantity,*,origin='crossing_book'):
        cash_before=self.cash;realized_before=self.realized
        s=order.spec;key=(s.exchange,s.side);fee=quantity*self.mechanics.limits.fee_per_share
        if s.action=='BUY':
            self.cash-=quantity*price+fee;self.realized-=fee
            self.lots.setdefault(key,[]).append([quantity,price])
            other=(s.exchange,'NO'if s.side=='YES'else'YES')
            paired=min(self.held(*key),self.held(*other))
            if paired:
                basis=self._take(key,paired)+self._take(other,paired)
                self.cash+=paired*UNIT;self.realized+=paired*UNIT-basis
        else:
            basis=self._take(key,quantity);self.cash+=quantity*price-fee
            self.realized+=quantity*price-basis-fee
        order.remaining-=quantity;self.fees+=fee
        if self.order_history is not None:self.order_history.filled(order,quantity,self.now)
        self.finance_revision+=1
        self.api_fills.append({'order':order.id,'exchange':s.exchange,'side':s.side,'action':s.action,
                               'quantity':quantity,'price':price,'filled_at_ms':self.now,'fee':fee,
                               'cash_delta_micros':self.cash-cash_before,
                               'realized_delta_micros':self.realized-realized_before})
        if self.passive is not None:
            self.passive.on_fill(s.exchange,quantity,origin)
            self.api_fills[-1]['execution_model']=origin
        # A component-funded fill finalizes cash/debt in its overriding method.
        if getattr(self,'component_ledger',None)is None:self._funding_changed()

    def _match_api(self,order):
        s=order.spec;world=self.books.get(s.exchange)
        if world is None or world.invalidated:return
        quantity=order.remaining
        if s.action=='BUY':
            # Real cash/verified floor only; other exchanges' quotes do not escrow it.
            bound=(s.price if not s.market else
                   (world.book.asks[0][0]if s.side=='YES'and world.book.asks else
                    UNIT-world.book.bids[0][0]if s.side=='NO'and world.book.bids else UNIT))
            quantity=min(quantity,self.buying_power()//max(1,bound+self.mechanics.limits.fee_per_share))
        else:quantity=min(quantity,self.held(s.exchange,s.side))
        limit=(UNIT-1 if s.action=='BUY'else 1)if s.market else s.price
        # Historical gaps remain invalid. A recorded book is not renewed by a poll.
        age=max(self.now-world.book.available_at_ms,self.now-(world.book.source_at_ms or world.book.available_at_ms))
        preview=deepcopy(world)
        fills=preview.cross(s.side,s.action,limit,quantity,self.now,max(0,age)+1,
            depth_fraction_ppm=self.mechanics.depth_fraction_ppm,
            adverse_execution_micros=self.mechanics.adverse_execution_micros)
        resource='asks'if(s.side,s.action)in(('YES','BUY'),('NO','SELL'))else'bids'
        for price,n in fills:
            base=price-(self.mechanics.adverse_execution_micros if s.action=='BUY'else-self.mechanics.adverse_execution_micros)
            yes_price=base if s.side=='YES'else UNIT-base
            remaining=n
            while remaining:
                affordable=self.buying_power()//max(1,price+self.mechanics.limits.fee_per_share)if s.action=='BUY'else self.held(s.exchange,s.side)
                # A fill can trigger a funding-driven shrink of this same
                # resting order before the next previewed price level.
                take=min(remaining,affordable,order.remaining)
                if not take:return
                world.depleted[(resource,yes_price)]=world.depleted.get((resource,yes_price),0)+take
                self._apply_fill(order,price,take);remaining-=take

    def _place(self,spec):
        spec.validate(self.now)
        if spec.exchange not in self.exchanges:raise ValueError('INELIGIBLE_EXCHANGE')
        spec=self._canonical(spec);remaining=spec.quantity
        if spec.action=='BUY':
            if self.buying_power()<=0:raise ValueError('INSUFFICIENT_BUYING_POWER')
            cost=spec.price if not spec.market else UNIT
            committed=self._buy_commitment(spec.exchange)
            if committed>=self.buying_power():return None
            # Initial partial-size admission follows actual buying power, without
            # the old max-order/market/portfolio/target-fraction projection.
            if not spec.market:
                remaining=min(remaining,(self.buying_power()-committed)//max(1,cost+self.mechanics.limits.fee_per_share))
                if not remaining:raise ValueError('INSUFFICIENT_BUYING_POWER')
        self.api_id+=1;order=Resting(self.api_id,spec,remaining,self.now)
        if self.order_history is not None:self.order_history.placed(order,self.now)
        self._match_api(order)
        if order.remaining and not spec.market:
            # Never execute against our own injected resting quotes. Other
            # counterparties were matched first, then STP cancels crossing remainder.
            yes_price=spec.price if spec.side=='YES'else UNIT-spec.price
            direction=(spec.action=='BUY')==(spec.side=='YES')
            crossed=any(o.spec.exchange==spec.exchange and
                (((o.spec.action=='BUY')==(o.spec.side=='YES'))!=direction)and
                ((yes_price>=(o.spec.price if o.spec.side=='YES'else UNIT-o.spec.price))if direction else
                 (yes_price<=(o.spec.price if o.spec.side=='YES'else UNIT-o.spec.price)))for o in self.api_orders.values())
            if not crossed:
                self._prepare_resting(order)
                if order.remaining:
                    self.api_orders[order.id]=order
                    if self.passive is not None:self.passive.on_rest(order)
            elif self.order_history is not None:self.order_history.closed(order.id,self.now,'SelfTradePrevented')
        if self.order_history is not None and order.id not in self.api_orders:
            self.order_history.closed(order.id,self.now)
        return order

    def _snapshot_finance(self):
        debt=None if self.passive is None else (self.passive.crossing_debt,getattr(self.passive,'queues',None))
        return deepcopy((self.cash,self.lots,self.realized,self.fees,self.api_orders,self.api_id,self.api_fills,self.books,self.finance_revision,debt,self.order_history))

    def _restore_finance(self,state):
        self.cash,self.lots,self.realized,self.fees,self.api_orders,self.api_id,self.api_fills,self.books,self.finance_revision,debt,self.order_history=state
        if self.passive is not None:
            self.passive.crossing_debt,queues=debt
            if queues is not None:self.passive.queues=queues

    def _execute(self,operation,payload,relationship_constraint=None):
        if operation in ('ORDER','BATCH','MULTI','ALL_OR_NONE'):
            specs=list(payload)
            maximum=1 if operation=='ORDER'else 50 if operation=='BATCH'else 10
            if not 1<=len(specs)<=maximum:raise ValueError('API_LEG_COUNT')
            for spec in specs:spec.validate(self.now)
            if relationship_constraint is not None:
                if operation not in ('MULTI','ALL_OR_NONE')or len({s.exchange for s in specs})!=len(specs):
                    raise ValueError('RELATIONSHIP_CONSTRAINT_LEGS')
                if relationship_constraint not in self.relationships or self.constraint_validator is None:
                    raise ValueError('RELATIONSHIP_DATA_UNAVAILABLE')
                if self.constraint_validator(relationship_constraint,specs,self.books)is not True:
                    raise ValueError('RELATIONSHIP_VIOLATION')
            if operation=='BATCH':
                results=[]
                for spec in specs:
                    try:results.append(self._place(spec))
                    except ValueError as exc:results.append(str(exc))
                return results
            before=self._snapshot_finance()
            try:
                results=[self._place(spec)for spec in specs]
                if operation=='ALL_OR_NONE'and any(o is None or o.remaining or
                    sum(f['quantity']for f in self.api_fills if f['order']==o.id)!=s.quantity for o,s in zip(results,specs)):
                    raise ValueError('ALL_OR_NONE_NOT_FILLED')
                return results
            except Exception:
                self._restore_finance(before);raise
        if operation=='CANCEL':
            value=self.api_orders.pop(payload,None)
            if value is not None and self.order_history is not None:self.order_history.closed(payload,self.now)
            return value
        if operation=='CANCEL_ALL':
            ids=[q for q,o in self.api_orders.items()if payload is None or
                 (isinstance(payload,str)and o.spec.exchange==payload)or
                 (isinstance(payload,dict)and self.mapping.get(o.spec.exchange)==payload.get('market'))]
            for q in ids:
                self.api_orders.pop(q)
                if self.order_history is not None:self.order_history.closed(q,self.now)
            return ids
        raise ValueError('UNSUPPORTED_API_OPERATION')

    def dispatch(self,operation,payload,*,idempotency_key,relationship_constraint=None):
        if operation not in ('ORDER','BATCH','MULTI','ALL_OR_NONE','CANCEL','CANCEL_ALL'):raise ValueError('API_OPERATION')
        if not isinstance(idempotency_key,str)or not idempotency_key or len(idempotency_key)>255:raise ValueError('IDEMPOTENCY_KEY')
        signature=repr((operation,payload,relationship_constraint))
        if idempotency_key in self.idempotency:
            old=self.idempotency[idempotency_key]
            if old['signature']!=signature:raise ValueError('IDEMPOTENCY_CONFLICT')
            return old
        if self.budget.active is not None:return {'status':'BUSY','ready_ms':max(self.now+1,self.busy_until)}
        try:ready=self.budget.ready_at(self.now,True)
        except ValueError as exc:
            if str(exc)=='UNKNOWN_ACCEPTANCE_RECONCILE_FIRST':return {'status':'UNRESOLVED_EXECUTION'}
            raise
        if ready>self.now:return {'status':'RATE_WAIT','ready_ms':ready}
        rid='api:'+idempotency_key
        assert self.budget.admit(rid,self.now,True)
        self.busy_until=self.now+self.mechanics.arrival_latency_ms+self.mechanics.receipt_latency_ms
        record={'signature':signature,'status':'IN_FLIGHT','request_id':rid}
        self.idempotency[idempotency_key]=record
        self.clock.schedule(self.now+self.mechanics.arrival_latency_ms,20,'api_arrival',(operation,payload,idempotency_key,relationship_constraint))
        self.clock.schedule(self.busy_until,30,'api_receipt',idempotency_key)
        return record

    def request_observation(self,venue,key,*,trade_limit=50,continue_trade_page=True,trade_from_ms=0,trade_to_ms=None,
                            order_status='open',order_limit=50,continue_order_page=False,order_cursor=None,
                            order_exchange=None,order_market=None):
        if self.order_history is not None and venue=='SIG'and key[0]=='owned_orders':
            from .order_pages import request
            return request(self,key,status=order_status,limit=order_limit,continuation=continue_order_page,cursor=order_cursor,
                exchange=order_exchange,market=order_market)
        if venue not in ('SIG','Kalshi','Polymarket'):raise ValueError('READ_ONLY_REFERENCE_VENUE')
        if key not in self.keys[venue]:return {'status':'SOURCE_NOT_AVAILABLE'}
        if venue=='SIG'and key[0]=='realtime_token':return self.realtime.mint()
        if venue=='SIG'and self.budget.active is not None:return {'status':'BUSY','ready_ms':self.busy_until}
        if venue!='SIG'and self.now<self.external_ready[venue]:return {'status':'RATE_WAIT','ready_ms':self.external_ready[venue]}
        if venue=='SIG':
            ready=self.budget.ready_at(self.now,False)
            if ready>self.now:return {'status':'RATE_WAIT','ready_ms':ready}
            if key[0]=='trade_flow':
                previous=self.known_trade_pages.get(key[1],{})
                cursor=previous.get('pagination',{}).get('nextCursor')if continue_trade_page else None
                payload=self.trade_history.page(key[1],self.now,limit=trade_limit,cursor=cursor,
                    from_ms=trade_from_ms,to_ms=trade_to_ms)
                self.feed_sequence+=1;rid=f'trade-read:{self.feed_sequence}'
                assert self.budget.admit(rid,self.now,False)
                self.busy_until=self.now+self.mechanics.read_latency_ms
                self.feed_requests.append({'venue':venue,'at_ms':self.now,'key':key,'request_id':rid})
                self.feed_request_counts[venue]+=1
                self.clock.schedule(self.busy_until,30,'api_trade_receipt',(rid,key,payload))
                return {'status':'IN_FLIGHT'}
            if key[0]in('account','collateral','owned_orders','positions'):
                from .account_reads import capture
                snapshot=capture(self,key[0])
                self.feed_sequence+=1;rid=f'api-read:{self.feed_sequence}'
                assert self.budget.admit(rid,self.now,False)
                self.busy_until=self.now+self.mechanics.read_latency_ms
                self.feed_requests.append({'venue':venue,'at_ms':self.now,'key':key,'request_id':rid})
                self.feed_request_counts[venue]+=1
                self.clock.schedule(self.busy_until,30,'api_read_receipt',(rid,key,snapshot))
                return {'status':'IN_FLIGHT'}
        # Reuse one causal, charged parent request, without starting auto polling.
        self.cursor[venue]=self.keys[venue].index(key)
        self._poll(self.now,venue)
        if venue!='SIG':self.external_ready[venue]=self.external_next[venue]
        return {'status':'IN_FLIGHT'}

    def _poll(self,at,venue):
        # Parent schedules follow-up polls; only explicitly requested ones run.
        self._manual_poll=True
        try:super()._poll(at,venue)
        finally:self._manual_poll=False

    def _capture_account(self):
        return deepcopy((self.finance_revision,self.cash,self.lots,self.api_orders,self.advance_capacity()))

    def _publish_account(self,snapshot):
        from .account_reads import publish
        revision,cash,lots,orders,advance=snapshot
        publish(self,{'revision':revision,'fields':{'cash':cash,'lots':lots,'orders':orders,'advance':advance}})

    def _event(self,at,kind,value):
        if kind=='api_source_cursor':value.consume(self,at);return
        if kind=='trade_flow':
            self.trade_history.accept(at,value)
            if self.realtime is not None:self.realtime.source_trade(at,value)
            return
        if kind=='api_trade_receipt':
            rid,key,page=value;self.budget.complete(rid,at)
            self.known_trade_pages[key[1]]=page
            for original in page['data']:
                row=dict(original);row['original_available_at_ms']=row['available_at_ms']
                row['available_at_ms']=at;self.flow.accept(at,row)
            self.feed_receipts.append({'venue':'SIG','at_ms':at,'key':key})
            self.feed_receipt_counts['SIG']+=1;self.observation_revision+=1;return
        if kind=='api_world_book_expiry':
            from .world_confidence import expire
            expire(self,at,value);return
        if kind=='api_snapshot_world':
            from .snapshot_clock import world_event
            world_event(self,at,value);return
        if kind=='api_snapshot_information':
            from .snapshot_clock import information_event
            information_event(self,at,value);return
        if kind=='api_counterparty_trade':
            if self.passive is not None:
                before=len(self.api_fills);self.passive.trade(at,value)
                if len(self.api_fills)>before:
                    self.notify_fill()
            return
        if self.realtime is not None and self.realtime.event(at,kind,value):return
        if kind=='economic_boundary':
            value.capture_boundary(self,at);return
        if kind=='feed_poll':return  # no compulsory observation schedule
        if kind=='api_arrival':
            operation,payload,key,constraint=value;record=self.idempotency[key]
            fill_start=len(self.api_fills)
            try:
                record['result']=deepcopy(self._execute(operation,payload,constraint));record['status']='EXECUTED'
                self.finance_revision+=1
            except ValueError as exc:record['error']=str(exc);record['status']='REJECTED'
            from .execution_receipts import enabled,ack
            record['receipt_snapshot']=(ack(self,operation,payload,record,fill_start)if enabled(self)
                                        else self._capture_account())
            if enabled(self)and len(self.api_fills)>fill_start:self.notify_fill()
            for order in self.api_orders.values():
                if order.spec.expiration_ms is not None and order.spec.expiration_ms>=at and order.id not in self.expiry_scheduled:
                    self.expiry_scheduled.add(order.id)
                    self.clock.schedule(order.spec.expiration_ms,20,'api_expiry',order.id)
            return
        if kind=='api_receipt':
            record=self.idempotency[value];self.budget.complete(record['request_id'],at)
            self.responses.append((at,value,record['status']))
            from .execution_receipts import enabled,publish_ack
            if enabled(self):publish_ack(self,record['receipt_snapshot'])
            else:self._publish_account(record['receipt_snapshot'])
            return
        if kind=='api_expiry':
            expired=self.api_orders.pop(value,None);self.finance_revision+=1
            if expired is not None and self.order_history is not None:self.order_history.closed(value,at,'Expired')
            from .execution_receipts import enabled
            if enabled(self):
                if self.realtime is not None:self.realtime.source_order_close(value)
            else:self.known_orders.pop(value,None);self.observation_revision+=1
            return
        if kind=='api_account_receipt':self._publish_account(value);return
        if kind=='api_read_receipt':
            from .account_reads import publish
            rid,key,snapshot=value;self.budget.complete(rid,at);publish(self,snapshot)
            self.feed_receipts.append({'venue':'SIG','at_ms':at,'key':key})
            self.feed_receipt_counts['SIG']+=1;return
        if kind=='api_order_page_receipt':
            from .order_pages import publish
            rid,key,page=value;self.budget.complete(rid,at,status=400 if'error'in page else 200);publish(self,page)
            self.feed_receipts.append({'venue':'SIG','at_ms':at,'key':key});self.feed_receipt_counts['SIG']+=1;return
        if kind=='feed_receipt':
            venue,rid,key,payload=value
            if key[0]=='book':
                from .snapshot_clock import older
                if older(payload,self.actor_books.get(key[1])):
                    # Complete the charged request, but never regress a newer
                    # stream/read snapshot or use stale data as a new baseline.
                    value=(venue,rid,key,None)
        before=len(self.api_fills)
        super()._event(at,kind,value)
        if kind=='tape'and value['kind']=='book':
            if self.passive is not None:self.passive.on_book(value['exchange_id'])
            for q,order in list(self.api_orders.items()):
                if order.spec.exchange==value['exchange_id']:
                    self._match_api(order)
                    if not order.remaining:self.api_orders.pop(q,None)
            if self.realtime is not None:self.realtime.source_book(at,value['exchange_id'])
        if self.realtime is not None:
            if kind=='feed_receipt':
                venue,rid,key,payload=value
                if venue=='SIG':self.realtime.authoritative_book(key,payload)
            elif kind=='tape'and value['kind']=='gap':self.realtime.source_gap(value)
        if len(self.api_fills)>before:self.notify_fill()
        if kind=='feed_receipt':self.observation_revision+=1

    def notify_fill(self):
        from .execution_receipts import enabled
        if enabled(self):
            if self.realtime is not None:self.realtime.source_private_fill()
        elif self.private_fill_push:
            self.clock.schedule(self.now+self.mechanics.receipt_latency_ms,30,'api_account_receipt',self._capture_account())
        elif self.realtime is not None:self.realtime.source_private_fill()

    def decision_times(self,alarm_ms=None):
        """No 1Hz grid. Wake on events, request capacity or a model-chosen alarm."""
        candidates=[]
        if alarm_ms is not None:
            if type(alarm_ms)is not int or alarm_ms<self.now:raise ValueError('PAST_ALARM')
            candidates.append(alarm_ms)
        # Hidden source arrivals may update execution, but cannot wake a policy
        # until its next observable receipt. Process them while advancing there.
        wake=self.clock.next_observable()
        if wake is not None:candidates.append(wake)
        if self.budget.active is not None:candidates.append(self.busy_until)
        else:
            candidates.append(self.budget.ready_at(self.now,False))
            if not self.budget.unknown:candidates.append(self.budget.ready_at(self.now,True))
        return min((v for v in candidates if v>self.now),default=self.end)

    def advance_until_observable(self,target):
        """A future hidden fill can create a receipt that interrupts WAIT.

        Hidden events alone never wake the actor. Process all events tied at
        a timestamp before exposing its resulting observation revision.
        """
        revision=self.observation_revision
        while self.clock._events and self.clock._events[0][0]<=target:
            self.advance(self.clock._events[0][0])
            if self.observation_revision!=revision:return self.now
        self.advance(target);return self.now

    def observe(self):
        """Dynamic entity tokens: all contracts, own order IDs and read targets.

        Fields contain only delivered observations, known holdings and local
        request-budget state. Raw API choices do not require external marks.
        """
        entities=[('global',None)]+[('contract',ex)for ex in self.exchanges[1:]]
        entities += [('market',m)for m in sorted(self.scope)]
        entities += [('relationship',r)for r in sorted(self.relationships)]
        entities += [('order',q)for q in sorted(self.known_orders)]
        entities += [('read',(v,key))for v in ('SIG','Kalshi','Polymarket')for key in self.keys[v]]
        entities += [('trade_print',(ex,j))for ex,page in self.known_trade_pages.items()
                     for j in range(len(page['data']))]
        entities += [('execution_ack',j)for j in range(len(self.known_acknowledgments))]
        entities += [('private_fill',j)for j in range(len(self.known_private_fills))]
        if self.realtime is not None:entities.append(('stream_control','disconnect'))
        features=np.zeros((len(entities),40),np.float32)
        masks={'market':np.ones(len(entities),bool),'family':np.zeros((len(entities),12),bool),
               'trade_query':np.zeros((len(entities),1),bool)}
        def ready(write):
            if self.budget.active is not None or self.budget.poisoned:return False
            if write and self.budget.unknown:return False
            return self.budget.ready_at(self.now,write)<=self.now
        read_ready,write_ready=ready(False),ready(True)
        builder_size=len(self.builder)
        from .feature_cache import identity,log_quantity,book_statistics,entity_identities
        features[:,33:40]=entity_identities(self,entities)
        for i,(kind,key)in enumerate(entities):
            if kind=='global':
                masks['family'][i,[0,11]]=True
                if write_ready:
                    masks['family'][i,4]=True
                    if 1<=builder_size<=50:masks['family'][i,7]=True
                    if 1<=builder_size<=10:masks['family'][i,[8,9]]=True
            elif kind=='contract':
                if write_ready:masks['family'][i,[1,2,4]]=True
                if builder_size<50:masks['family'][i,[5,6]]=True
                q=self.actor_books.get(key)
                if q is not None:
                    b=q.book;features[i,:8]=book_statistics(self,key,b)
                    features[i,3]=(self.now-b.available_at_ms)/1000
                    features[i,7]=float(not q.invalidated)
                for j,venue in enumerate(('Kalshi','Polymarket')):
                    rows=[r for _,r in self.reference_cache.for_market(self.mapping.get(key))if r.get('venue')==venue]
                    if rows:
                        r=max(rows,key=lambda r:r['available_at_ms'])
                        features[i,8+3*j:11+3*j]=[r.get('bid_yes_micros',0)/UNIT,r.get('ask_yes_micros',UNIT)/UNIT,
                                                   (self.now-r['available_at_ms'])/1000]
                features[i,14:16]=[log_quantity(sum(n for n,p in self.known_lots.get((key,s),[])))for s in ('YES','NO')]
                features[i,16:24]=self.flow.observe(key,self.now)
                features[i,24]=1.
            elif kind=='market':
                masks['family'][i,4]=write_ready;features[i,24]=2.
            elif kind=='relationship':
                if write_ready and 1<=builder_size<=10:masks['family'][i,[8,9]]=True
                features[i,24]=3.
            elif kind=='order':
                masks['family'][i,3]=write_ready;o=self.known_orders[key]
                features[i,25:30]=[1.,log_quantity(o.remaining),(o.spec.price or 0)/UNIT,
                                   float(o.spec.side=='YES'),float(o.spec.action=='BUY')]
            elif kind=='stream_control':
                masks['family'][i,11]=True;features[i,:6]=self.realtime.features();features[i,24]=4.
            elif kind=='execution_ack':
                row=self.known_acknowledgments[key]
                features[i,:8]=[log_quantity(row['quantity'])/16.,log_quantity(row['remaining'])/16.,
                    log_quantity(row['quantity_traded'])/16.,row['total_cost_micros']/max(1,self.initial_wealth),
                    (row['last_price_micros']or 0)/UNIT,float(row['side']=='YES'),
                    float(row['action']=='BUY'),(self.now-row['at_ms'])/1000.]
                features[i,24]=8.;features[i,33:40]=identity('contract',row['exchange'])
                features[i,8:15]=identity('order',row['order']);features[i,15]=float(row['open'])
            elif kind=='private_fill':
                row=self.known_private_fills[key]
                features[i,:3]=[row['yes_price_micros']/UNIT,log_quantity(row['quantity'])/16.,
                    (self.now-row['executed_at_ms'])/1000.]
                features[i,8:15]=identity('order',row['order'])
                features[i,24]=9.;features[i,33:40]=identity('contract',row['exchange'])
            elif kind=='trade_print':
                ex,j=key;page=self.known_trade_pages[ex];row=page['data'][j]
                features[i,:5]=[row['yes_price_micros']/UNIT,log_quantity(row['quantity'])/16.,
                    np.log1p(self.now-row['executed_at_ms'])/32.,
                    np.log1p(self.now-row['received_at_ms'])/32.,np.log1p(self.now-page['opened_at_ms'])/32.]
                features[i,24]=7.
                features[i,33:40]=identity('contract',ex)
            else:
                masks['family'][i,10]=((write_ready if key[1][0]=='realtime_token'else read_ready)
                    if key[0]=='SIG'else self.now>=self.external_ready[key[0]])
                features[i,30+('SIG','Kalshi','Polymarket').index(key[0])]=1.
                if key[0]=='SIG'and key[1][0]=='trade_flow':
                    masks['trade_query'][i,0]=True
                    page=self.known_trade_pages.get(key[1][1])
                    if page is not None:features[i,25:28]=[1.,float(page['pagination']['hasMore']),len(page['data'])/200.]
                if key[0]!='SIG':
                    target=key[1];r=self.reference_cache.get((target[2],target[3]))
                    if r is not None:features[i,8:11]=[r.get('bid_yes_micros',0)/UNIT,r.get('ask_yes_micros',UNIT)/UNIT,
                                                      (self.now-r['available_at_ms'])/1000]
        masks['market']=masks['family'].any(-1)
        # Shared policy heads accept the complete legal price lattice. Quantity
        # and alarm/expiration are float64 latent samples converted to exact ints.
        n=len(entities)
        # Every side/tick/expiration choice is legal at the policy layer; avoid
        # allocating/transferring millions of identical True mask entries.
        state=np.zeros(16,np.float32)
        state[:9]=[self.known_cash/max(1,self.initial_wealth),self.known_advance/max(1,self.initial_wealth),
            (self.now-self.start)/1000,(self.end-self.now)/1000,len(self.known_orders),len(self.builder),
            float(self.budget.active is not None),max(0,self.budget.next_at-self.now)/1000,
            float(bool(self.budget.unknown))]
        if self.responses:
            _,key,status=self.responses[-1];record=self.idempotency[key]
            state[9]=float(status=='EXECUTED');state[10]=float(status=='REJECTED')
            state[11]=int.from_bytes(hashlib.sha256(record.get('error','').encode()).digest()[:2],'big')/65535.
            result=record.get('result')
            if isinstance(result,list):state[12]=sum(isinstance(item,str)for item in result)
        state[13]=len(self.budget.reads)/100.;state[14]=len(self.budget.writes)/30.
        state[15]=self.finance_observation_count
        from .state_encoding import encode
        state=encode(state,self.global_state_encoding)
        return {'contracts':features,'global_state':state,'global_state_encoding':self.global_state_encoding,
                'observation_protocol':self.observation_protocol,
                'present':np.ones(n,bool),'masks':masks,
                'entities':entities,'observation_revision':self.observation_revision}

    def act_raw(self,action,observation):
        family=int(action['family']);kind,key=observation['entities'][int(action['market'])]
        if not observation['masks']['family'][int(action['market']),family]:raise ValueError('ACTION_ENTITY_MISMATCH')
        # No target or portfolio fraction. This latent variable addresses the
        # full signed-32-bit positive quantity domain accepted by the API.
        from .contract import MAX_QUANTITY
        fraction=float(action['target_fraction']);clock_fraction=float(action['child_fraction'])
        if not 0<=fraction<=1 or not 0<=clock_fraction<=1:raise ValueError('RAW_LATENT_RANGE')
        quantity=min(MAX_QUANTITY,1+int(math.expm1(fraction*math.log(MAX_QUANTITY))))
        # Full representable RFC3339 UTC future, bounded only by datetime format.
        span=max(1,253402300799999-self.now-1)
        timestamp=self.now+max(1,int(math.expm1(clock_fraction*math.log1p(span))))
        if family==0:return {'status':'ALARM','ready_ms':timestamp}
        if family in (1,2,5,6):
            spec=OrderSpec(key,('YES','NO')[int(action['side'])],
                'BUY'if family in (1,5)else'SELL',quantity,
                None if int(action['price'])==0 else int(action['price'])*TICK,
                timestamp if int(action['patience'])and int(action['price'])else None)
            if family in (5,6):
                if len(self.builder)>=50:return {'status':'API_BATCH_MAX_50'}
                self.builder.append(spec);return {'status':'BUILDER_UPDATED'}
            operation,payload='ORDER',[spec]
        elif family==3:operation,payload='CANCEL',key
        elif family==4:operation,payload='CANCEL_ALL',key if kind=='contract'else{'market':key}if kind=='market'else None
        elif family in (7,8,9):
            operation=('BATCH','MULTI','ALL_OR_NONE')[family-7];payload=list(self.builder)
        elif family==10:
            query={}
            if self.order_history is not None and key[0]=='SIG'and key[1][0]=='owned_orders':
                from .order_history import STATUSES
                filters={};target=key[1]
                if target[1:2]!=('cursor',)and int(action.get('query_filter',0)):
                    markets=sorted(self.scope);exchanges=self.exchanges[1:]
                    if len(target)==1 or target[1]=='exchange':
                        if markets:filters['order_market']=markets[min(len(markets)-1,int(float(action['query_from_fraction'])*len(markets)))]
                    if len(target)==1 or target[1]=='market':
                        if exchanges:filters['order_exchange']=exchanges[min(len(exchanges)-1,int(float(action['query_to_fraction'])*len(exchanges)))]
                return self.request_observation(*key,order_status=STATUSES[int(action['price'])%4],
                    order_limit=min(200,1+int(fraction*200)),continue_order_page=bool(int(action['side'])),**filters)
            if 'query_filter'in action:
                from .observation_protocol import QUERY_WINDOW,SCOPED_EXECUTION,MEMORY_JOINT,ORDER_PAGES,COMPONENT_DEBT,BULK_QUOTES,API_TARGETS,HISTORICAL_INFORMATION
                if self.observation_protocol not in(QUERY_WINDOW,SCOPED_EXECUTION,MEMORY_JOINT,ORDER_PAGES,COMPONENT_DEBT,BULK_QUOTES,API_TARGETS,HISTORICAL_INFORMATION):raise ValueError('QUERY_WINDOW_PROTOCOL_REQUIRED')
                if key[0]=='SIG'and key[1][0]in('trade_flow','historical_trades'):
                    from .query_window import parameters
                    query=parameters(action)
            return self.request_observation(*key,trade_limit=min(200,1+int(fraction*200)),
                continue_trade_page=bool(int(action['side'])),**query)
        else:
            if kind=='stream_control':return self.realtime.disconnect()
            self.builder.clear();return {'status':'BUILDER_CLEARED'}
        self.api_sequence+=1
        result=self.dispatch(operation,payload,idempotency_key=f'policy-{self.api_sequence}',
                             relationship_constraint=key if kind=='relationship'else None)
        if result['status']=='IN_FLIGHT'and family in (7,8,9):self.builder.clear()
        return result


class ApiTape(PreparedTape):
    episode_type=ApiEpisode

    def episode(self,mechanics,start_ms,end_ms,*,initial_cash_micros,target_wealth_micros,
                burnin_ms=60000,advance_to_start=True):
        import bisect
        from moloch.learning_realism.protocol import valid_sha
        lo,hi=self.split.bounds(self.role)
        if not lo<=start_ms-burnin_ms<end_ms<=hi:
            raise ValueError('WHOLE_RAW_API_EPISODE_MUST_STAY_IN_ROLE')
        if self.role=='test'and not valid_sha(self.finalist):raise ValueError('FROZEN_FINALIST_REQUIRED')
        gap=getattr(self,'maximum_boundary_gap_ms',0)
        separate_world=getattr(self,'snapshot_world_mode',None)is not None
        first=self.times[0]
        if separate_world:
            info_lo,info_hi=self.snapshot_world_information_bounds
            if not info_lo<=start_ms-burnin_ms<end_ms<=info_hi:raise ValueError('WHOLE_SNAPSHOT_EPISODE_MUST_STAY_IN_INFORMATION_ROLE')
            if self.snapshot_world_times:first=min(first,self.snapshot_world_times[0])
        if start_ms-burnin_ms<first or end_ms>self.times[-1]+gap:
            raise ValueError('EPISODE_OUTSIDE_AVAILABLE_CHRONOLOGY')
        env=self.episode_type(mechanics,start_ms-burnin_ms,initial_cash_micros,self.exchanges,
                       target_wealth_micros,start_ms,end_ms)
        env.role=self.role;env.delivery=self.delivery;env.prepared_books=self.prepared_books
        from .observation_protocol import validate as protocol,CURRENT
        env.observation_protocol=protocol(getattr(self,'observation_protocol',CURRENT))
        from .world_confidence import validate_lifetime
        env.world_book_lifetime_ms=validate_lifetime(getattr(self,'world_book_lifetime_ms',None))
        from .state_encoding import validate
        env.global_state_encoding=validate(getattr(self,'global_state_encoding','raw'))
        if env.world_book_lifetime_ms is not None and not separate_world:
            raise ValueError('WORLD_BOOK_LIFETIME_REQUIRES_SEPARATE_SNAPSHOT_WORLD')
        if hasattr(self,'disk_store'):
            if separate_world:
                from .snapshot_clock import initialize_role_prefix
                env.prepared_world_books=self.prepared_world_books;env.world_market_mapping={}
                if getattr(self,'role_prefix_initial_state',False):initialize_role_prefix(self,env,start_ms-burnin_ms)
            from .trade_history import seed_role_history
            seed_role_history(self,env,start_ms-burnin_ms)
            from .disk_replay import schedule
            schedule(self,env,start_ms-burnin_ms,end_ms,separate_world)
            env.start_delivery()
            if advance_to_start:env.advance(start_ms)
            return env
        if separate_world:
            from .snapshot_clock import source_clock
            env.prepared_world_books=self.prepared_world_books;env.world_market_mapping={}
            if getattr(self,'role_prefix_initial_state',False):
                from .snapshot_clock import initialize_role_prefix
                initialize_role_prefix(self,env,start_ms-burnin_ms)
            a=bisect.bisect_left(self.snapshot_world_times,start_ms-burnin_ms)
            b=bisect.bisect_left(self.snapshot_world_times,end_ms)
            for row in self.snapshot_world_rows[a:b]:
                env.clock.schedule(source_clock(row),10,'api_snapshot_world',row)
        # Half-open source interval: an observation at next role's boundary
        # cannot change this role's terminal wealth. Receipt clocks remain real.
        a=bisect.bisect_left(self.times,start_ms-burnin_ms);b=bisect.bisect_left(self.times,end_ms)
        for row in self.rows[a:b]:
            kind=('api_snapshot_information'if separate_world and row['venue']=='SIG'and row['kind']in('book','gap')else'tape')
            env.clock.schedule(row['available_at_ms'],10,kind,row)
        from .trade_history import seed_role_history
        seed_role_history(self,env,start_ms-burnin_ms)
        a=bisect.bisect_left(self.flow_times,start_ms-burnin_ms);b=bisect.bisect_left(self.flow_times,end_ms)
        for row in self.flow_rows[a:b]:
            env.clock.schedule(row['available_at_ms'],10,'trade_flow',row)
        if getattr(self,'counterparty_replay_enabled',False):
            if not hasattr(self,'flow_execution_index'):
                self.flow_execution_index=tuple(sorted(self.flow_rows,key=lambda r:(r['executed_at_ms'],r['available_at_ms'],r['match_id'])))
                self.flow_execution_times=tuple(r['executed_at_ms']for r in self.flow_execution_index)
            a=bisect.bisect_left(self.flow_execution_times,start_ms-burnin_ms)
            b=bisect.bisect_left(self.flow_execution_times,end_ms)
            info_lo,info_hi=getattr(self,'counterparty_information_bounds',self.split.bounds(self.role))
            for row in self.flow_execution_index[a:b]:
                # Late public reporting inside the same authorized role must
                # not hide a real fill before the shorter episode horizon.
                # A row reserved to another role remains excluded entirely.
                if info_lo<=row['available_at_ms']<info_hi:
                    env.clock.schedule(row['executed_at_ms'],10,'api_counterparty_trade',row)
        env.start_delivery()
        if advance_to_start:env.advance(start_ms)
        return env
