// live-paper/contract-evidence.mjs — scoped contract pair evidence.
use baal_execution::core::payload_hash;
use baal_service_runtime::contract::parse_iso_ms;
use serde_json::{json, Value as V};

fn need(ok: bool, code: &str) -> Result<(), String> { if !ok { return Err(code.into()); } Ok(()) }
fn id(v: &V) -> bool { v.as_str().is_some_and(|s| !s.is_empty() && s.as_bytes()[0] != b'0' && s.bytes().all(|b| b.is_ascii_digit()) && s.parse::<i64>().is_ok()) }
fn uuid(s: &str) -> bool {
    s.len() == 36 && s.bytes().enumerate().all(|(i, b)| match i { 8 | 13 | 18 | 23 => b == b'-', _ => b.is_ascii_hexdigit() })
}
fn stamp(v: &V) -> bool { v.as_str().and_then(parse_iso_ms).is_some() }
fn text(v: &V) -> bool { v.as_str().is_some_and(|s| !s.is_empty() && s.len() <= 300) }
fn context(rows: &V, tournament_id: &str) -> Result<V, String> {
    need(rows.as_array().is_some_and(|r| r.len() == 1 && r[0]["type"].as_str() == Some("tournament")
        && r[0]["tournament"]["id"].as_str() == Some(tournament_id)), "CONTRACT_CONTEXT_MISMATCH")?;
    Ok(rows[0].clone())
}
fn receipt(response: &V, endpoint: &str, after: f64, now: f64) -> Result<V, String> {
    need(response["endpoint"].as_str() == Some(endpoint) && response["status"].as_i64() == Some(200)
        && stamp(&response["requestStartedAt"]) && stamp(&response["receivedAt"]), "CONTRACT_RECEIPT_INVALID")?;
    let request = parse_iso_ms(response["requestStartedAt"].as_str().unwrap()).unwrap();
    let received = parse_iso_ms(response["receivedAt"].as_str().unwrap()).unwrap();
    need(request >= after && received >= request && received <= now && received - request <= 20_000.0, "CONTRACT_RECEIPT_CLOCK_INVALID")?;
    Ok(json!({"endpoint": endpoint, "requestStartedAt": response["requestStartedAt"], "receivedAt": response["receivedAt"]}))
}
fn leg(market_response: &V, nodes_response: &V, scope: &V, tournament_id: &str, after: f64, now: f64) -> Result<V, String> {
    let market_endpoint = format!("/markets/{}?tournamentId={}", scope["marketId"].as_str().unwrap(), tournament_id);
    let node_endpoint = format!("/markets/{}/nodes?tournamentId={}", scope["marketId"].as_str().unwrap(), tournament_id);
    let market_receipt = receipt(market_response, &market_endpoint, after, now)?;
    let nodes_receipt = receipt(nodes_response, &node_endpoint,
        parse_iso_ms(market_receipt["receivedAt"].as_str().unwrap()).unwrap(), now)?;
    let m = &market_response["data"];
    let n = &nodes_response["data"];
    let root = &n["root"];
    need(m["id"].as_str() == scope["marketId"].as_str() && n["market_id"].as_str() == scope["marketId"].as_str(), "CONTRACT_MARKET_MISMATCH")?;
    let c = context(&m["contexts"], tournament_id)?;
    context(&n["contexts"], tournament_id)?;
    need(m["status"].as_str() == Some("open") && m["settledWith"].is_null() && m["settledOn"].is_null()
        && c["status"].as_str() == Some("open") && c["settledWith"].is_null() && c["settledOn"].is_null(),
        "CONTRACT_ALREADY_CLOSED_OR_SETTLED")?;
    need(m["isComposite"] == V::Bool(false) && m["isMultiOutcome"] == V::Bool(false)
        && m["exchanges"].as_array().is_some_and(|a| a.len() == 1 && a[0]["id"].as_str() == scope["exchangeId"].as_str())
        && c["exchanges"].as_array().is_some_and(|a| a.len() == 1 && a[0]["id"].as_str() == scope["exchangeId"].as_str()),
        "CONTRACT_EXCHANGE_MISMATCH")?;
    need(root["node_type"].as_str() == Some("contract") && root["node_id"].as_str() == scope["contractId"].as_str()
        && root["contract_id"].as_str() == scope["contractId"].as_str()
        && root["contract_type"].as_str() == Some("Election Outcome") && root["settled_with"].is_null()
        && stamp(&root["settlement_date"]) && stamp(&m["settlementDate"])
        && parse_iso_ms(root["settlement_date"].as_str().unwrap()) == parse_iso_ms(m["settlementDate"].as_str().unwrap())
        && parse_iso_ms(root["settlement_date"].as_str().unwrap()).unwrap() > now, "CONTRACT_NODE_INVALID")?;
    let d = &root["contract_details"];
    let keys = ["raceId","stageId","usState","raceName","raceStage","winnerName","officeLevel","contractType","electionDate","resolutionType"];
    need(keys.iter().all(|k| text(&d[*k])) && id(&d["raceId"]) && id(&d["stageId"])
        && d["contractType"].as_str() == Some("Election Outcome") && d["resolutionType"].as_str() == Some("Party Winner")
        && d["raceStage"].as_str() == Some("General")
        && d["electionDate"].as_str().is_some_and(|s| s.len() == 10 && s.as_bytes()[4] == b'-' && s.as_bytes()[7] == b'-')
        && d["electionDate"].as_str().is_some_and(|s| baal_service_runtime::contract::ms_to_iso(
            parse_iso_ms(&format!("{}T00:00:00Z", s)).unwrap_or(f64::NAN)).starts_with(s))
        && ["Democratic Party", "Republican Party"].contains(&d["winnerName"].as_str().unwrap_or("")),
        "CONTRACT_PREDICATE_INVALID")?;
    let predicate: serde_json::Map<String, V> = keys.iter().map(|k| (k.to_string(), d[*k].clone())).collect();
    Ok(json!({"marketId": scope["marketId"], "exchangeId": scope["exchangeId"], "contractId": scope["contractId"],
        "settlementDate": root["settlement_date"], "predicate": predicate,
        "receipts": [market_receipt, nodes_receipt],
        "sourceHashes": [payload_hash(m)?, payload_hash(n)?]}))
}
/// projectContractPairEvidence({tournamentId, legs, responses, startedAt, finishedAt}, now)
pub fn project_contract_pair_evidence(tournament_id: &str, legs: &[V], responses: &[V],
    started_at: &str, finished_at: &str, now: i64) -> Result<V, String> {
    need(uuid(tournament_id) && legs.len() == 2
        && legs.iter().all(|s| ["marketId","exchangeId","contractId"].iter().all(|k| id(&s[*k])))
        && ["marketId","exchangeId","contractId"].iter().all(|k| legs[0][*k] != legs[1][*k]), "CONTRACT_PAIR_SCOPE_INVALID")?;
    need(stamp(&json!(started_at)) && stamp(&json!(finished_at))
        && parse_iso_ms(finished_at).unwrap() <= now as f64
        && parse_iso_ms(finished_at).unwrap() >= parse_iso_ms(started_at).unwrap()
        && now as f64 - parse_iso_ms(started_at).unwrap() <= 120_000.0, "CONTRACT_CAPTURE_STALE")?;
    need(responses.len() == 4, "CONTRACT_CAPTURE_INCOMPLETE")?;
    let mut captured = Vec::new();
    let mut after = parse_iso_ms(started_at).unwrap();
    for i in 0..2 {
        let result = leg(&responses[i * 2], &responses[i * 2 + 1], &legs[i], tournament_id, after, parse_iso_ms(finished_at).unwrap())?;
        after = parse_iso_ms(result["receipts"][1]["receivedAt"].as_str().unwrap()).unwrap();
        captured.push(result);
    }
    let (a, b) = (&captured[0], &captured[1]);
    need(a["settlementDate"] == b["settlementDate"]
        && a["predicate"]["winnerName"] != b["predicate"]["winnerName"]
        && a["predicate"].as_object().unwrap().keys().filter(|k| *k != "winnerName")
            .all(|k| a["predicate"][k] == b["predicate"][k]), "CONTRACT_PAIR_PREDICATES_DIFFER")?;
    let mut evidence = json!({"schemaVersion": 1, "kind": "scoped_contract_pair_evidence",
        "tournamentId": tournament_id, "startedAt": started_at, "finishedAt": finished_at,
        "validUntil": baal_service_runtime::contract::ms_to_iso(parse_iso_ms(started_at).unwrap() + 120_000.0),
        "legs": captured, "conditionalNormalMutualExclusion": true, "engineRelationshipProof": false,
        "adminOverrideProtection": false, "refundCompatibilityProven": false,
        "orderEligible": false, "executionAuthority": false});
    evidence["evidenceHash"] = json!(payload_hash(&evidence)?);
    Ok(evidence)
}
/// createContractEvidenceReader({tournamentId, legs, dispatch, canOperate, now})
pub struct ContractEvidenceReader<F: Fn(&V) -> Result<V, String>> {
    tournament_id: String,
    scope: Vec<V>,
    dispatch: F,
    busy: bool,
}
impl<F: Fn(&V) -> Result<V, String>> ContractEvidenceReader<F> {
    pub fn new(tournament_id: &str, legs: &[V], dispatch: F) -> Result<Self, String> {
        need(uuid(tournament_id) && legs.len() == 2, "CONTRACT_READER_DEPENDENCIES")?;
        let scope = legs.to_vec();
        need(scope.iter().all(|s| ["marketId","exchangeId","contractId"].iter().all(|k| id(&s[*k])))
            && ["marketId","exchangeId","contractId"].iter().all(|k| scope[0][*k] != scope[1][*k]), "CONTRACT_PAIR_SCOPE_INVALID")?;
        Ok(Self { tournament_id: tournament_id.into(), scope, dispatch, busy: false })
    }
    /// capture(canOperate, now) — synchronous port.
    pub fn capture(&mut self, can_operate: &dyn Fn() -> bool, now: &dyn Fn() -> i64) -> Result<V, String> {
        need(!self.busy, "CONTRACT_CAPTURE_BUSY")?;
        self.busy = true;
        let r = (|| {
            let started_at = baal_service_runtime::contract::ms_to_iso(now() as f64);
            let mut responses = Vec::new();
            for s in &self.scope {
                for suffix in ["", "/nodes"] {
                    need(can_operate(), "CONTRACT_CAPTURE_STOPPED")?;
                    let endpoint = format!("/markets/{}{}?tournamentId={}",
                        s["marketId"].as_str().unwrap(), suffix, self.tournament_id);
                    responses.push((self.dispatch)(&json!({"method": "GET", "endpoint": endpoint}))?);
                }
            }
            project_contract_pair_evidence(&self.tournament_id, &self.scope, &responses,
                &started_at, &baal_service_runtime::contract::ms_to_iso(now() as f64), now())
        })();
        self.busy = false;
        r
    }
}
