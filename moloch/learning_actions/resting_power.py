"""Opt-in cash/advance-driven remainder sizing, not calibrated queue priority.

The saved API requires shrink/cancel after lost buying power but does not state
which same-exchange quote survives. Both explicit hypotheses keep exchanges
independent: resting orders never escrow account cash across exchanges.
"""
MODES=('per_exchange_oldest_first_uncalibrated','per_order_independent_uncalibrated')


def validate(mode):
    if mode is not None and(type(mode)is not str or mode not in MODES):
        raise ValueError('EXPLICIT_RESTING_BUY_FUNDING_HYPOTHESIS_REQUIRED')
    return mode


def enabled(env):return getattr(env,'resting_buy_funding',None)is not None


def live(env,order):
    s=order.spec
    return order.remaining>0 and s.action=='BUY'and not s.market and(s.expiration_ms is None or s.expiration_ms>env.now)


def unit_cost(env,order):return order.spec.price+env.mechanics.limits.fee_per_share


def commitment(env,exchange):
    return sum(o.remaining*unit_cost(env,o)for o in env.api_orders.values()if o.spec.exchange==exchange and live(env,o))


def resize(env,order,quantity):
    before=order.remaining
    if type(quantity)is not int or not 0<=quantity<=before:raise ValueError('RESTING_SIZE_CAN_ONLY_DECREASE')
    if quantity==before:return
    order.remaining=quantity
    if env.order_history is not None:env.order_history.resized(order,before,env.now)
    if not quantity:env.api_orders.pop(order.id,None)
    env.finance_revision+=1
    # This is hidden physical state. No free order/account baseline or guessed
    # private resize event is published; a paid read reconciles the projection.


def prepare(env,order):
    if not live(env,order):return
    available=max(0,env.buying_power()-commitment(env,order.spec.exchange))
    resize(env,order,min(order.remaining,available//unit_cost(env,order)))


def reconcile(env):
    mode=validate(env.resting_buy_funding)
    power=env.buying_power();remaining={}
    for order in sorted(list(env.api_orders.values()),key=lambda o:(o.placed_at_ms,o.id)):
        if not live(env,order):continue
        ex=order.spec.exchange
        budget=remaining.get(ex,power)if mode==MODES[0]else power
        target=min(order.remaining,budget//unit_cost(env,order));resize(env,order,target)
        if mode==MODES[0]:remaining[ex]=budget-target*unit_cost(env,order)


def reported_quantity(env,order):
    record=None if env.order_history is None else env.order_history.records.get(order.id)
    filled=(record.filled if record is not None else sum(r['quantity']for r in env.api_fills if r['order']==order.id))
    return filled+order.remaining
