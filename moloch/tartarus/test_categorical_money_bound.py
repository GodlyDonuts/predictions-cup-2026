import copy
import unittest

from .categorical_money_bound import ordinary_money_bounds


def fixture(same_event=False):
    registry, marginals, diagnostics = [], [], []
    for i, party in enumerate(('Republican Party', 'Democratic Party')):
        event = dict(raceId=0 if same_event else i, electionDate='2026-11-03',
                     raceName='Synthetic' if same_event else str(i), stageId=1, officeLevel='Federal', usState='AK')
        registry.append(dict(exchangeId=str(i), winnerName=party, **event))
        marginals.append(dict(exchangeId=str(i), lowerPpm=100000, upperPpm=900000))
        diagnostics.append(dict(eventIdentity=event))
    return registry, marginals, [], diagnostics


def leg(ex, side, action, n=10, price=500000):
    return dict(ex=ex, side=side, action=action, quantity=n, price=price)


class CategoricalMoneyBounds(unittest.TestCase):
    def test_separate_adverse_races_expose_loss_hidden_by_common_points(self):
        result = ordinary_money_bounds(*fixture(), [leg('0', 'YES', 'BUY'), leg('1', 'YES', 'SELL')])
        self.assertEqual(result['ordinaryMinimumExpectedChange'], '-8.000000')
        self.assertEqual(result['ordinaryMaximumExpectedChange'], '8.000000')
        self.assertFalse(result['executionAuthority'])

    def test_mutually_exclusive_parties_cannot_both_take_upper_probability(self):
        result = ordinary_money_bounds(*fixture(True), [leg('0', 'YES', 'SELL'), leg('1', 'YES', 'SELL')])
        self.assertEqual(result['ordinaryMinimumExpectedChange'], '0.000000')
        self.assertEqual(result['ordinaryMaximumExpectedChange'], '8.000000')

    def test_other_party_winner_is_allowed_not_implicitly_exhausted(self):
        f = fixture(True)
        for m in f[1]: m.update(lowerPpm=0, upperPpm=400000)
        result = ordinary_money_bounds(*f, [leg('0', 'NO', 'BUY'), leg('1', 'NO', 'BUY')])
        self.assertEqual(result['ordinaryMinimumExpectedChange'], '2.000000')
        self.assertEqual(result['ordinaryMaximumExpectedChange'], '10.000000')
        self.assertEqual(result['eventWitnesses'][0]['minimumOtherWinnerMassPpm'], 200000)

    def test_profile_intervals_widen_and_inputs_are_immutable(self):
        f = fixture(); f[2].append(dict(marginalOverrides=[dict(exchangeId='0', lowerPpm=0, upperPpm=1000000)]))
        original = copy.deepcopy(f); result = ordinary_money_bounds(*f, [leg('0', 'NO', 'BUY')])
        self.assertEqual(result['ordinaryMinimumExpectedChange'], '-5.000000')
        self.assertEqual(f, original)

    def test_incoherent_masses_and_ambiguous_event_identity_are_rejected(self):
        f = fixture(True)
        for m in f[1]: m['lowerPpm'] = 600000
        with self.assertRaisesRegex(ValueError, 'INCOHERENT'): ordinary_money_bounds(*f, [])
        f = fixture(); f[3].append(dict(eventIdentity=dict(f[3][0]['eventIdentity'], stageId=2)))
        with self.assertRaisesRegex(ValueError, 'AMBIGUOUS'): ordinary_money_bounds(*f, [])


if __name__ == '__main__': unittest.main()
