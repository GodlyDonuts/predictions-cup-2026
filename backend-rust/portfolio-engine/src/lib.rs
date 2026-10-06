pub mod history;
pub mod publication;
pub use history::{import_history,import_historical_base_for_overlay,ImportEvidence};
pub mod dual_v1;
pub mod money;
pub mod settlement_value;
pub use baal_dual_no_engine as dual_no;
use serde_json::{Value as V, json};
use sha2::{Digest, Sha256};
pub type Result<T> = std::result::Result<T, String>;
pub fn engine_hash() -> &'static str {
    env!("PORTFOLIO_ENGINE_HASH")
}
pub struct PortfolioEngine {
    bank: V,
    state: V,
}
impl PortfolioEngine {
    pub fn new(bank: V) -> Result<Self> {
        let state = create_state(&bank)?;
        Ok(Self { bank, state })
    }
    pub fn restore(bank: V, state: V) -> Result<Self> {
        validate_bank(&bank)?;
        assert_state(&state, &bank)?;
        Ok(Self { bank, state })
    }
    pub fn apply(&mut self, row: &V, available_at: &str, account: Option<&V>) -> Result<V> {
        let next = advance(&self.state, row, &self.bank, available_at, account)?;
        self.state = next;
        Ok(self.state.clone())
    }
    pub fn apply_with_entry_permission(
        &mut self,
        row: &V,
        available_at: &str,
        account: Option<&V>,
        allow_new_entries: bool,
    ) -> Result<V> {
        let next = advance_with_entry_permission(
            &self.state,
            row,
            &self.bank,
            available_at,
            account,
            allow_new_entries,
        )?;
        self.state = next;
        Ok(self.state.clone())
    }
    pub fn state(&self) -> &V {
        &self.state
    }
    pub fn report(&self, at: &str) -> Result<V> {
        report(&self.state, &self.bank, at)
    }
    pub fn bank(&self) -> &V {
        &self.bank
    }
}
fn need(v: bool, c: &str) -> Result<()> {
    if v {
        Ok(())
    } else {
        Err(format!("SETTLEMENT_PORTFOLIO_{c}"))
    }
}
fn s(v: &V) -> &str {
    v.as_str().unwrap_or("")
}
fn n(v: &V) -> i64 {
    v.as_i64().unwrap_or(0)
}
fn array(v: &V) -> Vec<V> {
    v.as_array().cloned().unwrap_or_default()
}
fn index(k: &str) -> Option<u32> {
    let v = k.parse::<u32>().ok()?;
    if v < u32::MAX && v.to_string() == k {
        Some(v)
    } else {
        None
    }
}
fn entries(v: &V) -> Vec<(String, V)> {
    let mut a: Vec<_> = v
        .as_object()
        .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();
    a.sort_by(|(a, _), (b, _)| match (index(a), index(b)) {
        (Some(a), Some(b)) => a.cmp(&b),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        _ => std::cmp::Ordering::Equal,
    });
    a
}
pub fn js_stringify(v: &V) -> String {
    fn write(v: &V, out: &mut String) {
        match v {
            V::Object(_) => {
                out.push('{');
                for (i, (k, v)) in entries(v).iter().enumerate() {
                    if i > 0 {
                        out.push(',')
                    }
                    out.push_str(&serde_json::to_string(k).unwrap());
                    out.push(':');
                    write(v, out)
                }
                out.push('}')
            }
            V::Array(a) => {
                out.push('[');
                for (i, v) in a.iter().enumerate() {
                    if i > 0 {
                        out.push(',')
                    }
                    write(v, out)
                }
                out.push(']')
            }
            V::Number(n) => {
                let mut b = ryu_js::Buffer::new();
                out.push_str(b.format_finite(n.as_f64().unwrap()));
            }
            _ => out.push_str(&serde_json::to_string(v).unwrap()),
        }
    }
    let mut out = String::new();
    write(v, &mut out);
    out
}
pub fn digest(v: &V) -> String {
    format!("{:x}", Sha256::digest(js_stringify(v).as_bytes()))
}
pub fn canonical_hash(v: &V) -> String {
    dual_no::hash(v)
}
fn a(v: &V) -> Result<i128> {
    money::amount(s(v))
}
fn m(x: i128) -> V {
    json!(money::money(x))
}
fn time(v: &V) -> Result<i64> {
    let t = chrono::DateTime::parse_from_rfc3339(s(v)).map_err(|_| "SETTLEMENT_PORTFOLIO_TIME")?;
    let ms = t.timestamp_millis();
    need(
        t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true) == s(v),
        "TIME",
    )?;
    Ok(ms)
}
pub fn canonical_js_stringify(v: &V) -> String {
    js_stringify(v)
}
pub fn forecast_metadata(bank: &V) -> V {
    json!({"experiment":bank["experiment"],"pointForecastsAreNotConfidenceBounds":true,"research":array(&bank["research"]).into_iter().map(|r|json!({"id":r["id"],"marketId":r["marketId"],"lower":r["lower"],"upper":r["upper"],"kind":if r["lower"]==r["upper"]{"expected_value_point_not_confidence_bound"}else{"supplied_scenario_range_not_guaranteed_confidence_bound"},"sourceAt":r["sourceAt"],"recordedAt":r["recordedAt"],"expiresAt":r["expiresAt"]})).collect::<Vec<_>>(),"executionAuthority":false,"profitabilityEstablished":false})
}
fn hex(v: &V) -> bool {
    let x = s(v);
    x.len() == 64
        && x.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
fn numeric_id(v: &V) -> bool {
    let x = s(v);
    !x.is_empty()
        && !x.starts_with('0')
        && x.bytes().all(|c| c.is_ascii_digit())
        && x.parse::<u64>().is_ok_and(|n| n <= 9007199254740991)
}
fn fill_policy() -> V {
    json!({"initialCash":"100.000000","fees":0,"sharesPerOrder":1,"maxPositionShares":1,"latenciesMs":[1000,3000],"intentLifetimeMs":180000,"cooldownMs":900000})
}
pub const RECEIPT_BASELINE_MODE:&str="funded_settlement_portfolio_receipt_baseline_v3";
pub fn receipt_fill_policy()->V{let mut p=fill_policy();p["fillAdverseTicks"]=json!(0);p["liquidationAdverseTicks"]=json!(0);p}
fn receipt_baseline(bank:&V)->bool{bank["mode"]==RECEIPT_BASELINE_MODE}
pub fn event_group(root: &V) -> Result<String> {
    let p = &root["contract_details"];
    let keys = [
        "raceId",
        "stageId",
        "usState",
        "raceStage",
        "officeLevel",
        "electionDate",
        "resolutionType",
    ];
    need(keys.iter().all(|k| !s(&p[*k]).is_empty()), "EVENT")?;
    let mut v = json!({});
    for k in keys {
        v[k] = p[k].clone();
    }
    Ok(format!("race_{}", &canonical_hash(&v)[..24]))
}
pub fn portfolio_policy(evidence: &V) -> Result<V> {
    let mut groups = json!({"national_election":"3.000000"});
    for e in array(evidence) {
        groups[&event_group(&e["root"])?] = json!("1.000000");
    }
    Ok(
        json!({"minimumEdge":"0.020000","maximumSpread":"0.020000","maximumBookAgeMs":20000,"maximumResearchAgeMs":21600000,"fractionalKellyBps":2500,"slippageTicks":1,"depthFractionBps":100,"maxOrderCost":"1.000000","maxExposure":"3.000000","cashReserve":"90.000000","cumulativeBuyBudget":"100.000000","groupCaps":groups}),
    )
}
fn validate_research(r: &V) -> Result<()> {
    let keys = [
        "schemaVersion",
        "id",
        "marketId",
        "exchangeId",
        "side",
        "predicateHash",
        "recordedAt",
        "sourceAt",
        "expiresAt",
        "lower",
        "upper",
        "method",
        "settlementReviewed",
        "flags",
        "sources",
    ];
    need(
        r.as_object()
            .is_some_and(|m| m.len() == keys.len() && keys.iter().all(|k| m.contains_key(*k))),
        "RESEARCH_FIELDS",
    )?;
    let id = s(&r["id"]);
    need(
        r["schemaVersion"] == 1
            && !id.is_empty()
            && id.len() <= 80
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
            && numeric_id(&r["marketId"])
            && numeric_id(&r["exchangeId"])
            && r["side"] == "YES"
            && hex(&r["predicateHash"])
            && (r["method"] == "subjective_range" || r["method"] == "calibrated_model")
            && r["settlementReviewed"].is_boolean(),
        "RESEARCH_IDENTITY",
    )?;
    for key in ["lower", "upper"] {
        let v = s(&r[key]);
        need(
            v == "1.000000"
                || (v.len() == 8
                    && v.starts_with("0.")
                    && v[2..].bytes().all(|x| x.is_ascii_digit())),
            "PROBABILITY",
        )?;
    }
    need(
        a(&r["lower"])? <= a(&r["upper"])?
            && time(&r["sourceAt"])? <= time(&r["recordedAt"])?
            && time(&r["expiresAt"])? > time(&r["recordedAt"])?,
        "RESEARCH_CHRONOLOGY",
    )?;
    let flags = array(&r["flags"]);
    let mut seen = std::collections::HashSet::new();
    need(
        r["flags"].is_array()
            && flags.iter().all(|f| {
                [
                    "predicate_uncertain",
                    "settlement_uncertain",
                    "source_conflict",
                    "superseded",
                ]
                .contains(&s(f))
                    && seen.insert(s(f))
            }),
        "RESEARCH_FLAGS",
    )?;
    need(
        r["sources"].is_array() && !array(&r["sources"]).is_empty(),
        "RESEARCH_SOURCES",
    )?;
    for source in array(&r["sources"]) {
        let url = s(&source["url"]);
        need(
            source.as_object().is_some_and(|m| {
                m.len() == 2 && m.contains_key("url") && m.contains_key("contentHash")
            }) && url.starts_with("https://")
                && !url[8..].is_empty()
                && !url.contains(['?', '#', '@'])
                && !url.contains("ace_")
                && hex(&source["contentHash"]),
            "RESEARCH_REFERENCE",
        )?;
    }
    Ok(())
}
pub fn validate_bank(b: &V) -> Result<()> {
    if b["mode"]=="funded_settlement_value_paper_v1" {return validate_legacy_settlement_bank(b)}
    let research = array(&b["research"]);
    let evidence = array(&b["researchEvidence"]);
    need(
        ((b["schemaVersion"] == 2 && b["mode"] == "funded_settlement_portfolio_paper_v2")
            || (b["schemaVersion"]==3&&receipt_baseline(b)))
            && (1..=3).contains(&research.len()),
        "BANK",
    )?;
    need(
        evidence.len() == research.len() && entries(&b["identities"]).len() == research.len(),
        "SCOPE",
    )?;
    need(
        digest(&b["policy"]) == digest(&portfolio_policy(&b["researchEvidence"])?)
            && digest(&b["fillPolicy"]) == digest(&if receipt_baseline(b){receipt_fill_policy()}else{fill_policy()}),
        "POLICY",
    )?;
    time(&b["cutoff"])?;
    let mut seen = std::collections::HashSet::new();
    let mut probabilities = std::collections::BTreeMap::<
        String,
        std::collections::BTreeMap<String, (i128, i128)>,
    >::new();
    for r in research {
        validate_research(&r)?;
        let e = evidence
            .iter()
            .find(|e| e["researchId"] == r["id"] && e["marketId"] == r["marketId"])
            .ok_or("SETTLEMENT_PORTFOLIO_EVIDENCE")?;
        let root = &e["root"];
        let id = &b["identities"][s(&r["marketId"])];
        need(
            seen.insert(s(&r["marketId"]).to_owned())
                && time(&r["recordedAt"])? <= time(&b["cutoff"])?
                && id["exchangeId"] == r["exchangeId"]
                && id["predicateHash"] == r["predicateHash"]
                && array(&b["universe"]["exchanges"]).iter().any(|x| {
                    x["marketId"] == r["marketId"]
                        && x["exchangeId"] == r["exchangeId"]
                        && x["option"] == "YES"
                }),
            "RESEARCH_BINDING",
        )?;
        need(
            root["node_type"] == "contract"
                && root["contract_type"] == "Election Outcome"
                && root.get("settled_with") == Some(&V::Null)
                && root["contract_details"]["raceStage"] == "General"
                && root["contract_details"]["resolutionType"] == "Party Winner"
                && canonical_hash(root) == s(&r["predicateHash"])
                && e["rootHash"] == r["predicateHash"],
            "EXACT_ROOT",
        )?;
        need(
            root["contract_id"] == root["node_id"]
                && numeric_id(&root["contract_id"])
                && time(&root["settlement_date"])? == n(&b["valuationDeadlineMs"])
                && n(&b["valuationDeadlineMs"]) > time(&b["cutoff"])?,
            "RESOLUTION_HORIZON",
        )?;
        let group = event_group(root)?;
        need(
            b["riskGroups"][s(&r["marketId"])] == json!(["national_election", group]),
            "EVENT_GROUP_BINDING",
        )?;
        let winner = s(&root["contract_details"]["winnerName"]);
        need(!winner.is_empty(), "WINNER")?;
        let value = (a(&r["lower"])?, a(&r["upper"])?);
        let slot = probabilities
            .entry(group)
            .or_default()
            .entry(winner.to_owned())
            .or_insert(value);
        slot.0 = slot.0.max(value.0);
        slot.1 = slot.1.min(value.1);
    }
    for winners in probabilities.values() {
        need(
            winners.values().all(|(lo, hi)| lo <= hi),
            "CONFLICTING_DUPLICATE_VALUE",
        )?;
        need(
            winners.values().map(|(lo, _)| lo).sum::<i128>() <= money::SCALE,
            "INCOHERENT_EVENT_PROBABILITY",
        )?;
    }
    Ok(())
}
// Historical V1 is a separate frozen bank family. Its original labels/caps
// stay exact; they are never substituted into the generic V2 event policy.
fn validate_legacy_settlement_bank(b:&V)->Result<()>{need(b["schemaVersion"]==1&&array(&b["research"]).len()==1&&entries(&b["identities"]).len()==1,"LEGACY_SCOPE")?;let expected=json!({"minimumEdge":"0.020000","maximumSpread":"0.020000","maximumBookAgeMs":20000,"maximumResearchAgeMs":21600000,"fractionalKellyBps":2500,"slippageTicks":1,"depthFractionBps":100,"maxOrderCost":"1.000000","maxExposure":"3.000000","cashReserve":"90.000000","cumulativeBuyBudget":"10.000000","groupCaps":{"national_election":"3.000000","texas_governor":"1.000000"}});need(digest(&b["policy"])==digest(&expected)&&digest(&b["fillPolicy"])==digest(&fill_policy()),"LEGACY_POLICY")?;let r=&b["research"][0];validate_research(r)?;let identity=&b["identities"][s(&r["marketId"])];need(time(&r["recordedAt"])?<=time(&b["cutoff"])?&&identity["exchangeId"]==r["exchangeId"]&&identity["predicateHash"]==r["predicateHash"]&&array(&b["universe"]["exchanges"]).iter().any(|x|x["marketId"]==r["marketId"]&&x["exchangeId"]==r["exchangeId"]&&x["option"]=="YES"),"LEGACY_RESEARCH_BINDING")?;need(b["riskGroups"][s(&r["marketId"]) ]==json!(["national_election","texas_governor"])&&n(&b["valuationDeadlineMs"])>time(&b["cutoff"] )?,"LEGACY_HORIZON")?;Ok(())}
pub fn create_state(b: &V) -> Result<V> {
    validate_bank(b)?;
    let mut wallets = json!({});
    for latency in array(&b["fillPolicy"]["latenciesMs"]) {
        wallets[&n(&latency).to_string()] = json!({"cash":b["fillPolicy"]["initialCash"],"realizedPnl":"0.000000","cumulativeBuys":"0.000000","positions":{},"pending":{},"fills":[],"consumedVersions":{},"lastExitedAtMs":{},"intents":0});
    }
    Ok(
        json!({"bankHash":digest(b),"lastAvailableAt":b["cutoff"],"barriers":{"global":b["cutoff"],"markets":{}},"counts":{},"decisions":[],"books":{},"wallets":wallets}),
    )
}
pub fn assert_state(state: &V, b: &V) -> Result<()> {
    validate_bank(b)?;
    need(s(&state["bankHash"]) == digest(b), "BANK_CHANGED")?;
    need(
        entries(&state["wallets"])
            .iter()
            .map(|(k, _)| k.clone())
            .collect::<Vec<_>>()
            == vec!["1000".to_owned(), "3000".to_owned()],
        "WALLET_SCOPE",
    )?;
    for (_, w) in entries(&state["wallets"]) {
        let (mut cash, mut realized, mut buys) = (a(&b["fillPolicy"]["initialCash"])?, 0, 0);
        let mut positions = json!({});
        for f in array(&w["fills"]) {
            need(
                f["quantity"] == 1
                    && f["fee"] == "0.000000"
                    && (f["action"] == "buy" || f["action"] == "sell"),
                "FILL",
            )?;
            let id = s(&f["marketId"]);
            let value = a(&f["price"])?;
            need(
                b["identities"][id]["exchangeId"] == f["exchangeId"]
                    && f["instrument"] == format!("{}:YES", s(&f["exchangeId"])),
                "FILL_SCOPE",
            )?;
            if f["action"] == "buy" {
                need(positions[id].is_null(), "DUPLICATE_HOLDING")?;
                positions[id] = json!({"quantity":1,"cost":f["price"],"enteredAt":f["at"]});
                cash -= value;
                buys += value;
            } else {
                need(!positions[id].is_null(), "OVERSELL")?;
                realized += value - a(&positions[id]["cost"])?;
                cash += value;
                positions.as_object_mut().unwrap().shift_remove(id);
            }
        }
        need(
            cash == a(&w["cash"])?
                && realized == a(&w["realizedPnl"])?
                && buys == a(&w["cumulativeBuys"])?
                && digest(&positions) == digest(&w["positions"]),
            "CONSERVATION",
        )?;
        let mut held = 0;
        let mut reserved = 0;
        let mut groups = std::collections::BTreeMap::<String, i128>::new();
        for (id, p) in entries(&w["positions"]) {
            need(!b["identities"][&id].is_null(), "UNBOUND_POSITION")?;
            let value = a(&p["cost"])?;
            held += value;
            for g in array(&b["riskGroups"][&id]) {
                *groups.entry(s(&g).to_owned()).or_default() += value;
            }
        }
        for (id, p) in entries(&w["pending"]) {
            need(
                !b["identities"][&id].is_null()
                    && p["quantity"] == 1
                    && p["marketId"] == id
                    && (!w["positions"][&id].is_null()) == (p["action"] == "sell")
                    && a(&p["limit"])? <= a(&b["policy"]["maxOrderCost"])?,
                "UNBOUND_INTENT",
            )?;
            if p["action"] == "buy" {
                let value = a(&p["limit"])?;
                reserved += value;
                for g in array(&b["riskGroups"][&id]) {
                    *groups.entry(s(&g).to_owned()).or_default() += value;
                }
            }
        }
        need(
            cash - reserved >= a(&b["policy"]["cashReserve"])?
                && held + reserved <= a(&b["policy"]["maxExposure"])?
                && buys + reserved <= a(&b["policy"]["cumulativeBuyBudget"])?,
            "RESERVATIONS",
        )?;
        for (g, value) in groups {
            need(
                value <= a(&b["policy"]["groupCaps"][&g])?,
                "CORRELATED_FUNDING",
            )?;
        }
    }
    Ok(())
}
fn boundary(state: &mut V, id: Option<&str>, at: &V, reason: &str) {
    if let Some(id) = id {
        state["barriers"]["markets"][id] = at.clone();
        state["books"].as_object_mut().unwrap().shift_remove(id);
    } else {
        state["barriers"]["global"] = at.clone();
        state["books"] = json!({});
    }
    for (_, w) in state["wallets"].as_object_mut().unwrap() {
        let keys: Vec<_> = w["pending"].as_object().unwrap().keys().cloned().collect();
        for key in keys {
            if id.is_none_or(|id| id == key) {
                w["pending"].as_object_mut().unwrap().shift_remove(&key);
            }
        }
    }
    inc(state, reason);
}
fn inc(state: &mut V, key: &str) {
    state["counts"][key] = json!(n(&state["counts"][key]) + 1)
}
fn account_gate(account: &V, book: &V, at: &V) -> Option<&'static str> {
    let check = (|| -> Result<Option<&'static str>> {
        let required = [
            "schemaVersion",
            "positionsAt",
            "ordersAt",
            "excludedExchangeIds",
        ];
        if !account
            .as_object()
            .is_some_and(|m| m.len() == 4 && required.iter().all(|k| m.contains_key(*k)))
            || account["schemaVersion"] != 1
            || !account["excludedExchangeIds"].is_array()
            || array(&account["excludedExchangeIds"])
                .iter()
                .any(|v| !numeric_id(v))
        {
            return Ok(Some("account_unavailable"));
        }
        let now = time(at)?;
        let request = time(&book["requestStartedAt"])?;
        for k in ["positionsAt", "ordersAt"] {
            let t = time(&account[k])?;
            if t > request || now - t > 20000 {
                return Ok(Some("account_not_causal_or_fresh"));
            }
        }
        Ok(
            if array(&account["excludedExchangeIds"]).contains(&book["exchangeId"]) {
                Some("own_account_exposure")
            } else {
                None
            },
        )
    })();
    check.unwrap_or(Some("account_unavailable"))
}
fn native_input(state: &V, b: &V, latency: &str, row: &V, at: &V, account: &V) -> Result<V> {
    let book = &row["payload"]["book"];
    let w = &state["wallets"][latency];
    let mut candidates = vec![];
    for r in array(&b["research"]) {
        let id = s(&r["marketId"]);
        candidates.push(json!({"id":format!("{id}:YES"),"marketId":r["marketId"],"exchangeId":r["exchangeId"],"side":r["side"],"predicateHash":r["predicateHash"],"riskGroups":b["riskGroups"][id],"settlesAtMs":b["valuationDeadlineMs"],"research":{"id":r["id"],"predicateHash":r["predicateHash"],"side":r["side"],"lower":r["lower"],"upper":r["upper"],"recordedAtMs":time(&r["recordedAt"])? ,"sourceAtMs":time(&r["sourceAt"])? ,"expiresAtMs":time(&r["expiresAt"])? ,"method":r["method"],"settlementReviewed":r["settlementReviewed"],"flags":r["flags"],"sources":r["sources"]},"book":if r["marketId"]==book["marketId"]{json!({"requestStartedAtMs":time(&book["requestStartedAt"])? ,"receivedAtMs":time(&book["receivedAt"])? ,"availableAtMs":time(at)? ,"sequence":row["sequence"],"sourceHash":row["hash"],"representation":"direct_yes","bids":book["bids"],"asks":book["asks"]})}else{V::Null},"account":{"positionsAtMs":time(&account["positionsAt"])? ,"ordersAtMs":time(&account["ordersAt"])? ,"knownAtMs":time(&account["positionsAt"] )?.max(time(&account["ordersAt"])?),"complete":true,"positionExcluded":array(&account["excludedExchangeIds"]).contains(&r["exchangeId"]),"openOrderExcluded":array(&account["excludedExchangeIds"]).contains(&r["exchangeId"])}}));
    }
    let positions=entries(&w["positions"]).into_iter().map(|(id,p)|Ok(json!({"candidateId":format!("{id}:YES"),"quantity":p["quantity"],"cost":p["cost"],"enteredAtMs":time(&p["enteredAt"])?}))).collect::<Result<Vec<_>>>()?;
    let reservations = entries(&w["pending"])
        .into_iter()
        .filter(|(id, p)| *id != s(&book["marketId"]) && p["action"] == "buy")
        .map(|(id, p)| json!({"candidateId":format!("{id}:YES"),"maximumCost":p["limit"]}))
        .collect::<Vec<_>>();
    Ok(
        json!({"schemaVersion":1,"mode":"settlement_value_policy_v1","asOfMs":time(at)? ,"policy":b["policy"],"wallet":{"capital":b["fillPolicy"]["initialCash"],"cash":w["cash"],"cumulativeBuys":w["cumulativeBuys"],"positions":positions,"reservations":reservations},"candidates":candidates}),
    )
}
pub fn advance(input: &V, row: &V, b: &V, available_at: &str, account: Option<&V>) -> Result<V> {
    advance_with_entry_permission(input, row, b, available_at, account, true)
}
pub fn advance_with_entry_permission(
    input: &V,
    row: &V,
    b: &V,
    available_at: &str,
    account: Option<&V>,
    allow_new_entries: bool,
) -> Result<V> {
    assert_state(input, b)?;
    let mut state = input.clone();
    let at = json!(available_at);
    let now = time(&at)?;
    need(
        time(&row["at"])? <= now && now >= time(&state["lastAvailableAt"])?,
        "CLOCK",
    )?;
    state["lastAvailableAt"] = at.clone();
    let kind = s(&row["kind"]);
    if kind != "book" {
        need(
            ["session_start", "session_stop", "invalidation"].contains(&kind),
            "ROW_KIND",
        )?;
        let id = if kind == "invalidation" {
            row["payload"]["marketId"].as_str()
        } else {
            None
        };
        boundary(&mut state, id, &at, "source_boundary");
        return Ok(state);
    }
    let book = &row["payload"]["book"];
    let id = s(&book["marketId"]);
    if b["identities"][id].is_null() {
        return Ok(state);
    }
    need(
        book["exchangeId"] == b["identities"][id]["exchangeId"]
            && book["instrument"] == format!("{}:YES", s(&book["exchangeId"])),
        "BOOK_SCOPE",
    )?;
    let request = time(&book["requestStartedAt"])?;
    if now - time(&book["receivedAt"])? > n(&b["policy"]["maximumBookAgeMs"])
        || [
            &state["barriers"]["global"],
            &state["barriers"]["markets"][id],
            &b["cutoff"],
        ]
        .iter()
        .any(|v| !v.is_null() && time(v).is_ok_and(|t| request <= t))
    {
        boundary(&mut state, Some(id), &at, "stale_or_pre_boundary");
        return Ok(state);
    }
    let account = account.unwrap_or(&V::Null);
    if let Some(reason) = account_gate(account, book, &at) {
        boundary(&mut state, Some(id), &at, reason);
        return Ok(state);
    }
    let mut mark = book.clone();
    mark["availableAt"] = at.clone();
    mark["sourceHash"] = row["hash"].clone();
    mark["publicationSequence"] = row["sequence"].clone();
    state["books"][id] = mark;
    for (latency, _) in entries(&state["wallets"]) {
        let native = native_input(&state, b, &latency, row, &at, account)?;
        let policy: settlement_value::Input =
            serde_json::from_value(native).map_err(|_| "SETTLEMENT_PORTFOLIO_NATIVE_INPUT")?;
        let output = settlement_value::run(&policy)?;
        let mut d = array(&output["decisions"])
            .into_iter()
            .find(|d| d["marketId"] == id)
            .ok_or("SETTLEMENT_PORTFOLIO_NATIVE_SCOPE")?;
        need(
            output["executionAuthority"] == false
                && output["competitionOrders"] == 0
                && output["fills"] == 0,
            "NATIVE_SCOPE",
        )?;
        if !allow_new_entries
            && d["action"] == "candidate_buy"
            && state["wallets"][&latency]["pending"][id].is_null()
        {
            d["action"] = json!("observe");
            d["reason"] = json!("operator_entry_drain");
            d["quantity"] = json!(0);
        }
        let expected_action = if d["action"] == "candidate_buy" {
            Some("buy")
        } else if d["action"] == "reduce" {
            Some("sell")
        } else {
            None
        };
        let mut filled = false;
        let mut w = state["wallets"][&latency].clone();
        let mut pending = w["pending"][id].clone();
        if !pending.is_null() {
            if expected_action != Some(s(&pending["action"]))
                || now - time(&pending["createdAt"])? > n(&b["fillPolicy"]["intentLifetimeMs"])
            {
                w["pending"].as_object_mut().unwrap().shift_remove(id);
                pending = V::Null;
            } else if request >= time(&pending["createdAt"])? + latency.parse::<i64>().unwrap()
                && request > time(&pending["createdAt"])?
                && n(&row["sequence"]) > n(&pending["sourceSequence"])
                && w["consumedVersions"][id] != book["versionKey"]
            {
                let action = s(&pending["action"]);
                for (level_index,level) in array(&book[if action == "buy" { "asks" } else { "bids" }]).into_iter().enumerate() {
                    let price = a(&level["price"])?
                        + if receipt_baseline(b){0}else if action == "buy" {
                            money::TICK
                        } else {
                            -money::TICK
                        };
                    if price < money::TICK
                        || price > money::SCALE - money::TICK
                        || (action == "buy" && price > a(&pending["limit"])?)
                        || (action == "sell" && price < a(&pending["limit"])?)
                    {
                        break;
                    }
                    if n(&level["quantity"]) * n(&b["policy"]["depthFractionBps"]) / 10000 < 1 {
                        continue;
                    }
                    if action == "buy"
                        && (!w["positions"][id].is_null()
                            || a(&w["cash"])? - price < a(&b["policy"]["cashReserve"])?
                            || a(&w["cumulativeBuys"])? + price
                                > a(&b["policy"]["cumulativeBuyBudget"])?)
                    {
                        break;
                    }
                    let mut fill = json!({"intentId":pending["id"],"marketId":id,"exchangeId":book["exchangeId"],"instrument":book["instrument"],"action":action,"quantity":1,"price":m(price),"fee":"0.000000","at":at,"requestStartedAt":book["requestStartedAt"],"receivedAt":book["receivedAt"],"sourceVersion":book["versionKey"],"sourceSequence":row["sequence"],"sourceHash":row["hash"],"intentSourceHash":pending["sourceHash"],"researchHash":pending["researchHash"],"latencyMs":latency.parse::<i64>().unwrap(),"model":"later_rest_request_counterfactual","reason":pending["reason"]});
                    if receipt_baseline(b){fill["model"]=json!("later_rest_request_raw_receipt_counterfactual");fill["rawObservedPrice"]=level["price"].clone();fill["protectiveOrderLimit"]=pending["limit"].clone();fill["limitCushionTicks"]=json!(1);fill["fillAdverseTicks"]=json!(0);fill["eligibleDepthShares"]=json!(n(&level["quantity"])*n(&b["policy"]["depthFractionBps"])/10000);fill["levelIndex"]=json!(level_index);}
                    if action == "buy" {
                        w["cash"] = m(a(&w["cash"])? - price);
                        w["cumulativeBuys"] = m(a(&w["cumulativeBuys"])? + price);
                        w["positions"][id] = json!({"quantity":1,"cost":m(price),"enteredAt":at});
                    } else {
                        need(!w["positions"][id].is_null(), "SELL_WITHOUT_POSITION")?;
                        w["cash"] = m(a(&w["cash"])? + price);
                        w["realizedPnl"] =
                            m(a(&w["realizedPnl"])? + price - a(&w["positions"][id]["cost"])?);
                        w["positions"].as_object_mut().unwrap().shift_remove(id);
                        w["lastExitedAtMs"][id] = json!(now);
                    }
                    w["fills"].as_array_mut().unwrap().push(fill);
                    w["consumedVersions"][id] = book["versionKey"].clone();
                    w["pending"].as_object_mut().unwrap().shift_remove(id);
                    filled = true;
                    break;
                }
            }
        }
        let reason = if filled {
            "later_receipt_fill"
        } else {
            s(&d["reason"])
        };
        inc(&mut state, reason);
        state["decisions"].as_array_mut().unwrap().push(json!({"at":at,"latencyMs":latency.parse::<i64>().unwrap(),"marketId":id,"sourceSequence":row["sequence"],"sourceHash":row["hash"],"action":d["action"],"reason":reason,"scenarioEdge":d["topScenarioEdge"],"liquidation":d["liquidation"]}));
        if state["decisions"].as_array().unwrap().len() > 200 {
            state["decisions"].as_array_mut().unwrap().remove(0);
        }
        if !filled
            && w["pending"][id].is_null()
            && expected_action.is_some()
            && n(&d["quantity"]) >= 1
            && (expected_action != Some("buy")
                || now - n(&w["lastExitedAtMs"][id]) >= n(&b["fillPolicy"]["cooldownMs"]))
        {
            let action = expected_action.unwrap();
            let limit = if action == "buy" {
                d["hardLimitWithCushion"].clone()
            } else {
                let bid = a(&book["bids"][0]["price"])?;
                m(if bid > money::TICK {
                    bid - money::TICK
                } else {
                    money::TICK
                })
            };
            w["intents"] = json!(n(&w["intents"]) + 1);
            let r = array(&b["research"])
                .into_iter()
                .find(|r| r["marketId"] == id)
                .unwrap();
            w["pending"][id] = json!({"id":w["intents"],"marketId":id,"action":action,"quantity":1,"limit":limit,"createdAt":at,"sourceSequence":row["sequence"],"sourceHash":row["hash"],"researchHash":digest(&r),"reason":d["reason"]});
        }
        state["wallets"][&latency] = w;
    }
    assert_state(&state, b)?;
    Ok(state)
}
pub fn report(state: &V, b: &V, available_at: &str) -> Result<V> {
    assert_state(state, b)?;
    let at = json!(available_at);
    let now = time(&at)?;
    let mut wallets = json!({});
    for (latency, w) in entries(&state["wallets"]) {
        let (mut proceeds, mut complete) = (0, true);
        let mut positions = vec![];
        for (id, p) in entries(&w["positions"]) {
            let book = &state["books"][&id];
            let fresh = !book.is_null()
                && now >= time(&book["availableAt"])?
                && now - time(&book["receivedAt"])? <= n(&b["policy"]["maximumBookAgeMs"]);
            let level = if fresh {
                array(&book["bids"]).into_iter().find(|l| {
                    n(&l["quantity"]) * n(&b["policy"]["depthFractionBps"]) / 10000
                        >= n(&p["quantity"])
                })
            } else {
                None
            };
            let liquidation = if let Some(level) = level {
                let value = a(&level["price"])? - if receipt_baseline(b){0}else{money::TICK};
                proceeds += value;
                m(value)
            } else {
                complete = false;
                V::Null
            };
            let r = array(&b["research"])
                .into_iter()
                .find(|r| r["marketId"] == id)
                .unwrap();
            positions.push(json!({"marketId":id,"instrument":format!("{}:YES",s(&b["identities"][&id]["exchangeId"])),"quantity":p["quantity"],"cost":p["cost"],"enteredAt":p["enteredAt"],"liquidation":liquidation,"liquidationPnl":if liquidation.is_null(){V::Null}else{m(a(&liquidation)?-a(&p["cost"])? )},"researchExpired":now>=time(&r["expiresAt"])?}));
        }
        let fills = array(&w["fills"]);
        let reserved = entries(&w["pending"])
            .iter()
            .filter(|(_, p)| p["action"] == "buy")
            .try_fold(0, |sum, (_, p)| Ok::<_, String>(sum + a(&p["limit"])?))?;
        wallets[&latency] = json!({"cash":w["cash"],"realizedPnl":w["realizedPnl"],"cumulativeBuys":w["cumulativeBuys"],"reservedCost":m(reserved),"fills":fills.len(),"closedTrades":fills.iter().filter(|f|f["action"]=="sell").count(),"positions":positions,"pending":entries(&w["pending"]).into_iter().map(|(_,p)|p).collect::<Vec<_>>(),"recentFills":fills.into_iter().rev().take(25).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>(),"liquidationPnl":if complete{m(a(&w["cash"])?+proceeds-a(&b["fillPolicy"]["initialCash"] )?)}else{V::Null}});
    }
    let groups: V = array(&b["research"])
        .into_iter()
        .map(|r| {
            (
                s(&r["marketId"]).to_owned(),
                b["riskGroups"][s(&r["marketId"])].clone(),
            )
        })
        .collect();
    let mut result=
        json!({"mode":b["mode"],"cohort":b["name"],"updatedAt":at,"bankHash":state["bankHash"],"cutoff":b["cutoff"],"counts":state["counts"],"decisions":state["decisions"],"wallets":wallets,"executionAuthority":false,"networkRequests":0,"competitionOrders":0,"profitabilityEstablished":false,"fees":0,"holdingPolicy":"current_fundamental_value_without_fixed_timeout_or_price_only_stop","limitations":["Settlement probabilities describe scenarios, not a near-term repricing forecast or a guaranteed payout.","Expired research stops new entries and requires review; existing held risk remains attributed.","One-share counterfactual fills require a later REST request, causal account exclusions, one adverse tick and one-percent depth.","Latency wallets share the same opportunities; do not add their P&L together.","A deadline is not a settlement event. This engine never manufactures a payout.","One funded cash ledger covers all markets within each latency case; correlated cases are never summed.","Exact event groups include both held cost and every pending purchase; source probabilities are not guaranteed returns."],"stateHash":digest(state),"sharedCashPerLatency":true,"independentLatencySamples":false,"eventGroups":groups,"maximumMarkets":3})
    ;if b["mode"]=="funded_settlement_value_paper_v1"{for key in ["stateHash","sharedCashPerLatency","independentLatencySamples","eventGroups","maximumMarkets"]{result.as_object_mut().unwrap().shift_remove(key);}result["limitations"].as_array_mut().unwrap().truncate(5);}
    if receipt_baseline(b){result["schemaVersion"]=json!(3);result["fillModel"]=json!({"entryLimitCushionTicks":1,"fillAdverseTicks":0,"liquidationAdverseTicks":0,"counterfactualNotProvenExecution":true,"zeroFees":true});result["limitations"][2]=json!("One-share counterfactual fills use actual later eligible receipt levels within the original protective limit, independent request/source latency, causal exclusions and one-percent integer depth. Exchange acceptance and inter-request depletion remain unproven.");let mut sensitivities=json!({});for(latency,w)in entries(&state["wallets"]){let mut cases=json!({});for ticks in [0i128,1,2]{let(mut total,mut complete)=(0,true);let mut marks=vec![];for(id,p)in entries(&w["positions"]){let book=&state["books"][&id];let fresh=!book.is_null()&&now>=time(&book["availableAt"])?&&now-time(&book["receivedAt"])?<=n(&b["policy"]["maximumBookAgeMs"]);let level=if fresh{array(&book["bids"]).into_iter().find(|l|n(&l["quantity"])*n(&b["policy"]["depthFractionBps"])/10000>=n(&p["quantity"]))}else{None};let value=if let Some(level)=level{let unit=a(&level["price"])?-ticks*money::TICK;if unit>=money::TICK{total+=unit;m(unit)}else{complete=false;V::Null}}else{complete=false;V::Null};marks.push(json!({"marketId":id,"estimatedProceeds":value}));}cases[&format!("{ticks}tick")]=json!({"residualAdverseTicks":ticks,"estimatedProceeds":if complete{m(total)}else{V::Null},"estimatedLiquidationPnl":if complete{m(a(&w["cash"])?+total-a(&b["fillPolicy"]["initialCash"] )?)}else{V::Null},"positions":marks,"realized":false});}sensitivities[&latency]=cases;}result["liquidationSensitivities"]=sensitivities;result["prospectiveOnly"]=json!(true);}
    Ok(result)
}

