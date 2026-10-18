//! Finite read-only discovery after a write returned no venue order ID.
//! Preserves the original unknown binding and permanent key. Never retries a
//! write, applies financial events, clears STOP, or decides that absence is proof.
use baal_transport_service::{Config, Endpoint, ReadOnlyAuthority, Scope, SystemClock,
    Transport, credentials::UnixFileCredentials, decimal_units};
use moloch_live_runtime::dispatch::DispatchLog;
use moloch_replay::session::ExclusiveLock;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, io::Write, os::unix::fs::{DirBuilderExt, OpenOptionsExt}, path::PathBuf,
    process::Command, time::{SystemTime, UNIX_EPOCH}};

fn digest(v: &Value) -> Result<String, Box<dyn std::error::Error>> {
    Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(v)?)))
}
fn save(path: &std::path::Path, value: &Value) -> Result<(), Box<dyn std::error::Error>> {
    let mut f = fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(path)?;
    f.write_all(&serde_json::to_vec(value)?)?;
    f.sync_all()?;
    Ok(())
}
fn same_amount(a: &Value, b: &Value) -> bool {
    let parse = |v: &Value| decimal_units(&v.as_str().map(str::to_owned).unwrap_or_else(|| v.to_string()));
    matches!((parse(a),parse(b)),(Ok(x),Ok(y)) if x==y)
}
fn same_request_fields(row: &Value, body: &Value) -> bool {
    ["exchangeId", "side", "action", "tournamentId", "expirationDate"].iter()
        .all(|key| row[*key] == body[*key])
        && same_amount(&row["priceLimit"], &body["price"])
        && same_amount(&row["quantity"], &body["quantity"])
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len()!=4 { return Err("usage: moloch-discover-order CONTROL_JSON QID EXPECTED_CONTROL_SHA256".into()); }
    let raw=fs::read(&args[1])?;
    if format!("{:x}",Sha256::digest(&raw))!=args[3] {return Err("DISCOVERY_CONTROL_CHANGED".into());}
    let c:Value=serde_json::from_slice(&raw)?;
    let root=PathBuf::from(c["root"].as_str().ok_or("DISCOVERY_ROOT")?);
    let _owner=ExclusiveLock::acquire(&PathBuf::from(c["ownerLock"].as_str().ok_or("DISCOVERY_OWNER_LOCK")?))?;
    let pid=Command::new("systemctl").args(["--user","show","moloch-live.service","-p","MainPID","--value"]).output()?;
    if !pid.status.success() || String::from_utf8(pid.stdout)?.trim()!="0" || !root.join("EXECUTION_STOP").is_file() {
        return Err("DISCOVERY_REQUIRES_STOPPED_WRITER".into());
    }
    let qid:u64=args[2].parse()?;
    let log=DispatchLog::open(&root.join("dispatch.ndjson"))?;
    let intent=log.intents.get(&qid).ok_or("DISCOVERY_ORIGINAL_INTENT_REQUIRED")?;
    if !log.attempted.contains(&qid) || log.acknowledgments.contains_key(&qid) || log.closed.contains(&qid) {
        return Err("DISCOVERY_NOT_UNCERTAIN_ORIGINAL_ATTEMPT".into());
    }
    let key=intent["spec"]["body"]["idempotencyKey"].as_str().ok_or("DISCOVERY_ORIGINAL_KEY")?;
    let runtime:Value=serde_json::from_slice(&fs::read(root.join("tartarus-current-native-state.json"))?)?;
    let scope:Scope=serde_json::from_value(runtime["scope"].clone())?;
    if scope.tournament_id!=c["scope"]["tournamentId"].as_str().ok_or("DISCOVERY_TOURNAMENT")? {
        return Err("DISCOVERY_SCOPE_CHANGED".into());
    }
    let payload=digest(&json!({"scope":scope,"spec":intent["spec"]}))?;
    let mut t=Transport::new_with_read_budget(Config{
        queue_dir:root.join("account-queue"), account_scope:c["accountScope"].as_str().ok_or("DISCOVERY_ACCOUNT_SCOPE")?.into(),
        scope, maximum_pages:20, writes_enabled:false},
        Box::new(UnixFileCredentials::approved_file(PathBuf::from(c["credentials"].as_str().ok_or("DISCOVERY_CREDENTIAL_PATH")?))?),
        Box::new(SystemClock),Box::new(ReadOnlyAuthority),100,600)?.with_full_account_observation();
    let before=t.admission_state()?;
    let unknown=before["state"]["unknown"].as_object().ok_or("DISCOVERY_UNKNOWN_STATE")?;
    let binding=unknown.get(key).ok_or("DISCOVERY_EXACT_UNKNOWN_REQUIRED")?;
    if unknown.len()!=1 || binding["key"]!=key || binding["payload_hash"]!=payload || !before["state"]["active"].is_null() {
        return Err("DISCOVERY_UNKNOWN_BINDING_OR_ACTIVE_CHANGED".into());
    }
    let stamp=SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
    let out=root.join(format!("unknown-order-{qid}-readonly-{stamp}"));
    fs::DirBuilder::new().mode(0o700).create(&out)?;
    save(&out.join("original-intent.private.json"),intent)?;
    save(&out.join("queue-before.private.json"),&before)?;
    let mut observations=Vec::new();
    for (label,endpoint) in [("orders",Endpoint::Orders{status:"all".into()}),
        ("account",Endpoint::Account),("positions",Endpoint::Positions),("collateral",Endpoint::Collateral)] {
        let receipt=t.request(endpoint)?;
        let value=serde_json::to_value(receipt)?;
        save(&out.join(format!("{label}.original.json")),&value)?;
        observations.push(value);
    }
    let after=t.admission_state()?;
    if before["state"]["unknown"]!=after["state"]["unknown"] {return Err("DISCOVERY_UNKNOWN_CHANGED".into());}
    save(&out.join("queue-after.private.json"),&after)?;
    let orders=&observations[0]["data"];
    let rows=orders["data"].as_array().ok_or("DISCOVERY_ORDER_ROWS")?;
    let exact:Vec<_>=rows.iter().filter(|r|same_request_fields(r,&intent["spec"]["body"])).collect();
    let key_matches:Vec<_>=rows.iter().filter(|r|r.get("idempotencyKey").is_some_and(|v|v==key)).collect();
    let summary=json!({"schemaVersion":1,"qid":qid,"output":out,"originalPayloadHash":payload,
        "logicalReads":4,"ordersSubmitted":0,"unknownAcceptanceCleared":false,"bankEdited":false,
        "historyRowCount":rows.len(),"historyCoverage":orders["coverage"],"pagination":orders["pagination"],
        "exactRequestFieldCandidateCount":exact.len(),"exactRequestFieldCandidates":exact,
        "exactKeyMatchCount":key_matches.len(),"exactKeyMatches":key_matches,
        "automaticOrderIdentityProven":false,"absenceProvesNonExecution":false,"executionAuthority":false});
    save(&out.join("summary.private.json"),&summary)?;
    println!("{}",json!({"output":out,"qid":qid,"historyRowCount":rows.len(),
        "exactRequestFieldCandidateCount":exact.len(),"exactKeyMatchCount":key_matches.len(),
        "ordersSubmitted":0,"unknownAcceptanceCleared":false}));
    Ok(())
}

#[cfg(test)]mod tests {
    use super::*;
    #[test]fn exact_expiry_and_original_fields_are_hints_not_acknowledgments(){
        let body=json!({"exchangeId":"1068","side":"yes","action":"sell","quantity":7061,
            "price":0.985,"tournamentId":"cup","expirationDate":"2026-10-18T05:19:42.555Z"});
        let mut row=body.clone();row["priceLimit"]=json!("0.985000");
        assert!(same_request_fields(&row,&body));
        for key in ["exchangeId","side","action","quantity","priceLimit","tournamentId","expirationDate"]{
            let mut changed=row.clone();changed[key]=json!("different");assert!(!same_request_fields(&changed,&body));
        }
        row["quantity"]=json!("7061.000000");assert!(same_request_fields(&row,&body));
    }
}
