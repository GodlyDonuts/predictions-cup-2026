"""Contract C: declared joint opposing-field thresholds, never private holdings.

build_models(observation, scenario_ids, case_specs) -> list[dict].
Observation: Contract metadata + source_url, published_at_ms, participant_count
(includes us), our_participant_id, observed_top[{participant_id, wealth_micros
OR wealth_low_micros/wealth_high_micros}], unobserved_field_statement.
Case: id, kind(flat|growing|factor_correlated|adverse_overtaking), assumption_label,
decision_ms, projection_at_ms, unknown_envelope_micros {scenario_id: maximum}.
For unknown opponents this envelope is REQUIRED and is an assumption, not a cap
established by seeing the top128. Optional unknown_top3_micros maps each scenario
to min(3, unknown_count) descending values. growing adds exact ceiling of a
nonnegative growth_micros_per_day over publication-to-projection time. factor
uses scenario_factors {factor:{scenario_id:signed micros}} and factor_loadings_ppm
{factor:signed ppm}, with optional participant_loadings_ppm overrides. adverse
requires overtaking_margin_micros {scenario_id:nonnegative micros}, applied above
the known opposing maximum as an additional unknown-field assumption.

Stale research requires explicit allow_stale_observation=True, remains marked
stale, and never becomes a current leaderboard. Missing ties/prizes stay None.
Public marked wealth is not terminal wealth: even flat projection is an explicit
terminal-threshold assumption. A byte digest authenticates neither the publisher
nor a probability model. Source metadata and all case parameters remain attached.
All scenario mappings must match the declared scenario IDs exactly. No sampling,
independent shuffling, inferred portfolio, API, authority or calibration.
"""
import re

LIMIT = (1 << 127) - 1
DAY_MS = 86_400_000
MAX_SCENARIOS = 4096

class Unavailable(ValueError):
    pass


def require(ok, code):
    if not ok:
        raise Unavailable(code)


def integer(value, code, minimum=-LIMIT, maximum=LIMIT):
    require(type(value) is int and minimum <= value <= maximum, code)
    return value


def atom(value, code):
    require(isinstance(value, str) and 0 < len(value) <= 256
            and not any(ord(c) < 32 for c in value), code)
    return value


def vector(mapping, ids, code, minimum=-LIMIT):
    require(isinstance(mapping, dict) and set(mapping) == set(ids), code)
    return [integer(mapping[s], code, minimum) for s in ids]


