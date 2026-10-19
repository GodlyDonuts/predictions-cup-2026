"""Opt-in replay of producer-validated derived SIG book deliveries.

This adapter does NOT invent raw WebSocket revisions from engine sequence IDs.
It delivers only original compiled book/gap observations, at their recorded
availability clocks. Raw topic continuity/reconnect parity still needs the
original transport frames. Token minting is a write; connected delivery is not.
"""
from copy import deepcopy
from math import log1p


class DerivedStream:
    mode='producer_validated_derived_books_not_raw_topic_replay'
    token_lifetime_ms=3*60*60*1000  # Official public API docs observed October19.
    reconciliation_interval_ms=120000  # Frozen example cadence, not an API limit.

    def __init__(self,env):
        self.env=env;self.enabled=False;self.expires_at=0
        self.control_revision=0;self.connection=0;self.synced={}
        self.deliveries=0;self.mints=0;self.gaps=0

    def invalidate(self):
        self.synced.clear()
        for world in self.env.actor_books.values():world.invalidate()
        self.env.observation_revision+=1

    def disconnect(self):
        self.control_revision+=1;self.connection+=1;self.enabled=False
        self.invalidate()
        return {'status':'STREAM_DISCONNECTED'}

    def mint(self):
        env=self.env
        if env.budget.active is not None:return {'status':'BUSY','ready_ms':env.busy_until}
        try:ready=env.budget.ready_at(env.now,True)
        except ValueError as exc:
            if str(exc)=='UNKNOWN_ACCEPTANCE_RECONCILE_FIRST':return {'status':'UNRESOLVED_EXECUTION'}
            raise
        if ready>env.now:return {'status':'RATE_WAIT','ready_ms':ready}
        env.feed_sequence+=1;rid='realtime-token:'+str(env.feed_sequence)
        env.budget.admit(rid,env.now,True)
        env.busy_until=env.now+env.mechanics.read_latency_ms
        self.control_revision+=1;self.mints+=1
        env.clock.schedule(env.busy_until,30,'derived_token_receipt',(rid,self.control_revision))
        return {'status':'IN_FLIGHT'}

    def event(self,at,kind,value):
        env=self.env
        if kind=='derived_token_receipt':
            rid,revision=value;env.budget.complete(rid,at)
            if revision!=self.control_revision:return True
            self.connection+=1;self.enabled=True;self.expires_at=at+self.token_lifetime_ms
            self.invalidate()
            env.clock.schedule(self.expires_at,30,'derived_token_expiry',self.connection)
            return True
        if kind=='derived_token_expiry':
            if value==self.connection:self.enabled=False;self.invalidate()
            return True
        if kind=='derived_baseline_expiry':
            exchange,baseline,connection=value
            if self.enabled and connection==self.connection and self.synced.get(exchange)==baseline:
                self.synced.pop(exchange,None)
                if exchange in env.actor_books:env.actor_books[exchange].invalidate()
                env.observation_revision+=1
            return True
        if kind=='derived_book_receipt':
            connection,exchange,book=value
            if not self.enabled or connection!=self.connection:return True
            baseline=self.synced.get(exchange)
            if baseline is None or at-baseline>self.reconciliation_interval_ms:book.invalidate()
            env.actor_books[exchange]=book;env.observation_revision+=1;self.deliveries+=1
            return True
        return False

    def source_book(self,at,exchange):
        if self.enabled:
            # The compiler already records first observation availability.
            # No earlier synthetic receipt or reset of original source age.
            self.env.clock.schedule(at,30,'derived_book_receipt',
                (self.connection,exchange,deepcopy(self.env.books[exchange])))

    def authoritative_book(self,key,payload):
        if self.enabled and key[0]=='book'and payload is not None and not payload.invalidated:
            self.synced[key[1]]=self.env.now
            self.env.clock.schedule(self.env.now+self.reconciliation_interval_ms,30,'derived_baseline_expiry',
                                    (key[1],self.env.now,self.connection))

    def source_gap(self,row):
        self.gaps+=1
        ex=row.get('exchange_id');market=row.get('market_id')
        affected=([ex]if ex is not None else
            [e for e in self.env.actor_books if market is None or self.env.mapping.get(e)==market])
        for exchange in affected:
            self.synced.pop(exchange,None)
            if exchange in self.env.actor_books:self.env.actor_books[exchange].invalidate()
        self.env.observation_revision+=1

    def features(self):
        return [float(self.enabled),max(0,self.expires_at-self.env.now)/self.token_lifetime_ms,
                log1p(len(self.synced)),log1p(self.deliveries),log1p(self.gaps),log1p(self.mints)]