fn capacity_review(r: &V) -> Result<()> {
    need(
        r["schemaVersion"] == 1
            && r["kind"] == "conditional_no_pair_review"
            && r["id"].is_string()
            && r["tournamentId"].is_string()
            && hex(&r["evidenceHash"])
            && array(&r["legs"]).len() == 2,
        "CAPACITY_REVIEW",
    )?;
    need(
        time(&r["expiresAt"])? > time(&r["recordedAt"])?,
        "CAPACITY_REVIEW_CLOCK",
    )?;
    let legs = array(&r["legs"]);
    need(
        legs.iter().all(|l| {
            numeric_id(&l["marketId"])
                && numeric_id(&l["exchangeId"])
                && l["outcome"] == "NO"
                && hex(&l["rulesHash"])
        }) && legs[0]["exchangeId"] != legs[1]["exchangeId"]
            && legs[0]["marketId"] != legs[1]["marketId"],
        "CAPACITY_REVIEW_SCOPE",
    )?;
    need(
        !array(&r["assumptions"]).is_empty()
            && array(&r["assumptions"]).iter().all(|v| !s(v).is_empty()),
        "CAPACITY_ASSUMPTIONS",
    )?;
    let scenarios = array(&r["scenarios"]);
    let mut ids = std::collections::HashSet::new();
    need(
        scenarios.len() > 1
            && scenarios.iter().all(|x| {
                x["id"].is_string()
                    && ids.insert(s(&x["id"]))
                    && (x["category"] == "ordinary" || x["category"] == "exception")
                    && array(&x["payouts"]).len() == 2
                    && array(&x["payouts"]).iter().all(|v| *v == 0 || *v == 1)
            })
            && scenarios.iter().any(|x| x["category"] == "ordinary")
            && scenarios
                .iter()
                .any(|x| x["category"] == "exception" && x["payouts"] == json!([0, 0])),
        "CAPACITY_SCENARIOS",
    )?;
    Ok(())
}
pub fn dual_capacity(
    review: &V,
    policy: &V,
    books: &[V],
    account: Option<&V>,
    available_at: &str,
) -> Result<V> {
    capacity_review(review)?;
    for k in [
        "maxAgeMs",
        "maxSkewMs",
        "maximumAccountAgeMs",
        "maximumReviewAgeMs",
        "adverseTicksPerLeg",
        "depthFractionBps",
        "maxQuantity",
    ] {
        need(
            policy[k]
                .as_i64()
                .is_some_and(|x| x >= 0 && x <= 9007199254740991),
            "CAPACITY_POLICY",
        )?;
    }
    need(
        n(&policy["depthFractionBps"]) > 0
            && n(&policy["depthFractionBps"]) <= 10000
            && n(&policy["adverseTicksPerLeg"]) <= 5
            && n(&policy["maxSkewMs"]) <= n(&policy["maxAgeMs"]),
        "CAPACITY_POLICY",
    )?;
    for k in [
        "minimumMarginalEdge",
        "minimumTotalProfit",
        "feePerShare",
        "fixedFeePerLeg",
        "maxCost",
        "maxUnmatchedCost",
    ] {
        need(a(&policy[k])? >= 0, "CAPACITY_POLICY_COST")?;
    }
    need(
        policy["representation"] == "documented_same_exchange_yes_bid_complement"
            && !s(&policy["feeEvidence"]).is_empty(),
        "CAPACITY_REPRESENTATION",
    )?;
    need(books.len() == 2, "CAPACITY_BOOKS")?;
    let at = json!(available_at);
    let now = time(&at)?;
    let base = json!({"schemaVersion":1,"mode":"canonical_no_conditional_capacity","evaluatedAt":at,"reviewId":review["id"],"reviewHash":digest(review),"policyHash":digest(policy),"representation":policy["representation"],"nativeNoObserved":false,"scenarioCompletenessVerified":false,"executionAuthority":false,"ordersSubmitted":0,"ruleBindingKind":"manual_review_attestation_not_live_api_metadata","networkRequests":0,"fills":null,"realizedPnl":null,"profitabilityEstablished":false});
    let reject = |reason: &str| {
        let mut v = base.clone();
        v["reason"] = json!(reason);
        v["positiveConditionalCapacity"] = json!(false);
        v["matchedQuantity"] = json!("0");
        v["acquisitionCost"] = json!("0.000000");
        v["segments"] = json!([]);
        v
    };
    if now < time(&review["recordedAt"])?
        || now >= time(&review["expiresAt"])?
        || now - time(&review["recordedAt"])? > n(&policy["maximumReviewAgeMs"])
    {
        return Ok(reject("review_not_current"));
    }
    let legs = array(&review["legs"]);
    let (mut starts, mut receipts, mut ladders) = (vec![], vec![], vec![]);
    for (i, book) in books.iter().enumerate() {
        let leg = &legs[i];
        need(
            book["tournamentId"] == review["tournamentId"]
                && book["marketId"] == leg["marketId"]
                && book["exchangeId"] == leg["exchangeId"]
                && book["instrument"] == format!("{}:YES", s(&leg["exchangeId"]))
                && book["freshnessPolicy"] == "authoritative_receipt_v1"
                && hex(&book["publicationHash"])
                && n(&book["publicationSequence"]) > 0,
            "CAPACITY_BOOK_SCOPE",
        )?;
        let start = time(&book["requestStartedAt"])?;
        let received = time(&book["receivedAt"])?;
        starts.push(start);
        receipts.push(received);
        need(
            start <= received
                && received <= now
                && time(&book["publishedAt"])? >= received
                && time(&book["publishedAt"])? <= now,
            "CAPACITY_BOOK_CLOCK",
        )?;
        let source = chrono::DateTime::parse_from_rfc3339(s(&book["sourceAt"]))
            .map_err(|_| "CAPACITY_BOOK_VERSION")?;
        let sequence = s(&book["sequence"]);
        need(
            !sequence.is_empty()
                && (sequence == "0" || !sequence.starts_with('0'))
                && sequence.bytes().all(|c| c.is_ascii_digit())
                && source
                    .timestamp_nanos_opt()
                    .is_some_and(|ns| ns as i128 <= (received as i128 + 2000) * 1000000)
                && book["versionKey"] == format!("{sequence}:{}", s(&book["sourceAt"])),
            "CAPACITY_BOOK_VERSION",
        )?;
        let bids = array(&book["bids"]);
        let asks = array(&book["asks"]);
        need(
            !bids.is_empty() && bids.len() <= 200 && !asks.is_empty(),
            "CAPACITY_DEPTH",
        )?;
        need(
            money::price(s(&bids[0]["price"]))? < money::price(s(&asks[0]["price"]))?,
            "CAPACITY_CROSSED_BOOK",
        )?;
        let mut previous = money::SCALE;
        let mut ladder = vec![];
        for l in bids {
            let bid = money::price(s(&l["price"]))?;
            need(bid < previous && n(&l["quantity"]) > 0, "CAPACITY_DEPTH")?;
            previous = bid;
            let raw = money::SCALE - bid;
            let effective = raw
                + n(&policy["adverseTicksPerLeg"]) as i128 * money::TICK
                + a(&policy["feePerShare"])?;
            need(
                raw + n(&policy["adverseTicksPerLeg"]) as i128 * money::TICK <= money::SCALE,
                "CAPACITY_ADVERSE_PRICE",
            )?;
            let q = n(&l["quantity"]) as i128 * n(&policy["depthFractionBps"]) as i128 / 10000;
            if q > 0 {
                ladder.push((raw, effective, q));
            }
        }
        ladders.push(ladder);
    }
    if starts
        .iter()
        .any(|s| *s < time(&review["recordedAt"]).unwrap())
    {
        return Ok(reject("review_not_known_at_request"));
    }
    if starts.iter().any(|s| now - *s > n(&policy["maxAgeMs"])) {
        return Ok(reject("stale_request"));
    }
    if receipts.iter().max().unwrap() - starts.iter().min().unwrap() > n(&policy["maxSkewMs"]) {
        return Ok(reject("whole_window_skew"));
    }
    let account = account.unwrap_or(&V::Null);
    if account["schemaVersion"] != 1
        || !account["excludedExchangeIds"].is_array()
        || array(&account["excludedExchangeIds"])
            .iter()
            .any(|v| !numeric_id(v))
    {
        return Ok(reject("account_unavailable"));
    }
    for k in ["positionsAt", "ordersAt"] {
        let t = time(&account[k])?;
        if t > *starts.iter().min().unwrap() || now - t > n(&policy["maximumAccountAgeMs"]) {
            return Ok(reject("account_not_causal_or_fresh"));
        }
    }
    if legs
        .iter()
        .any(|l| array(&account["excludedExchangeIds"]).contains(&l["exchangeId"]))
    {
        return Ok(reject("own_account_exposure"));
    }
    if ladders.iter().any(|l| l.is_empty()) {
        return Ok(reject("fractional_depth"));
    }
    let scenarios = array(&review["scenarios"]);
    let floor = scenarios
        .iter()
        .filter(|s| s["category"] == "ordinary")
        .map(|s| n(&s["payouts"][0]) + n(&s["payouts"][1]))
        .min()
        .unwrap() as i128
        * money::SCALE;
    let mut costs = [a(&policy["fixedFeePerLeg"])?; 2];
    let mut indexes = [0usize; 2];
    let mut left = [ladders[0][0].2, ladders[1][0].2];
    let budget = a(&policy["maxCost"])?;
    let unmatched = a(&policy["maxUnmatchedCost"])?;
    let edge = a(&policy["minimumMarginalEdge"])?;
    let maximum = n(&policy["maxQuantity"]) as i128;
    let mut quantity = 0;
    let mut stop = "depth_exhausted";
    let mut segments = vec![];
    while indexes
        .iter()
        .enumerate()
        .all(|(i, j)| *j < ladders[i].len())
        && quantity < maximum
    {
        let levels = [ladders[0][indexes[0]], ladders[1][indexes[1]]];
        let unit_cost = levels[0].1 + levels[1].1;
        if floor - unit_cost < edge || floor <= unit_cost {
            stop = "marginal_edge";
            break;
        }
        if costs[0] + costs[1] > budget || costs.iter().any(|c| *c > unmatched) {
            stop = "capital_limit";
            break;
        }
        let take = left[0]
            .min(left[1])
            .min(maximum - quantity)
            .min((budget - costs[0] - costs[1]) / unit_cost)
            .min((unmatched - costs[0]) / levels[0].1)
            .min((unmatched - costs[1]) / levels[1].1);
        if take <= 0 {
            stop = "capital_limit";
            break;
        }
        for i in 0..2 {
            costs[i] += levels[i].1 * take;
            left[i] -= take;
        }
        quantity += take;
        segments.push(json!({"quantity":take.to_string(),"noPrices":[m(levels[0].0),m(levels[1].0)],"adverseUnitCost":m(unit_cost)}));
        for i in 0..2 {
            if left[i] == 0 {
                indexes[i] += 1;
                left[i] = ladders[i].get(indexes[i]).map(|l| l.2).unwrap_or(0);
            }
        }
    }
    if quantity == maximum {
        stop = "quantity_limit"
    }
    let cost = if quantity > 0 { costs[0] + costs[1] } else { 0 };
    let profit = quantity * floor - cost;
    let positive = quantity > 0 && profit > 0 && profit >= a(&policy["minimumTotalProfit"])?;
    let mut result = base;
    result["reason"] = json!(
        if quantity > 0 && profit >= a(&policy["minimumTotalProfit"])? {
            "conditional_positive_capacity"
        } else {
            stop
        }
    );
    result["positiveConditionalCapacity"] = json!(positive);
    result["stopReason"] = json!(stop);
    result["matchedQuantity"] = json!(quantity.to_string());
    result["acquisitionCost"] = m(cost);
    result["conditionalOrdinaryMinimumProfit"] = m(profit);
    let worst = scenarios
        .iter()
        .map(|s| n(&s["payouts"][0]) + n(&s["payouts"][1]))
        .min()
        .unwrap() as i128;
    result["stressedMinimumProfit"] = m(worst * money::SCALE * quantity - cost);
    result["maximumSingleLegCostAtRisk"] = m(if quantity > 0 {
        costs[0].max(costs[1])
    } else {
        0
    });
    result["segments"] = json!(segments);
    result["scenarios"] = json!(
        scenarios
            .into_iter()
            .map(|mut s| {
                s["profit"] = m((n(&s["payouts"][0]) + n(&s["payouts"][1])) as i128
                    * money::SCALE
                    * quantity
                    - cost);
                s
            })
            .collect::<Vec<_>>()
    );
    result["sources"]=json!(books.iter().map(|b|json!({"marketId":b["marketId"],"exchangeId":b["exchangeId"],"requestStartedAt":b["requestStartedAt"],"receivedAt":b["receivedAt"],"publicationSequence":b["publicationSequence"],"publicationHash":b["publicationHash"],"versionKey":b["versionKey"]})).collect::<Vec<_>>());
    result["limitations"] = json!([
        "Canonical YES-bid complement, not independently captured NO depth.",
        "Current snapshot capacity is not atomic liquidity, a fill, a queue model or a profitable near-term exit.",
        "Ordinary election assumptions can fail through fusion, affiliation, rule changes or administrative settlement.",
        "Zero-payout exception stress may lose the full package cost; an unmatched-leg cap does not cover that loss."
    ]);
    Ok(result)
}