def ceil_ratio(numerator, denominator):
    return -(-numerator // denominator)


def _metadata(observation):
    require(type(observation.get('schema_version')) is int and observation['schema_version'] == 1, 'SCHEMA')
    for name in ('as_of_ms', 'valid_until_ms'):
        integer(observation.get(name), 'SOURCE_CLOCK', 0, (1 << 63)-1)
    published = observation.get('published_at_ms')
    if published is not None:
        integer(published, 'SOURCE_CLOCK', 0, observation['as_of_ms'])
    require(observation['as_of_ms'] <= observation['valid_until_ms'], 'SOURCE_CLOCK')
    require(isinstance(observation.get('source_sha256'), str) and
            re.fullmatch('[0-9a-f]{64}', observation['source_sha256']), 'SOURCE_SHA')
    atom(observation.get('evidence_class'), 'EVIDENCE_CLASS')
    url = observation.get('source_url')
    require(isinstance(url, str) and url.startswith('https://') and len(url) <= 4096,
            'PUBLIC_SOURCE_URL')
    atom(observation.get('unobserved_field_statement'), 'UNOBSERVED_STATEMENT')


def build_models(observation, scenario_ids, case_specs):
    """Create canonical scenario-aligned declared models; absence is an error."""
    require(isinstance(observation, dict), 'OBSERVATION')
    _metadata(observation)
    require(isinstance(scenario_ids, list) and 1 <= len(scenario_ids) <= MAX_SCENARIOS,
            'SCENARIOS')
    ids = [atom(s, 'SCENARIO_ID') for s in scenario_ids]
    require(len(set(ids)) == len(ids), 'DUPLICATE_SCENARIO')
    participants = integer(observation.get('participant_count'), 'PARTICIPANT_COUNT', 2, 100000)
    opponents = participants - 1
    ours = atom(observation.get('our_participant_id'), 'OUR_ID')
    rows = observation.get('observed_top')
    require(isinstance(rows, list) and 1 <= len(rows) <= min(opponents, 4096), 'OBSERVED_ROWS')
    observed = []
    seen = set()
    for row in rows:
        require(isinstance(row, dict), 'OBSERVED_ROW')
        identity = atom(row.get('participant_id'), 'PARTICIPANT_ID')
        require(identity != ours and identity not in seen, 'DUPLICATE_OR_SELF')
        seen.add(identity)
        if 'wealth_micros' in row:
            require('wealth_low_micros' not in row and 'wealth_high_micros' not in row,
                    'AMBIGUOUS_WEALTH')
            low = high = integer(row['wealth_micros'], 'OBSERVED_WEALTH')
        else:
            low = integer(row.get('wealth_low_micros'), 'OBSERVED_WEALTH')
            high = integer(row.get('wealth_high_micros'), 'OBSERVED_WEALTH')
            require(low <= high, 'WEALTH_INTERVAL')
        observed.append((identity, low, high))
    unknown = opponents - len(observed)
    require(isinstance(case_specs, list) and 1 <= len(case_specs) <= 64, 'CASES')
    require(len(observed) * len(ids) * len(case_specs) <= 2000000, 'WORK_BOUND')
    result, names = [], set()
    for case in case_specs:
        require(isinstance(case, dict), 'CASE')
        name = atom(case.get('id'), 'MODEL_ID')
        require(name not in names, 'DUPLICATE_MODEL')
        names.add(name)
        label = atom(case.get('assumption_label'), 'ASSUMPTION_LABEL')
        kind = case.get('kind')
        require(kind in ('flat', 'growing', 'factor_correlated', 'adverse_overtaking'), 'KIND')
        decision = integer(case.get('decision_ms'), 'DECISION_CLOCK', 0, (1 << 63)-1)
        horizon = integer(case.get('projection_at_ms'), 'HORIZON_CLOCK', decision, (1 << 63)-1)
        require(observation['as_of_ms'] <= decision, 'SOURCE_NOT_AVAILABLE')
        stale = decision > observation['valid_until_ms']
        allow_stale = case.get('allow_stale_observation', False)
        require(type(allow_stale) is bool and (not stale or allow_stale), 'STALE_OBSERVATION')
        envelope = vector(case.get('unknown_envelope_micros'), ids, 'UNKNOWN_ENVELOPE') if unknown else None
        if not unknown:
            require(case.get('unknown_envelope_micros') is None, 'NO_UNKNOWN_FIELD')
        unknown_top = case.get('unknown_top3_micros')
        if unknown_top is not None:
            require(unknown > 0 and isinstance(unknown_top, dict) and set(unknown_top) == set(ids), 'UNKNOWN_TOP3')
            for sid in ids:
                values = unknown_top[sid]
                require(isinstance(values, list) and len(values) == min(3, unknown), 'UNKNOWN_TOP3')
                for value in values:
                    integer(value, 'UNKNOWN_TOP3')
                require(all(a >= b for a, b in zip(values, values[1:])) and
                        values[0] == envelope[ids.index(sid)], 'UNKNOWN_TOP3')
        growth = 0
        if kind == 'growing':
            rate = integer(case.get('growth_micros_per_day'), 'GROWTH_RATE', 0)
            require(observation.get('published_at_ms') is not None, 'GROWTH_PUBLICATION_CLOCK_UNAVAILABLE')
            growth = ceil_ratio(rate * (horizon - observation['published_at_ms']), DAY_MS)
        factors, loadings, overrides = {}, {}, {}
        if kind == 'factor_correlated':
            factors = case.get('scenario_factors')
            loadings = case.get('factor_loadings_ppm')
            overrides = case.get('participant_loadings_ppm', {})
            require(isinstance(factors, dict) and 1 <= len(factors) <= 32 and
                    isinstance(loadings, dict) and set(factors) == set(loadings), 'FACTOR_SCHEMA')
            factors = {atom(f, 'FACTOR_ID'): vector(v, ids, 'FACTOR_VECTOR') for f, v in factors.items()}
            for value in loadings.values():
                integer(value, 'FACTOR_LOADING', -100000000, 100000000)
            require(isinstance(overrides, dict) and set(overrides) <= seen, 'PARTICIPANT_LOADING')
            for values in overrides.values():
                require(isinstance(values, dict) and set(values) == set(factors), 'PARTICIPANT_LOADING')
                for value in values.values():
                    integer(value, 'FACTOR_LOADING', -100000000, 100000000)
        margins = vector(case.get('overtaking_margin_micros'), ids, 'OVERTAKING_MARGIN', 0) if kind == 'adverse_overtaking' else None
        require(kind != 'adverse_overtaking' or unknown > 0, 'NO_UNKNOWN_OVERTAKER')
        lower, upper, top3, observed_top3 = [], [], [], []
        for i, sid in enumerate(ids):
            lows, highs = [], []
            for identity, low, high in observed:
                shift = growth
                if factors:
                    use = overrides.get(identity, loadings)
                    shift += ceil_ratio(sum(factors[f][i] * use[f] for f in factors), 1000000)
                lows.append(integer(low + shift, 'PROJECTION_RANGE'))
                highs.append(integer(high + shift, 'PROJECTION_RANGE'))
            known_low, known_high = max(lows), max(highs)
            hidden = envelope[i] if envelope is not None else None
            if margins is not None:
                hidden = integer(max(hidden, known_high + margins[i]), 'PROJECTION_RANGE')
            lower.append(max(known_low, hidden) if hidden is not None else known_low)
            upper.append(max(known_high, hidden) if hidden is not None else known_high)
            observed_top3.append({'lower': sorted(lows, reverse=True)[:3], 'upper': sorted(highs, reverse=True)[:3]})
            if unknown_top is not None:
                require(margins is None or hidden == envelope[i], 'OVERTAKER_TOP3_CONFLICT')
                top3.append({'lower': sorted(lows + unknown_top[sid], reverse=True)[:3],
                             'upper': sorted(highs + unknown_top[sid], reverse=True)[:3]})
            elif not unknown:
                top3.append(observed_top3[-1])
            else:
                top3.append(None)
        tie_rule = case.get('tie_rule')
        require(tie_rule in (None, 'lose_ties', 'split_occupied_prizes'), 'TIE_RULE')
        prizes = case.get('prizes_micros')
        if prizes is not None:
            require(isinstance(prizes, list) and 1 <= len(prizes) <= participants, 'PRIZES')
            for p in prizes:
                integer(p, 'PRIZES', 0)
            require(all(a >= b for a, b in zip(prizes, prizes[1:])), 'PRIZE_ORDER')
        result.append({'id': name, 'thresholds_micros': upper,
                       'threshold_lower_micros': lower, 'scenario_ids': list(ids),
                       'assumption_label': label, 'kind': kind, 'complete_field': False,
                       'participant_count': participants, 'opponent_count': opponents,
                       'observed_opponent_count': len(observed), 'unobserved_opponent_count': unknown,
                       'top3_micros': top3, 'observed_top3_bounds_micros': observed_top3,
                       'tie_rule': tie_rule, 'prizes_micros': prizes,
                       'source_sha256': observation['source_sha256'], 'source_url': observation['source_url'],
                       'published_at_ms': observation.get('published_at_ms'), 'as_of_ms': observation['as_of_ms'],
                       'valid_until_ms': observation['valid_until_ms'], 'decision_ms': decision,
                       'projection_at_ms': horizon, 'source_stale': stale,
                       'projection_parameters': {
                           'growth_micros_per_day': case.get('growth_micros_per_day') if kind == 'growing' else None,
                           'scenario_factors': {f: dict(zip(ids, v)) for f, v in factors.items()} if factors else None,
                           'factor_loadings_ppm': dict(loadings) if factors else None,
                           'participant_loadings_ppm': {p: dict(v) for p, v in overrides.items()} if factors else None,
                           'unknown_envelope_micros': dict(zip(ids, envelope)) if envelope is not None else None,
                           'unknown_top3_micros': {s: list(v) for s, v in unknown_top.items()} if unknown_top is not None else None,
                           'overtaking_margin_micros': dict(zip(ids, margins)) if margins is not None else None,
                           'allow_stale_observation': allow_stale},
                       'evidence_class': 'synthetic' if observation['evidence_class'] == 'synthetic' else 'declared_model',
                       'observation_evidence_class': observation['evidence_class'],
                       'unobserved_field_statement': observation['unobserved_field_statement'],
                       'execution_authorized': False, 'calibrated_probability_claimed': False})
    return result


def first_place_summary(model, wealth_micros, weights_ppm):
    """Conditional strict/inclusive bounds, not a calibrated winning probability.

    No tie rule implies no point estimate. Split-prize expectation remains None:
    a maximum/top3 does not establish every tied opponent or their multiplicity.
    """
    require(isinstance(model, dict) and isinstance(model.get('scenario_ids'), list), 'SUMMARY_MODEL')
    n = len(model['scenario_ids'])
    require(1 <= n <= MAX_SCENARIOS and len(set(model['scenario_ids'])) == n, 'SUMMARY_MODEL')
    require(isinstance(wealth_micros, list) and len(wealth_micros) == n and
            isinstance(weights_ppm, list) and len(weights_ppm) == n, 'SUMMARY_DIMENSION')
    for value in wealth_micros:
        integer(value, 'OUR_WEALTH')
    for value in weights_ppm:
        integer(value, 'WEIGHT', 0, 1000000)
    require(sum(weights_ppm) == 1000000, 'WEIGHT_MASS')
    low, high = model['threshold_lower_micros'], model['thresholds_micros']
    require(isinstance(low, list) and isinstance(high, list) and len(low) == len(high) == n, 'SUMMARY_THRESHOLDS')
    for a, b in zip(low, high):
        integer(a, 'SUMMARY_THRESHOLDS'); integer(b, 'SUMMARY_THRESHOLDS')
        require(a <= b, 'SUMMARY_THRESHOLDS')
    strict_low = sum(w for v, t, w in zip(wealth_micros, high, weights_ppm) if v > t)
    strict_high = sum(w for v, t, w in zip(wealth_micros, low, weights_ppm) if v > t)
    inclusive_low = sum(w for v, t, w in zip(wealth_micros, high, weights_ppm) if v >= t)
    inclusive_high = sum(w for v, t, w in zip(wealth_micros, low, weights_ppm) if v >= t)
    return {'strict_first_probability_ppm_bounds': [strict_low, strict_high],
            'inclusive_first_probability_ppm_bounds': [inclusive_low, inclusive_high],
            'first_prize_probability_ppm': strict_low if model['tie_rule'] == 'lose_ties' and strict_low == strict_high else None,
            'expected_prize_micros': None, 'execution_authorized': False,
            'calibrated_probability_claimed': False}
