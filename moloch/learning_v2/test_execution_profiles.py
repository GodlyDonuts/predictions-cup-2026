import json
from pathlib import Path
import tempfile
import unittest
from .run_experiment import load_mechanics
from .execution_profiles import load_profiles


class FixedExecutionProfiles(unittest.TestCase):
    def test_stresses_preserve_account_limits_and_only_worsen_execution(self):
        root = Path(__file__).resolve().parents[2]
        base = load_mechanics(root/'research/tartarus-learning-v2/development-mechanics-v2.json')
        profiles = load_profiles(root/'research/tartarus-learning-v2/execution-profiles-v3.json', base)
        self.assertEqual(profiles[0], ('nominal', base))
        for _, m in profiles:
            self.assertEqual(m.limits, base.limits)
            self.assertEqual(m.budget, base.budget)
        with tempfile.TemporaryDirectory() as d:
            p = Path(d)/'invalid.json'
            obj = {'classification':'fixed_development_execution_scenarios', 'production_authority':False,
                   'profiles':[{'name':'nominal', 'overrides':{}}, {'name':'fake', 'overrides':{'arrival_latency_ms':0}}]}
            p.write_text(json.dumps(obj), encoding='utf-8')
            with self.assertRaisesRegex(ValueError, 'CANNOT_IMPROVE'):
                load_profiles(p, base)
            obj['profiles'][1]['overrides'] = {'preflight_reads':0}
            p.write_text(json.dumps(obj), encoding='utf-8')
            with self.assertRaisesRegex(ValueError, 'IMMUTABLE_MECHANICS'):
                load_profiles(p, base)
