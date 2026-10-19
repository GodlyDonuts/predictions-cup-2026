"""Exact ledger valuation and the human's growth/evidence reward.

SIG midpoint wealth is a declared marking convention, not executable proceeds,
settlement value, dashboard parity or production ROI. No external contract is
assumed equivalent. Unknown SIG marks remain missing, never cost basis or zero.
"""
from dataclasses import dataclass
import math
from moloch.learning_realism.protocol import DAY,growth_score
from .contract import UNIT


@dataclass(frozen=True)
class Wealth:
    at_ms:int
    twice_micros:int
    position_values_twice:dict
    convention:str='causal_SIG_midpoint_not_executable_liquidation'
    stale_exchanges:tuple=()


def snapshot(env,maximum_mark_age_ms=120000,*,historical_last_mark=False):
    if type(maximum_mark_age_ms)is not int or maximum_mark_age_ms<=0:
        raise ValueError('POSITIVE_MARK_AGE_REQUIRED')
    values={};stale=[]
    for (exchange,side),lots in env.lots.items():
        quantity=sum(n for n,_ in lots)
        if not quantity:continue
        world=env.books.get(exchange)
        if world is None:
            raise ValueError('INCOMPLETE_SIG_WEALTH_MARK:'+exchange)
        if not world.visible(env.now,maximum_mark_age_ms):
            if not historical_last_mark:raise ValueError('INCOMPLETE_SIG_WEALTH_MARK:'+exchange)
            stale.append(exchange)
        book=world.book
        if not book.bids or not book.asks:
            raise ValueError('TWO_SIDED_SIG_WEALTH_MARK_REQUIRED:'+exchange)
        # Twice-micro accounting preserves half-micro midpoints exactly.
        price=book.bids[0][0]+book.asks[0][0]
        if side=='NO':price=2*UNIT-price
        values[exchange]=values.get(exchange,0)+quantity*price
    convention=('last_observed_SIG_midpoint_with_explicit_staleness_not_current_wealth'
                if historical_last_mark else 'causal_SIG_midpoint_not_executable_liquidation')
    return Wealth(env.now,2*env.cash+sum(values.values()),values,convention,tuple(sorted(stale)))


class GrowthLedger:
    """Whole consecutive 24-hour blocks, continuous cash/inventory across days.

    All contracts share one conservative event cluster by default. This cannot
    inflate independent opportunities through fill splitting or many correlated
    markets. A finer map requires a pinned external economic grouping.
    """
    def __init__(self,env,*,maximum_mark_age_ms=120000,clusters=None,cluster_evidence=None,
                 development=False,historical_last_mark=False):
        if clusters is not None and not cluster_evidence:
            raise ValueError('PINNED_ECONOMIC_CLUSTER_EVIDENCE_REQUIRED')
        self.env=env;self.maximum_mark_age_ms=maximum_mark_age_ms
        self.clusters=clusters;self.cluster_evidence=cluster_evidence
        if historical_last_mark and not development:
            raise ValueError('STALE_HISTORICAL_MARKING_IS_DEVELOPMENT_ONLY')
        self.development=development;self.historical_last_mark=historical_last_mark
        self.stale_exchanges=set();self.stale_observations=0
        self.start=self._value(env)
        self.previous=self.start;self.boundaries=[self.start];self.reward_sum=0.
        self.closed=False
        self.days=(env.end-env.now)//DAY
        if not development and ((env.end-env.now)%DAY or self.days<2):
            raise ValueError('AT_LEAST_TWO_COMPLETE_CONSECUTIVE_DAYS_REQUIRED')
        self.initial_fill_count=len(env.api_fills)
        for at in range(env.now+DAY,env.end+1,DAY):
            env.clock.schedule(at,40,'economic_boundary',self)

    def _value(self,env):
        value=snapshot(env,self.maximum_mark_age_ms,historical_last_mark=self.historical_last_mark)
        self.stale_exchanges.update(value.stale_exchanges)
        self.stale_observations+=bool(value.stale_exchanges)
        return value

    def capture_boundary(self,env,at):
        value=self._value(env)
        if value.at_ms!=at:raise ValueError('EXACT_DAILY_BOUNDARY_CLOCK_REQUIRED')
        self.boundaries.append(value)

    def transition(self):
        if self.closed:raise ValueError('REWARD_LEDGER_ALREADY_FINALIZED')
        current=self._value(self.env)
        if current.twice_micros<=0 or self.previous.twice_micros<=0:
            raise ValueError('BANKRUPT_OR_NONPOSITIVE_WEALTH')
        reward=math.log(current.twice_micros/self.previous.twice_micros)/max(1,self.days)
        self.previous=current;self.reward_sum+=reward
        return reward

    def _opportunities(self,terminal):
        values={ex:-value for ex,value in self.start.position_values_twice.items()}
        for ex,value in terminal.position_values_twice.items():values[ex]=values.get(ex,0)+value
        for fill in self.env.api_fills[self.initial_fill_count:]:
            if 'cash_delta_micros'not in fill:raise ValueError('EXACT_FILL_CASH_DELTA_REQUIRED')
            ex=fill['exchange'];values[ex]=values.get(ex,0)+2*fill['cash_delta_micros']
        grouped={}
        for ex,pnl in values.items():
            key='whole_competition_correlated_cluster'if self.clusters is None else self.clusters.get(ex)
            if not key:raise ValueError('UNCLASSIFIED_ECONOMIC_EVENT:'+ex)
            grouped[key]=grouped.get(key,0)+pnl
        return list(grouped.items())

    def finish(self):
        if self.closed:raise ValueError('REWARD_LEDGER_ALREADY_FINALIZED')
        if self.env.now!=self.env.end:raise ValueError('WHOLE_EVALUATION_HORIZON_REQUIRED')
        terminal=self._value(self.env)
        if min(self.start.twice_micros,terminal.twice_micros)<=0:
            raise ValueError('BANKRUPT_OR_NONPOSITIVE_WEALTH')
        if self.development:
            reward=math.log(terminal.twice_micros/self.start.twice_micros)
            result={'reward':reward,'D':None,'N':None,'eligible':False,
                    'classification':'partial_history_development_log_wealth_not_daily_growth_qualification'}
        else:
            if len(self.boundaries)!=self.days+1 or self.boundaries[-1].at_ms!=self.env.end:
                raise ValueError('COMPLETE_DAILY_BOUNDARY_WEALTH_REQUIRED')
            rows=[(a.twice_micros,b.twice_micros)for a,b in zip(self.boundaries,self.boundaries[1:])]
            result=growth_score(rows,self._opportunities(terminal))
            if result['reward']is None:raise ValueError('INELIGIBLE_GROWTH_SCORE')
        correction=result['reward']-self.reward_sum
        self.closed=True
        return {**result,'terminal_reward_correction':correction,
            'initial_wealth_twice_micros':self.start.twice_micros,
            'terminal_wealth_twice_micros':terminal.twice_micros,
            'marked_roi':terminal.twice_micros/self.start.twice_micros-1,
            'valuation':terminal.convention,'fees_micros':self.env.fees,
            'realized_fifo_micros':self.env.realized,
            'fill_count':len(self.env.api_fills)-self.initial_fill_count,
            'stale_mark_observations':self.stale_observations,
            'stale_mark_exchanges':sorted(self.stale_exchanges),
            'cluster_evidence':self.cluster_evidence or 'all_contracts_one_conservative_correlated_cluster',
            'depth_executable_roi_established':False,'production_profitability_established':False}
