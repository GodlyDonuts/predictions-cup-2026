// live-paper/basket-relations.mjs — public market relationship evidence reader.
// No relationship in this result is promoted to settlement proof or order
// authorization.
use serde_json::{json, Value as V};
use std::collections::BTreeSet;

fn fail<T>() -> Result<T, String> { Err("RELATION_RESPONSE_INVALID".into()) }
fn uuid(s: &str) -> bool {
    let re = s.len() == 36 && s.bytes().enumerate().all(|(i, b)| match i { 8 | 13 | 18 | 23 => b == b'-', _ => b.is_ascii_hexdigit() });
    re && !s.chars().all(|c| c.is_ascii_uppercase() && c.is_ascii_alphabetic())
}
fn plain(v: &V) -> Result<V, String> {
    if !v.is_object() { return fail(); }
    let m = v.as_object().unwrap();
    if m.keys().any(|k| ["__proto__", "constructor", "prototype"].contains(&k.as_str())) { return fail(); }
    Ok(v.clone())
}
fn text(v: &V, nullable: bool, max: usize) -> Result<V, String> {
    if nullable && v.is_null() { return Ok(V::Null); }
    match v.as_str() { Some(s) if s.len() <= max => Ok(json!(s)), _ => fail() }
}
fn id(v: &V, nullable: bool) -> Result<V, String> {
    if nullable && v.is_null() { return Ok(V::Null); }
    match v.as_str() {
        Some(s) if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && s.as_bytes()[0] != b'0'
            && s.parse::<i64>().is_ok() => Ok(json!(s)),
        _ => fail(),
    }
}
fn member<'a>(v: &'a V, set: &[&'a str]) -> Result<&'a str, String> {
    v.as_str().filter(|s| set.contains(s)).ok_or_else(|| "RELATION_RESPONSE_INVALID".into())
}
fn clock(v: &V, nullable: bool) -> Result<V, String> {
    if nullable && v.is_null() { return Ok(V::Null); }
    let s = v.as_str().ok_or("RELATION_RESPONSE_INVALID")?;
    let re_ok = s.len() >= 20 && s.len() <= 35 && s.as_bytes()[4] == b'-' && s.as_bytes()[7] == b'-'
        && s.as_bytes()[10] == b'T' && s.as_bytes()[13] == b':' && s.as_bytes()[16] == b':'
        && (s.ends_with('Z') || s[s.len() - 6..].contains(|c| c == '+' || c == '-'));
    if !re_ok || baal_service_runtime::contract::parse_iso_ms(s).is_none() { return fail(); }
    // JS checks first-19-chars round-trip
    if let Some(ms) = baal_service_runtime::contract::parse_iso_ms(&format!("{}Z", &s[..19])) {
        if baal_service_runtime::contract::ms_to_iso(ms)[..19] != s[..19] { return fail(); }
    } else { return fail(); }
    Ok(json!(s))
}
fn array(v: &V, max: usize) -> Result<Vec<V>, String> {
    match v.as_array() { Some(a) if a.len() <= max => Ok(a.clone()), _ => fail() }
}
fn canonical(v: &V) -> V {
    match v {
        V::Array(a) => V::Array(a.iter().map(canonical).collect()),
        V::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            V::Object(keys.into_iter().map(|k| (k.clone(), canonical(&m[k]))).collect())
        }
        x => x.clone(),
    }
}
fn expression(v: &V, depth: usize, budget: &mut i64) -> Result<V, String> {
    if v.is_null() { return Ok(V::Null); }
    if depth > 32 { return fail(); }
    *budget -= 1;
    if *budget < 0 { return fail(); }
    plain(v)?;
    let desc = |x: &V, depth: usize, budget: &mut i64| expression(x, depth + 1, budget);
    match v["kind"].as_str() {
        Some("leaf") => {
            if v["exchangeId"].as_i64().filter(|x| *x >= 1).is_none() { return fail(); }
            Ok(json!({"kind": "leaf", "exchangeId": v["exchangeId"]}))
        }
        Some(k @ ("and" | "or")) => {
            let children = array(&v["children"], 1000)?;
            if children.is_empty() { return fail(); }
            let mut out = Vec::new();
            for ch in &children { out.push(desc(ch, depth, budget)?); }
            Ok(json!({"kind": k, "children": out}))
        }
        Some("not") => Ok(json!({"kind": "not", "child": desc(&v["child"], depth, budget)?})),
        Some("cond") => Ok(json!({"kind": "cond", "condition": desc(&v["condition"], depth, budget)?, "consequence": desc(&v["consequence"], depth, budget)?})),
        _ => fail(),
    }
}
fn relationship(v: &V, tournament_id: &str) -> Result<V, String> {
    plain(v)?;
    if !v["id"].as_str().is_some_and(uuid)
        || v["version"].as_i64().filter(|x| *x >= 1).is_none()
        || !v["isExhaustive"].is_boolean()
        || (v.get("tournamentId").is_some() && v["tournamentId"].as_str() != Some(tournament_id)) {
        return fail();
    }
    let roles = ["member", "antecedent", "consequent", "output", "input"];
    let directions = ["antecedent_to_consequent", "condition_consequence_to_output", "inputs_to_output", "symmetric", "ordered_members"];
    let types = ["monotonic", "complementary", "mutually_exclusive", "implication", "boolean_and", "boolean_or", "boolean_not", "conditional_if", "boolean_expression"];
    let statuses = ["active", "stale", "disabled", "settled", "deleted"];
    let mut nodes = Vec::new();
    for node in array(&v["nodes"], 1000)? {
        plain(&node)?;
        if !node["currentPrice"].is_null()
            && !node["currentPrice"].as_f64().is_some_and(|p| p.is_finite() && (0.0..=1.0).contains(&p)) {
            return fail();
        }
        nodes.push(json!({"exchangeId": id(&node["exchangeId"], false)?, "marketId": id(&node["marketId"], true)?,
            "marketTitle": text(&node["marketTitle"], true, 8192)?, "outcome": text(&node["outcome"], true, 8192)?,
            "currentPrice": node["currentPrice"], "role": member(&node["role"], &roles)?,
            "contractId": text(&node["contractId"], true, 8192)?}));
    }
    let direction = plain(&v["direction"])?;
    let constraint = plain(&v["constraint"])?;
    let mut fe = Vec::new();
    for x in array(&direction["fromExchangeIds"], 1000)? { fe.push(id(&x, false)?); }
    let mut te = Vec::new();
    for x in array(&direction["toExchangeIds"], 1000)? { te.push(id(&x, false)?); }
    Ok(json!({"id": v["id"], "type": member(&v["type"], &types)?,
        "source": member(&v["source"], &["auto_derived", "admin_tagged"])?,
        "status": member(&v["status"], &statuses)?, "label": text(&v["label"], true, 8192)?,
        "reason": text(&v["reason"], true, 8192)?, "isExhaustive": v["isExhaustive"],
        "expression": expression(&v["expression"], 0, &mut 1000)?, "nodes": nodes,
        "direction": {"kind": member(&direction["kind"], &directions)?, "fromExchangeIds": fe,
            "toExchangeIds": te, "description": text(&direction["description"], false, 8192)?},
        "constraint": {"description": text(&constraint["description"], false, 8192)?,
            "priceRule": text(&constraint["priceRule"], false, 8192)?,
            "boundSemantics": text(&constraint["boundSemantics"], false, 8192)?,
            "evaluationEndpoint": text(&constraint["evaluationEndpoint"], false, 8192)?},
        "createdAt": clock(&v["createdAt"], true)?, "updatedAt": clock(&v["updatedAt"], true)?,
        "version": v["version"]}))
}
pub struct BasketRelationshipReader<F: Fn(&V) -> Result<V, String>> {
    tournament_id: String,
    max_pages: i64,
    dispatch: F,
    busy: bool,
}
/// createBasketRelationshipReader({tournamentId, dispatch, canOperate, now, maxPages})
impl<F: Fn(&V) -> Result<V, String>> BasketRelationshipReader<F> {
    pub fn new(tournament_id: &str, dispatch: F, can_operate: &dyn Fn() -> bool, max_pages: i64) -> Result<Self, String> {
        if !uuid(tournament_id) || !(1..=10).contains(&max_pages) || !can_operate() && false { return Err("BASKET_RELATION_DEPENDENCIES".into()); }
        let _ = can_operate;
        Ok(Self { tournament_id: tournament_id.into(), max_pages, dispatch, busy: false })
    }
    /// capture() — synchronous port: dispatch(spec) -> Result<response>.
    pub fn capture(&mut self, can_operate: &dyn Fn() -> bool, now: &dyn Fn() -> i64) -> Result<V, String> {
        if self.busy { return Err("BASKET_RELATION_CAPTURE_BUSY".into()); }
        self.busy = true;
        let r = self.capture_inner(can_operate, now);
        self.busy = false;
        r
    }
    fn capture_inner(&mut self, can_operate: &dyn Fn() -> bool, now: &dyn Fn() -> i64) -> Result<V, String> {
        let stamp = |now: &dyn Fn() -> i64| -> Result<String, String> {
            let v = now();
            if v < 0 || v > 86_400_000_000_000_000 { return Err("BASKET_RELATION_CLOCK".into()); }
            Ok(baal_service_runtime::contract::ms_to_iso(v as f64))
        };
        let allowed = |can_operate: &dyn Fn() -> bool| -> bool {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(can_operate)).unwrap_or(false)
        };
        let base = format!("/relationships?tournamentId={}&limit=200",
            baal_service_runtime::history::encode_uri_component(&self.tournament_id));
        let mut snapshots = Vec::new();
        let mut relationships = Vec::new();
        let mut seen_ids: BTreeSet<String> = BTreeSet::new();
        let mut cursors: BTreeSet<String> = BTreeSet::new();
        let mut cursor: Option<String> = None;
        let mut complete = false;
        let mut reason = "PAGE_LIMIT".to_string();
        let started_at = stamp(now)?;
        for _page in 0..self.max_pages {
            if !allowed(can_operate) { reason = "STOPPED".into(); break; }
            let endpoint = match &cursor {
                None => base.clone(),
                Some(c) => format!("{}&cursor={}", base, baal_service_runtime::history::encode_uri_component(c)),
            };
            let spec = json!({"method": "GET", "endpoint": endpoint});
            let response = match (self.dispatch)(&spec) {
                Ok(r) => r,
                Err(_) => { reason = if allowed(can_operate) { "DISPATCH_UNAVAILABLE" } else { "STOPPED" }.into(); break; }
            };
            match self.check_response(&response, &endpoint, &snapshots, &started_at, now, &mut seen_ids, &mut relationships) {
                Ok(Some(next)) => {
                    snapshots.push(next.0);
                    for row in next.1 { relationships.push(row); }
                    match next.2 {
                        None => { complete = true; reason = "COMPLETE".into(); break; }
                        Some(nc) => {
                            if cursors.contains(&nc) { reason = "CURSOR_CYCLE".into(); break; }
                            cursors.insert(nc.clone());
                            cursor = Some(nc);
                        }
                    }
                }
                Ok(None) => { reason = "INVALID_RESPONSE".into(); break; }
                Err(e) if e == "DUPLICATE_RELATIONSHIP" => { reason = "DUPLICATE_RELATIONSHIP".into(); break; }
                Err(_) => { reason = "INVALID_RESPONSE".into(); break; }
            }
        }
        let result = json!({"schemaVersion": 1, "kind": "basket_relationship_capture",
            "tournamentId": self.tournament_id, "startedAt": started_at, "finishedAt": stamp(now)?,
            "complete": complete, "reasonCode": reason, "snapshots": snapshots,
            "relationships": relationships, "orderEligible": false, "proofConstructed": false});
        let mut result = result;
        result["evidenceHash"] = json!(baal_service_runtime::json_text::sha256(
            baal_service_runtime::json_text::js_stringify(&canonical(&result)).as_bytes()));
        Ok(result)
    }
    fn check_response(&self, response: &V, endpoint: &str, snapshots: &[V], started_at: &str,
        now: &dyn Fn() -> i64, seen_ids: &mut BTreeSet<String>, _rels: &mut Vec<V>)
        -> Result<Option<(V, Vec<V>, Option<String>)>, String>
    {
        let r = (|| -> Result<(V, Vec<V>, Option<String>), String> {
            plain(response)?;
            if response["endpoint"].as_str() != Some(endpoint) || response["status"].as_i64() != Some(200) { return fail(); }
            let request_started_at = clock(&response["requestStartedAt"], false)?;
            let received_at = clock(&response["receivedAt"], false)?;
            let data = plain(&response["data"])?;
            let prev_received = snapshots.last().map(|s| s["receivedAt"].as_str().unwrap_or("")).unwrap_or(started_at);
            let req_ms = baal_service_runtime::contract::parse_iso_ms(request_started_at.as_str().unwrap()).unwrap_or(f64::NAN);
            let rec_ms = baal_service_runtime::contract::parse_iso_ms(received_at.as_str().unwrap()).unwrap_or(f64::NAN);
            let prev_ms = baal_service_runtime::contract::parse_iso_ms(prev_received).unwrap_or(f64::NAN);
            if req_ms < prev_ms || rec_ms < req_ms || rec_ms > now() as f64 { return fail(); }
            if data.get("tournamentId").is_some() && data["tournamentId"].as_str() != Some(self.tournament_id.as_str()) { return fail(); }
            let rows_raw = array(&data["data"], 200)?;
            let mut rows = Vec::new();
            for v in &rows_raw { rows.push(relationship(v, &self.tournament_id)?); }
            let pagination = plain(&data["pagination"])?;
            if pagination["limit"].as_i64() != Some(200)
                || !pagination["hasMore"].is_boolean()
                || !(pagination["nextCursor"].is_null() || pagination["nextCursor"].is_string())
                || (pagination["nextCursor"].as_str().is_some_and(|c| c.is_empty() || c.len() > 4096 || c.chars().any(|ch| (ch as u32) < 0x20 || ch as u32 == 0x7f)))
                || (pagination["hasMore"].as_bool() == Some(true) && pagination["nextCursor"].is_null()) {
                return fail();
            }
            let mut page_ids = BTreeSet::new();
            for row in &rows {
                let key = row["id"].as_str().unwrap().to_lowercase();
                if seen_ids.contains(&key) || page_ids.contains(&key) { return Err("DUPLICATE_RELATIONSHIP".into()); }
                page_ids.insert(key.clone());
                seen_ids.insert(key);
            }
            let snapshot = json!({"endpoint": endpoint, "requestStartedAt": request_started_at,
                "receivedAt": received_at, "status": 200,
                "data": {"data": rows, "pagination": {"limit": pagination["limit"],
                    "hasMore": pagination["hasMore"], "nextCursor": pagination["nextCursor"]}}});
            let next = if pagination["hasMore"].as_bool() == Some(true) { pagination["nextCursor"].as_str().map(String::from) } else { None };
            Ok((snapshot, rows, next))
        })();
        match r { Ok(t) => Ok(Some(t)), Err(e) => Err(e) }
    }
}
