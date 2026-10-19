"""Integer accounting for accepted simulated fills and ALL component debt.

This is not order admission, a topology discoverer or production collateral.
Callers must supply complete allowed joint outcomes from verified SIG rules.
Other-group draw priority is not specified by the saved OpenAPI: whenever it
matters it must be supplied explicitly, never inferred from a forecast/mark.
"""
from dataclasses import dataclass
from .contract import UNIT,TICK,MAX_QUANTITY
from moloch.learning_realism.protocol import valid_sha


def integer(value):return type(value)is int


@dataclass(frozen=True)
class Component:
    id:str
    exchanges:tuple
    outcomes:tuple
    relationship_ids:tuple
    scope:str
    source_sha256:str
    eligible:bool=True

    def __post_init__(self):
        if (not isinstance(self.id,str)or not self.id or not isinstance(self.scope,str)or not self.scope or
            not valid_sha(self.source_sha256)or type(self.eligible)is not bool):
            raise ValueError('EXPLICIT_COMPONENT_SCOPE_AND_EVIDENCE_REQUIRED')
        if (type(self.exchanges)is not tuple or not self.exchanges or
            any(type(x)is not str or not x for x in self.exchanges)or len(set(self.exchanges))!=len(self.exchanges)):
            raise ValueError('UNIQUE_COMPONENT_EXCHANGES_REQUIRED')
        if (type(self.outcomes)is not tuple or not self.outcomes or any(type(row)is not tuple or
            len(row)!=len(self.exchanges)or any(not integer(x)or x not in(0,1)for x in row)for row in self.outcomes)
            or len(set(self.outcomes))!=len(self.outcomes)):
            raise ValueError('EXPLICIT_COMPLETE_ALLOWED_BINARY_OUTCOMES_REQUIRED')
        if (type(self.relationship_ids)is not tuple or not self.relationship_ids or
            any(type(x)is not str or not x for x in self.relationship_ids)or len(set(self.relationship_ids))!=len(self.relationship_ids)):
            raise ValueError('ORIGINAL_SIG_RELATIONSHIP_IDENTITIES_REQUIRED')

    def floor(self,positions):
        quantities=[positions.get(ex,0)for ex in self.exchanges]
        if any(not integer(q)for q in quantities):raise ValueError('INTEGER_NET_POSITIONS_REQUIRED')
        return UNIT*min(sum(q*yes if q>=0 else -q*(1-yes)for q,yes in zip(quantities,row))for row in self.outcomes)


@dataclass(frozen=True)
class Fill:
    exchange:str
    side:str
    action:str
    quantity:int
    price:int

    def __post_init__(self):
        if (type(self.exchange)is not str or not self.exchange or self.side not in('YES','NO')or
            self.action not in('BUY','SELL')or not integer(self.quantity)or not 1<=self.quantity<=MAX_QUANTITY or
            not integer(self.price)or not TICK<=self.price<=UNIT-TICK or self.price%TICK):
            raise ValueError('VALID_CANONICAL_SIG_FILL_REQUIRED')


