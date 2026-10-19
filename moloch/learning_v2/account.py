"""Exact sizing boundary; floats belong only to the policy observation.

Amounts and prices use integer millionths. A target is a marked position,
not a recurring purchase budget. Pending buys count toward the target while
their cash remains reserved. Pending sales never create spendable cash.
"""
from dataclasses import dataclass
from typing import Mapping

UNIT = 1_000_000


def natural(value: int, name: str) -> int:
    if type(value) is not int or value < 0:
        raise ValueError(name)
    return value


@dataclass(frozen=True)
class Limits:
    maximum_order_quantity: int
    maximum_committed: int
    maximum_market_cost: int
    maximum_position_quantity: int | None = None
    fee_per_share: int = 0
    maximum_fresh_markets: int | None = None
    inherited_exchanges: frozenset[str] = frozenset()

    def __post_init__(self):
        for k in ['maximum_order_quantity', 'maximum_committed', 'maximum_market_cost', 'fee_per_share']:
            natural(getattr(self, k), k)
        if self.maximum_position_quantity is not None:
            natural(self.maximum_position_quantity, 'maximum_position_quantity')
        if self.maximum_fresh_markets is not None:
            natural(self.maximum_fresh_markets, 'maximum_fresh_markets')


@dataclass(frozen=True)
class Account:
    cash: int
    reserved_cash: int
    other_cash_encumbrances: int
    liabilities: int
    owned_cost: int
    reserved_commitment: int
    quantities: Mapping[tuple[str, str], int]
    market_cost: Mapping[str, int]
    pending_buys: Mapping[tuple[str, str], int]
    pending_sells: Mapping[tuple[str, str], int]
    pending_market_cost: Mapping[str, int]

    def __post_init__(self):
        for k in ['cash', 'reserved_cash', 'other_cash_encumbrances', 'liabilities', 'owned_cost', 'reserved_commitment']:
            natural(getattr(self, k), k)
        for name in ['quantities', 'market_cost', 'pending_buys', 'pending_sells', 'pending_market_cost']:
            for value in getattr(self, name).values():
                natural(value, name)
        if self.reserved_cash + self.other_cash_encumbrances > self.cash:
            raise ValueError('ENCUMBRANCES_EXCEED_CASH')
        if any(n > self.quantities.get(key, 0) for key, n in self.pending_sells.items()):
            raise ValueError('UNCOVERED_PENDING_SALE')
        for (ex, side), held in self.quantities.items():
            opposite = 'NO' if side == 'YES' else 'YES'
            if self.pending_sells.get((ex,side),0)+min(held,self.pending_buys.get((ex,opposite),0)) > held:
                raise ValueError('SAME_SHARES_PLEDGED_TO_SELL_AND_OPPOSITE_BUY')

    @property
    def spendable(self) -> int:
        return self.cash - self.reserved_cash - self.other_cash_encumbrances


def wealth(account: Account, marks: Mapping[tuple[str, str], int]) -> int | None:
    """Unknown marks propagate; reservations are restricted cash, not a loss."""
    total = account.cash - account.liabilities
    for key, n in account.quantities.items():
        if not n:
            continue
        p = marks.get(key)
        if type(p) is not int or not 0 <= p <= UNIT:
            return None
        total += n * p
    return total if total > 0 else None


def observation(account: Account, marks: Mapping[tuple[str, str], int], target: int) -> dict:
    natural(target, 'TARGET')
    w = wealth(account, marks)
    if w is None:
        return {'available': False, 'reason': 'WEALTH_REFERENCE_UNAVAILABLE'}
    return {'available': True, 'wealth_micros': w, 'cash_fraction': account.cash / w,
            'spendable_fraction': account.spendable / w,
            'reservation_fraction': account.reserved_cash / w,
            'liability_fraction': account.liabilities / w, 'target_ratio': target / w,
            'position_fractions': {key: n * marks[key] / w
                                   for key, n in account.quantities.items() if n}}


@dataclass(frozen=True)
class Target:
    exchange: str
    side: str
    target_fraction_ppm: int
    child_budget_fraction_ppm: int
    limit_price: int
    action: str  # BUY or SELL; policy can separately choose WAIT/CANCEL.

    def __post_init__(self):
        if self.side not in ['YES', 'NO'] or self.action not in ['BUY', 'SELL']:
            raise ValueError('ACTION_OR_SIDE')
        for k in ['target_fraction_ppm', 'child_budget_fraction_ppm']:
            if type(getattr(self, k)) is not int or not 0 <= getattr(self, k) <= UNIT:
                raise ValueError(k)
        if type(self.limit_price) is not int or not 0 < self.limit_price < UNIT:
            raise ValueError('LIMIT_PRICE')


