import copy
import unittest

from moloch.tartarus import scenario_projection as P
from moloch.tartarus.party_scenarios import compile_scenarios
from moloch.tartarus.test_party_scenarios import fixture


def matrix():
    rows=[]
    for i in range(12):
        payouts={str(c):"resolved_yes" if (c+i)%3 else "resolved_no" for c in range(139)}
        payouts["0"]="resolved_yes" if i%2 else "resolved_no"
        rows.append({"id":str(i),"kind":"ordinary","assumptionLabel":"Synthetic full-universe state",
                     "payouts":payouts})
    models=[{"id":"a","assumptionLabel":"Synthetic a","weightsPpm":[100000]*10+[0,0]},
            {"id":"b","assumptionLabel":"Synthetic b","weightsPpm":[0]*10+[1,999999]}]
    fields=[{"id":"flat","completeField":False,"assumptionLabel":"Synthetic flat opponent",
             "thresholds":["100.000000"]*12},
            {"id":"correlated","completeField":False,"assumptionLabel":"Synthetic dependent opponent",
             "thresholds":["110.000000" if i%3 else "90.000000" for i in range(12)]}]
    return rows,models,fields


class ProjectionTests(unittest.TestCase):
    def test_all_supplied_model_joint_payout_field_distributions_survive(self):
        rows,models,fields=matrix();saved=copy.deepcopy((rows,models,fields))
        r=P.project(rows,models,fields,retained_contract_ids=["0","1"],complete_held_contract_ids=["0"])
        self.assertLess(len(r["scenarios"]),len(rows))
        def distribution(rs,ms,fs,subset):
            out={}
            for i,row in enumerate(rs):
                key=(row["kind"],tuple(row["payouts"][c] for c in subset),tuple(f["thresholds"][i] for f in fs))
                out[key]=out.get(key,0)+ms["weightsPpm"][i]
            return out
        for before,after in zip(models,r["probabilityModels"]):
            self.assertEqual(before["id"],after["id"])
            self.assertEqual(distribution(rows,before,fields,["0","1"]),
                             distribution(r["scenarios"],after,r["fieldModels"],["0","1"]))
        self.assertEqual((rows,models,fields),saved)
        self.assertEqual(r["projection"]["originalContractCount"],139)
        self.assertFalse(r["projection"]["executionAuthority"])

    def test_omitting_a_holding_or_exceeding_native_scope_rejects(self):
        args=matrix()
        for retained,held,error in [(["1"],["0"],"OMITS_HOLDING"),
            ([str(i) for i in range(65)],["0"],"PROJECTION_SCOPE"),
            (["0","0"],["0"],"PROJECTION_SCOPE"),(["0"],None,"HELD_SCOPE_REQUIRED")]:
            with self.assertRaisesRegex(ValueError,error):
                P.project(*args,retained_contract_ids=retained,complete_held_contract_ids=held)

    def test_incomplete_original_scope_and_latent_fields_reject(self):
        for mutation in ["missing","latent"]:
            rows,models,fields=matrix()
            if mutation=="missing":del rows[0]["payouts"]["138"]
            else:rows[0]["future_latent_state"]="must not disappear"
            with self.assertRaises(ValueError):
                P.project(rows,models,fields,retained_contract_ids=["0"],complete_held_contract_ids=["0"])

    def test_field_dependence_can_prevent_native_compression(self):
        rows,models,fields=matrix()
        rows=[dict(rows[0],id=str(i)) for i in range(129)]
        models=[dict(models[0],weightsPpm=[1]*128+[999872])]
        fields=[dict(fields[0],thresholds=[str(i) for i in range(129)])]
        with self.assertRaisesRegex(ValueError,"EXCEEDS_NATIVE_BOUND"):
            P.project(rows,models,fields,retained_contract_ids=["0"],complete_held_contract_ids=["0"])

    def test_wide_contract_research_is_explicit_and_does_not_expand_native_defaults(self):
        args=fixture(65)
        with self.assertRaisesRegex(ValueError,"REGISTRY_BOUND"):
            compile_scenarios(*args,150)
        wide=compile_scenarios(*args,150,research_contract_limit=512,research_scenario_limit=4096)
        self.assertTrue(wide["requiresCompleteHoldingPreservingProjection"])
        self.assertFalse(wide["executionAuthority"])
        self.assertTrue(all(len(r["payouts"])==65 for r in wide["scenarios"]))
        for invalid in [True,63,513]:
            with self.assertRaisesRegex(ValueError,"RESEARCH_CONTRACT_LIMIT"):
                compile_scenarios(*args,150,research_contract_limit=invalid)


if __name__ == "__main__":
    unittest.main()
