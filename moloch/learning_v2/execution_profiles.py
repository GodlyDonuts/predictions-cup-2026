"""Fixed research stresses, chosen by the experiment rather than the policy."""
from dataclasses import replace
import json


def load_profiles(path, base):
    if path is None:
        return [('nominal', base)]
    obj = json.loads(path.read_text(encoding='utf-8'))
    if obj.get('classification') != 'fixed_development_execution_scenarios' or obj.get('production_authority') is not False:
        raise ValueError('EXPLICIT_RESEARCH_SCENARIOS_REQUIRED')
    allowed = {'read_latency_ms', 'arrival_latency_ms', 'receipt_latency_ms',
               'depth_fraction_ppm', 'adverse_execution_micros'}
    profiles = []
    for row in obj['profiles']:
        changes = row['overrides']
        if not set(changes) <= allowed or not row['name'] or row['name'] in [n for n, _ in profiles]:
            raise ValueError('SCENARIO_NAME_OR_IMMUTABLE_MECHANICS')
        if any(type(v) is not int or v < 0 for v in changes.values()):
            raise ValueError('EXACT_SCENARIO_PARAMETERS_REQUIRED')
        for name in ['read_latency_ms', 'arrival_latency_ms', 'receipt_latency_ms', 'adverse_execution_micros']:
            if name in changes and changes[name] < getattr(base, name):
                raise ValueError('SCENARIO_CANNOT_IMPROVE_EXECUTION')
        if changes.get('depth_fraction_ppm', base.depth_fraction_ppm) > base.depth_fraction_ppm:
            raise ValueError('SCENARIO_CANNOT_INVENT_DEPTH')
        profiles.append((row['name'], replace(base, **changes)))
    if not profiles or profiles[0] != ('nominal', base):
        raise ValueError('UNCHANGED_NOMINAL_FIRST_REQUIRED')
    return profiles
