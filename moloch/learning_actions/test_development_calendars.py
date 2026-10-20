import unittest
from .development_calendars import calendars


class DevelopmentCalendarTests(unittest.TestCase):
    def test_fixed_stratified_screens_and_full_confirmation(self):
        args=(1001,3000001,2)
        screen=calendars(*args,development=True,episode_ms=120000)
        self.assertEqual(screen,[(690751,810751),(2190251,2310251)])
        full=calendars(*args,development=True,episode_ms=120000,confirmation=True)
        self.assertEqual(full,[(1001,1500501),(1500501,3000001)])
        self.assertEqual(calendars(*args,development=True),full)

    def test_shortening_cannot_enter_full_history_or_exceed_its_stratum(self):
        for value in(0,-1,True,1.5,501):
            with self.assertRaises(ValueError):calendars(1,1001,2,development=True,episode_ms=value)
        with self.assertRaises(ValueError):calendars(1,1001,1,development=False,episode_ms=50)
        with self.assertRaises(ValueError):calendars(1,1001,2,development=False)
        self.assertEqual(calendars(1,1001,1,development=False),[(1,1001)])

    def test_positive_screen_gets_full_development_confirmation_before_any_profit_claim(self):
        import tempfile,torch
        from pathlib import Path
        from .test_population_experiment import FixtureExperiment
        class PositiveFixture(FixtureExperiment):
            def evaluate(self,*args,**kwargs):
                r=super().evaluate(*args,**kwargs)
                if not kwargs.get('confirmation'):
                    r['reward']=.01  # Fixture score, not ROI evidence.
                    for case in r['cases']:case['reward']=.01
                return r
        torch.set_num_threads(1)
        with tempfile.TemporaryDirectory()as d:
            e=PositiveFixture(Path(d));e.config.update(cycles=1,development_selection_episode_ms=50)
            r=e.run();checks=[x for x in e.events if x['event']=='development_full_window_confirmation']
            self.assertEqual(len(checks),1)
            self.assertFalse(checks[0]['confirmation']['development_screen'])
            self.assertEqual([(c['start_ms'],c['end_ms'])for c in checks[0]['confirmation']['cases']],
                [(501,751),(501,751),(751,1001),(751,1001)])
            self.assertTrue(r['best']['selection']['development_screen'])
            self.assertFalse(r['independent_test_used']);self.assertFalse(r['production_profitability_established'])
