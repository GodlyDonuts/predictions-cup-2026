"""Exact ordinary money sensitivity, conditional on supplied categorical rules.

This is neither forecast calibration nor funding/execution certification. The
intervals are assumed inputs; their original clocks are not renewed. Exceptional
refund/fusion/replacement outcomes still require the separate full-world audit.
"""
from collections import defaultdict
from decimal import Decimal
from itertools import product

from .party_scenarios import PPM, canonical
from .sequence_audit import need


def _interval_groups(registry, marginals, profiles, diagnostics):
    """Intersect same-party bounds after unioning each source/profile interval."""
    byex = {r['exchangeId']: r for r in registry}
    need(0 < len(byex) == len(registry) <= 512, 'DUPLICATE_REGISTRY_EXCHANGE')
    intervals = {}
    for m in marginals:
        ex = m['exchangeId']; lo, hi = m['lowerPpm'], m['upperPpm']
        need(ex in byex and ex not in intervals and type(lo) is int
             and type(hi) is int and 0 <= lo <= hi <= PPM, 'MARGINAL_SCOPE_OR_INTERVAL')
        intervals[ex] = (lo, hi)
    need(set(intervals) == set(byex), 'COMPLETE_MARGINAL_SCOPE')
    for profile in profiles:
        seen = set()
        for m in profile.get('marginalOverrides', []):
            ex = m['exchangeId']; lo, hi = m['lowerPpm'], m['upperPpm']
            need(ex in intervals and ex not in seen and type(lo) is int
                 and type(hi) is int and 0 <= lo <= hi <= PPM, 'OVERRIDE_SCOPE_OR_INTERVAL')
            seen.add(ex); a, b = intervals[ex]; intervals[ex] = min(a, lo), max(b, hi)

    # Use the compiler's complete official-root event identity, not race-name
    # similarity or an external venue's market label.
    identities = defaultdict(set)
    for d in diagnostics:
        event = d['eventIdentity']
        need(all(k in event for k in ('raceId', 'stageId', 'electionDate',
                                     'raceName', 'officeLevel', 'usState')), 'COMPLETE_EVENT_IDENTITY')
        identities[(event['raceId'], event['electionDate'], event['raceName'])].add(canonical(event))
    groups = {}
    exchange_group = {}
    for ex, r in byex.items():
        matches = identities[r['raceId'], r['electionDate'], r['raceName']]
        need(len(matches) == 1, 'AMBIGUOUS_OR_MISSING_OFFICIAL_EVENT')
        event = next(iter(matches)); party = r['winnerName']
        need(type(party) is str and party, 'PARTY_IDENTITY')
        cell = groups.setdefault(event, {}).setdefault(party, {'lower': 0, 'upper': PPM, 'coefficient': 0})
        lo, hi = intervals[ex]; cell['lower'] = max(cell['lower'], lo); cell['upper'] = min(cell['upper'], hi)
        exchange_group[ex] = event, party

    for parties in groups.values():
        need(all(c['lower'] <= c['upper'] for c in parties.values())
             and sum(c['lower'] for c in parties.values()) <= PPM, 'INCOHERENT_CATEGORICAL_INTERVALS')
    return groups, exchange_group


def ordinary_probability_vertices(registry, marginals, profiles, diagnostics):
    """Exact interval/simplex vertices for the supported at-most-two parties."""
    groups, exchanges = _interval_groups(registry, marginals, profiles, diagnostics)
    result = {}
    for event, cells in groups.items():
        parties = sorted(cells); need(len(parties) <= 2, 'INTERVAL_VERTEX_PARTY_BOUND')
        candidates = set(product(*[(cells[p]['lower'], cells[p]['upper']) for p in parties]))
        if len(parties) == 2:
            for i in range(2):
                for fixed in (cells[parties[i]]['lower'], cells[parties[i]]['upper']):
                    v = [0, 0]; v[i] = fixed; v[1-i] = PPM - fixed; candidates.add(tuple(v))
        valid = [v for v in sorted(candidates) if sum(v) <= PPM
                 and all(cells[p]['lower'] <= n <= cells[p]['upper'] for p, n in zip(parties, v))]
        need(valid, 'INCOHERENT_CATEGORICAL_INTERVALS')
        result[event] = [{ex: v[parties.index(party)] for ex, (key, party) in exchanges.items() if key == event}
                         for v in valid]
    return result


def ordinary_money_bounds(registry, marginals, profiles, diagnostics, legs):
    """Min/max ordinary expected change versus holding, not a payoff guarantee.

    Exact greedy minimization over each categorical interval/simplex allows
    different events to move adversely in different directions. Other-party
    winners remain possible. Funding and exceptional outcomes are not certified.
    """
    groups, exchange_group = _interval_groups(registry, marginals, profiles, diagnostics)

    cash_change = 0
    for leg in legs:
        ex, side, action, n, price = (leg[k] for k in ('ex', 'side', 'action', 'quantity', 'price'))
        need(ex in exchange_group and side in ('YES', 'NO') and action in ('BUY', 'SELL')
             and type(n) is int and n > 0 and type(price) is int and 0 < price < PPM, 'LEG_SCOPE_OR_UNITS')
        sign = 1 if action == 'BUY' else -1
        cash_change -= sign * n * price
        if side == 'NO': cash_change += sign * n * PPM
        event, party = exchange_group[ex]
        groups[event][party]['coefficient'] += sign * n * (1 if side == 'YES' else -1)

    def extreme(parties, minimize):
        need(all(c['lower'] <= c['upper'] for c in parties.values())
             and sum(c['lower'] for c in parties.values()) <= PPM, 'INCOHERENT_CATEGORICAL_INTERVALS')
        masses = {p: c['lower'] for p, c in parties.items()}
        remaining = PPM - sum(masses.values())
        for party, cell in sorted(parties.items(), key=lambda v: (v[1]['coefficient'] * (1 if minimize else -1), v[0])):
            if cell['coefficient'] * (1 if minimize else -1) >= 0: continue
            add = min(remaining, cell['upper'] - masses[party]); masses[party] += add; remaining -= add
        return sum(parties[p]['coefficient'] * n for p, n in masses.items()), masses

    low, high, witnesses = cash_change, cash_change, []
    for event, parties in sorted(groups.items()):
        a, lo = extreme(parties, True); b, hi = extreme(parties, False); low += a; high += b
        if any(c['coefficient'] for c in parties.values()):
            witnesses.append({'eventIdentity': event.decode(), 'partyCoefficients': {p: c['coefficient'] for p, c in parties.items()},
                              'minimumMassesPpm': lo, 'maximumMassesPpm': hi,
                              'minimumOtherWinnerMassPpm': PPM - sum(lo.values())})
    return {'ordinaryMinimumExpectedChangeMicros': low, 'ordinaryMaximumExpectedChangeMicros': high,
            'ordinaryMinimumExpectedChange': format(Decimal(low) / PPM, '.6f'),
            'ordinaryMaximumExpectedChange': format(Decimal(high) / PPM, '.6f'),
            'eventWitnesses': witnesses, 'classification': 'conditional_ordinary_interval_sensitivity_vs_holding',
            'crossEventDependenceRequiredForMoneyExpectation': False, 'calibratedProbabilities': False,
            'exceptionalOutcomesCovered': False, 'fundingOrExecutionCertified': False,
            'originalClocksRenewed': False, 'executionAuthority': False}
