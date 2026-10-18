import copy
import json
import unittest

from moloch.tartarus.party_scenarios import compile_scenarios, canonical, sha, PPM


def fixture(count=3):
    registry, originals, marginals = [], {}, []
    for i in range(count):
        cid, ex, market = str(100+i), str(200+i), str(300+i)
        root = {"node_type":"contract", "node_id":cid, "contract_id":cid,
                "contract_details":{"resolutionType":"Party Winner", "raceStage":"General",
                 "winnerName":"Democratic Party" if i == 1 else "Republican Party",
                 "raceId":1 if i < 2 else i, "stageId":9, "electionDate":"2026-11-03",
                 "raceName":"Same race" if i < 2 else str(i), "officeLevel":"Federal", "usState":"AK"}}
        raw = canonical({"data":{"market_id":market, "root":root}})
        originals[sha(raw)] = raw
        registry.append({"contractId":cid, "exchangeId":ex, "marketId":market,
                         "semanticSourceSha256":sha(raw), "officialContractRootSha256":sha(canonical(root))})
        source = canonical({"synthetic_marginal":i})
        originals[sha(source)] = source
        point = 300_000 if i == 1 else 400_000
        marginals.append({"exchangeId":ex, "lowerPpm":point, "upperPpm":point,
                          "sourceSha256":sha(source), "observedAtMs":100, "maximumAgeMs":100,
                          "assumptionLabel":"Explicit synthetic marginal; not market calibration"})
    profiles = [{"id":"shared", "marginalPoint":"midpoint", "dependence":"common_party_swing",
                 "hypotheticalRefundWeightPpm":0, "assumptionLabel":"Synthetic shared latent rank"},
                {"id":"independent", "marginalPoint":"midpoint", "dependence":"independent",
                 "hypotheticalRefundWeightPpm":10000, "assumptionLabel":"Synthetic independent stratified sample"}]
    return registry, originals, marginals, profiles


