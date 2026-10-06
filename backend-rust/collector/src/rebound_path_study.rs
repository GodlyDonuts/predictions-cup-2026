// live-paper/rebound-path-study.mjs — causal confirmed-rebound path comparison
// (v1). Caller supplies original causal dip opportunities, never filtered by
// their future result.
use num_bigint::BigInt;
use serde_json::{json, Value as V};
use std::path::Path;

fn need(ok: bool, code: &str) -> Result<(), String> {
    if !ok { return Err(format!("REBOUND_STUDY_{}", code)); }
    Ok(())
}
fn sha(b: &[u8]) -> String { baal_service_runtime::json_text::sha256(b) }
fn price(n: f64) -> Result<String, String> {
    Ok(baal_paper::fixed::money(&BigInt::from((n * 1e6).round() as i64)))
}
fn ms(v: &V) -> i64 {
    baal_paper::fixed::instant(v.as_str().unwrap_or("")).unwrap_or(i64::MIN)
}
fn money(v: &BigInt) -> String { baal_paper::fixed::money(v) }
fn amount(v: &V) -> Result<BigInt, String> {
    baal_paper::fixed::big_amount(v.as_str().unwrap_or(""))
}
fn rows(b: &[u8]) -> Result<Vec<V>, String> {
    let end = b.iter().rposition(|x| *x == b'\n').map(|p| p + 1).unwrap_or(0);
    String::from_utf8(b[..end].to_vec()).map_err(|e| e.to_string())?
        .split('\n').filter(|s| !s.is_empty())
        .map(|l| serde_json::from_str(l).map_err(|e| e.to_string())).collect()
}
fn identity(workspace_dir: &Path) -> Result<V, String> {
    let keys = ["./rebound-entry.mjs", "./rebound-path-study.mjs", "./dip-markout-study.mjs",
        "../execution/ledger.mjs"];
    let files = ["live-paper/rebound-entry.mjs", "live-paper/rebound-path-study.mjs",
        "live-paper/dip-markout-study.mjs", "execution/ledger.mjs"];
    let mut m = serde_json::Map::new();
    for (k, p) in keys.iter().zip(files.iter()) {
        m.insert(k.to_string(), json!(sha(&std::fs::read(workspace_dir.join(p)).map_err(|e| e.to_string())?)));
    }
    Ok(V::Object(m))
}
/// REBOUND_STUDY_POLICY
pub fn rebound_study_policy() -> V {
    json!({"schemaVersion":1,"dipThreshold":"0.010000","latenciesMs":[1000,3000],
        "horizonsMs":[60000,180000,300000],"signal":crate::rebound_entry::rebound_entry_policy(),
        "confirmationLifetimeMs":60000,"entryLifetimeMs":60000,"labelLatenessMs":60000,
        "maximumSpread":"0.020000","depthFractionBps":100,"orderShares":1,"feeBps":0,
        "priceScenarios":["observed_price","one_adverse_tick_each_side"],
        "developmentPurgeMs":660000})
}
/// createReboundStudyBank(cutoff, baseBank)
pub fn create_rebound_study_bank(cutoff: &str, base_bank: &V,
    workspace_dir: &Path) -> Result<V, String> {
    baal_paper::fixed::instant(cutoff)?;
    Ok(json!({"name":"confirmed-rebound-path-20261003-v1","cutoff":cutoff,
        "baseBank":base_bank,"protocol":rebound_study_policy(),
        "codeIdentity":identity(workspace_dir)?}))
}
fn valid_quote(q: &V) -> bool {
    q["option"].as_str() == Some("YES") && !baal_service_runtime::json_text::js_truthy(Some(&q["crossed"]))
        && q["bestBid"].is_number() && q["bestAsk"].is_number()
        && q["bestBid"].as_f64().unwrap_or(0.0) > 0.0 && q["bestAsk"].as_f64().unwrap_or(1.0) < 1.0
        && q["bestBid"].as_f64().unwrap_or(1.0) < q["bestAsk"].as_f64().unwrap_or(0.0)
}
fn depth(levels: &V, action: &str, limit: Option<&V>) -> Result<Option<String>, String> {
    for x in levels.as_array().cloned().unwrap_or_default() {
        if (x["quantity"].as_f64().unwrap_or(0.0) / 100.0).floor() < 1.0 { continue; }
        let ok = limit.is_none()
            || (action == "buy" && amount(&x["price"])? <= amount(limit.unwrap())?)
            || (action == "sell" && amount(&x["price"])? >= amount(limit.unwrap())?);
        if ok {
            return Ok(x["price"].as_str().map(String::from));
        }
    }
    Ok(None)
}
fn make_branch(signal: &V, policy: &V) -> V {
    let mut trials = serde_json::Map::new();
    for latency in policy["latenciesMs"].as_array().cloned().unwrap_or_default() {
        let lat = latency.as_i64().unwrap_or(0);
        trials.insert(latency.as_i64().unwrap_or(0).to_string(), json!({"latencyMs":lat,
            "quote":{"stage":"awaiting_entry",
                "expiresAt":baal_service_runtime::contract::ms_to_iso(
                    (ms(&signal["availableAt"]) + policy["entryLifetimeMs"].as_i64().unwrap_or(0)) as f64)},
            "depth":{"stage":"awaiting_confirmation",
                "expiresAt":baal_service_runtime::contract::ms_to_iso(
                    (ms(&signal["availableAt"]) + policy["confirmationLifetimeMs"].as_i64().unwrap_or(0)) as f64)}}));
    }
    json!({"signal": signal, "trials": trials})
}
fn enter(trial: &mut V, entry_price: &V, at: &V, received: &V, version: &V, policy: &V) {
    trial["stage"] = json!("entered");
    trial["entryPrice"] = entry_price.clone();
    trial["entryAvailableAt"] = at.clone();
    trial["entryReceivedAt"] = received.clone();
    trial["entryVersion"] = version.clone();
    let mut exits = serde_json::Map::new();
    for h in policy["horizonsMs"].as_array().cloned().unwrap_or_default() {
        let hv = h.as_i64().unwrap_or(0);
        let due = ms(at) + hv + trial["latencyMs"].as_i64().unwrap_or(0);
        exits.insert(hv.to_string(), json!({"status":"awaiting_outcome",
            "dueAt": baal_service_runtime::contract::ms_to_iso(due as f64),
            "expiresAt": baal_service_runtime::contract::ms_to_iso(
                (due + policy["labelLatenessMs"].as_i64().unwrap_or(0)) as f64),
            "lastReceivedAt": received}));
    }
    trial["exits"] = V::Object(exits);
}
fn label(cell: &mut V, exit: &mut V, value: &V, row: &V, received: &V, request: &V) {
    exit["status"] = json!("observed");
    exit["exitPrice"] = value.clone();
    exit["rawMarkout"] = json!(money(&(amount(value).unwrap_or_default()
        - amount(&cell["entryPrice"]).unwrap_or_default())));
    exit["adverseMarkout"] = json!(money(&(amount(&exit["rawMarkout"]).unwrap_or_default()
        - BigInt::from(2) * BigInt::from(baal_paper::fixed::TICK))));
    exit["sourceSequence"] = row["sequence"].clone();
    exit["sourceHash"] = row["hash"].clone();
    exit["availableAt"] = row["at"].clone();
    exit["receivedAt"] = received.clone();
    exit["requestStartedAt"] = request.clone();
}
fn quote_trial(signal: &V, t: &mut V, q: &V, snapshot: &V, row: &V, policy: &V) {
    let t_latency = t["latencyMs"].as_i64().unwrap_or(0);
    let cell = &mut t["quote"];
    let now = ms(&row["at"]);
    let request = ms(&snapshot["requestStartedAt"]);
    if cell["stage"].as_str() == Some("awaiting_entry") {
        if now > ms(&cell["expiresAt"]) { cell["stage"] = json!("entry_not_observed"); return; }
        let ask = amount(&json!(price(q["bestAsk"].as_f64().unwrap_or(0.0)).unwrap_or_default())).unwrap_or_default();
        let bid = amount(&json!(price(q["bestBid"].as_f64().unwrap_or(0.0)).unwrap_or_default())).unwrap_or_default();
        if request < ms(&signal["availableAt"]) + t_latency
            || ask > amount(&signal["limit"]).unwrap_or_default()
            || &ask - &bid > amount(&policy["maximumSpread"]).unwrap_or_default()
            || (!signal["troughBid"].is_null()
                && bid < amount(&signal["troughBid"]).unwrap_or_default()
                    + BigInt::from(baal_paper::fixed::TICK)) {
            return;
        }
        cell["latencyMs"] = json!(t_latency);
        enter(cell, &json!(price(q["bestAsk"].as_f64().unwrap_or(0.0)).unwrap_or_default()),
            &row["at"], &snapshot["receivedAt"], &row["sequence"], policy);
        return;
    }
    if cell["stage"].as_str() != Some("entered") { return; }
    let keys: Vec<String> = cell["exits"].as_object().cloned().unwrap_or_default()
        .keys().cloned().collect();
    for h in keys {
        let exit = cell["exits"][&h].clone();
        if exit["status"].as_str() != Some("awaiting_outcome") { continue; }
        if ms(&snapshot["receivedAt"]) - ms(&exit["lastReceivedAt"])
            > policy["signal"]["maximumQuoteGapMs"].as_i64().unwrap_or(0) {
            cell["exits"][&h]["status"] = json!("coverage_gap");
            continue;
        }
        cell["exits"][&h]["lastReceivedAt"] = snapshot["receivedAt"].clone();
        if now > ms(&exit["expiresAt"]) {
            cell["exits"][&h]["status"] = json!("outcome_not_observed");
            continue;
        }
        if request < ms(&exit["dueAt"]) { continue; }
        let mut ex = cell["exits"][&h].clone();
        let mut c2 = cell.clone();
        label(&mut c2, &mut ex, &json!(price(q["bestBid"].as_f64().unwrap_or(0.0)).unwrap_or_default()),
            row, &snapshot["receivedAt"], &snapshot["requestStartedAt"]);
        cell["exits"][&h] = ex;
    }
}
fn book_trial(signal: &V, t: &mut V, b: &V, row: &V, policy: &V) -> Result<(), String> {
    let t_latency = t["latencyMs"].as_i64().unwrap_or(0);
    let cell = &mut t["depth"];
    let now = ms(&row["at"]);
    let request = ms(&b["requestStartedAt"]);
    if cell["stage"].as_str() == Some("awaiting_confirmation") {
        if now > ms(&cell["expiresAt"]) { cell["stage"] = json!("confirmation_not_observed"); return Ok(()); }
        if request <= ms(&signal["availableAt"]) { return Ok(()); }
        let ask = depth(&b["asks"], "buy", Some(&signal["limit"]))?;
        let bid = depth(&b["bids"], "sell", None)?;
        let trough_ok = signal["troughBid"].is_null()
            || (bid.is_some()
                && amount(&json!(bid.clone().unwrap()))?
                    >= amount(&signal["troughBid"])? + BigInt::from(baal_paper::fixed::TICK));
        if ask.is_none() || bid.is_none()
            || amount(&b["asks"][0]["price"])? - amount(&b["bids"][0]["price"])?
                > amount(&policy["maximumSpread"])?
            || !trough_ok {
            return Ok(());
        }
        cell["stage"] = json!("awaiting_entry");
        cell["preparedAt"] = row["at"].clone();
        cell["preparedVersion"] = b["versionKey"].clone();
        cell["expiresAt"] = json!(baal_service_runtime::contract::ms_to_iso(
            (now + policy["entryLifetimeMs"].as_i64().unwrap_or(0)) as f64));
        return Ok(());
    }
    if cell["stage"].as_str() == Some("awaiting_entry") {
        if now > ms(&cell["expiresAt"]) { cell["stage"] = json!("entry_not_observed"); return Ok(()); }
        if request < ms(&cell["preparedAt"]) + t_latency
            || b["versionKey"] == cell["preparedVersion"] {
            return Ok(());
        }
        let ask = depth(&b["asks"], "buy", Some(&signal["limit"]))?;
        let bid = depth(&b["bids"], "sell", None)?;
        let trough_ok = signal["troughBid"].is_null()
            || (bid.is_some()
                && amount(&json!(bid.clone().unwrap()))?
                    >= amount(&signal["troughBid"])? + BigInt::from(baal_paper::fixed::TICK));
        if ask.is_none() || bid.is_none()
            || amount(&b["asks"][0]["price"])? - amount(&b["bids"][0]["price"])?
                > amount(&policy["maximumSpread"])?
            || !trough_ok {
            return Ok(());
        }
        cell["latencyMs"] = json!(t_latency);
        enter(cell, &json!(ask.unwrap()), &row["at"], &b["receivedAt"], &b["versionKey"], policy);
        return Ok(());
    }
    if cell["stage"].as_str() != Some("entered") { return Ok(()); }
    let keys: Vec<String> = cell["exits"].as_object().cloned().unwrap_or_default()
        .keys().cloned().collect();
    for h in keys {
        let exit = cell["exits"][&h].clone();
        if exit["status"].as_str() != Some("awaiting_outcome") { continue; }
        if now > ms(&exit["expiresAt"]) {
            cell["exits"][&h]["status"] = json!("outcome_not_observed");
            continue;
        }
        if request < ms(&exit["dueAt"]) || b["versionKey"] == cell["entryVersion"] { continue; }
        if let Some(bid) = depth(&b["bids"], "sell", None)? {
            let mut ex = exit.clone();
            let mut c2 = cell.clone();
            label(&mut c2, &mut ex, &json!(bid), row, &b["receivedAt"], &b["requestStartedAt"]);
            cell["exits"][&h] = ex;
        }
    }
    Ok(())
}
/// compareReboundPaths({events, prices, books, cutoff, policy})
pub fn compare_rebound_paths(events: &[V], prices: &[V], books: &[V], cutoff: &str,
    policy: &V) -> Result<V, String> {
    baal_paper::fixed::instant(cutoff)?;
    need(crate::model::digest(policy) == crate::model::digest(&rebound_study_policy()), "POLICY")?;
    let mut cases: Vec<V> = Vec::new();
    let mut active: std::collections::BTreeMap<String, Vec<usize>> = std::collections::BTreeMap::new();
    let mut barriers = json!({"global": V::Null, "markets": {}});
    let mut by_sequence: std::collections::BTreeMap<i64, Vec<V>> = std::collections::BTreeMap::new();
    for e in events.iter().filter(|e|
        e["thresholds"].as_array().map(|t| t.contains(&policy["dipThreshold"])).unwrap_or(false)) {
        by_sequence.entry(e["priceSequence"].as_i64().unwrap_or(0)).or_default().push(e.clone());
    }
    macro_rules! revoke {
        ($cases:expr, $ci:expr, $reason:expr) => {{
            let c = &mut $cases[$ci];
            if c["watch"]["status"].as_str() == Some("watching") {
                c["watch"]["status"] = json!($reason);
            }
            for (_, branch) in c["branches"].as_object_mut().unwrap().iter_mut() {
                if branch.is_null() { continue; }
                for (_, t) in branch["trials"].as_object_mut().unwrap().iter_mut() {
                    for kind in ["quote", "depth"] {
                        let cell = &mut t[kind];
                        if cell["stage"].as_str() != Some("entered") {
                            cell["stage"] = json!($reason);
                        } else {
                            for (_, exit) in cell["exits"].as_object_mut().unwrap().iter_mut() {
                                if exit["status"].as_str() == Some("awaiting_outcome") {
                                    exit["status"] = json!($reason);
                                }
                            }
                        }
                    }
                }
            }
        }};
    }
    macro_rules! boundary {
        ($cases:expr, $active:expr, $id:expr, $at:expr, $reason:expr) => {{
            let ids: Vec<String> = match $id {
                None => { barriers["global"] = $at.clone();
                    $active.keys().cloned().collect() }
                Some(id) => { barriers["markets"][&id] = $at.clone(); vec![id] }
            };
            for market in ids {
                for ci in $active.get(&market).cloned().unwrap_or_default() {
                    revoke!($cases, ci, $reason);
                }
            }
        }};
    }
    let mut merged: Vec<(bool, V)> = prices.iter().cloned().map(|r| (false, r))
        .chain(books.iter().cloned().map(|r| (true, r))).collect();
    merged.sort_by(|a, b| {
        ms(&a.1["at"]).cmp(&ms(&b.1["at"]))
            .then((!a.0).cmp(&!b.0))
            .then(a.1["sequence"].as_i64().unwrap_or(0).cmp(&b.1["sequence"].as_i64().unwrap_or(0)))
    });
    need(!merged.is_empty(), "EMPTY")?;
    for (is_book, row) in &merged {
        if *is_book {
            if ["session_start", "session_stop", "invalidation"]
                .contains(&row["kind"].as_str().unwrap_or("")) {
                let id = if row["kind"].as_str() == Some("invalidation") {
                    row["payload"]["marketId"].as_str().map(String::from)
                } else { None };
                boundary!(cases, active, id, row["at"], "source_boundary");
                continue;
            }
            if row["kind"].as_str() != Some("book") { continue; }
            let b = &row["payload"]["book"];
            need(ms(&b["requestStartedAt"]) <= ms(&b["receivedAt"])
                && ms(&b["receivedAt"]) <= ms(&row["at"]), "BOOK_CLOCK")?;
            if ms(&row["at"]) - ms(&b["receivedAt"])
                > policy["signal"]["maximumReceiptLagMs"].as_i64().unwrap_or(0) {
                boundary!(cases, active,
                    Some(b["marketId"].as_str().unwrap_or("").to_string()), row["at"],
                    "stale_publication");
                continue;
            }
            let market = b["marketId"].as_str().unwrap_or("").to_string();
            for ci in active.get(&market).cloned().unwrap_or_default() {
                need(b["exchangeId"] == cases[ci]["exchangeId"], "BOOK_SCOPE")?;
                let keys: Vec<String> = cases[ci]["branches"].as_object().cloned()
                    .unwrap_or_default().keys().cloned().collect();
                for bk in keys {
                    if cases[ci]["branches"][&bk].is_null() { continue; }
                    let mut branch = cases[ci]["branches"][&bk].clone();
                    let signal = branch["signal"].clone();
                    for (_, t) in branch["trials"].as_object_mut().unwrap().iter_mut() {
                        book_trial(&signal, t, b, row, policy)?;
                    }
                    cases[ci]["branches"][&bk] = branch;
                }
            }
            continue;
        }
        let snapshot = &row["event"]["snapshot"];
        need(ms(&snapshot["requestStartedAt"]) <= ms(&snapshot["receivedAt"])
            && ms(&snapshot["receivedAt"]) <= ms(&row["at"]), "PRICE_CLOCK")?;
        let stale = ms(&row["at"]) - ms(&snapshot["receivedAt"])
            > policy["signal"]["maximumReceiptLagMs"].as_i64().unwrap_or(0);
        if stale {
            for q in snapshot["quotes"].as_array().cloned().unwrap_or_default() {
                boundary!(cases, active,
                    Some(q["marketId"].as_str().unwrap_or("").to_string()), row["at"],
                    "stale_publication");
            }
        }
        for id in snapshot["missingIds"].as_array().cloned().unwrap_or_default() {
            let target: Vec<usize> = cases.iter().enumerate()
                .filter(|(_, c)| c["exchangeId"] == id).map(|(i, _)| i).collect();
            for ci in target { revoke!(cases, ci, "missing_quote"); }
        }
        for q in snapshot["quotes"].as_array().cloned().unwrap_or_default() {
            if stale { continue; }
            if !valid_quote(&q) {
                boundary!(cases, active,
                    Some(q["marketId"].as_str().unwrap_or("").to_string()), row["at"],
                    "invalid_quote");
                continue;
            }
            let permitted = [&barriers["global"],
                &barriers["markets"][q["marketId"].as_str().unwrap_or("")]].iter()
                .all(|at| at.is_null() || ms(&snapshot["requestStartedAt"]) > ms(at));
            if !permitted { continue; }
            let market = q["marketId"].as_str().unwrap_or("").to_string();
            for ci in active.get(&market).cloned().unwrap_or_default() {
                let keys: Vec<String> = cases[ci]["branches"].as_object().cloned()
                    .unwrap_or_default().keys().cloned().collect();
                for bk in keys {
                    if cases[ci]["branches"][&bk].is_null() { continue; }
                    let mut branch = cases[ci]["branches"][&bk].clone();
                    let signal = branch["signal"].clone();
                    for (_, t) in branch["trials"].as_object_mut().unwrap().iter_mut() {
                        quote_trial(&signal, t, &q, snapshot, row, policy);
                    }
                    cases[ci]["branches"][&bk] = branch;
                }
                let mut watch = cases[ci]["watch"].clone();
                crate::rebound_entry::advance_rebound_watch(&mut watch, &q, snapshot,
                    row["at"].as_str().unwrap_or(""))?;
                cases[ci]["watch"] = watch.clone();
                if !watch["signal"].is_null() && cases[ci]["branches"]["rebound"].is_null() {
                    cases[ci]["branches"]["rebound"] = make_branch(&watch["signal"], policy);
                }
            }
        }
        for e in by_sequence.get(&row["sequence"].as_i64().unwrap_or(0)).cloned().unwrap_or_default() {
            let q = snapshot["quotes"].as_array().cloned().unwrap_or_default().into_iter()
                .find(|q| q["marketId"] == e["marketId"] && q["exchangeId"] == e["exchangeId"]);
            need(q.is_some()
                && e["priceHash"] == row["hash"] && e["availableAt"] == row["at"]
                && e["receivedAt"] == snapshot["receivedAt"]
                && e["quote"]["entryAsk"].as_str()
                    == Some(price(q.as_ref().unwrap()["bestAsk"].as_f64().unwrap_or(f64::NAN))
                        .unwrap_or_default().as_str()),
                "OPPORTUNITY_SOURCE")?;
            let q = q.unwrap();
            let dip = json!({"marketId":e["marketId"],"exchangeId":e["exchangeId"],
                "availableAt":e["availableAt"],"receivedAt":e["receivedAt"],
                "reference":e["reference"],
                "bid": money(&(amount(&e["reference"]["anchor"])?
                    - amount(&e["reference"]["bidDrop"])?)),
                "ask": e["quote"]["entryAsk"]});
            let phase = if ms(&e["availableAt"]) >= ms(&json!(cutoff)) { "prospective" }
                else if ms(&e["availableAt"]) + policy["developmentPurgeMs"].as_i64().unwrap_or(0)
                    <= ms(&json!(cutoff)) { "development" } else { "purged_boundary" };
            let mut branches = serde_json::Map::new();
            branches.insert("immediate".to_string(),
                make_branch(&json!({"availableAt": e["availableAt"],
                    "limit": e["quote"]["entryAsk"]}), policy));
            branches.insert("rebound".to_string(), V::Null);
            let c = json!({"id":e["id"],"marketId":e["marketId"],"exchangeId":e["exchangeId"],
                "phase":phase,"watch":crate::rebound_entry::create_rebound_watch(&dip)?,
                "branches":branches});
            cases.push(c);
            active.entry(e["marketId"].as_str().unwrap_or("").to_string())
                .or_default().push(cases.len() - 1);
            if stale { let ci = cases.len() - 1; revoke!(cases, ci, "stale_publication"); }
        }
    }
    let through = merged.last().map(|x| x.1["at"].clone()).unwrap_or(V::Null);
    for c in &mut cases {
        if c["watch"]["status"].as_str() == Some("watching") {
            c["watch"]["status"] = json!(if ms(&through) > ms(&c["watch"]["expiresAt"]) {
                "confirmation_not_observed" } else { "right_censored" });
        }
        for (_, branch) in c["branches"].as_object_mut().unwrap().iter_mut() {
            if branch.is_null() { continue; }
            for (_, t) in branch["trials"].as_object_mut().unwrap().iter_mut() {
                for kind in ["quote", "depth"] {
                    let cell = &mut t[kind];
                    if cell["stage"].as_str() != Some("entered") {
                        let stage = cell["stage"].as_str().unwrap_or("").to_string();
                        cell["stage"] = json!(if ms(&through) > ms(&cell["expiresAt"])
                            && stage.starts_with("awaiting_") {
                            format!("{}_not_observed", stage.trim_start_matches("awaiting_"))
                        } else if stage.starts_with("awaiting_") {
                            "right_censored".to_string()
                        } else { stage });
                    } else {
                        for (_, exit) in cell["exits"].as_object_mut().unwrap().iter_mut() {
                            if exit["status"].as_str() == Some("awaiting_outcome") {
                                exit["status"] = json!(if ms(&through) > ms(&exit["expiresAt"]) {
                                    "outcome_not_observed" } else { "right_censored" });
                            }
                        }
                    }
                }
            }
        }
    }
    fn metrics(items: &[V]) -> V {
        let mut statuses = serde_json::Map::new();
        let done: Vec<&V> = items.iter().filter(|x| x["status"].as_str() == Some("observed")).collect();
        for x in items {
            let s = x["status"].as_str().unwrap_or("");
            let n = statuses.get(s).and_then(|v| v.as_i64()).unwrap_or(0) + 1;
            statuses.insert(s.to_string(), json!(n));
        }
        let measure = |key: &str| -> V {
            let values: Vec<BigInt> = done.iter()
                .map(|x| amount(&x[key]).unwrap_or_default()).collect();
            let total: BigInt = values.iter().sum();
            json!({"count":values.len(),
                "positive":values.iter().filter(|x| **x > BigInt::from(0)).count(),
                "negative":values.iter().filter(|x| **x < BigInt::from(0)).count(),
                "zero":values.iter().filter(|x| **x == BigInt::from(0)).count(),
                "average": if values.is_empty() { V::Null }
                    else { json!(money(&(total / BigInt::from(values.len() as i64)))) }})
        };
        json!({"opportunities":items.len(),"statuses":statuses,
            "observedPrices":measure("rawMarkout"),"adverseTicks":measure("adverseMarkout")})
    }
    let mut groups = serde_json::Map::new();
    for phase in ["development", "purged_boundary", "prospective"] {
        let chosen: Vec<&V> = cases.iter().filter(|c| c["phase"].as_str() == Some(phase)).collect();
        let markets: std::collections::BTreeSet<String> = chosen.iter()
            .filter_map(|c| c["marketId"].as_str().map(String::from)).collect();
        let mut strategies = serde_json::Map::new();
        for strategy in ["immediate", "rebound"] {
            let signals = chosen.iter().filter(|c| !c["branches"][strategy].is_null()).count();
            let mut outcomes = serde_json::Map::new();
            for latency in policy["latenciesMs"].as_array().cloned().unwrap_or_default() {
                let mut horizons = serde_json::Map::new();
                for horizon in policy["horizonsMs"].as_array().cloned().unwrap_or_default() {
                    let mut kinds = serde_json::Map::new();
                    for kind in ["quote", "depth"] {
                        let items: Vec<V> = chosen.iter().map(|c| {
                            let branch = &c["branches"][strategy];
                            if branch.is_null() {
                                return json!({"status": c["watch"]["status"]});
                            }
                            let cell = &branch["trials"]
                                [latency.as_i64().unwrap_or(0).to_string()][kind];
                            if cell["stage"].as_str() == Some("entered") {
                                cell["exits"][horizon.as_i64().unwrap_or(0).to_string()].clone()
                            } else {
                                json!({"status": cell["stage"]})
                            }
                        }).collect();
                        kinds.insert(kind.to_string(), metrics(&items));
                    }
                    horizons.insert(horizon.as_i64().unwrap_or(0).to_string(), V::Object(kinds));
                }
                outcomes.insert(latency.as_i64().unwrap_or(0).to_string(), V::Object(horizons));
            }
            strategies.insert(strategy.to_string(),
                json!({"signals": signals, "outcomes": outcomes}));
        }
        groups.insert(phase.to_string(), json!({"opportunities":chosen.len(),
            "markets":markets.len(),"strategies":strategies}));
    }
    Ok(json!({"through":through,"groups":groups,"cases":cases,"networkRequests":0,
        "competitionOrders":0,"paperFills":0,"profitabilityEstablished":false}))
}
/// runReboundStudy({priceBytes, bookBytes, universe, bank})
pub fn run_rebound_study(price_bytes: &[u8], book_bytes: &[u8], universe: &V, bank: &V,
    workspace_dir: &Path) -> Result<V, String> {
    need(crate::model::digest(&bank["protocol"]) == crate::model::digest(&rebound_study_policy())
        && crate::model::digest(&bank["codeIdentity"]) == crate::model::digest(&identity(workspace_dir)?),
        "FROZEN_PROTOCOL")?;
    crate::dip_markout_study::verify_markout_protocol(&bank["baseBank"], universe, &workspace_dir.join("live-paper"))?;
    let original = crate::dip_markout_study::study_dip_markouts(price_bytes, book_bytes, universe,
        &bank["baseBank"], &workspace_dir.join("live-paper"))?;
    let result = compare_rebound_paths(original["events"].as_array().cloned().unwrap_or_default()
        .as_slice(), &rows(price_bytes)?, &rows(book_bytes)?, bank["cutoff"].as_str().unwrap_or(""),
        &rebound_study_policy())?;
    let mut out = result.clone();
    out["createdAt"] = json!(baal_service_runtime::contract::ms_to_iso(
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default().as_secs_f64() * 1000.0));
    out["mode"] = json!("causal_confirmed_rebound_path_comparison");
    out["bankHash"] = json!(crate::model::digest(bank));
    out["cutoff"] = bank["cutoff"].clone();
    out["protocol"] = bank["protocol"].clone();
    out["sources"] = original["sources"].clone();
    out["limitations"] = json!([
        "Quote outcomes have no verified size/source age and are not paper fills, orders, wallet returns or proof of profitability.",
        "Depth paths require post-signal confirmation, a later independent request/version, original price cap, one-percent depth and a fixed later exit quote. Actual fills are unobserved.",
        "Unconfirmed, missing and revoked opportunities remain in unconditional denominators. Complete-path averages may be selected.",
        "Latencies, horizons and strategy variants share opportunities; market/common-event samples correlate.",
        "Publication times model availability for capture replay, not live consumer receipt. All added fees are zero."]);
    Ok(out)
}
