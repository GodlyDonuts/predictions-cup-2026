"""Causal synthetic starting inventory, never a historical or official fill.

Only current recorded books/marks determine eligibility; random side/market
selection is independent of subsequent prices. The evaluator must reset its
initial marked-wealth denominator after installation, before scoring returns.
"""
import random
from .account import UNIT


def seed_inventory(env, allocation_ppm, seed, *, selection='shuffle'):
    if type(allocation_ppm)is not int or not 0<=allocation_ppm<=UNIT:
        raise ValueError('EXACT_INITIAL_ALLOCATION')
    if env.lots or env.orders or env.budget.active or env.budget.unknown:
        raise ValueError('ONLY_UNENCUMBERED_INITIAL_STATE')
    if selection not in ['shuffle','current_depth_capacity']:raise ValueError('INITIAL_SELECTION_RULE')
    initial_cash=env.cash;limit=env.mechanics.limits;marks=env.marks()
    target=min(initial_cash*allocation_ppm//UNIT,limit.maximum_committed)
    rng=random.Random(seed);exchanges=sorted(env.books);rng.shuffle(exchanges)
    sides={};capacities={}
    if selection=='current_depth_capacity':
        for ex in exchanges:
            q=env.books[ex];side=rng.choice(['YES','NO']);sides[ex]=side
            if not q.visible(env.now,env.mechanics.maximum_book_age_ms)or (ex,side)not in marks:continue
            levels=q.book.asks if side=='YES'else q.book.bids
            if not levels:continue
            yes_price,depth=levels[0];price=yes_price if side=='YES'else UNIT-yes_price
            if 0<price<UNIT:capacities[ex]=min(depth*price,limit.maximum_market_cost)
        # Capacity is known at initialization, with seeded ties. No subsequent
        # price, outcome or policy score enters this initial-state selection.
        exchanges.sort(key=lambda ex:capacities.get(ex,0),reverse=True)
    rows=[];spent=0;owned_cost=0
    for ex in exchanges:
        if owned_cost>=target:break
        if limit.maximum_fresh_markets is not None and len(rows)>=limit.maximum_fresh_markets:break
        q=env.books[ex]
        if not q.visible(env.now,env.mechanics.maximum_book_age_ms):continue
        side=sides[ex]if selection=='current_depth_capacity'else rng.choice(['YES','NO']);mark=marks.get((ex,side))
        if mark is None or not 0<mark<UNIT:continue
        levels=q.book.asks if side=='YES'else q.book.bids
        if not levels:continue
        yes_price,depth=levels[0];price=yes_price if side=='YES'else UNIT-yes_price
        if not 0<price<UNIT:continue
        quantity=min(depth,(target-owned_cost)//price,limit.maximum_market_cost//price,
                     (initial_cash-spent)//(price+limit.fee_per_share))
        if quantity<=0:continue
        cost=quantity*price;fee=quantity*limit.fee_per_share
        # Positions are stipulated initial state at a conservative current ask,
        # not an instantaneous synthetic trade. Do not invent receipts, consume
        # displayed liquidity or mutate source snapshots/limiter counters.
        env.lots[(ex,side)]=[[quantity,price]];spent+=cost+fee;owned_cost+=cost
        rows.append({'exchange_id':ex,'side':side,'quantity':quantity,'basis_price_micros':price,
                     'cost_micros':cost,'stipulated_entry_fee_micros':fee,
                     'causal_book_sha256':q.book.original_sha256,'source_book_available_at_ms':q.book.available_at_ms,
                     'mark_micros_at_initialization':mark})
    env.cash=initial_cash-spent
    initial_wealth=env.marked_wealth()
    if initial_wealth is None or initial_wealth<=0:raise ValueError('INITIAL_MARKED_WEALTH_UNAVAILABLE')
    return {'classification':'causal_synthetic_initial_portfolio_not_historical_or_actual_fills',
            'at_ms':env.now,'seed':seed,'desired_allocation_ppm':allocation_ppm,
            'selection_rule':selection,
            'initial_cash_before_micros':initial_cash,'initial_cash_after_micros':env.cash,
            'owned_cost_micros':owned_cost,'initial_marked_wealth_micros':initial_wealth,
            'initial_mark_change_is_profit':False,'initial_lots':rows,
            'source_depth_scaled':False,'book_or_limiter_mutated':False,
            'initial_position_is_not_single_order':True,'execution_authority':False}
