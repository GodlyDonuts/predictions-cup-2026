import copy
from decimal import Decimal
import json
from pathlib import Path
import tempfile
import unittest

import analyze as a


def forecast(rows):
    article = {"@type": "AnalysisNewsArticle", "url": "https://www.callthemap.com/senate/iowa", "dateModified": "2026-10-16T17:57:26Z"}
    raw = '<script type="application/ld+json">'+json.dumps(article)+'</script>'
    for name, party, p, share in rows:
        raw += f'<div class="mn-card-row" data-side="{party.lower()}"><span class="mn-card-cand"><span>{name}<small> ( {party} )</small></span></span><span class="mn-card-chance"><b>50 in 100</b><small>{p}</small></span><span class="mn-card-share"><b>{share}</b></span></div>'
    return raw.encode()


PIN = {"url": "https://www.callthemap.com/senate/iowa", "receivedAt": "2026-10-16T22:00:00Z"}


def node():
    item = {"marketId": "260", "path": "nodes/n.json", "sha256": "a"*64, "bytes": 1,
            "endpoint": f"/markets/260/nodes?tournamentId={a.TID}", "requestStartedAt": "2026-10-16T21:00:00Z", "receivedAt": "2026-10-16T21:00:01Z", "status": 200}
    r = {k: item[k] for k in ["endpoint", "requestStartedAt", "receivedAt", "status"]}
    r["data"] = {"market_id": "260", "contexts": [{"type":"tournament","tournament":{"id":a.TID}}], "root": {
        "node_id":"261","contract_id":"261","node_type":"contract","settlement_date":"2026-11-04T17:00:00Z", "settled_with":None,
        "contract_details":{"contractType":"Election Outcome","electionDate":"2026-11-03","raceName":"U.S. Senate Iowa","officeLevel":"Federal","raceStage":"General","resolutionType":"Party Winner","usState":"IA","winnerName":"Democratic Party"}}}
    return r,item


def book():
    item = {"path":"orderbook/b.json","sha256":"b"*64,"bytes":1,"endpoint":f"/markets/260/orderbook?tournamentId={a.TID}&depth=20", "requestStartedAt":"2026-10-16T21:00:00Z","receivedAt":"2026-10-16T21:00:01Z","status":200}
    exchange = {"exchangeId":"949","option":"YES","asOf":{"at":"2026-10-16T20:59:59Z","sequence":123},"bids":[{"price":Decimal(".46"),"quantity":13}],"asks":[{"price":Decimal(".47"),"quantity":7}]}
    r = {k:item[k] for k in ["endpoint","requestStartedAt","receivedAt","status"]}
    r["data"] = {"exchanges":[exchange],"contexts":[{"type":"tournament","tournament":{"id":a.TID},"orderbook":{"exchanges":[copy.deepcopy(exchange)]}}]}
    return r,item


