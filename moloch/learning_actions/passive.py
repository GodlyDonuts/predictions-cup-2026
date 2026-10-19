"""Opt-in, conservative strict-price-priority counterfactual on actual prints.

Not a calibrated queue model. Equal-price queue fills are never assumed. A
strictly better-priced resting quote may intercept recorded counterparty volume
at its own limit (the less favorable price). Original-owner exclusions and full
counterfactual market response need separate evidence before qualification.
"""
from .contract import UNIT


class StrictPriorityFlow:
    mode='strict_price_priority_print_interception_uncalibrated'

    def __init__(self,env,tournament,*,excluded_match_ids=(),exclusion_source_sha256=None):
        if not isinstance(tournament,str)or not tournament:raise ValueError('EXPLICIT_FLOW_TOURNAMENT_REQUIRED')
        self.env=env;self.tournament=tournament;self.excluded=set(excluded_match_ids)
        if self.excluded and (not isinstance(exclusion_source_sha256,str)or len(exclusion_source_sha256)!=64):
            raise ValueError('OWN_MATCH_EXCLUSIONS_REQUIRE_SOURCE_PIN')
        self.exclusion_source_sha256=exclusion_source_sha256
        self.seen={};self.crossing_debt={};self.source_volume=0;self.intercepted_volume=0
        self.excluded_prints=0
        self.unknown_liquidity_print_volume=0

    def on_fill(self,exchange,quantity,origin):
        if origin=='crossing_book':self.crossing_debt[exchange]=self.crossing_debt.get(exchange,0)+quantity

    def on_rest(self,order):pass
    def on_book(self,exchange):pass
    def equal_price_trade(self,at,row,available):pass

    def fill(self,order,quantity,price,origin):
        self.env._apply_fill(order,price,quantity,origin=origin)
        self.intercepted_volume+=quantity
        if not order.remaining:self.env.api_orders.pop(order.id,None)
        world=self.env.books.get(order.spec.exchange)
        if world is not None:
            for resource in('asks','bids'):
                level=(resource,price if order.spec.side=='YES'else UNIT-price)
                world.depleted[level]=world.depleted.get(level,0)+quantity

    def trade(self,at,row):
        if row['tournament_id']!=self.tournament:return
        if row['executed_at_ms']!=at:raise ValueError('PASSIVE_MATCH_AT_ORIGINAL_EXECUTION_CLOCK')
        key=(row['tournament_id'],row['exchange_id'],row['match_id'])
        signature=(row['executed_at_ms'],row['yes_price_micros'],row['quantity'],row['market_id'])
        if key in self.seen:
            if self.seen[key]!=signature:raise ValueError('ORIGINAL_TRADE_FACTS_CHANGED')
            return
        self.seen[key]=signature
        if row['match_id']in self.excluded:self.excluded_prints+=1;return
        exchange=row['exchange_id'];quantity=row['quantity'];price=row['yes_price_micros']
        if type(quantity)is not int or quantity<=0 or type(price)is not int or not 0<=price<=UNIT:
            raise ValueError('EXACT_PUBLIC_PRINT_UNITS_REQUIRED')
        self.source_volume+=quantity
        # Crossing-book fills may already have consumed these counterparties.
        # Subtract them first, even when this conservatively excludes unrelated
        # volume. A print cannot also manufacture fresh displayed-book depth.
        debt=self.crossing_debt.get(exchange,0);paid=min(debt,quantity)
        self.crossing_debt[exchange]=debt-paid
        available=(quantity-paid)*self.env.mechanics.depth_fraction_ppm//UNIT
        from .world_confidence import known
        if not known(self.env,exchange):
            self.unknown_liquidity_print_volume+=available
            return
        orders=[]
        for order in self.env.api_orders.values():
            s=order.spec
            if s.exchange!=exchange or s.market or order.placed_at_ms>=at:continue
            if s.expiration_ms is not None and at>=s.expiration_ms:continue
            yes_price=s.price if s.side=='YES'else UNIT-s.price
            buy_yes=(s.action=='BUY')==(s.side=='YES')
            if (buy_yes and yes_price>price)or(not buy_yes and yes_price<price):
                orders.append((buy_yes,yes_price,order))
        if len({direction for direction,_,_ in orders})>1:
            raise ValueError('CROSSED_OWN_QUOTES_CANNOT_SHARE_ONE_TRADE')
        orders.sort(key=lambda x:(-x[1]if x[0]else x[1],x[2].placed_at_ms,x[2].id))
        for _,_,order in orders:
            if not available:break
            s=order.spec
            affordable=(self.env.buying_power()//max(1,s.price+self.env.mechanics.limits.fee_per_share)
                        if s.action=='BUY'else self.env.held(s.exchange,s.side))
            take=min(available,order.remaining,affordable)
            if not take:continue
            self.fill(order,take,s.price,'passive_strict_priority')
            world=self.env.books.get(exchange)
            yes_limit=s.price if s.side=='YES'else UNIT-s.price
            if world is not None and yes_limit!=price:
                for resource in('asks','bids'):
                    level=(resource,price);world.depleted[level]=world.depleted.get(level,0)+take
            available-=take
        if available:self.equal_price_trade(at,row,available)