class PartyScenarioTests(unittest.TestCase):
    def test_conflicting_source_is_retained_only_in_its_own_profile(self):
        a = list(fixture(1))
        raw = canonical({"different_declared_publisher": "conditional point .8"})
        a[1][sha(raw)] = raw
        override = dict(a[2][0], lowerPpm=800000, upperPpm=800000,
                        sourceSha256=sha(raw), observedAtMs=120,
                        assumptionLabel="Separate synthetic publisher assumption")
        profile = dict(a[3][0], id="conflicting", marginalOverrides=[override])
        a[3] = [a[3][0], profile]
        original = copy.deepcopy(a)
        result = compile_scenarios(*a, 150)
        probabilities = {}
        for model in result["probabilityModels"]:
            probabilities[model["id"]] = sum(w for s, w in zip(result["scenarios"], model["weightsPpm"])
                                             if s["payouts"]["100"] == "resolved_yes")
        self.assertEqual(probabilities, {"shared": 400000, "conflicting": 800000})
        self.assertEqual(a, original)
        # At a .6 sale price, the two retained models disagree about selling.
        self.assertGreater(600000-probabilities["shared"], 0)
        self.assertLess(600000-probabilities["conflicting"], 0)

    def test_overrides_require_current_original_bytes_and_existing_scope(self):
        for case in ("stale", "future", "forged", "foreign", "duplicate"):
            a = list(fixture(1))
            override = dict(a[2][0])
            if case == "stale": override["observedAtMs"] = 0
            if case == "future": override["observedAtMs"] = 151
            if case == "forged": override["sourceSha256"] = "f"*64
            if case == "foreign": override["exchangeId"] = "unmodeled"
            overrides = [override, override] if case == "duplicate" else [override]
            a[3] = [dict(a[3][0], marginalOverrides=overrides)]
            with self.subTest(case=case), self.assertRaises(ValueError):
                compile_scenarios(*a, 150)

    def test_conflicting_same_race_sources_need_explicit_coherent_completion(self):
        a = list(fixture(2))
        republican = dict(a[2][0], lowerPpm=800000, upperPpm=800000)
        # Existing Democratic .3 and alternative Republican .8 cannot coexist.
        a[3] = [dict(a[3][0], marginalOverrides=[republican])]
        with self.assertRaisesRegex(ValueError, "INCOHERENT_PARTY_MARGINALS"):
            compile_scenarios(*a, 150)
        democratic = dict(a[2][1], lowerPpm=200000, upperPpm=200000,
                          assumptionLabel="Explicit synthetic two-party completion")
        a[3][0]["marginalOverrides"].append(democratic)
        result = compile_scenarios(*a, 150)
        for state in result["scenarios"]:
            if state["kind"] == "ordinary":
                self.assertNotEqual(state["payouts"]["100"], state["payouts"]["101"])

    def test_same_race_parties_never_both_win_and_other_winner_is_retained(self):
        r = compile_scenarios(*fixture(), 150)
        rows = [s for s in r["scenarios"] if s["kind"] == "ordinary"]
        self.assertTrue(any(s["payouts"]["100"] == s["payouts"]["101"] == "resolved_no" for s in rows))
        self.assertTrue(all(not(s["payouts"]["100"] == s["payouts"]["101"] == "resolved_yes") for s in rows))
        self.assertEqual(r, compile_scenarios(*fixture(), 150))
        for p in r["probabilityModels"]:
            self.assertEqual(sum(p["weightsPpm"]), PPM)
            self.assertEqual(len(p["weightsPpm"]), len(r["scenarios"]))

    def test_exact_common_swing_preserves_rare_state_instead_of_rounding_it_out(self):
        registry, originals, marginals, profiles = fixture(1)
        marginals[0].update(lowerPpm=1000, upperPpm=1000)
        r = compile_scenarios(registry, originals, marginals, profiles[:1], 150, draws_per_profile=2)
        probability = sum(w for s, w in zip(r["scenarios"], r["probabilityModels"][0]["weightsPpm"])
                          if s["payouts"]["100"] == "resolved_yes")
        self.assertEqual(probability, 1000)
        self.assertFalse(r["calibratedProbabilitiesClaimed"])
        self.assertFalse(r["executionAuthority"])

    def test_missing_stale_future_forged_and_incoherent_inputs_reject(self):
        base = fixture()
        for case in ("missing", "stale", "future", "forged", "incoherent"):
            a = copy.deepcopy(base)
            if case == "missing": a[2].pop()
            if case == "stale": a[2][0]["observedAtMs"] = 0
            if case == "future": a[2][0]["observedAtMs"] = 151
            if case == "forged": a[0][0]["officialContractRootSha256"] = "f"*64
            if case == "incoherent":
                for m in a[2][:2]: m.update(lowerPpm=700000, upperPpm=800000)
            with self.subTest(case=case), self.assertRaises(ValueError):
                compile_scenarios(*a, 150)

    def test_23_contracts_all_modeled_and_unknown_interval_remains_explicit(self):
        a = fixture(23)
        a[2][-1].update(lowerPpm=0, upperPpm=PPM, assumptionLabel="Explicit ignorance, no pricing evidence")
        r = compile_scenarios(*a, 150)
        self.assertLessEqual(len(r["scenarios"]), 128)
        self.assertTrue(all(set(s["payouts"]) == {c["contractId"] for c in a[0]} for s in r["scenarios"]))
        self.assertTrue(r["commonSwingIntervalsExactWithinDeclaredCopula"])
        self.assertTrue(r["independentRareEventsMayBeUnrepresented"])
        a = list(a)
        a[3] = [dict(a[3][1], id=str(i)) for i in range(8)]
        with self.assertRaisesRegex(ValueError, "NATIVE_SCENARIO_BOUND"):
            compile_scenarios(*a, 150, draws_per_profile=64)


    def test_explicit_wide_research_preserves_models_until_field_aware_reduction(self):
        from moloch.tartarus.scenario_coalescing import coalesce
        a = list(fixture())
        a[3] = [dict(a[3][1], id=f"independent-{i}") for i in range(8)]
        with self.assertRaisesRegex(ValueError, "NATIVE_SCENARIO_BOUND"):
            compile_scenarios(*a, 150, draws_per_profile=64)
        wide = compile_scenarios(*a, 150, draws_per_profile=64, research_scenario_limit=1024)
        self.assertEqual(len(wide["scenarios"]), 513)
        self.assertTrue(wide["requiresFieldAwareNativeReduction"])
        self.assertFalse(wide["executionAuthority"])
        fields = [{"id": "hypothetical_flat", "completeField": False,
                   "assumptionLabel": "Synthetic fixed opposing threshold, not a field observation",
                   "thresholds": ["100.000000"]*513}]
        reduced = coalesce(wide["scenarios"], wide["probabilityModels"], fields)
        self.assertLessEqual(len(reduced["scenarios"]), 128)
        self.assertEqual(len(reduced["probabilityModels"]), 8)
        def distribution(rows, model):
            out = {}
            for row, weight in zip(rows, model["weightsPpm"]):
                signature = canonical([row["kind"], row["payouts"]])
                out[signature] = out.get(signature, 0)+weight
            return out
        for before, after in zip(wide["probabilityModels"], reduced["probabilityModels"]):
            self.assertEqual(before["id"], after["id"])
            self.assertEqual(distribution(wide["scenarios"], before),
                             distribution(reduced["scenarios"], after))
        for invalid in [True, 127, 4097]:
            with self.assertRaisesRegex(ValueError, "RESEARCH_SCENARIO_LIMIT"):
                compile_scenarios(*a, 150, research_scenario_limit=invalid)


if __name__ == "__main__":
    unittest.main()