class CollateralLedger:
    def __init__(self,*,cash,positions,components,scope,outstanding=None):
        if not integer(cash)or type(scope)is not str or not scope:raise ValueError('EXPLICIT_INTEGER_CASH_AND_SCOPE')
        if any(type(ex)is not str or not ex or not integer(q)for ex,q in positions.items()):raise ValueError('INTEGER_NET_POSITIONS_REQUIRED')
        components=tuple(components)
        if any(type(c)is not Component or c.scope!=scope for c in components):raise ValueError('ONE_VERIFIED_SIG_COLLATERAL_SCOPE')
        if len({c.id for c in components})!=len(components):raise ValueError('UNIQUE_COMPONENT_IDENTITIES_REQUIRED')
        covered=[ex for c in components for ex in c.exchanges]
        if len(set(covered))!=len(covered):raise ValueError('OVERLAPPING_COMPONENTS_WOULD_DOUBLE_COUNT_COLLATERAL')
        self.components={c.id:c for c in components};self.owner={ex:c.id for c in components for ex in c.exchanges}
        debt=dict(outstanding or{})
        if any(k not in self.components or not integer(v)or v<0 for k,v in debt.items()):raise ValueError('KNOWN_NONNEGATIVE_COMPONENT_DEBT_REQUIRED')
        self.debt={k:debt.get(k,0)for k in self.components};self.cash=cash;self.positions=dict(positions);self.scope=scope;self.events={}

    def floors(self,positions=None):return {k:c.floor(self.positions if positions is None else positions)for k,c in self.components.items()}

    def available(self,positions=None,debt=None):
        floors=self.floors(positions);debt=self.debt if debt is None else debt
        return {k:max(0,floors[k]-debt[k])if c.eligible else 0 for k,c in self.components.items()}

    def buying_power(self):return max(self.cash,0)+sum(self.available().values())

    def equity(self,yes_marks):
        # Principal is a liability, not trading income. Marks are an explicit
        # caller convention, never a substitute for the component payout floor.
        if any(ex not in yes_marks or not integer(yes_marks[ex])or not 0<=yes_marks[ex]<=UNIT for ex,q in self.positions.items()if q):
            raise ValueError('COMPLETE_INTEGER_SIG_VALUATION_REQUIRED')
        return self.cash-sum(self.debt.values())+sum(q*yes_marks[ex]if q>=0 else -q*(UNIT-yes_marks[ex])for ex,q in self.positions.items()if q)

    def apply(self,event_id,fill,*,other_draw_order=None):
        if type(event_id)is not str or not event_id or type(fill)is not Fill:raise ValueError('IMMUTABLE_FILL_IDENTITY_REQUIRED')
        if other_draw_order is not None and(type(other_draw_order)is not tuple or
            any(type(x)is not str for x in other_draw_order)or len(set(other_draw_order))!=len(other_draw_order)):
            raise ValueError('EXPLICIT_OTHER_COMPONENT_DRAW_PRIORITY')
        signature=(fill,other_draw_order)
        if event_id in self.events:
            old,result=self.events[event_id]
            if old!=signature:raise ValueError('COLLATERAL_EVENT_IDENTITY_CONFLICT')
            return dict(result)
        own=self.owner.get(fill.exchange)
        if other_draw_order is not None and(set(other_draw_order)!=(set(self.components)-{own})):
            raise ValueError('EXACT_OTHER_COMPONENT_DRAW_PRIORITY_REQUIRED')
        q=self.positions.get(fill.exchange,0);sign=1 if fill.side=='YES'else -1
        canonical=fill
        if fill.action=='SELL'and q*sign<fill.quantity:
            canonical=Fill(fill.exchange,'NO'if fill.side=='YES'else'YES','BUY',fill.quantity,UNIT-fill.price)
            sign=-sign
        positions=dict(self.positions);delta=canonical.quantity*sign*(1 if canonical.action=='BUY'else -1)
        positions[fill.exchange]=q+delta
        redemption=(min(abs(q),canonical.quantity)*UNIT if canonical.action=='BUY'and q*sign<0 else 0)
        notional=canonical.price*canonical.quantity;cash=self.cash;debt=dict(self.debt)
        before_power=self.buying_power();floors=self.floors(positions);draws={};repayment=0
        if own is not None:
            repayment=max(0,debt[own]-floors[own]);debt[own]-=repayment;cash-=repayment
        if canonical.action=='BUY':
            available=self.available(positions,debt)
            own_draw=min(notional,available.get(own,0));remaining=notional-own_draw
            if own_draw:debt[own]+=own_draw;draws[own]=own_draw
            # Redemptions belong to this accepted fill's accounting, not a
            # promise that a zero-buying-power order would have been admitted.
            cash+=redemption;from_cash=min(max(cash,0),remaining);remaining-=from_cash
            if remaining and sum(v for k,v in available.items()if k!=own):
                if other_draw_order is None:raise ValueError('CROSS_COMPONENT_DRAW_ORDER_UNCALIBRATED')
                for k in other_draw_order:
                    take=min(remaining,available[k]);debt[k]+=take;remaining-=take
                    if take:draws[k]=take
                    if not remaining:break
            if remaining:raise ValueError('ACCEPTED_FILL_CANNOT_BE_FUNDED')
            cash+=sum(draws.values())-notional
        else:cash+=notional
        after_power=max(cash,0)+sum(self.available(positions,debt).values())
        result={'canonical':canonical,'fullNotionalCost':notional,'effectiveEntryCost':notional-sum(draws.values()),
            'collateralSavings':sum(draws.values()),'collateralRepayment':repayment,'redemptionCredit':redemption,
            'componentId':own,'guaranteedPayoutFloorAfter':floors.get(own),
            'outstandingAdvanceAfter':debt.get(own),'netBuyingPowerImpact':before_power-after_power,
            'componentDraws':tuple(sorted(draws.items())),'cashDelta':cash-self.cash,
            'totalDebtAfter':sum(debt.values())}
        self.cash=cash;self.positions=positions;self.debt=debt;self.events[event_id]=(signature,dict(result))
        return dict(result)
