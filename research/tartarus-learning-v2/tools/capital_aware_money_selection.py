"""Compare audited direct trades and conditional reallocations; no execution."""
from decimal import Decimal


def score(candidate, rotation):
    sampled = Decimal(str(candidate['weakestDeclaredExpectedChange']))
    ordinary = Decimal(str(candidate['ordinaryMinimumExpectedChange'] if rotation else
        candidate['exactOrdinaryIntervalSensitivity']['ordinaryMinimumExpectedChange']))
    if not sampled.is_finite() or not ordinary.is_finite():
        raise ValueError('FINITE_MONEY_COMPARISON_REQUIRED')
    return min(sampled, ordinary)


def select(direct, rotations, ownership, minimum='0.01'):
    """Rank by the weaker sampled/interval gain, retaining stranded-sale risk.

    Rotation scores apply to the entire conditional plan. Selecting one does
    not make its first sale profitable or guarantee the later acquisition.
    Prefer a standalone trade on an equal score because it needs one fill.
    """
    floor = Decimal(minimum)
    if not floor.is_finite() or floor <= 0:
        raise ValueError('POSITIVE_MONEY_FLOOR_REQUIRED')
    eligible, deferred = [], []
    for rotation, candidates in ((False, direct), (True, rotations)):
        for candidate in candidates:
            legs = candidate['legs'] if rotation else [candidate['leg']]
            if rotation and (len(legs) < 2 or legs[0]['action'] != 'SELL'
                             or not any(l['action'] == 'BUY' for l in legs[1:])):
                raise ValueError('OWNED_SALE_AND_REPLACEMENT_REQUIRED')
            blocked = [leg for leg in legs if leg['action'] == 'BUY'
                       and ownership.get(leg['ex'], {}).get('side') != leg['side']]
            if blocked:
                deferred.append(dict(kind='rotation' if rotation else 'direct',
                    reason='VALUE_BUY_REQUIRES_ONGOING_REVIEWED_INVENTORY_MANAGEMENT',
                    legs=blocked))
                continue
            value = score(candidate, rotation)
            if value >= floor:
                eligible.append((value, not rotation, candidate))
    eligible.sort(key=lambda row: (row[0], row[1]), reverse=True)
    if not eligible:
        return None, deferred
    value, direct_trade, candidate = eligible[0]
    return dict(kind='direct' if direct_trade else 'rotation',
                comparisonGain=str(value), candidate=candidate,
                conditionalReplacementRequired=not direct_trade,
                actualFutureProceedsCredited=False, executionAuthority=False), deferred
