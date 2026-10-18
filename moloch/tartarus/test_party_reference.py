import copy
import json
import unittest

from moloch.tartarus import party_reference as R


def fixture():
    root = {"node_type":"contract", "node_id":"379", "contract_id":"379",
        "contract_details":{"resolutionType":"Party Winner", "raceStage":"General",
        "raceName":"U.S. Senate Alaska", "officeLevel":"Federal", "usState":"AK",
        "electionDate":"2026-11-03", "winnerName":"Republican Party"}}
    raw = R.canonical({"status":200, "data":{"market_id":"378", "root":root}})
    row = {"exchangeId":"1067", "marketId":"378", "contractId":"379",
        "semanticSourceSha256":R.sha(raw), "officialContractRootSha256":R.sha(R.canonical(root))}
    item = {"ticker":"SENATEAK-26-R", "office":"Senate", "state":"AK", "district":None,
            "party":"Republican", "electionYear":2026, "sigMarketIds":["wrong-legacy-id"]}
    def receipt(path, body, **extra):
        text = json.dumps(body)
        return R.canonical(dict(source="kalshi_official_v2", path=path, status=200,
            requestStartedAt="2026-10-18T07:00:00Z", receivedAt="2026-10-18T07:00:01Z",
            bodyText=text, bodySha256=R.sha(text.encode()), body=body, **extra))
    ticker = item["ticker"]
    quote = receipt(f"/trade-api/v2/markets/{ticker}/orderbook?depth=20",
        {"orderbook_fp":{"yes_dollars":[["0.2500","10"],["0.9","0"]],
                         "no_dollars":[["0.7400","5"]]}}, ticker=ticker)
    metadata = receipt(f"/trade-api/v2/markets/{ticker}", {"market":{"ticker":ticker,
        "status":"active", "result":"", "rules_primary":
        "If a representative of the Republican party wins in the 2026 election, then Yes."}})
    return row, {R.sha(raw):raw}, [item], {ticker:quote}, {ticker:metadata}


class PartyReferenceTests(unittest.TestCase):
    def run_observation(self, args, **changes):
        clocks = dict(decision_ms=R.clock("2026-10-18T07:00:02Z"),
            maximum_receipt_age_ms=30000, maximum_metadata_age_ms=3600000)
        clocks.update(changes)
        return R.observe(*args, **clocks)

    def test_market_contract_exchange_ids_stay_distinct(self):
        r = self.run_observation(fixture())
        self.assertEqual((r["marketId"], r["contractId"], r["exchangeId"]), ("378","379","1067"))
        self.assertEqual((r["yesBid"],r["yesAsk"]), ("0.2500","0.2600"))
        self.assertFalse(r["executionAuthority"])
        self.assertFalse(r["calibratedProbabilityClaimed"])
        self.assertFalse(r["sourceTimestampAvailable"])

    def test_wrong_party_state_office_district_and_year_do_not_match(self):
        for k,v in [("party","Democratic"),("state","MA"),("office","Governor"),
                    ("district",1),("electionYear",2027)]:
            a = list(fixture());a[2][0][k] = v
            with self.assertRaisesRegex(ValueError,"REFERENCE_IDENTITY_MISSING_OR_AMBIGUOUS"):
                self.run_observation(a)

    def test_duplicate_identity_is_not_arbitrarily_selected(self):
        a = list(fixture());a[2].append(copy.deepcopy(a[2][0]))
        with self.assertRaisesRegex(ValueError,"AMBIGUOUS"):
            self.run_observation(a)

    def test_house_district_keeps_exact_canonical_catalog_string(self):
        a = list(fixture());root=json.loads(a[1][a[0]["semanticSourceSha256"]])["data"]["root"]
        root["contract_details"]["raceName"]="U.S. House Alaska District 1"
        raw=R.canonical({"status":200,"data":{"market_id":"378","root":root}})
        a[0]["semanticSourceSha256"]=R.sha(raw);a[0]["officialContractRootSha256"]=R.sha(R.canonical(root));a[1][R.sha(raw)]=raw
        a[2][0].update(office="House",district="1")
        self.assertEqual(self.run_observation(a)["identity"][2],"1")
        a[2][0]["district"]="01"
        with self.assertRaisesRegex(ValueError,"IDENTITY_MISSING"):
            self.run_observation(a)

    def test_original_root_and_body_changes_reject(self):
        a = list(fixture());a[0]["marketId"] = "379"
        with self.assertRaisesRegex(ValueError,"OFFICIAL_IDENTITY_CHANGED"):
            self.run_observation(a)
        for index in [3,4]:
            a = list(fixture());key = next(iter(a[index]));v = json.loads(a[index][key])
            v["bodyText"] += " "
            a[index][key] = R.canonical(v)
            with self.assertRaisesRegex(ValueError,"REFERENCE_BODY_CHANGED"):
                self.run_observation(a)

    def test_freshness_uses_request_start_and_never_renews_at_invocation(self):
        with self.assertRaisesRegex(ValueError,"QUOTE_STALE"):
            self.run_observation(fixture(), maximum_receipt_age_ms=1500)
        with self.assertRaisesRegex(ValueError,"FUTURE"):
            self.run_observation(fixture(), decision_ms=R.clock("2026-10-18T06:59:59Z"))
        with self.assertRaisesRegex(ValueError,"METADATA_STALE"):
            self.run_observation(fixture(), maximum_metadata_age_ms=1500)

    def test_rule_year_and_party_conflicts_reject_but_suffix_is_not_event_year(self):
        for old,new,error in [("2026 election","2027 election","YEAR_CONFLICT"),
                              ("Republican party","Democratic party","PARTY_CONFLICT")]:
            a = list(fixture());key = next(iter(a[4]));v = json.loads(a[4][key]);v["body"]["market"]["rules_primary"] = v["body"]["market"]["rules_primary"].replace(old,new)
            v["bodyText"] = json.dumps(v["body"]);v["bodySha256"] = R.sha(v["bodyText"].encode());a[4][key] = R.canonical(v)
            with self.assertRaisesRegex(ValueError,error):self.run_observation(a)
        # A ticker suffix alone must not override the original election rule.
        a = list(fixture());old = a[2][0]["ticker"];new = "SENATEAK-27-R"
        a[2][0]["ticker"] = new
        for i in [3,4]:a[i][new] = a[i].pop(old).replace(old.encode(),new.encode())
        # Re-pin the deliberately changed synthetic bodies.
        for i in [3,4]:
            v=json.loads(a[i][new]);v["bodySha256"]=R.sha(v["bodyText"].encode());a[i][new]=R.canonical(v)
        self.assertEqual(self.run_observation(a)["explicitRuleElectionYears"],["2026"])

    def test_census_retains_missing_sources_and_rejects_duplicate_registry(self):
        a = list(fixture());a[3] = {}
        r = R.census([a[0]], *a[1:], decision_ms=R.clock("2026-10-18T07:00:02Z"),
            maximum_receipt_age_ms=30000, maximum_metadata_age_ms=3600000)
        self.assertEqual(r["rows"],[]);self.assertEqual(len(r["unavailable"]),1)
        with self.assertRaisesRegex(ValueError,"REGISTRY_DUPLICATE"):
            R.census([a[0],a[0]], *a[1:], decision_ms=0, maximum_receipt_age_ms=1,
                     maximum_metadata_age_ms=1)


if __name__ == "__main__":
    unittest.main()
