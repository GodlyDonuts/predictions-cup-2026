// live-paper/quote-projection-recovery.mjs — expired paper quote coverage requests.
use baal_execution::core::payload_hash;
use serde_json::{json, Value as V};

fn need(ok: bool, code: &str) -> Result<(), String> { if !ok { return Err(format!("QUOTE_RECOVERY_{}", code)); } Ok(()) }
fn id(v: &V) -> bool { v.as_str().is_some_and(|s| !s.is_empty() && s.as_bytes()[0] != b'0' && s.bytes().all(|b| b.is_ascii_digit()) && s.parse::<i64>().is_ok()) }
fn instant(v: &V) -> i64 { baal_service_runtime::contract::parse_iso_ms(v.as_str().unwrap_or("")).map(|m| m as i64).unwrap_or(i64::MIN) }
/// validateQuoteRecoveryRequest(r, scope, now)
pub fn validate_quote_recovery_request(r: &V, scope: &V, now: i64) -> Result<V, String> {
    let mut keys: Vec<&String> = r.as_object().map(|m| m.keys().collect()).unwrap_or_default();
    keys.sort();
    need(keys.iter().map(|k| k.as_str()).collect::<Vec<_>>().join("|") == "exchangeId|from|key|kind|marketId|requiredCheckpoint|schemaVersion|to", "SHAPE")?;
    need(r["schemaVersion"].as_i64() == Some(1) && r["kind"].as_str() == Some("expired_paper_quote_coverage")
        && id(&r["marketId"]) && id(&r["exchangeId"])
        && scope["exchanges"].as_array().unwrap_or(&vec![]).iter().any(|x| x["marketId"] == r["marketId"] && x["exchangeId"] == r["exchangeId"]), "SCOPE")?;
    let from = instant(&r["from"]);
    let to = instant(&r["to"]);
    need(from < to && to <= now && to - from <= 120_000 && now - from <= 7 * 86_400_000, "WINDOW")?;
    need(r["requiredCheckpoint"].as_str().is_some_and(|s| (s == "0" || !s.starts_with('0')) && s.bytes().all(|b| b.is_ascii_digit()) && s.parse::<i64>().is_ok()), "CHECKPOINT")?;
    let mut body = r.clone();
    need(body.as_object_mut().unwrap().remove("key").is_some() && r["key"].as_str() == payload_hash(&body).ok().as_deref(), "HASH")?;
    Ok(r.clone())
}
/// quoteProjectionRecoveryRequests(state, bank, now)
pub fn quote_projection_recovery_requests(state: &V, bank: &V, now: i64) -> Vec<V> {
    let mut requests: std::collections::BTreeMap<String, V> = Default::default();
    for (_k, w) in state["wallets"].as_object().cloned().unwrap_or_default() {
        for (_qk, q) in w["quotes"].as_object().cloned().unwrap_or_default() {
            if !q["resolvedAt"].is_null() || q["coverageBarrierSequence"].is_null()
                || instant(&q["expiresAt"]) > now || instant(&q["activationAt"]) >= instant(&q["expiresAt"]) {
                continue;
            }
            let body = json!({"schemaVersion":1,"kind":"expired_paper_quote_coverage","marketId":q["marketId"],
                "exchangeId":q["exchangeId"],"from":q["activationAt"],"to":q["expiresAt"],
                "requiredCheckpoint":q["coverageBarrierSequence"]});
            let mut request = body.clone();
            request["key"] = json!(payload_hash(&body).unwrap_or_default());
            if validate_quote_recovery_request(&request, &json!({"exchanges": bank["legs"]}), now).is_ok() {
                requests.insert(request["key"].as_str().unwrap_or("").to_string(), request);
            }
        }
    }
    let mut out: Vec<V> = requests.into_values().collect();
    out.sort_by(|a, b| instant(&a["from"]).cmp(&instant(&b["from"]))
        .then(a["marketId"].as_str().unwrap_or("").parse::<i64>().unwrap_or(0)
            .cmp(&b["marketId"].as_str().unwrap_or("").parse::<i64>().unwrap_or(0))));
    out.truncate(4);
    out
}
