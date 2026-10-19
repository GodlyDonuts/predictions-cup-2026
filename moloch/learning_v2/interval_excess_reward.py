"""Exact HOLD-relative wealth intervals, not executable or realized profit.

One YES probability values both complementary sides of each SIG contract.
External bid/ask ranges are conditional valuation assumptions, not proof that
SIG will converge or that contracts resolve identically. Cash is never changed.
"""
from copy import deepcopy
from dataclasses import dataclass
from .account import UNIT


@dataclass(frozen=True)
class IntervalRewardObservation:
    available:bool
    normalized_lower_increment:float
    lower_excess_micros:int|None
    upper_excess_micros:int|None


class IntervalHoldReward:
    def __init__(self,initial_account,capital_scale_micros):
        if type(capital_scale_micros)is not int or capital_scale_micros<=0:raise ValueError('EXACT_POSITIVE_CAPITAL_SCALE')
        self._initial=deepcopy(initial_account);self._scale=capital_scale_micros;self._previous_lower=0

    def observe(self,actual,yes_intervals):
        original=self._initial
        # Reserved cash is already part of cash. Subtracting it again would
        # manufacture losses when a quote is placed and gains on cancellation.
        lower=upper=(actual.cash-actual.other_cash_encumbrances-actual.liabilities
                     -original.cash+original.other_cash_encumbrances+original.liabilities)
        exchanges={e for e,s in set(actual.quantities)|set(original.quantities)}
        for ex in exchanges:
            dy=actual.quantities.get((ex,'YES'),0)-original.quantities.get((ex,'YES'),0)
            dn=actual.quantities.get((ex,'NO'),0)-original.quantities.get((ex,'NO'),0)
            if type(dy)is not int or type(dn)is not int:raise ValueError('EXACT_POSITION_QUANTITIES')
            lower+=dn*UNIT;upper+=dn*UNIT;net=dy-dn
            if not net:continue
            if ex not in yes_intervals:return IntervalRewardObservation(False,0.,None,None)
            bid,ask=yes_intervals[ex]
            if type(bid)is not int or type(ask)is not int or not 0<=bid<=ask<=UNIT:raise ValueError('ORDERED_EXACT_YES_INTERVAL')
            lower+=net*(bid if net>0 else ask);upper+=net*(ask if net>0 else bid)
        increment=lower-self._previous_lower;self._previous_lower=lower
        return IntervalRewardObservation(True,increment/self._scale,lower,upper)