@dataclass(frozen=True)
class Projection:
    requested_quantity: int
    admitted_quantity: int
    maximum_cost: int
    constraints: tuple[str, ...]


def project(account: Account, marks: Mapping[tuple[str, str], int], request: Target,
            limits: Limits, available_depth: int | None = None, *,
            sizing_capital_micros: int | None = None) -> Projection:
    """Physical projection only: no predictor, alpha threshold or strategy menu.

    Depth is optional for a resting limit request. A simulator must independently
    match against shared liquidity at arrival; this function does not fill it.
    """
    # An explicit funded-capital denominator permits partial observability.
    # It is a sizing convention, never a replacement mark or reported wealth.
    w = wealth(account, marks)
    if sizing_capital_micros is not None:
        natural(sizing_capital_micros, 'SIZING_CAPITAL')
        if not sizing_capital_micros: raise ValueError('POSITIVE_SIZING_CAPITAL')
        w = sizing_capital_micros
    key = (request.exchange, request.side)
    mark = marks.get(key)
    exiting = sizing_capital_micros is not None and request.action == 'SELL' and request.target_fraction_ppm == 0
    if w is None or (not exiting and (type(mark) is not int or not 0 < mark <= UNIT)):
        return Projection(0, 0, 0, ('WEALTH_OR_SIDE_MARK_UNAVAILABLE',))
    if available_depth is not None:
        natural(available_depth, 'DEPTH')
    current = account.quantities.get(key, 0)
    opposite_key = (request.exchange, 'NO' if request.side == 'YES' else 'YES')
    opposite_current = account.quantities.get(opposite_key,0)
    desired = 0 if exiting else (request.target_fraction_ppm * w // UNIT) // mark
    effective = (current-opposite_current+account.pending_buys.get(key,0)-account.pending_buys.get(opposite_key,0)
                 -account.pending_sells.get(key,0)+account.pending_sells.get(opposite_key,0))
    requested = max(0, desired - effective if request.action == 'BUY' else effective - desired)
    budget = request.child_budget_fraction_ppm * w // UNIT
    bounds = {'TARGET': requested, 'ORDER_QUANTITY': limits.maximum_order_quantity}
    if request.action == 'BUY':
        per_share = request.limit_price + limits.fee_per_share
        bounds.update(CHILD_BUDGET=budget // per_share, CASH=account.spendable // per_share,
                      COMMITMENT=max(0, limits.maximum_committed - account.owned_cost - account.reserved_commitment) // per_share,
                      MARKET_COST=max(0, limits.maximum_market_cost - account.market_cost.get(request.exchange, 0)
                                      - account.pending_market_cost.get(request.exchange, 0)) // per_share)
        if limits.maximum_position_quantity is not None:
            bounds['POSITION_QUANTITY'] = max(0, limits.maximum_position_quantity - current - account.pending_buys.get(key, 0))
        if account.pending_sells.get(opposite_key,0):
            bounds['OPPOSITE_SHARE_PLEDGE'] = max(0,opposite_current-account.pending_sells[opposite_key]
                                                 -min(opposite_current,account.pending_buys.get(key,0)))
        if limits.maximum_fresh_markets is not None:
            active = {ex for (ex, side), n in account.quantities.items() if n}
            active |= {ex for (ex, side), n in account.pending_buys.items() if n}
            fresh = active-limits.inherited_exchanges
            if request.exchange not in active and request.exchange not in limits.inherited_exchanges and len(fresh) >= limits.maximum_fresh_markets:
                bounds['MARKET_SLOTS'] = 0
    else:
        bounds['COVERED'] = current-account.pending_sells.get(key,0)-min(current,account.pending_buys.get(opposite_key,0))
        bounds['CHILD_BUDGET'] = budget // request.limit_price
    if available_depth is not None:
        bounds['DISPLAYED_DEPTH'] = available_depth
    admitted = min(bounds.values())
    cost = admitted * (request.limit_price + limits.fee_per_share) if request.action == 'BUY' else 0
    return Projection(requested, admitted, cost, tuple(k for k, n in bounds.items() if n == admitted))
