"""Install a pinned, causally available wallet in an independent simulator.

Original FIFO is stipulated starting state, never a new fill or a valuation.
Funded capital is a sizing denominator, not extra spendable cash.
"""
from datetime import datetime
from decimal import Decimal
import hashlib
import json
from pathlib import Path


def money(value):
    if not isinstance(value,str):raise ValueError('EXACT_WALLET_MONEY')
    n=Decimal(value)*1_000_000
    if not n.is_finite() or n<0 or n!=n.to_integral_value():raise ValueError('EXACT_WALLET_MONEY')
    return int(n)


def install_wallet(env,spec):
    raw=Path(spec['path']).read_bytes()
    if hashlib.sha256(raw).hexdigest()!=spec['sha256']:raise ValueError('HISTORICAL_WALLET_CHANGED')
    x=json.loads(raw)
    observed=int(datetime.fromisoformat(x['observedAt'].replace('Z','+00:00')).timestamp()*1000)
    if observed>env.now:raise ValueError('FUTURE_WALLET_NOT_AVAILABLE')
    if x['executionAuthority'] is not False or x['originalFifoBasisPreserved'] is not True:
        raise ValueError('EXPLICIT_RESEARCH_WALLET_REQUIRED')
    if x['liabilitiesMicros']!=0 or x['reservationsMicros']!=0:raise ValueError('ONLY_CASH_COVERED_QUIET_WALLET')
    if env.lots or env.orders or env.fill_events or env.budget.active or env.budget.unknown:
        raise ValueError('INITIAL_WALLET_ONLY')
    funded=x['fundedCapitalMicros']
    if type(funded)is not int or funded<=0 or funded!=env.initial_wealth:raise ValueError('EXACT_FUNDED_SIZING_ANCHOR')
    lots={};cost=0
    for ex,sides in x['positions'].items():
        for side,rows in sides.items():
            if side not in ['YES','NO']:raise ValueError('WALLET_SIDE')
            copied=[]
            for row in rows:
                n=row['quantity'];p=money(row['price'])
                if type(n)is not int or n<=0 or p>1_000_000:raise ValueError('WALLET_LOT')
                copied.append([n,p]);cost+=n*p
            if copied:lots[(ex,side)]=copied
    if cost!=money(x['ownedCost']):raise ValueError('WALLET_FIFO_COST_MISMATCH')
    env.cash=money(x['cash']);env.lots=lots
    return {'classification':'historical_actual_wallet_not_new_episode_fills_or_profit',
            'source_sha256':spec['sha256'],'observed_at_ms':observed,
            'cash_micros':env.cash,'owned_cost_micros':cost,'funded_sizing_anchor_micros':funded,
            'initial_marked_wealth_micros':None,'original_recorded_fifo_preserved':True,
            'initial_mark_change_is_profit':False,'execution_authority':False}