class CoverageTests(unittest.TestCase):
    def test_original_candidate_win_table_is_not_vote_share_or_market_quote(self):
        raw = forecast([("A &amp; B", "D", "43.2%", "99.9"),("C", "R", "56.8%", "0.1")])
        raw += b'<script>43.2% = 100%</script><div>Market probability 95%</div>'
        got = a.parse_forecast(raw,PIN)["candidates"]
        self.assertEqual(got[0]["name"],"A & B")
        self.assertEqual(got[0]["candidateWinModelPoint"],"0.432")
        self.assertEqual(got[1]["candidateWinModelPoint"],"0.568")

    def test_censored_and_unpublished_probabilities_stay_unimputed(self):
        f = a.parse_forecast(forecast([("Safe","D","&gt;99.9%","60"),("Rare","R","&lt;0.1%","39"),("Other","O","—","1")]),PIN)
        self.assertIsNone(f["candidates"][0]["candidateWinModelPoint"])
        self.assertEqual(f["candidates"][0]["strictLowerIfCensored"],"0.999")
        self.assertEqual(f["candidates"][1]["strictUpperIfCensored"],"0.001")
        self.assertTrue(f["candidates"][2]["unpublishedProbability"])
        self.assertEqual(a.proposals(f,{"party":"Republican Party"},None),[])

    def test_nonparty_and_second_same_party_candidate_are_not_one_minus_opponent(self):
        f = a.parse_forecast(forecast([("D","D","49.3%","48"),("R1","R","50.0%","47"),("R2","R","0.2%","2"),("I","I","0.1%","1"),("Other","O","—","2")]),PIN)
        sides = a.proposals(f,{"party":"Republican Party"},None)
        self.assertEqual(sides[0]["ordinaryCandidateSubsetModelPoint"],"0.502")
        self.assertEqual(sides[1]["ordinaryCandidateSubsetModelPoint"],"0.494")
        self.assertNotEqual(Decimal(sides[1]["ordinaryCandidateSubsetModelPoint"]),1-Decimal(".502"))

    def test_published_clock_and_exact_url_are_bound(self):
        raw = forecast([("A","D","50%","50"),("B","R","50%","50")])
        for bad in [{**PIN,"receivedAt":"2026-10-16T17:00:00Z"},{**PIN,"url":"https://www.callthemap.com/senate/kansas"}]:
            with self.assertRaisesRegex(ValueError,"scope_or_clock"):
                a.parse_forecast(raw,bad)

    def test_node_identity_state_party_and_scope_are_structured(self):
        r,i = node();got=a.parse_node(r,i)
        self.assertEqual((got["marketId"],got["contractId"],got["state"],got["party"]),("260","261","IA","Democratic Party"))
        for mutate in [lambda x:x["data"]["root"].update(contract_id="262"), lambda x:x["data"]["root"]["contract_details"].update(usState="KS"),lambda x:x["data"]["contexts"][0]["tournament"].update(id="elsewhere"), lambda x:x.update(requestStartedAt="2026-10-16T22:00:00Z")]:
            v=copy.deepcopy(r);mutate(v)
            with self.assertRaises(ValueError):a.parse_node(v,i)
        v=copy.deepcopy(r);v["endpoint"]+="&invented=true"
        with self.assertRaisesRegex(ValueError,"node_endpoint_or_clock"):a.parse_node(v,{**i,"endpoint":v["endpoint"]})

    def test_semantic_stage_does_not_fall_back_to_title(self):
        r,i=node();r["data"]["root"]["title"]="Will the Democratic Party win Iowa?"
        r["data"]["root"]["contract_details"]["raceStage"]="Primary"
        with self.assertRaisesRegex(ValueError,"node_semantics"):a.parse_node(r,i)

    def test_exact_complement_and_original_depths_clocks_retained(self):
        r,i=book();got=a.reference_book(r,i,"260")
        self.assertEqual((got["yesAsk"],got["yesAskDepth"],got["noAskComplement"],got["noAskDepth"]),("0.47","7","0.54","13"))
        self.assertEqual(got["originalAsOf"]["at"],"2026-10-16T20:59:59Z")
        self.assertFalse(got["currentExecutable"])

    def test_book_wrong_scope_query_conflict_and_future_original_are_refused(self):
        original,i=book()
        for mutate in [lambda x:x.update(endpoint=x["endpoint"]+"&invented=true"),lambda x:x["data"]["contexts"][0]["tournament"].update(id="elsewhere"),lambda x:x["data"]["contexts"][0]["orderbook"]["exchanges"][0]["asOf"].update(at="2026-10-16T21:01:00Z"),lambda x:x["data"]["exchanges"][0].update(exchangeId="950")]:
            r=copy.deepcopy(original);mutate(r)
            with self.assertRaises(ValueError):a.reference_book(r,i,"260")

    def test_missing_one_sided_depth_or_non_native_tick_is_unavailable(self):
        r,i=book()
        for levels in [[],[{"price":Decimal(".471"),"quantity":10}],[{"price":Decimal(".47"),"quantity":0}]]:
            v=copy.deepcopy(r)
            v["data"]["exchanges"][0]["asks"]=levels
            v["data"]["contexts"][0]["orderbook"]["exchanges"][0]["asks"]=copy.deepcopy(levels)
            with self.assertRaisesRegex(ValueError,"book_levels"):a.reference_book(v,i,"260")

    def test_owner_style_stress_and_tick_floor_are_explicit_not_probabilities(self):
        f=a.parse_forecast(forecast([("A","D","33.2%","49"),("B","R","66.8%","51")]),PIN)
        result=a.proposals(f,{"party":"Democratic Party"},{"yesAsk":".14","noAskComplement":".86","yesAskDepth":"8372","noAskDepth":"1"})[0]
        self.assertEqual(result["tickFlooredResearchCeiling"],"0.310")
        self.assertEqual(result["ceilingMinusReferenceAsk"],"0.170")
        self.assertEqual(result["exceptionAllowanceAssumption"],"0.03")

    def test_original_source_hash_and_path_are_not_caller_assertions(self):
        with tempfile.TemporaryDirectory() as tmp:
            p=Path(tmp);(p/"a").write_bytes(b"original")
            entry={"file":"a","bytes":8,"sha256":a.sha(b"original")}
            self.assertEqual(a.pinned(p,entry),b"original")
            (p/"a").write_bytes(b"tampered")
            with self.assertRaises(ValueError):a.pinned(p,entry)
            with self.assertRaises(ValueError):a.pinned(p,{**entry,"file":"../a"})


if __name__ == "__main__":
    unittest.main()
