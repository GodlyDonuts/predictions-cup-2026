import copy
from decimal import Decimal
from fractions import Fraction
import random
import unittest

from moloch.tartarus.scenario_coalescing import coalesce, PPM


def fixture():
    tags = [("resolved_yes", "resolved_no"), ("resolved_yes", "resolved_no"),
            ("resolved_no", "resolved_yes"), ("refund_principal", "refund_principal")]
    rows = [{"id": f"state-{i}", "kind": "exceptional" if i == 3 else "ordinary",
             "assumptionLabel": f"Original source label {i}",
             "payouts": dict(zip(["101", "102"], values))}
            for i, values in enumerate(tags)]
    probabilities = [{"id": name, "assumptionLabel": name,
                      "weightsPpm": weights} for name, weights in
                     [("quoted", [100000, 200000, 690000, 10000]),
                      ("retained", [490000, 10000, 490000, 10000])]]
    fields = [{"id": name, "assumptionLabel": "Explicit hypothetical field; positions unknown",
               "completeField": False, "thresholds": [str(x) for x in levels]}
              for name, levels in [("flat", [90]*4), ("growing", [120]*4),
                                   ("correlated", [100, 100, 140, 90])]]
    return rows, probabilities, fields


def scores(rows, probabilities, fields, cash, holdings):
    wealth = []
    for row in rows:
        total = Fraction(cash)
        for contract, (yes, no, basis) in holdings.items():
            tag = row["payouts"][contract]
            total += basis if tag == "refund_principal" else yes if tag == "resolved_yes" else no
        wealth.append(total)
    out = {}
    for model in probabilities:
        out[(model["id"], "money")] = sum(w*v for w, v in zip(model["weightsPpm"], wealth))/PPM
        for field in fields:
            strict, ties = 0, 0
            for weight, value, threshold in zip(model["weightsPpm"], wealth, field["thresholds"]):
                threshold = Fraction(Decimal(threshold))
                strict += weight*(value > threshold)
                ties += weight*(value == threshold)
            out[(model["id"], field["id"])] = (strict, ties, Fraction(2*strict+ties, 2*PPM))
    return out


class CoalescingTests(unittest.TestCase):
    def test_all_source_money_win_and_tie_scores_preserved_for_many_portfolios(self):
        rows, probabilities, fields = fixture()
        before = copy.deepcopy((rows, probabilities, fields))
        out = coalesce(rows, probabilities, fields)
        self.assertEqual(len(out["scenarios"]), 3)
        self.assertEqual(out["probabilityModels"][0]["weightsPpm"], [300000, 690000, 10000])
        self.assertEqual(out["probabilityModels"][1]["weightsPpm"], [500000, 490000, 10000])
        rng = random.Random(20261018)
        for _ in range(100):
            holdings = {c: (rng.randrange(100), rng.randrange(100), Fraction(rng.randrange(1000), 10))
                        for c in ["101", "102"]}
            cash = rng.randrange(-50, 200)
            self.assertEqual(scores(rows, probabilities, fields, cash, holdings),
                             scores(out["scenarios"], out["probabilityModels"], out["fieldModels"], cash, holdings))
        # Exact threshold ties retain their original source-model masses too.
        self.assertEqual(scores(rows, probabilities, fields, 90, {"101": (0, 0, 0), "102": (0, 0, 0)}),
                         scores(out["scenarios"], out["probabilityModels"], out["fieldModels"], 90,
                                {"101": (0, 0, 0), "102": (0, 0, 0)}))
        self.assertEqual((rows, probabilities, fields), before)
        self.assertFalse(out["executionAuthority"])

    def test_same_own_payout_different_latent_field_threshold_cannot_merge(self):
        rows, probabilities, fields = fixture()
        fields[-1]["thresholds"][1] = "101"
        out = coalesce(rows, probabilities, fields)
        self.assertEqual(len(out["scenarios"]), 4)
        with self.assertRaisesRegex(ValueError, "EXCEEDS_NATIVE_BOUND"):
            coalesce(rows, probabilities, fields, maximum_output=3)

    def test_one_ppm_exception_and_separate_source_weights_are_not_dropped(self):
        rows, probabilities, fields = fixture()
        probabilities[0]["weightsPpm"] = [1, 999998, 0, 1]
        out = coalesce(rows, probabilities, fields)
        self.assertEqual(out["probabilityModels"][0]["weightsPpm"], [999999, 0, 1])
        self.assertEqual(out["probabilityModels"][1]["weightsPpm"], [500000, 490000, 10000])
        self.assertEqual(out["scenarios"][-1]["payouts"]["101"], "refund_principal")

    def test_exception_class_and_original_labels_remain_distinct(self):
        rows, probabilities, fields = fixture()
        rows[1]["kind"] = "exceptional"
        out = coalesce(rows, probabilities, fields)
        self.assertEqual(len(out["scenarios"]), 4)
        self.assertEqual([g["originalRows"][0] for g in out["coalescingEvidence"]["groups"]], rows)

    def test_wide_research_can_fit_only_through_complete_economic_equivalence(self):
        row = fixture()[0][0]
        rows = [dict(copy.deepcopy(row), id=f"state-{i}") for i in range(129)]
        models = [{"id": "rare", "assumptionLabel": "Synthetic", "weightsPpm": [1]*128+[PPM-128]}]
        fields = [{"id": "flat", "assumptionLabel": "Synthetic", "completeField": False,
                   "thresholds": ["100"]*129}]
        out = coalesce(rows, models, fields)
        self.assertEqual(len(out["scenarios"]), 1)
        self.assertEqual(out["probabilityModels"][0]["weightsPpm"], [PPM])
        fields[0]["thresholds"] = [str(i) for i in range(129)]
        with self.assertRaisesRegex(ValueError, "EXCEEDS_NATIVE_BOUND"):
            coalesce(rows, models, fields)

    def test_incomplete_vectors_false_completeness_and_hidden_fields_are_rejected(self):
        for case in range(6):
            rows, models, fields = fixture()
            if case == 0:
                fields[0]["thresholds"].pop()
            elif case == 1:
                fields[0]["completeField"] = True
            elif case == 2:
                rows[1]["latentWealth"] = 1
            elif case == 3:
                models[0]["weightsPpm"][0] = True
            elif case == 4:
                rows[1]["payouts"].pop("102")
            else:
                rows[1]["payouts"]["101"] = {}
            with self.assertRaises(ValueError):
                coalesce(rows, models, fields)


if __name__ == "__main__":
    unittest.main()