class DisplayedQueueFlow(StrictPriorityFlow):
    mode='displayed_queue_ahead_no_cancel_credit_uncalibrated'

    def __init__(self,*args,**kwargs):
        super().__init__(*args,**kwargs);self.queues={};self.ahead_consumed=0

    @staticmethod
    def key(order):
        s=order.spec
        return (s.exchange,(s.action=='BUY')==(s.side=='YES'),s.price if s.side=='YES'else UNIT-s.price)

    def displayed(self,key):
        exchange,buy_yes,price=key;world=self.env.books.get(exchange)
        if world is None or world.invalidated:return None
        return sum(n for p,n in(world.book.bids if buy_yes else world.book.asks)if p==price)

    def on_rest(self,order):
        key=self.key(order)
        others=[o for o in self.env.api_orders.values()if o.id!=order.id and self.key(o)==key
                and o.remaining>0 and(o.spec.expiration_ms is None or o.spec.expiration_ms>self.env.now)]
        if not others or key not in self.queues:
            quantity=self.displayed(key);self.queues[key]={'ahead':quantity,'last_displayed':quantity}

    def on_book(self,exchange):
        for key,queue in self.queues.items():
            if key[0]!=exchange:continue
            quantity=self.displayed(key)
            if quantity is None or queue['ahead']is None:continue
            queue['ahead']+=max(0,quantity-queue['last_displayed'])
            queue['last_displayed']=quantity

    def equal_price_trade(self,at,row,available):
        exchange,price=row['exchange_id'],row['yes_price_micros']
        orders=[o for o in self.env.api_orders.values()if o.spec.exchange==exchange
            and self.key(o)[2]==price and o.placed_at_ms<at and
            (o.spec.expiration_ms is None or at<o.spec.expiration_ms)]
        if not orders:return
        if len({self.key(o)[1]for o in orders})!=1:raise ValueError('CROSSED_EQUAL_PRICE_OWN_QUOTES')
        key=self.key(orders[0]);queue=self.queues.get(key)
        if queue is None or queue['ahead']is None:return
        ahead=min(queue['ahead'],available);queue['ahead']-=ahead
        available-=ahead;self.ahead_consumed+=ahead
        for order in sorted(orders,key=lambda o:(o.placed_at_ms,o.id)):
            if not available:break
            s=order.spec
            affordable=(self.env.buying_power()//max(1,s.price+self.env.mechanics.limits.fee_per_share)
                        if s.action=='BUY'else self.env.held(s.exchange,s.side))
            take=min(available,order.remaining,affordable)
            if take:self.fill(order,take,s.price,'passive_displayed_queue');available-=take
