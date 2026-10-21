"""Opt-in replay of producer-validated derived SIG book deliveries.

This adapter does NOT invent raw WebSocket revisions from engine sequence IDs.
It delivers only original compiled book/gap observations and opt-in public
prints, at their recorded
availability clocks. Raw topic continuity/reconnect parity still needs the
original transport frames. Token minting is a write; connected delivery is not.
"""
from copy import deepcopy
from math import log1p

LEGACY_REFRESH='reconnect_on_token_receipt_v1'
CONTINUOUS_REFRESH='preserve_active_channel_on_token_refresh_v2'
# V2 is retained to reproduce its experiment, not as protocol qualification:
# official SIG documentation requires REST refetch even on a healthy refresh.
RECONCILE_REFRESH='preserve_channel_require_refetch_on_token_refresh_v3'


def refresh_profile(value):
    if value is None:return RECONCILE_REFRESH
    if type(value)is not str or value not in(LEGACY_REFRESH,CONTINUOUS_REFRESH,RECONCILE_REFRESH):
        raise ValueError('EXPLICIT_REALTIME_REFRESH_PROFILE_REQUIRED')
    return value


class DerivedStream:
    mode='producer_validated_derived_books_not_raw_topic_replay'
    token_lifetime_ms=3*60*60*1000  # Official public API docs observed October19.
    reconciliation_interval_ms=120000  # Frozen example cadence, not an API limit.

    def __init__(self,env,*,public_trades=False,refresh_mode=None):
        if type(public_trades)is not bool:raise ValueError('EXPLICIT_DERIVED_PUBLIC_TRADE_PROFILE')
        self.refresh_mode=refresh_profile(refresh_mode);self.token_revision=0
        self.env=env;self.enabled=False;self.expires_at=0
        self.public_trades=public_trades;self.public_deliveries=0;self.public_duplicates=0;self.public_suppressed=0
        self.public_global_revision=0;self.public_market_revisions={};self.public_exchange_revisions={}
        self.control_revision=0;self.connection=0;self.synced={}
        self.deliveries=0;self.mints=0;self.gaps=0
        self.private_pending=False
        self.private_fills=[];self.private_orders=[];self.private_fill_cursor=len(env.api_fills)

    def invalidate(self):
        self.synced.clear()
        for world in self.env.actor_books.values():world.invalidate()
        self.env.observation_revision+=1

    def disconnect(self):
        self.control_revision+=1;self.connection+=1;self.enabled=False
        self.private_pending=False
        self.private_fills.clear();self.private_orders.clear();self.private_fill_cursor=len(self.env.api_fills)
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
        if kind=='derived_public_trade_receipt':
            generation,row=value
            if (not self.public_trades or not self.enabled or at>=self.expires_at
                    or generation!=self.public_generation(row)):
                self.public_suppressed+=1;return True
            if env.flow.accept(at,row):
                self.public_deliveries+=1;env.observation_revision+=1
            else:self.public_duplicates+=1
            return True
        if kind=='derived_token_receipt':
            rid,revision=value;env.budget.complete(rid,at)
            if revision!=self.control_revision:return True
            # A healthy auth refresh preserves the channel and valid pending
            # receipts. SIG nevertheless requires refetching REST baselines.
            preserve=(self.refresh_mode!=LEGACY_REFRESH and self.enabled and at<self.expires_at)
            if not preserve:
                self.connection+=1;self.private_pending=False
                self.private_fills.clear();self.private_orders.clear();self.private_fill_cursor=len(env.api_fills)
                self.invalidate()
            elif self.refresh_mode==RECONCILE_REFRESH:self.invalidate()
            else:env.observation_revision+=1  # Explicit historical V2 behavior.
            self.enabled=True;self.expires_at=at+self.token_lifetime_ms;self.token_revision+=1
            expiry=(self.connection,self.token_revision,self.expires_at)if self.refresh_mode!=LEGACY_REFRESH else self.connection
            env.clock.schedule(self.expires_at,30,'derived_token_expiry',expiry)
            return True
        if kind=='derived_token_expiry':
            expected=((self.connection,self.token_revision,self.expires_at)
                      if self.refresh_mode!=LEGACY_REFRESH else self.connection)
            if value==expected:self.enabled=False;self.private_pending=False;self.invalidate()
            return True
        if kind=='derived_private_publish':
            if self.enabled and value==self.connection:
                self.private_pending=False
                from .execution_receipts import enabled
                if enabled(env):
                    snapshot={'fills':self.private_fills,'orders':self.private_orders}
                    self.private_fills=[];self.private_orders=[]
                else:snapshot=env._capture_account()
                env.clock.schedule(at+env.mechanics.receipt_latency_ms,30,'derived_account_receipt',
                                    (self.connection,snapshot))
            return True
        if kind=='derived_account_receipt':
            connection,snapshot=value
            if self.enabled and connection==self.connection:
                from .execution_receipts import enabled,publish_private
                if enabled(env):publish_private(env,snapshot)
                else:env._publish_account(snapshot)
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
            from .snapshot_clock import older
            if older(book,env.actor_books.get(exchange)):return True
            baseline=self.synced.get(exchange)
            if baseline is None or at-baseline>self.reconciliation_interval_ms:book.invalidate()
            env.actor_books[exchange]=book;env.observation_revision+=1;self.deliveries+=1
            return True
        return False

    def source_private_fill(self):
        from .execution_receipts import enabled,fill
        if enabled(self.env):
            if self.enabled:self.private_fills.extend(fill(r,self.env.finance_revision)
                for r in self.env.api_fills[self.private_fill_cursor:])
            self.private_fill_cursor=len(self.env.api_fills)
        if self.enabled and not self.private_pending:
            self.private_pending=True
            self.env.clock.schedule(self.env.now+1000,30,'derived_private_publish',self.connection)

    def source_order_close(self,order_id):
        from .execution_receipts import enabled
        if enabled(self.env)and self.enabled:
            self.private_orders.append((self.env.finance_revision,{order_id:None}))
            if not self.private_pending:
                self.private_pending=True
                self.env.clock.schedule(self.env.now+1000,30,'derived_private_publish',self.connection)

    def source_book(self,at,exchange,payload=None):
        if self.enabled:
            from .liquidity_snapshot import receipt_snapshot
            # The compiler already records first observation availability.
            # No earlier synthetic receipt or reset of original source age.
            self.env.clock.schedule(at,30,'derived_book_receipt',
                (self.connection,exchange,receipt_snapshot(self.env.books[exchange]if payload is None else payload)))

    def public_generation(self,row):
        return(self.connection,self.public_global_revision,
               self.public_market_revisions.get(row['market_id'],0),
               self.public_exchange_revisions.get(row['exchange_id'],0))

    def source_trade(self,at,row):
        # Only future source deliveries while connected, never a reporting
        # history replay. Partial observed flow remains partial after a gap;
        # newly received facts do not certify book/tape continuity or resync.
        if not self.public_trades or not self.enabled or at>=self.expires_at:return
        if row.get('kind')!='trade_flow'or row.get('venue')!='SIG'or row.get('available_at_ms')!=at or at!=self.env.now:
            raise ValueError('ORIGINAL_PUBLIC_TRADE_AVAILABILITY_REQUIRED')
        self.env.clock.schedule(at,30,'derived_public_trade_receipt',(self.public_generation(row),deepcopy(row)))

    def authoritative_book(self,key,payload):
        if self.enabled and key[0]=='book'and payload is not None and not payload.invalidated:
            self.synced[key[1]]=self.env.now
            self.env.clock.schedule(self.env.now+self.reconciliation_interval_ms,30,'derived_baseline_expiry',
                                    (key[1],self.env.now,self.connection))

    def source_gap(self,row,source_state_at_ms=None):
        self.gaps+=1
        ex=row.get('exchange_id');market=row.get('market_id')
        if ex is not None:self.public_exchange_revisions[ex]=self.public_exchange_revisions.get(ex,0)+1
        elif market is not None:self.public_market_revisions[market]=self.public_market_revisions.get(market,0)+1
        else:self.public_global_revision+=1
        affected=([ex]if ex is not None else
            [e for e in self.env.actor_books if market is None or self.env.mapping.get(e)==market])
        for exchange in affected:
            actor=self.env.actor_books.get(exchange)
            if source_state_at_ms is not None and getattr(actor,'source_state_at_ms',-1)>source_state_at_ms:continue
            self.synced.pop(exchange,None)
            if actor is not None:
                actor.invalidate()
                if source_state_at_ms is not None:actor.source_state_at_ms=source_state_at_ms
        self.env.observation_revision+=1

    def features(self):
        return [float(self.enabled),max(0,self.expires_at-self.env.now)/self.token_lifetime_ms,
                log1p(len(self.synced)),log1p(self.deliveries),log1p(self.gaps),log1p(self.mints)]
