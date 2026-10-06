// live-paper/passive-entry-study.mjs — conditional depth paths, not a
// trade-tape reconstruction. A quote touch or shrinking displayed queue can
// never simulate a maker fill.
use num_bigint::BigInt;
use serde_json::{json, Value as V};
use std::path::Path;

fn need(ok: bool, code: &str) -> Result<(), String> {
    if !ok { return Err(format!("PASSIVE_STUDY_{}", code)); }
    Ok(())
}
fn sha(b: &[u8]) -> String { baal_service_runtime::json_text::sha256(b) }
fn decimal(n: f64) -> Result<String, String> {
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
    let keys = ["./passive-entry-study.mjs", "./dip-markout-study.mjs", "./model.mjs",
        "../paper/fixed.mjs", "../execution/ledger.mjs"];
    let files = ["live-paper/passive-entry-study.mjs", "live-paper/dip-markout-study.mjs",
        "live-paper/model.mjs", "paper/fixed.mjs", "execution/ledger.mjs"];
    let mut m = serde_json::Map::new();
    for (k, p) in keys.iter().zip(files.iter()) {
        m.insert(k.to_string(), json!(sha(&std::fs::read(workspace_dir.join(p)).map_err(|e| e.to_string())?)));
    }
    Ok(V::Object(m))
}
/// PASSIVE_ENTRY_POLICY
pub fn passive_entry_policy() -> V {
    json!({"schemaVersion":1,"dipThreshold":"0.010000",
        "strategies":["cross_ask","rest_bid","improve_bid"],"latenciesMs":[1000,3000],
        "horizonsMs":[60000,180000,300000],"preparationLifetimeMs":60000,
        "admissionLifetimeMs":60000,"restLifetimeMs":120000,"labelLatenessMs":60000,
        "maximumRequestDurationMs":20000,"maximumPublicationLagMs":20000,
        "maximumSpread":"0.020000","strictCrossingTicks":1,"crossingConfirmations":2,
        "maximumCrossingGapMs":30000,"depthFractionBps":100,"orderShares":1,
        "entryCapPadTicks":1,"takerAdverseTicks":1,"exitAdverseTicks":1,"feeBps":0,
        "developmentPurgeMs":660000,
        "queuePolicy":"two_independent_price_through_observations_not_cancellations"})
}
/// createPassiveStudyBank(cutoff, baseBank)
pub fn create_passive_study_bank(cutoff: &str, base_bank: &V,
    workspace_dir: &Path) -> Result<V, String> {
    baal_paper::fixed::instant(cutoff)?;
    Ok(json!({"name":"passive-entry-path-20261003-v1","cutoff":cutoff,
        "baseBank":base_bank,"protocol":passive_entry_policy(),
        "codeIdentity":identity(workspace_dir)?}))
}
fn end_expired(cell: &mut V, now: i64) {
    let stage = cell["stage"].as_str().unwrap_or("").to_string();
    if stage.starts_with("awaiting_") && now > ms(&cell["expiresAt"]) {
        cell["stage"] = json!(format!("{}_not_observed", stage.trim_start_matches("awaiting_")));
    } else if stage == "resting" && now > ms(&cell["expiresAt"]) {
        cell["stage"] = json!("price_through_not_observed");
    } else if stage == "entered" {
        for (_, x) in cell["exits"].as_object_mut().unwrap().iter_mut() {
            if x["status"].as_str() == Some("awaiting_outcome") && now > ms(&x["expiresAt"]) {
                x["status"] = json!("outcome_not_observed");
            }
        }
    }
}
fn enough(level: &V, policy: &V) -> bool {
    (level["quantity"].as_f64().unwrap_or(0.0)
        * policy["depthFractionBps"].as_f64().unwrap_or(0.0) / 10000.0).floor()
        >= policy["orderShares"].as_f64().unwrap_or(0.0)
}
fn buy_level(levels: &V, limit: &str, adverse: i64, policy: &V) -> Result<Option<V>, String> {
    let limit_v = amount(&json!(limit))?;
    for x in levels.as_array().cloned().unwrap_or_default() {
        if !enough(&x, policy) { continue; }
        if amount(&x["price"])? + BigInt::from(adverse) * BigInt::from(baal_paper::fixed::TICK)
            <= limit_v {
            return Ok(Some(x));
        }
    }
    Ok(None)
}
fn sell_level(levels: &V, policy: &V) -> Option<V> {
    levels.as_array().cloned().unwrap_or_default().into_iter().find(|x| enough(x, policy))
}
fn provenance(b: &V, row: &V) -> V {
    json!({"availableAt":row["at"],"requestStartedAt":b["requestStartedAt"],
        "receivedAt":b["receivedAt"],"versionKey":b["versionKey"],
        "sourceSequence":row["sequence"],"sourceHash":row["hash"]})
}
fn enter(cell: &mut V, value: &str, b: &V, row: &V, latency: i64, policy: &V) {
    cell["stage"] = json!("entered");
    cell["entryPrice"] = json!(value);
    cell["entry"] = provenance(b, row);
    let mut exits = serde_json::Map::new();
    for h in policy["horizonsMs"].as_array().cloned().unwrap_or_default() {
        let hv = h.as_i64().unwrap_or(0);
        let due = ms(&row["at"]) + hv + latency;
        exits.insert(hv.to_string(), json!({"status":"awaiting_outcome",
            "dueAt":baal_service_runtime::contract::ms_to_iso(due as f64),
            "expiresAt":baal_service_runtime::contract::ms_to_iso(
                (due + policy["labelLatenessMs"].as_i64().unwrap_or(0)) as f64)}));
    }
    cell["exits"] = V::Object(exits);
}
fn book_trial(c: &V, strategy: &str, latency: i64, cell: &mut V, b: &V, row: &V,
    policy: &V) -> Result<(), String> {
    let now = ms(&row["at"]);
    let request = ms(&b["requestStartedAt"]);
    let top_ask = amount(&b["asks"][0]["price"])?;
    let top_bid = amount(&b["bids"][0]["price"])?;
    let spread = &top_ask - &top_bid;
    let tick = BigInt::from(baal_paper::fixed::TICK);
    if cell["stage"].as_str() == Some("awaiting_preparation") {
        if request <= ms(&c["availableAt"]) || spread > amount(&policy["maximumSpread"])? {
            return Ok(());
        }
        let ask = buy_level(&b["asks"], c["maximumCost"].as_str().unwrap_or(""),
            policy["takerAdverseTicks"].as_i64().unwrap_or(0), policy)?;
        let bid = sell_level(&b["bids"], policy);
        if ask.is_none() || bid.is_none() { return Ok(()); }
        if strategy == "improve_bid" && top_bid.clone() + tick.clone() >= top_ask {
            cell["stage"] = json!("spread_no_improvement");
            return Ok(());
        }
        cell["limit"] = if strategy == "cross_ask" {
            c["maximumCost"].clone()
        } else {
            json!(money(&(top_bid + if strategy == "improve_bid" { tick } else { BigInt::from(0) })))
        };
        cell["preparation"] = provenance(b, row);
        cell["stage"] = json!("awaiting_admission");
        cell["expiresAt"] = json!(baal_service_runtime::contract::ms_to_iso(
            (now + policy["admissionLifetimeMs"].as_i64().unwrap_or(0)) as f64));
        return Ok(());
    }
    if cell["stage"].as_str() == Some("awaiting_admission") {
        if request < ms(&cell["preparation"]["availableAt"]) + latency
            || b["versionKey"] == cell["preparation"]["versionKey"] {
            return Ok(());
        }
        if spread > amount(&policy["maximumSpread"])? { return Ok(()); }
        if strategy == "cross_ask" {
            let ask = buy_level(&b["asks"], cell["limit"].as_str().unwrap_or(""),
                policy["takerAdverseTicks"].as_i64().unwrap_or(0), policy)?;
            if let Some(ask) = ask {
                let v = money(&(amount(&ask["price"])?
                    + BigInt::from(policy["takerAdverseTicks"].as_i64().unwrap_or(0)) * tick));
                enter(cell, &v, b, row, latency, policy);
            }
            return Ok(());
        }
        if top_ask <= amount(&cell["limit"])? {
            cell["stage"] = json!("post_only_would_cross");
            return Ok(());
        }
        cell["admission"] = provenance(b, row);
        let ahead: f64 = b["bids"].as_array().cloned().unwrap_or_default().iter()
            .filter(|x| amount(&x["price"]).map(|p| p >= amount(&cell["limit"]).unwrap_or_default()).unwrap_or(false))
            .map(|x| x["quantity"].as_f64().unwrap_or(0.0)).sum();
        cell["displayedAhead"] = json!(ahead);
        cell["stage"] = json!("resting");
        cell["expiresAt"] = json!(baal_service_runtime::contract::ms_to_iso(
            (now + policy["restLifetimeMs"].as_i64().unwrap_or(0)) as f64));
        cell["crossing"] = V::Null;
        return Ok(());
    }
    if cell["stage"].as_str() == Some("resting") {
        if request <= ms(&cell["admission"]["availableAt"])
            || b["versionKey"] == cell["admission"]["versionKey"] {
            return Ok(());
        }
        let crossed_limit = money(&(amount(&cell["limit"])?
            - BigInt::from(policy["strictCrossingTicks"].as_i64().unwrap_or(0)) * tick));
        let crossed = buy_level(&b["asks"], &crossed_limit, 0, policy)?;
        if crossed.is_none() {
            cell["crossing"] = V::Null;
            return Ok(());
        }
        if cell["crossing"].is_null()
            || now - ms(&cell["crossing"]["availableAt"])
                > policy["maximumCrossingGapMs"].as_i64().unwrap_or(0) {
            cell["crossing"] = provenance(b, row);
            return Ok(());
        }
        if request <= ms(&cell["crossing"]["availableAt"])
            || b["versionKey"] == cell["crossing"]["versionKey"] {
            return Ok(());
        }
        cell["crossingConfirmation"] = provenance(b, row);
        let limit = cell["limit"].as_str().unwrap_or("").to_string();
        enter(cell, &limit, b, row, latency, policy);
        return Ok(());
    }
    if cell["stage"].as_str() != Some("entered") { return Ok(()); }
    let keys: Vec<String> = cell["exits"].as_object().cloned().unwrap_or_default()
        .keys().cloned().collect();
    for h in keys {
        let exit = cell["exits"][&h].clone();
        if exit["status"].as_str() != Some("awaiting_outcome")
            || request < ms(&exit["dueAt"])
            || b["versionKey"] == cell["entry"]["versionKey"] {
            continue;
        }
        let Some(bid) = sell_level(&b["bids"], policy) else { continue };
        let value = amount(&bid["price"])?
            - BigInt::from(policy["exitAdverseTicks"].as_i64().unwrap_or(0)) * tick.clone();
        if value <= BigInt::from(0) {
            cell["exits"][&h]["status"] = json!("invalid_exit_price");
            continue;
        }
        let mut ex = exit;
        ex["status"] = json!("observed");
        ex["exitPrice"] = json!(money(&value));
        ex["netMarkout"] = json!(money(&(value - amount(&cell["entryPrice"])?)));
        ex["source"] = provenance(b, row);
        cell["exits"][&h] = ex;
    }
    Ok(())
}
/// comparePassivePaths({events, prices, books, cutoff, policy})
pub fn compare_passive_paths(events: &[V], prices: &[V], books: &[V], cutoff: &str,
    policy: &V) -> Result<V, String> {
    baal_paper::fixed::instant(cutoff)?;
    need(crate::model::digest(policy) == crate::model::digest(&passive_entry_policy()), "POLICY")?;
    let mut cases: Vec<V> = Vec::new();
    let mut active: std::collections::BTreeMap<String, Vec<usize>> = std::collections::BTreeMap::new();
    let mut barriers = json!({"global": V::Null, "markets": {}});
    let mut by_sequence: std::collections::BTreeMap<i64, Vec<V>> = std::collections::BTreeMap::new();
    for e in events.iter().filter(|e|
        e["thresholds"].as_array().map(|t| t.contains(&policy["dipThreshold"])).unwrap_or(false)) {
        by_sequence.entry(e["priceSequence"].as_i64().unwrap_or(0)).or_default().push(e.clone());
    }
    let trial = |available_at: &V| -> V {
        json!({"stage":"awaiting_preparation",
            "expiresAt":baal_service_runtime::contract::ms_to_iso(
                (ms(available_at) + policy["preparationLifetimeMs"].as_i64().unwrap_or(0)) as f64)})
    };
    macro_rules! revoke {
        ($cases:expr, $active:expr, $id:expr, $at:expr, $reason:expr) => {{
            let _id: Option<String> = $id;
            if _id.is_none() { barriers["global"] = $at.clone(); }
            else { barriers["markets"][_id.as_ref().unwrap()] = $at.clone(); }
            let targets: Vec<usize> = if _id.is_none() {
                (0..$cases.len()).collect()
            } else {
                $active.get(_id.as_ref().unwrap()).cloned().unwrap_or_default()
            };
            for ci in targets {
                let c = &mut $cases[ci];
                for (_, trials) in c["strategies"].as_object_mut().unwrap().iter_mut() {
                    for (_, cell) in trials.as_object_mut().unwrap().iter_mut() {
                        end_expired(cell, ms(&$at));
                        let stage = cell["stage"].as_str().unwrap_or("").to_string();
                        if stage.starts_with("awaiting_") || stage == "resting" {
                            cell["stage"] = json!($reason);
                        } else if stage == "entered" {
                            for (_, x) in cell["exits"].as_object_mut().unwrap().iter_mut() {
                                if x["status"].as_str() == Some("awaiting_outcome") {
                                    x["status"] = json!($reason);
                                }
                            }
                        }
                    }
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
        for c in &mut cases {
            for (_, trials) in c["strategies"].as_object_mut().unwrap().iter_mut() {
                for (_, cell) in trials.as_object_mut().unwrap().iter_mut() {
                    end_expired(cell, ms(&row["at"]));
                }
            }
        }
        if *is_book {
            if ["session_start", "session_stop", "invalidation"]
                .contains(&row["kind"].as_str().unwrap_or("")) {
                let id = if row["kind"].as_str() == Some("invalidation") {
                    row["payload"]["marketId"].as_str().map(String::from)
                } else { None };
                revoke!(cases, active, id, row["at"], "source_boundary");
                continue;
            }
            if row["kind"].as_str() != Some("book") { continue; }
            let b = &row["payload"]["book"];
            need(ms(&b["requestStartedAt"]) <= ms(&b["receivedAt"])
                && ms(&b["receivedAt"]) <= ms(&row["at"]), "BOOK_CLOCK")?;
            if ms(&b["receivedAt"]) - ms(&b["requestStartedAt"])
                > policy["maximumRequestDurationMs"].as_i64().unwrap_or(0)
                || ms(&row["at"]) - ms(&b["receivedAt"])
                    > policy["maximumPublicationLagMs"].as_i64().unwrap_or(0) {
                revoke!(cases, active,
                    Some(b["marketId"].as_str().unwrap_or("").to_string()), row["at"], "stale_book");
                continue;
            }
            if ![&barriers["global"],
                &barriers["markets"][b["marketId"].as_str().unwrap_or("")]].iter()
                .all(|at| at.is_null() || ms(&b["requestStartedAt"]) > ms(at)) {
                continue;
            }
            if b["asks"].as_array().map(|a| a.is_empty()).unwrap_or(true)
                || b["bids"].as_array().map(|a| a.is_empty()).unwrap_or(true)
                || amount(&b["asks"][0]["price"])? <= amount(&b["bids"][0]["price"])? {
                revoke!(cases, active,
                    Some(b["marketId"].as_str().unwrap_or("").to_string()), row["at"], "invalid_book");
                continue;
            }
            let market = b["marketId"].as_str().unwrap_or("").to_string();
            for ci in active.get(&market).cloned().unwrap_or_default() {
                need(b["exchangeId"] == cases[ci]["exchangeId"], "BOOK_SCOPE")?;
                let mut c = cases[ci].clone();
                for (strategy, trials) in c["strategies"].as_object_mut().unwrap().iter_mut() {
                    for (latency, cell) in trials.as_object_mut().unwrap().iter_mut() {
                        book_trial(&cases[ci].clone(), strategy,
                            latency.parse::<i64>().unwrap_or(0), cell, b, row, policy)?;
                    }
                }
                cases[ci] = c;
            }
            continue;
        }
        let snapshot = &row["event"]["snapshot"];
        need(ms(&snapshot["requestStartedAt"]) <= ms(&snapshot["receivedAt"])
            && ms(&snapshot["receivedAt"]) <= ms(&row["at"]), "PRICE_CLOCK")?;
        let stale = ms(&row["at"]) - ms(&snapshot["receivedAt"])
            > policy["maximumPublicationLagMs"].as_i64().unwrap_or(0);
        for id in snapshot["missingIds"].as_array().cloned().unwrap_or_default() {
            let target: Vec<usize> = cases.iter().enumerate()
                .filter(|(_, c)| c["exchangeId"] == id).map(|(i, _)| i).collect();
            for ci in target {
                let mid = cases[ci]["marketId"].as_str().unwrap_or("").to_string();
                revoke!(cases, active, Some(mid), row["at"], "missing_quote");
            }
        }
        for q in snapshot["quotes"].as_array().cloned().unwrap_or_default() {
            if stale {
                revoke!(cases, active,
                    Some(q["marketId"].as_str().unwrap_or("").to_string()), row["at"], "stale_price");
            } else if q["option"].as_str() != Some("YES")
                || baal_service_runtime::json_text::js_truthy(Some(&q["crossed"]))
                || !q["bestBid"].is_number() || !q["bestAsk"].is_number()
                || q["bestBid"].as_f64().unwrap_or(0.0) <= 0.0
                || q["bestAsk"].as_f64().unwrap_or(1.0) >= 1.0
                || q["bestAsk"].as_f64().unwrap_or(0.0) <= q["bestBid"].as_f64().unwrap_or(1.0) {
                revoke!(cases, active,
                    Some(q["marketId"].as_str().unwrap_or("").to_string()), row["at"], "invalid_quote");
            }
        }
        for e in by_sequence.get(&row["sequence"].as_i64().unwrap_or(0)).cloned().unwrap_or_default() {
            let q = snapshot["quotes"].as_array().cloned().unwrap_or_default().into_iter()
                .find(|q| q["marketId"] == e["marketId"] && q["exchangeId"] == e["exchangeId"]);
            need(q.is_some()
                && e["priceHash"] == row["hash"] && e["availableAt"] == row["at"]
                && e["receivedAt"] == snapshot["receivedAt"]
                && e["quote"]["entryAsk"].as_str()
                    == Some(decimal(q.as_ref().unwrap()["bestAsk"].as_f64().unwrap_or(f64::NAN))
                        .unwrap_or_default().as_str()),
                "OPPORTUNITY_SOURCE")?;
            let phase = if ms(&e["availableAt"]) >= ms(&json!(cutoff)) { "prospective" }
                else if ms(&e["availableAt"]) + policy["developmentPurgeMs"].as_i64().unwrap_or(0)
                    <= ms(&json!(cutoff)) { "development" } else { "purged_boundary" };
            let mut strategies = serde_json::Map::new();
            for name in policy["strategies"].as_array().cloned().unwrap_or_default() {
                let mut trials = serde_json::Map::new();
                for latency in policy["latenciesMs"].as_array().cloned().unwrap_or_default() {
                    trials.insert(latency.as_i64().unwrap_or(0).to_string(), trial(&e["availableAt"]));
                }
                strategies.insert(name.as_str().unwrap_or("").to_string(), V::Object(trials));
            }
            let c = json!({"id":e["id"],"marketId":e["marketId"],"exchangeId":e["exchangeId"],
                "availableAt":e["availableAt"],"phase":phase,
                "maximumCost":money(&(amount(&e["quote"]["entryAsk"])?
                    + BigInt::from(policy["entryCapPadTicks"].as_i64().unwrap_or(0))
                        * BigInt::from(baal_paper::fixed::TICK))),
                "strategies":strategies});
            cases.push(c);
            active.entry(e["marketId"].as_str().unwrap_or("").to_string())
                .or_default().push(cases.len() - 1);
            if stale {
                revoke!(cases, active,
                    Some(e["marketId"].as_str().unwrap_or("").to_string()), row["at"], "stale_price");
            } else if ![&barriers["global"],
                &barriers["markets"][e["marketId"].as_str().unwrap_or("")]].iter()
                .all(|at| at.is_null() || ms(&snapshot["requestStartedAt"]) > ms(at)) {
                revoke!(cases, active,
                    Some(e["marketId"].as_str().unwrap_or("").to_string()), row["at"], "source_boundary");
            }
        }
    }
    let through = merged.last().map(|x| x.1["at"].clone()).unwrap_or(V::Null);
    for c in &mut cases {
        for (_, trials) in c["strategies"].as_object_mut().unwrap().iter_mut() {
            for (_, cell) in trials.as_object_mut().unwrap().iter_mut() {
                end_expired(cell, ms(&through));
                let stage = cell["stage"].as_str().unwrap_or("").to_string();
                if stage.starts_with("awaiting_") || stage == "resting" {
                    cell["stage"] = json!("right_censored");
                } else if stage == "entered" {
                    for (_, x) in cell["exits"].as_object_mut().unwrap().iter_mut() {
                        if x["status"].as_str() == Some("awaiting_outcome") {
                            x["status"] = json!("right_censored");
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
        let values: Vec<BigInt> = done.iter()
            .map(|x| amount(&x["netMarkout"]).unwrap_or_default()).collect();
        let total: BigInt = values.iter().sum();
        json!({"opportunities":items.len(),"statuses":statuses,
            "completedPaths":values.len(),
            "positive":values.iter().filter(|x| **x > BigInt::from(0)).count(),
            "negative":values.iter().filter(|x| **x < BigInt::from(0)).count(),
            "zero":values.iter().filter(|x| **x == BigInt::from(0)).count(),
            "meanNetMarkout": if values.is_empty() { V::Null }
                else { json!(money(&(total / BigInt::from(values.len() as i64)))) }})
    }
    let mut groups = serde_json::Map::new();
    for phase in ["development", "purged_boundary", "prospective"] {
        let chosen: Vec<&V> = cases.iter().filter(|c| c["phase"].as_str() == Some(phase)).collect();
        let markets: std::collections::BTreeSet<String> = chosen.iter()
            .filter_map(|c| c["marketId"].as_str().map(String::from)).collect();
        let mut strategies = serde_json::Map::new();
        for strategy in policy["strategies"].as_array().cloned().unwrap_or_default() {
            let strategy = strategy.as_str().unwrap_or("").to_string();
            let mut latencies = serde_json::Map::new();
            for latency in policy["latenciesMs"].as_array().cloned().unwrap_or_default() {
                let lat = latency.as_i64().unwrap_or(0).to_string();
                let entries = chosen.iter().filter(|c|
                    c["strategies"][&strategy][&lat]["stage"].as_str() == Some("entered")).count();
                let mut outcomes = serde_json::Map::new();
                for h in policy["horizonsMs"].as_array().cloned().unwrap_or_default() {
                    let hk = h.as_i64().unwrap_or(0).to_string();
                    let items: Vec<V> = chosen.iter().map(|c| {
                        let t = &c["strategies"][&strategy][&lat];
                        if t["stage"].as_str() == Some("entered") {
                            t["exits"][&hk].clone()
                        } else {
                            json!({"status": t["stage"]})
                        }
                    }).collect();
                    outcomes.insert(hk, metrics(&items));
                }
                latencies.insert(lat, json!({"entries":entries,"outcomes":outcomes}));
            }
            strategies.insert(strategy, V::Object(latencies));
        }
        groups.insert(phase.to_string(), json!({"opportunities":chosen.len(),
            "markets":markets.len(),"strategies":strategies}));
    }
    Ok(json!({"through":through,"cases":cases,"groups":groups,"networkRequests":0,
        "competitionOrders":0,"paperFills":0,"profitabilityEstablished":false}))
}
/// runPassiveStudy({priceBytes, bookBytes, universe, bank})
pub fn run_passive_study(price_bytes: &[u8], book_bytes: &[u8], universe: &V, bank: &V,
    workspace_dir: &Path) -> Result<V, String> {
    need(crate::model::digest(&bank["protocol"]) == crate::model::digest(&passive_entry_policy())
        && crate::model::digest(&bank["codeIdentity"]) == crate::model::digest(&identity(workspace_dir)?),
        "FROZEN_PROTOCOL")?;
    crate::dip_markout_study::verify_markout_protocol(&bank["baseBank"], universe, &workspace_dir.join("live-paper"))?;
    let original = crate::dip_markout_study::study_dip_markouts(price_bytes, book_bytes, universe,
        &bank["baseBank"], &workspace_dir.join("live-paper"))?;
    let mut out = compare_passive_paths(&original["events"].as_array().cloned().unwrap_or_default(),
        &rows(price_bytes)?, &rows(book_bytes)?, bank["cutoff"].as_str().unwrap_or(""),
        &passive_entry_policy())?;
    out["createdAt"] = json!(baal_service_runtime::contract::ms_to_iso(
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default().as_secs_f64() * 1000.0));
    out["mode"] = json!("conservative_passive_depth_path_comparison");
    out["bankHash"] = json!(crate::model::digest(bank));
    out["cutoff"] = bank["cutoff"].clone();
    out["protocol"] = bank["protocol"].clone();
    out["sources"] = original["sources"].clone();
    out["limitations"] = json!([
        "Maker entries are conditional local depth scenarios. Snapshots cannot prove transaction flow, queue clearance, fill probability or counterfactual competitor response.",
        "Resting entry requires two later independent price-through observations with one-percent size. Quote touches and displayed queue cancellations never create entries.",
        "Taker and exits include one adverse tick; maker cost is its immutable limit. All added fees are zero.",
        "Missing and censored opportunities stay in denominators. Completed averages are selected, markets/latencies/horizons correlate, and paths are not portfolio returns.",
        "Source publication models availability, not live receipt. No platform orders or paper-wallet fills are asserted."]);
    Ok(out)
}
