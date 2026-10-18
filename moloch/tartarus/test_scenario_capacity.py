import itertools
import unittest

from moloch.tartarus import scenario_capacity as C


def fixture():
    registry=[{"exchange_id":"a","contract_id":"1"},{"exchange_id":"b","contract_id":"2"}]
    holdings=[dict(registry[0],side="YES",quantity=2),dict(registry[1],side="NO",quantity=1)]
    books={"a":{"bids":[{"price_micros":200000,"quantity":3}],"asks":[{"price_micros":300000,"quantity":3}]},
           "b":{"bids":[{"price_micros":600000,"quantity":3}],"asks":[{"price_micros":700000,"quantity":3}]}}
    scenarios=[{"id":str(i),"kind":"ordinary","payouts":{"1":a,"2":b}}
        for i,(a,b) in enumerate(itertools.product(["resolved_yes","resolved_no"],repeat=2))]
    return dict(cash_micros=500000,holdings=holdings,registry=registry,books=books,scenarios=scenarios)


class CapacityTests(unittest.TestCase):
    def test_oracle_upper_dominates_every_tiny_integer_sale_purchase_portfolio(self):
        a=fixture();r=C.ordinary_oracle(**a)
        for s,bound in zip(a["scenarios"],r["rows"]):
            held_wins=[s["payouts"]["1"]=="resolved_yes",s["payouts"]["2"]=="resolved_no"]
            buy_prices=[300000 if s["payouts"]["1"]=="resolved_yes" else 800000,
                        700000 if s["payouts"]["2"]=="resolved_yes" else 400000]
            best=0
            for sa,sb,ba,bb in itertools.product(range(3),range(2),range(4),range(4)):
                cash=500000+sa*200000+sb*300000-ba*buy_prices[0]-bb*buy_prices[1]
                if cash<0:continue
                wealth=cash+(2-sa)*1000000*held_wins[0]+(1-sb)*1000000*held_wins[1]+(ba+bb)*1000000
                best=max(best,wealth)
            self.assertGreaterEqual(bound["upper_wealth_floor_micros"],best)
        self.assertFalse(r["modeled_sale_proceeds_are_actual_cash"])

    def test_losing_held_sale_raises_funding_without_fake_terminal_sacrifice(self):
        a=fixture();r=C.ordinary_oracle(**a)
        # Both held sides lose in this state: sell YES at.2 and NO at.3.
        row=next(x for x in r["rows"] if x["id"]=="2")
        self.assertEqual(row["original_winning_held_quantity"],0)
        self.assertEqual(row["conditional_losing_sale_proceeds_micros"],700000)
        self.assertEqual(row["upper_wealth_floor_micros"],12000000//7)

    def test_unknown_refund_probability_mass_is_unbounded_not_zero(self):
        a=fixture();a["scenarios"]=[dict(a["scenarios"][0],kind="exceptional")]
        r=C.ordinary_oracle(**a)
        p=C.first_place_upper(r,[{"id":"p","weightsPpm":[1000000]}],
            [{"id":"f","completeField":False,"thresholds":["10000000.000000"]}])
        self.assertEqual(p["pairs"][0]["staticOracleFirstPlaceMassUpperPpm"],1000000)
        self.assertEqual(p["pairs"][0]["unboundedExceptionalMassPpm"],1000000)
        a["scenarios"][0].update(kind="ordinary",payouts={"1":"refund_principal","2":"resolved_no"})
        self.assertIsNone(C.ordinary_oracle(**a)["rows"][0]["upper_wealth_floor_micros"])

    def test_strict_ties_and_every_probability_field_pair_are_retained(self):
        capacity={"rows":[{"id":"a","upper_wealth_floor_micros":1000000},
                          {"id":"b","upper_wealth_floor_micros":1000001}]}
        r=C.first_place_upper(capacity,[{"id":"p","weightsPpm":[999999,1]},
             {"id":"q","weightsPpm":[1,999999]}],
             [{"id":"flat","completeField":False,"thresholds":["1","1"]},
              {"id":"high","completeField":False,"thresholds":["2","2"]}])
        self.assertEqual([x["staticOracleFirstPlaceMassUpperPpm"] for x in r["pairs"]],[1,999999,0,0])

    def test_missing_holdings_crossed_books_and_bad_weight_mass_reject(self):
        a=fixture();a["holdings"][0]["contract_id"]="missing"
        with self.assertRaisesRegex(ValueError,"UNMODELED"):C.ordinary_oracle(**a)
        a=fixture();a["books"]["a"]["bids"][0]["price_micros"]=400000
        with self.assertRaisesRegex(ValueError,"CROSSED"):C.ordinary_oracle(**a)
        r=C.ordinary_oracle(**fixture())
        with self.assertRaisesRegex(ValueError,"MASS"):
            C.first_place_upper(r,[{"id":"p","weightsPpm":[1,1,1,1]}],
                [{"id":"f","completeField":False,"thresholds":["1"]*4}])


if __name__=="__main__":
    unittest.main()
