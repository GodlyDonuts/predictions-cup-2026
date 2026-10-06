// live-paper/probability-paper.mjs — research-bound probability paper cohort.
use baal_paper::fixed::{big_amount, big_fee, money, TICK};
use num_bigint::BigInt;
use serde_json::{json, Value as V};
use std::collections::BTreeSet;

pub const POLICY: &str = "source_probability_lower_bound_paper_v1";
fn digest(v: &V) -> String { baal_service_runtime::json_text::digest(v) }
fn exact(v: &V, keys: &[&str]) -> Result<(), String> {
    let m = v.as_object().ok_or("Unexpected research fields")?;
    let mut got: Vec<&str> = m.keys().map(String::as_str).collect();
    got.sort();
    let mut want = keys.to_vec();
    want.sort();
    if got != want { return Err("Unexpected research fields".into()); }
    Ok(())
}
fn time_of(v: &V) -> Result<i64, String> {
    crate::model::instant(v.as_str().unwrap_or("")).or_else(|_| {
        v.as_str().and_then(baal_service_runtime::contract::parse_iso_ms)
            .map(|m| m as i64).ok_or_else(|| "Invalid research time".into())
    })
}
fn validate_policy(p: &V) -> Result<(), String> {
    if p["maxResearchAgeMs"].as_i64().is_none_or(|x| !(1000..=86_400_000).contains(&x))
        || p["maxHoldMs"].as_i64().is_none_or(|x| !(1000..=604_800_000).contains(&x))
        || big_amount(p["minimumEdge"].as_str().unwrap_or("")).map(|x| x <= BigInt::from(0)).unwrap_or(true) {
        return Err("Invalid probability paper policy".into());
    }
    Ok(())
}
/// validateResearch(r)
pub fn validate_research(r: &V) -> Result<(), String> {
    exact(r, &["schemaVersion","id","marketId","exchangeId","side","predicateHash","recordedAt","sourceAt","expiresAt","lower","upper","method","settlementReviewed","flags","sources"])?;
    let id_ok = r["id"].as_str().is_some_and(|s| !s.is_empty() && s.len() <= 80 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'));
    if r["schemaVersion"].as_i64() != Some(1) || !id_ok
        || !r["marketId"].as_str().is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
        || !r["exchangeId"].as_str().is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
        || r["side"].as_str() != Some("YES")
        || !r["predicateHash"].as_str().is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()))
        || !["subjective_range", "calibrated_model"].contains(&r["method"].as_str().unwrap_or(""))
        || !r["settlementReviewed"].is_boolean() {
        return Err("Invalid research identity".into());
    }
    let p = |v: &V| v.as_str().is_some_and(|s| (s.starts_with("0.") && s.len() == 8 && s[2..].bytes().all(|b| b.is_ascii_digit())) || s == "1.000000");
    if !p(&r["lower"]) || !p(&r["upper"])
        || big_amount(r["lower"].as_str().unwrap())? > big_amount(r["upper"].as_str().unwrap())? {
        return Err("Invalid probability interval".into());
    }
    if time_of(&r["sourceAt"])? > time_of(&r["recordedAt"])? || time_of(&r["expiresAt"])? <= time_of(&r["recordedAt"])? {
        return Err("Invalid research chronology".into());
    }
    let flags = r["flags"].as_array().cloned().unwrap_or_default();
    if flags.iter().any(|f| !["predicate_uncertain","settlement_uncertain","source_conflict","superseded"].contains(&f.as_str().unwrap_or("")))
        || flags.iter().map(|x| x.as_str().unwrap_or("")).collect::<BTreeSet<_>>().len() != flags.len()
        || !r["sources"].as_array().is_some_and(|s| !s.is_empty()) {
        return Err("Invalid research evidence".into());
    }
    for s in r["sources"].as_array().unwrap() {
        exact(s, &["url", "contentHash"])?;
        let url = s["url"].as_str().unwrap_or("");
        let good = url.starts_with("https://")
            && !url.contains('@')
            && !url.contains('?') && !url.contains('#')
            && s["contentHash"].as_str().is_some_and(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()))
            && !url.to_lowercase().contains("ace_");
        if !good { return Err("Invalid research reference".into()); }
    }
    Ok(())
}
/// researchGate(r, b, c, identity, policy)
pub fn research_gate(r: Option<&V>, b: &V, c: &V, identity: &V, policy: &V) -> Result<V, String> {
    validate_policy(policy)?;
    let Some(r) = r else { return Ok(json!({"accepted": false, "reason": "missing_research"})); };
    validate_research(r)?;
    if r["marketId"] != b["marketId"] || r["exchangeId"] != b["exchangeId"]
        || r["predicateHash"] != identity["predicateHash"] || identity["exchangeId"] != b["exchangeId"] {
        return Ok(json!({"accepted": false, "reason": "contract_mismatch"}));
    }
    if !r["settlementReviewed"].as_bool().unwrap_or(false) || !r["flags"].as_array().unwrap().is_empty() {
        return Ok(json!({"accepted": false, "reason": "unresolved_evidence"}));
    }
    if time_of(&r["recordedAt"])? > time_of(&b["requestStartedAt"])? || time_of(&r["sourceAt"])? > time_of(&b["requestStartedAt"])? {
        return Ok(json!({"accepted": false, "reason": "not_yet_known"}));
    }
    if time_of(&b["receivedAt"])? >= time_of(&r["expiresAt"])?
        || time_of(&b["receivedAt"])? - time_of(&r["sourceAt"])? > policy["maxResearchAgeMs"].as_i64().unwrap() {
        return Ok(json!({"accepted": false, "reason": "expired_research"}));
    }
    if time_of(&b["receivedAt"])? - time_of(&b["requestStartedAt"])? > c["maxDataAgeMs"].as_i64().unwrap()
        || time_of(&b["receivedAt"])? - time_of(&b["sourceAt"])? > c["maxDataAgeMs"].as_i64().unwrap()
        || time_of(&b["sourceAt"])? > time_of(&b["receivedAt"])? {
        return Ok(json!({"accepted": false, "reason": "stale_book"}));
    }
    let price = big_amount(b["asks"][0]["price"].as_str().unwrap_or(""))? + BigInt::from(TICK) * BigInt::from(c["slippageTicks"].as_i64().unwrap_or(0));
    let entry = &price + big_fee(&price, &BigInt::from(1), &json!({"feeBps": c["feeBps"], "feePerShare": "0.000000"}))?;
    let edge = big_amount(r["lower"].as_str().unwrap())? - &entry;
    let min_edge = big_amount(policy["minimumEdge"].as_str().unwrap_or(""))?;
    Ok(json!({"accepted": edge >= min_edge,
        "reason": if edge >= min_edge { "lower_bound_edge" } else { "insufficient_edge" },
        "researchHash": digest(r), "edge": edge.to_string(), "lower": r["lower"]}))
}
/// stepProbability(input, b, c, research, identity, policy)
pub fn step_probability(input: &V, b: &V, c: &V, research: Option<&V>, identity: &V, policy: &V) -> Result<V, String> {
    let gate = research_gate(research, b, c, identity, policy)?;
    let mut s = input.clone();
    let ri = s["researchIntents"][b["instrument"].as_str().unwrap_or("")].clone();
    {
        let st = &mut s["strategies"]["mean_reversion"];
        let pending = st["pending"][b["instrument"].as_str().unwrap_or("")].clone();
        if pending["action"].as_str() == Some("buy")
            && (!gate["accepted"].as_bool().unwrap_or(false) || ri != gate["researchHash"]) {
            st["pending"].as_object_mut().unwrap().remove(b["instrument"].as_str().unwrap());
            st["expiredIntents"] = json!(st["expiredIntents"].as_i64().unwrap_or(0) + 1);
        }
    }
    let net_edge = if gate["accepted"].as_bool().unwrap_or(false) {
        Some(gate["edge"].as_str().and_then(|e| e.parse::<f64>().ok()).unwrap_or(f64::NAN) / 1e6)
    } else { None };
    let mut next = crate::model::step_forecast(&s, b, c, None, net_edge,
        policy["maxHoldMs"].as_i64().unwrap(), big_amount(policy["minimumEdge"].as_str().unwrap())?.to_string().parse::<f64>().unwrap_or(0.0) / 1e6)?;
    if next.get("researchIntents").is_none() { next["researchIntents"] = json!({}); }
    if next["strategies"]["mean_reversion"]["pending"][b["instrument"].as_str().unwrap_or("")]["action"].as_str() == Some("buy") {
        next["researchIntents"][b["instrument"].as_str().unwrap_or("")] = gate["researchHash"].clone();
    }
    next["researchDecision"] = json!({"at": b["receivedAt"], "marketId": b["marketId"],
        "researchId": research.map(|r| r["id"].clone()).unwrap_or(V::Null),
        "accepted": gate["accepted"], "reason": gate["reason"], "researchHash": gate["researchHash"],
        "edge": gate["edge"], "lower": gate["lower"]});
    Ok(next)
}
/// createProbabilityCohort(raw, c, {name, identities, research, policy})
pub fn create_probability_cohort(raw: &str, c: &V, name: &str, identities: &V, research: &V, policy: &V) -> Result<V, String> {
    crate::model::validate_config(c)?;
    validate_policy(policy)?;
    if name.is_empty() || name.len() > 80 || !name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        || !research.is_array() {
        return Err("Invalid probability paper policy".into());
    }
    let Some(src) = crate::ml_shadow::verify_source(raw)? else { return Err("Complete source required".into()); };
    if src.records.is_empty() { return Err("Complete source required".into()); }
    let last = src.records.last().unwrap();
    let cutoff = last["event"]["clockAt"].as_str().or(last["at"].as_str()).unwrap_or("").to_string();
    for (m, id) in identities.as_object().cloned().unwrap_or_default() {
        exact(&id, &["exchangeId", "predicateHash"])?;
        if !c["marketIds"].as_array().unwrap().contains(&json!(m))
            || !id["exchangeId"].as_str().is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
            || !id["predicateHash"].as_str().is_some_and(|s| s.len() == 64) {
            return Err("Invalid contract binding".into());
        }
    }
    let mut seen = BTreeSet::new();
    for r in research.as_array().unwrap() {
        validate_research(r)?;
        let mid = r["marketId"].as_str().unwrap_or("");
        if seen.contains(mid)
            || time_of(&r["recordedAt"])? > time_of(&json!(cutoff))?
            || r["predicateHash"] != identities[mid]["predicateHash"]
            || r["exchangeId"] != identities[mid]["exchangeId"] {
            return Err("Invalid frozen research binding".into());
        }
        seen.insert(mid.to_string());
    }
    let bank = json!({"schemaVersion": 1, "policy": POLICY, "name": name, "config": c,
        "identities": identities, "research": research, "parameters": policy,
        "cutoff": cutoff, "sourceRecords": src.records.len(),
        "sourceHash": src.prefix_hashes.last().cloned().unwrap_or_default()});
    let wallet = crate::model::initial_state(c, name, &cutoff)?;
    Ok(json!({"bank": bank, "state": {"schemaVersion": 1, "bankHash": digest(&bank),
        "sourceSequence": src.records.len(), "lastRecordHash": last["hash"],
        "wallet": wallet, "decisions": [], "skips": {}}}))
}
/// advanceProbability(input, raw, bank)
pub fn advance_probability(input: &V, raw: &str, bank: &V) -> Result<V, String> {
    let src = crate::ml_shadow::verify_source(raw)?;
    let mut s = input.clone();
    let c = &bank["config"];
    let ok = src.as_ref().is_some()
        && bank["policy"].as_str() == Some(POLICY)
        && s["bankHash"].as_str() == Some(digest(bank).as_str())
        && src.as_ref().and_then(|x| x.prefix_hashes.get(bank["sourceRecords"].as_i64().unwrap_or(0) as usize - 1).map(|h| h.as_str())) == bank["sourceHash"].as_str()
        && src.as_ref().map(|x| x.records.get(s["sourceSequence"].as_i64().unwrap_or(0) as usize - 1).map(|r| r["hash"].as_str().unwrap_or("")) == Some(s["lastRecordHash"].as_str().unwrap_or(""))).unwrap_or(false);
    if !ok { return Err("Probability cohort/source changed".into()); }
    let src = src.unwrap();
    for r in &src.records {
        if r["sequence"].as_i64().unwrap_or(0) <= s["sourceSequence"].as_i64().unwrap_or(0) { continue; }
        let e = &r["event"];
        let at = e["clockAt"].as_str().or(r["at"].as_str()).unwrap_or("").to_string();
        s["sourceSequence"] = r["sequence"].clone();
        s["lastRecordHash"] = r["hash"].clone();
        macro_rules! revoke {
            ($market:expr) => {{
                let pending = s["wallet"]["strategies"]["mean_reversion"]["pending"].as_object().cloned().unwrap_or_default();
                for (id, p) in pending {
                    let matches = $market.is_none()
                        || s["wallet"]["books"][&id]["marketId"].as_str() == $market;
                    if p["action"].as_str() == Some("buy") && matches {
                        s["wallet"]["strategies"]["mean_reversion"]["pending"].as_object_mut().unwrap().remove(&id);
                        s["wallet"]["strategies"]["mean_reversion"]["expiredIntents"] =
                            json!(s["wallet"]["strategies"]["mean_reversion"]["expiredIntents"].as_i64().unwrap_or(0) + 1);
                    }
                }
            }};
        }
        if e["type"].as_str() == Some("account_read_outage") {
            crate::model::invalidate(&mut s["wallet"], None, &at, "account_outage");
            revoke!(Option::<&str>::None);
            continue;
        }
        if e["type"].as_str() != Some("cycle") { continue; }
        for snap in crate::depth_scope::scoped_snapshots(e, c).unwrap_or_default() {
            if snap["skipped"].is_string() {
                let mid = snap["marketId"].as_str().unwrap_or("").to_string();
                crate::model::invalidate(&mut s["wallet"], Some(&mid), snap["at"].as_str().unwrap_or(&at), snap["skipped"].as_str().unwrap());
                revoke!(Some(mid.as_str()));
                continue;
            }
            let endpoint = snap["endpoint"].as_str().unwrap_or("");
            let Some(rest) = endpoint.strip_prefix("/markets/") else { continue };
            let mid: String = rest.chars().take_while(|ch| ch.is_ascii_digit()).collect();
            if !rest[mid.len()..].starts_with("/orderbook?") || !c["marketIds"].as_array().unwrap().contains(&json!(mid))
                || time_of(&snap["requestStartedAt"])? <= time_of(&json!(bank["cutoff"]))? {
                continue;
            }
            let excluded: BTreeSet<String> = e["accountExclusions"].as_array().cloned().unwrap_or_default().iter().filter_map(|x| x.as_str().map(String::from)).collect();
            let b = crate::model::normalize_book(&snap["data"], &mid, c["tournamentId"].as_str().unwrap(),
                snap["receivedAt"].as_str().unwrap(), snap["requestStartedAt"].as_str().unwrap(),
                c["maxDataAgeMs"].as_i64().unwrap_or(0), &excluded, "event_age_v1");
            let Ok(b) = b else {
                crate::model::invalidate(&mut s["wallet"], Some(&mid), snap["receivedAt"].as_str().unwrap(), "invalid_book");
                revoke!(Some(mid.as_str()));
                s["skips"]["invalid_book"] = json!(s["skips"]["invalid_book"].as_i64().unwrap_or(0) + 1);
                continue;
            };
            let signal = bank["research"].as_array().unwrap().iter().find(|x| x["marketId"] == b["marketId"]).cloned();
            s["wallet"] = step_probability(&s["wallet"], &b, c, signal.as_ref(), &bank["identities"][b["marketId"].as_str().unwrap()], &bank["parameters"])?;
            let decision = s["wallet"]["researchDecision"].clone();
            s["decisions"].as_array_mut().unwrap().push(decision);
        }
    }
    Ok(s)
}
/// probabilityReport(state, bank, at)
pub fn probability_report(state: &V, bank: &V, at: &str) -> V {
    json!({"mode": "source_probability_paper_only", "bankHash": state["bankHash"],
        "sourceSequence": state["sourceSequence"], "lastRecordHash": state["lastRecordHash"],
        "cutoff": bank["cutoff"], "decisions": state["decisions"], "skips": state["skips"],
        "wallet": crate::model::report(&state["wallet"], &bank["config"], at)["strategies"]["mean_reversion"],
        "competitionOrders": 0, "networkRequests": 0, "profitabilityEstablished": false,
        "limitations": ["Research intervals are supplied estimates, not verified calibrated probabilities",
            "Settlement lower-bound edge is a selection heuristic, not a guaranteed horizon exit price",
            "Forced horizon/risk exits use subsequent sampled depth and FIFO; open inventory retains loss and unavailable liquidation risk",
            "Source references/hashes and settlement review are supplied attestations; this adapter does not authenticate their truth"]})
}
