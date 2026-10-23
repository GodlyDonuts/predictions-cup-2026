"""Allocation comparisons that specifically exercise cash-starved rotation."""
from copy import deepcopy
import importlib.util
import json
from pathlib import Path
from tempfile import TemporaryDirectory
from types import SimpleNamespace
import unittest
from unittest.mock import patch
from capital_aware_money_selection import select


def direct(gain, interval=None, action='BUY', exchange='a', side='YES'):
    return dict(leg=dict(ex=exchange, side=side, action=action),
                weakestDeclaredExpectedChange=gain,
                exactOrdinaryIntervalSensitivity=dict(ordinaryMinimumExpectedChange=interval or gain))


def rotation(gain, interval=None):
    return dict(legs=[dict(ex='held', side='NO', action='SELL'),
                      dict(ex='a', side='YES', action='BUY')],
                weakestDeclaredExpectedChange=gain, ordinaryMinimumExpectedChange=interval or gain)


class AllocationTests(unittest.TestCase):
    ownership = {'a': {'side': 'YES'}}

    def test_cash_starved_portfolio_can_select_funded_replacement(self):
        chosen, _ = select([], [rotation('300')], self.ownership)
        self.assertEqual(chosen['kind'], 'rotation')
        self.assertTrue(chosen['conditionalReplacementRequired'])
        self.assertFalse(chosen['actualFutureProceedsCredited'])
        self.assertFalse(chosen['executionAuthority'])

    def test_compare_conservative_gain_instead_of_only_sampled_gain(self):
        chosen, _ = select([direct('100', '90')], [rotation('200', '10')], self.ownership)
        self.assertEqual(chosen['kind'], 'direct')
        self.assertEqual(chosen['comparisonGain'], '90')

    def test_direct_trade_wins_equal_score(self):
        chosen, _ = select([direct('100')], [rotation('100')], self.ownership)
        self.assertEqual(chosen['kind'], 'direct')

    def test_management_gate_applies_to_every_future_buy(self):
        plan = rotation('300')
        plan['legs'].append(dict(ex='unmanaged', side='YES', action='BUY'))
        chosen, deferred = select([], [plan], self.ownership)
        self.assertIsNone(chosen)
        self.assertEqual(deferred[0]['legs'][0]['ex'], 'unmanaged')

    def test_wrong_side_manager_does_not_qualify(self):
        chosen, deferred = select([direct('10', side='NO')], [], self.ownership)
        self.assertIsNone(chosen)
        self.assertEqual(len(deferred), 1)

    def test_existing_unmanaged_sale_still_competes(self):
        chosen, _ = select([direct('10', action='SELL')], [], {})
        self.assertEqual(chosen['kind'], 'direct')

    def test_both_money_bounds_must_clear_floor(self):
        chosen, _ = select([direct('10', '-1')], [rotation('10', '-1')], self.ownership)
        self.assertIsNone(chosen)

    def test_decimal_boundary_and_inputs_preserved(self):
        rows = [direct('0.010000000000000001')]
        before = deepcopy(rows)
        chosen, _ = select(rows, [], self.ownership, '0.010000000000000001')
        self.assertIsNotNone(chosen)
        self.assertEqual(rows, before)

    def test_nonfinite_or_invalid_floor_and_malformed_plan_refuse(self):
        for value in ('NaN', 'Infinity', '-1', '0'):
            with self.assertRaises(ValueError):
                select([], [], {}, value)
        with self.assertRaises(ValueError):
            select([direct('NaN')], [], self.ownership)
        plan = rotation('300'); plan['legs'][0]['action'] = 'BUY'
        with self.assertRaises(ValueError):
            select([], [plan], self.ownership)


class OrchestrationTests(unittest.TestCase):
    def exercise(self, direct_rows, rotation_rows, expected):
        spec = importlib.util.spec_from_file_location('capital_research',
            Path(__file__).with_name('prepare-capital-aware-money-research.py'))
        wrapper = importlib.util.module_from_spec(spec); spec.loader.exec_module(wrapper)
        with TemporaryDirectory() as folder:
            root = Path(folder); inputs = root/'inputs'; inputs.mkdir()
            (inputs/'compiled-current.private.json').write_text(json.dumps(dict(
                financial={}, unavailableReasons=[], managedOwnership={'a': {'side':'YES'}},
                reportSha256='original-report')))
            called = []

            def fake_module(name, path):
                def run(args):
                    called.append(name)
                    if name in ('capital_direct', 'capital_rotations'):
                        data = dict(sourcePortfolioSha256='original-report',
                            candidates=direct_rows if name=='capital_direct' else rotation_rows)
                        args.output.write_text(json.dumps(data))
                        return data
                    args.output.mkdir()
                    if name == 'capital_prepare_direct':
                        selected = json.loads(args.candidates.read_text())['candidates']
                        self.assertEqual(len(selected), 1)
                        self.assertEqual(selected[0]['leg']['quantity'], 3)
                    else:
                        self.assertEqual(json.loads(args.plan.read_text())['legs'][0]['action'], 'SELL')
                    (args.output/'prepared.public.json').write_text(json.dumps(dict(
                        rawSha256='prepared-original', sourceClocksRenewed=False)))
                    # Existing direct preparer returns None; read its artifact.
                    return None
                return SimpleNamespace(run=run)

            args = SimpleNamespace(source=root, tools=root, inputs=inputs, output=root/'out',
                minimum_expected_money='0.01', audit_limit=256, horizon_ms=300000,
                price_tolerance='0.01')
            with patch.object(wrapper, 'module', fake_module):
                result = wrapper.run(args)
            self.assertEqual(result['selection']['kind'], expected)
            self.assertEqual(result['prepared']['rawSha256'], 'prepared-original')
            self.assertFalse(result['approvalPublished'])
            self.assertFalse(result['submitted'])
            self.assertEqual(called[-1], 'capital_prepare_'+expected)

    def test_direct_keeps_exact_selected_quantity_for_same_market(self):
        large = direct('100', '1'); large['leg']['quantity'] = 1000
        small = direct('2', '2'); small['leg']['quantity'] = 3
        self.exercise([large, small], [], 'direct')

    def test_rotation_calls_existing_plan_preparer_without_publishing(self):
        self.exercise([], [rotation('300')], 'rotation')


if __name__ == '__main__':
    unittest.main()
