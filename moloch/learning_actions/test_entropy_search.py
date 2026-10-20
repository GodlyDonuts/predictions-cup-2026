import random,unittest
from .entropy_search import offspring,probability

SCALES={'local':(.8,1.2),'medium':(.5,2.),'large':(.25,4.),'head_reset':(.5,2.)}


class EntropySearchTests(unittest.TestCase):
    def test_zero_and_positive_settings_are_both_reachable_without_fresh_actor(self):
        for operator in SCALES:
            value,kind=offspring(0.,operator,SCALES,random.Random(2),[.002,.02],resample_probability=1)
            self.assertIn(value,(.002,.02));self.assertEqual(kind,'configured_grid_resample')
            self.assertEqual(offspring(.03,operator,SCALES,random.Random(2),[0.],resample_probability=1)[0],0.)

    def test_disabled_profile_preserves_legacy_choices_and_next_random_state(self):
        for seed in range(20):
            for operator in(*SCALES,'fresh'):
                a=random.Random(seed);b=random.Random(seed);parent=.003;grid=[0.,.001,.03]
                expected=b.choice(grid)if operator=='fresh'else min(.1,max(0.,parent*b.choice(SCALES[operator])))
                value,_=offspring(parent,operator,SCALES,a,grid)
                self.assertEqual(value,expected);self.assertEqual(a.getstate(),b.getstate())

    def test_invalid_probability_or_configured_choice_cannot_enter_optimizer(self):
        for p in(-.1,1.1,True,float('nan'),float('inf')):
            with self.assertRaises(ValueError):probability(p)
        for choices in([],[-.1],[True],[float('nan')]):
            with self.assertRaises(ValueError):offspring(0.,'local',SCALES,random.Random(1),choices)
