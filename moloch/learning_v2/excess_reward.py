"""Exact matched-HOLD reward; no change to wallets, marks or execution.

This is a research training objective, not realized profit. The initial account
is copied once. Unknown current or HOLD marks remain unavailable; later valid
observations include the whole intervening change instead of a fabricated zero.
"""
from copy import deepcopy
from dataclasses import dataclass
from .account import wealth


@dataclass(frozen=True)
class RewardObservation:
    available:bool
    normalized_increment:float
    excess_wealth_micros:int|None
    hold_wealth_micros:int|None


class MatchedHoldReward:
    def __init__(self,initial_account,initial_wealth_micros):
        if type(initial_wealth_micros)is not int or initial_wealth_micros<=0:raise ValueError('POSITIVE_INITIAL_WEALTH_REQUIRED')
        self._initial=deepcopy(initial_account)
        self._denominator=initial_wealth_micros
        self._previous_excess=0

    def observe(self,actual_wealth_micros,marks):
        hold=wealth(self._initial,marks)
        if actual_wealth_micros is None or hold is None:return RewardObservation(False,0.,None,hold)
        if type(actual_wealth_micros)is not int:raise ValueError('EXACT_ACTUAL_WEALTH_REQUIRED')
        excess=actual_wealth_micros-hold;increment=excess-self._previous_excess
        self._previous_excess=excess
        return RewardObservation(True,increment/self._denominator,excess,hold)
