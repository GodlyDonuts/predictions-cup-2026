// data/frozen-ledger-before-rename-20261005.mjs — frozen copy of the intent
// ledger module (canonical JSON journal + owner lease). Ported verbatim.
use serde_json::{json, Value as V};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const VERSION: i64 = 1;

fn identity_ok(s: &str) -> bool {
    !s.is_empty() && s.len() <= 160
        && s.chars().all(|c| c.is_ascii_alphanumeric() || "_".contains(c)
            || ".:".contains(c) || c == '-')
}
fn semantic_uncertainty(s: &str) -> bool {
    let t = s.to_lowercase();
    ["semantic_mismatch", "missing_order_id", "null_order_id", "conflict",
        "canonical_order_mismatch", "unattributable_order_id"].iter()
        .any(|p| t.contains(p))
}
fn clone(v: &V) -> V { v.clone() }
fn fail<T>(code: &str, _message: &str) -> Result<T, String> { Err(code.to_string()) }
fn is_object(v: &V) -> bool { matches!(v, V::Object(_)) }
fn id(value: &V, label: &str) -> Result<String, String> {
    match value {
        V::String(s) if identity_ok(s)
            && !["__proto__", "constructor", "prototype"].contains(&s.as_str()) => Ok(s.clone()),
        _ => fail("INVALID_INPUT", &format!("Invalid {}", label)),
    }
}
fn exact_keys(value: &V, allowed: &[&str], label: &str) -> Result<(), String> {
    if !is_object(value)
        || value.as_object().unwrap().keys().any(|k| !allowed.contains(&k.as_str())) {
        return fail("INVALID_INPUT", &format!("Invalid {}", label));
    }
    Ok(())
}
fn instant(value: &V) -> Result<String, String> {
    match value {
        V::String(s)
            if baal_service_runtime::contract::parse_iso_ms(s).is_some()
                && baal_service_runtime::contract::ms_to_iso(
                    baal_service_runtime::contract::parse_iso_ms(s).unwrap()) == *s =>
            Ok(s.clone()),
        _ => fail("INVALID_INPUT", "Invalid timestamp"),
    }
}

/// normalize for canonical JSON: only finite JSON values allowed; objects
/// sorted recursively (canonical_json already sorts keys).
fn check_finite(v: &V) -> Result<(), String> {
    match v {
        V::Null | V::String(_) | V::Bool(_) => Ok(()),
        V::Number(n) => {
            if n.is_f64() && !n.as_f64().unwrap().is_finite() {
                fail("INVALID_INPUT", "Canonical JSON requires finite JSON values")
            } else { Ok(()) }
        }
        V::Array(a) => a.iter().try_for_each(check_finite),
        V::Object(o) => o.values().try_for_each(check_finite),
    }
}
pub fn canonical_json(value: &V) -> Result<String, String> {
    check_finite(value)?;
    baal_service_runtime::json_text::canonical_json(value)
}
pub fn payload_hash(body: &V) -> Result<String, String> {
    Ok(format!("{:x}", Sha256::digest(canonical_json(body)?.as_bytes())))
}
fn hash(value: &V) -> Result<String, String> { payload_hash(value) }

fn write_all(f: &mut std::fs::File, text: &str) -> Result<(), String> {
    std::io::Write::write_all(f, text.as_bytes()).map_err(|e| e.to_string())?;
    std::io::Write::flush(f).map_err(|e| e.to_string())
}
fn sync_directory(dir: &Path) -> Result<(), String> {
    let f = std::fs::File::open(dir).map_err(|e| e.to_string())?;
    f.sync_all().map_err(|e| e.to_string())
}

/// Projections are replaceable caches. The append-only journal is authoritative.
pub fn atomic_json(file: &Path, value: &V) -> Result<(), String> {
    let temporary = file.with_file_name(format!("{}.{}.{}.tmp",
        file.file_name().unwrap_or_default().to_string_lossy(),
        std::process::id(), uuid::Uuid::new_v4()));
    let body = format!("{}\n", canonical_json(value)?);
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; o.mode(0o600); }
    let mut f = match o.open(&temporary) {
        Ok(f) => f,
        Err(e) => {
            let _ = std::fs::remove_file(&temporary);
            return Err(e.to_string());
        }
    };
    if let Err(e) = write_all(&mut f, &body).and_then(|_| f.sync_all().map_err(|e| e.to_string())) {
        drop(f);
        let _ = std::fs::remove_file(&temporary);
        return Err(e);
    }
    drop(f);
    if let Err(e) = std::fs::rename(&temporary, file).map_err(|e| e.to_string()) {
        let _ = std::fs::remove_file(&temporary);
        return Err(e);
    }
    sync_directory(file.parent().unwrap_or_else(|| Path::new(".")))?;
    Ok(())
}

pub struct OwnerLease {
    pub file: PathBuf,
    pub identity: V,
    dir: PathBuf,
    released: bool,
}
impl OwnerLease {
    pub fn assert_ownership(&self) -> Result<(), String> {
        if self.released {
            return fail("OWNER_RELEASED", "Execution owner lease has been released");
        }
        let current: V = std::fs::read_to_string(&self.file)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .ok_or_else(|| "OWNER_CHANGED".to_string())?;
        if canonical_json(&current)? != canonical_json(&self.identity)? {
            return fail("OWNER_CHANGED", "Execution owner lease identity changed");
        }
        Ok(())
    }
    pub fn release(&mut self) -> Result<(), String> {
        if self.released { return Ok(()); }
        self.assert_ownership()?;
        let _ = std::fs::remove_file(&self.file);
        let _ = sync_directory(&self.dir);
        self.released = true;
        Ok(())
    }
}

pub fn acquire_owner_lease(dir: &Path, owner_id: &str, run_id: &str, scope: &str,
        now_ms: f64) -> Result<OwnerLease, String> {
    id(&json!(owner_id), "owner")?;
    id(&json!(run_id), "run")?;
    id(&json!(scope), "scope")?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| e.to_string())?;
    }
    let file = dir.join("execution-owner.lock");
    let record = json!({"schemaVersion": VERSION, "ownerId": owner_id,
        "runId": run_id, "scope": scope, "pid": std::process::id(),
        "token": uuid::Uuid::new_v4().to_string(),
        "createdAt": baal_service_runtime::contract::ms_to_iso(now_ms)});
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; o.mode(0o600); }
    let mut fd = match o.open(&file) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists =>
            return fail("OWNER_LOCKED",
                "Execution owner lock already exists; explicit recovery is required"),
        Err(e) => return Err(e.to_string()),
    };
    let body = format!("{}\n", canonical_json(&record)?);
    if let Err(e) = write_all(&mut fd, &body)
        .and_then(|_| fd.sync_all().map_err(|e| e.to_string()))
        .and_then(|_| sync_directory(dir)) {
        drop(fd);
        let _ = std::fs::remove_file(&file);
        return Err(e);
    }
    drop(fd);
    Ok(OwnerLease { file, identity: record, dir: dir.to_path_buf(), released: false })
}
impl Drop for OwnerLease {
    fn drop(&mut self) { let _ = self.release(); }
}

const ORDER_FIELDS: [&str; 8] = ["idempotencyKey", "exchangeId", "side", "action",
    "quantity", "price", "expirationDate", "tournamentId"];
const ATTRIBUTION_FIELDS: [&str; 11] = ["ownerId", "runId", "strategyId", "decisionId",
    "policyId", "marketId", "exchangeId", "parentIntentId", "orderId", "signalId",
    "modelHash"];
const OUTCOME_FIELDS: [&str; 5] = ["status", "orderId", "reasonCode", "retryAllowed",
    "earliestRetryAt"];

fn normalize_intent(input: &V) -> Result<V, String> {
    exact_keys(input, &["intentId", "idempotencyKey", "method", "path", "body",
        "bodyJson", "payloadHash", "attribution"], "intent")?;
    let intent_id = id(&input["intentId"], "intent ID")?;
    let idempotency_key = id(&input["idempotencyKey"], "idempotency key")?;
    if input["method"].as_str() != Some("POST") || input["path"].as_str() != Some("/orders") {
        return fail("INVALID_INPUT", "Only order-placement intents belong in this ledger");
    }
    let mut body = input["body"].clone();
    let has_body_json = input.as_object()
        .map(|o| o.contains_key("bodyJson")).unwrap_or(false);
    if has_body_json {
        if !input["bodyJson"].is_string() {
            return fail("INVALID_INPUT", "Invalid frozen order body");
        }
        let parsed: V = serde_json::from_str(input["bodyJson"].as_str().unwrap())
            .map_err(|_| "INVALID_INPUT".to_string())?;
        if !body.is_null() && canonical_json(&body)? != canonical_json(&parsed)? {
            return fail("INTENT_CONFLICT", "Frozen body differs from order terms");
        }
        body = parsed;
    }
    exact_keys(&body, &ORDER_FIELDS, "order terms")?;
    if let V::Object(o) = &body {
        for value in o.values() {
            match value {
                V::Null => {}
                V::String(_) | V::Bool(_) => {}
                V::Number(n) if n.is_f64() && !n.as_f64().unwrap().is_finite() =>
                    return fail("INVALID_INPUT", "Order terms must be scalar JSON values"),
                V::Number(_) => {}
                _ => return fail("INVALID_INPUT", "Order terms must be scalar JSON values"),
            }
        }
    }
    if body["idempotencyKey"].as_str() != Some(idempotency_key.as_str()) {
        return fail("INTENT_CONFLICT", "Body and intent idempotency keys differ");
    }
    let body_json = canonical_json(&body)?;
    let digest = payload_hash(&body)?;
    if has_body_json && input["bodyJson"].as_str() != Some(body_json.as_str()) {
        return fail("INTENT_CONFLICT", "Frozen order body must use canonical JSON");
    }
    let has_payload_hash = input.as_object()
        .map(|o| o.contains_key("payloadHash")).unwrap_or(false);
    if has_payload_hash && input["payloadHash"].as_str() != Some(digest.as_str()) {
        return fail("INTENT_CONFLICT", "Order payload hash differs");
    }
    let attribution = if input["attribution"].is_null() { json!({}) }
        else { input["attribution"].clone() };
    exact_keys(&attribution, &ATTRIBUTION_FIELDS, "intent attribution")?;
    if let V::Object(o) = &attribution {
        for (key, value) in o { id(value, key)?; }
    }
    Ok(json!({"intentId": intent_id, "idempotencyKey": idempotency_key,
        "method": "POST", "path": "/orders", "bodyJson": body_json,
        "payloadHash": digest, "attribution": clone(&attribution)}))
}

fn normalize_outcome(input: &V) -> Result<V, String> {
    exact_keys(input, &OUTCOME_FIELDS, "intent outcome")?;
    if !["uncertain", "acknowledged", "reconciled", "closed"]
            .contains(&input["status"].as_str().unwrap_or("")) {
        return fail("INVALID_INPUT", "Invalid intent outcome status");
    }
    let mut out = serde_json::Map::new();
    out.insert("status".into(), input["status"].clone());
    let has_order_id = input.as_object()
        .map(|o| o.contains_key("orderId")).unwrap_or(false);
    if has_order_id && !input["orderId"].is_null() {
        out.insert("orderId".into(), json!(id(&input["orderId"], "order ID")?));
    }
    let has_reason_code = input.as_object()
        .map(|o| o.contains_key("reasonCode")).unwrap_or(false);
    if has_reason_code {
        let rc = match &input["reasonCode"] {
            V::String(s) if s.len() <= 80 && s.chars().next().map(|c| c.is_ascii_alphabetic())
                    .unwrap_or(false)
                && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') => s.clone(),
            _ => return fail("INVALID_INPUT", "Invalid outcome reason code"),
        };
        out.insert("reasonCode".into(), json!(rc));
    }
    let has_retry_allowed = input.as_object()
        .map(|o| o.contains_key("retryAllowed")).unwrap_or(false);
    let has_earliest_retry = input.as_object()
        .map(|o| o.contains_key("earliestRetryAt")).unwrap_or(false);
    if has_retry_allowed && !input["retryAllowed"].is_boolean() {
        return fail("INVALID_INPUT", "Invalid retry permission");
    }
    if input["status"].as_str() == Some("uncertain") {
        out.insert("retryAllowed".into(),
            json!(input["retryAllowed"].as_bool() == Some(true)));
        if semantic_uncertainty(out.get("reasonCode").and_then(|v| v.as_str()).unwrap_or(""))
                && out["retryAllowed"].as_bool() == Some(true) {
            return fail("INVALID_INPUT", "Semantic uncertainty cannot authorize retry");
        }
        if has_earliest_retry {
            out.insert("earliestRetryAt".into(), json!(instant(&input["earliestRetryAt"])?));
        }
    } else if has_retry_allowed || has_earliest_retry {
        return fail("INVALID_INPUT", "Only uncertain outcomes may authorize retries");
    }
    Ok(V::Object(out))
}

fn empty() -> V {
    json!({"schemaVersion": VERSION, "sequence": 0, "lastHash": "", "scope": V::Null,
        "intents": {}})
}
fn immutable(intent: &V) -> V {
    json!({"intentId": intent["intentId"], "idempotencyKey": intent["idempotencyKey"],
        "method": intent["method"], "path": intent["path"], "bodyJson": intent["bodyJson"],
        "payloadHash": intent["payloadHash"], "attribution": intent["attribution"]})
}

fn apply_event(state: &mut V, event: &V, at: &str) -> Result<(), String> {
    if !is_object(event) { return fail("JOURNAL_CORRUPT", "Invalid intent journal event"); }
    if event["type"].as_str() == Some("prepared") {
        exact_keys(event, &["type", "intent"], "prepared event")?;
        let intent = normalize_intent(&event["intent"])?;
        let parsed_body: V = serde_json::from_str(intent["bodyJson"].as_str().unwrap())
            .map_err(|e| e.to_string())?;
        if parsed_body["tournamentId"] != state["scope"] {
            return fail("SCOPE_MISMATCH", "Prepared order belongs to another execution scope");
        }
        let dup_id = state["intents"].as_object()
            .map(|m| m.contains_key(intent["intentId"].as_str().unwrap()))
            .unwrap_or(false);
        let dup_key = state["intents"].as_object().map(|m| m.values().any(|x|
            x["idempotencyKey"] == intent["idempotencyKey"])).unwrap_or(false);
        if dup_id || dup_key {
            return fail("INTENT_CONFLICT", "Duplicate intent or idempotency key in journal");
        }
        let mut stored = serde_json::Map::new();
        if let V::Object(o) = intent.clone() { for (k, v) in o { stored.insert(k, v); } }
        stored.insert("body".into(), parsed_body);
        stored.insert("status".into(), json!("prepared"));
        stored.insert("attempts".into(), json!(0));
        stored.insert("preparedAt".into(), json!(at));
        stored.insert("updatedAt".into(), json!(at));
        stored.insert("outcome".into(), V::Null);
        state["intents"][intent["intentId"].as_str().unwrap()] = V::Object(stored);
        return Ok(());
    }
    if !["dispatching", "outcome"].contains(&event["type"].as_str().unwrap_or("")) {
        return fail("JOURNAL_CORRUPT", "Unknown intent journal event");
    }
    let intent_id = id(&event["intentId"], "intent ID")?;
    if state["intents"][&intent_id].is_null() {
        return fail("JOURNAL_CORRUPT", "Intent event precedes preparation");
    }
    if event["type"].as_str() == Some("dispatching") {
        exact_keys(event, &["type", "intentId", "attempt"], "dispatch event")?;
        let intent = &state["intents"][&intent_id];
        let retry_wait = intent["status"].as_str() == Some("uncertain")
            && (intent["outcome"]["retryAllowed"].as_bool() != Some(true)
                || (!intent["outcome"]["earliestRetryAt"].is_null()
                    && baal_service_runtime::contract::parse_iso_ms(at).unwrap_or(f64::NAN)
                        < baal_service_runtime::contract::parse_iso_ms(
                            intent["outcome"]["earliestRetryAt"].as_str().unwrap_or(""))
                            .unwrap_or(f64::NAN)));
        if !intent["orderId"].is_null()
            || baal_service_runtime::json_text::js_truthy(Some(&intent["retryBlocked"]))
            || !["prepared", "uncertain"].contains(&intent["status"].as_str().unwrap_or(""))
            || retry_wait {
            return fail("INVALID_TRANSITION",
                "Intent requires reconciliation before dispatch");
        }
        let attempts = intent["attempts"].as_i64().unwrap_or(0);
        if event["attempt"].as_i64() != Some(attempts + 1) {
            return fail("JOURNAL_CORRUPT", "Invalid dispatch attempt sequence");
        }
        let intent = &mut state["intents"][&intent_id];
        intent["status"] = json!("dispatching");
        intent["attempts"] = event["attempt"].clone();
        intent["updatedAt"] = json!(at);
        intent["lastDispatchAt"] = json!(at);
        return Ok(());
    }
    exact_keys(event, &["type", "intentId", "outcome"], "outcome event")?;
    let outcome = normalize_outcome(&event["outcome"])?;
    let transitions: &[&[&str]] = &[
        &["closed"],                     // prepared
        &["uncertain", "acknowledged", "reconciled", "closed"], // dispatching
        &["uncertain", "acknowledged", "reconciled", "closed"], // uncertain
        &["uncertain", "reconciled", "closed"], // acknowledged
        &["reconciled", "closed"],       // reconciled
        &[]];                            // closed
    let status_index = match state["intents"][&intent_id]["status"].as_str() {
        Some("prepared") => 0, Some("dispatching") => 1, Some("uncertain") => 2,
        Some("acknowledged") => 3, Some("reconciled") => 4, _ => 5,
    };
    if !transitions[status_index].contains(&outcome["status"].as_str().unwrap_or("")) {
        return fail("INVALID_TRANSITION", "Invalid intent status transition");
    }
    let intent = state["intents"][&intent_id].clone();
    if (!intent["orderId"].is_null()
            || baal_service_runtime::json_text::js_truthy(Some(&intent["retryBlocked"])))
            && outcome["retryAllowed"].as_bool() == Some(true) {
        return fail("INVALID_TRANSITION",
            "Attributed or semantically uncertain orders cannot authorize another placement");
    }
    if !intent["orderId"].is_null() && !outcome["orderId"].is_null()
            && intent["orderId"] != outcome["orderId"] {
        return fail("INTENT_CONFLICT", "Official order attribution changed");
    }
    if ["acknowledged", "reconciled"].contains(&outcome["status"].as_str().unwrap_or(""))
            && outcome["orderId"].is_null() && intent["orderId"].is_null() {
        return fail("INVALID_INPUT",
            "Acknowledged or reconciled intent requires an order ID");
    }
    let intent = &mut state["intents"][&intent_id];
    if !outcome["orderId"].is_null() { intent["orderId"] = outcome["orderId"].clone(); }
    if outcome["status"].as_str() == Some("uncertain")
            && semantic_uncertainty(outcome["reasonCode"].as_str().unwrap_or("")) {
        intent["retryBlocked"] = json!(true);
    }
    intent["status"] = outcome["status"].clone();
    intent["outcome"] = outcome;
    intent["updatedAt"] = json!(at);
    Ok(())
}

pub fn replay_journal(raw: &str, scope: Option<&str>) -> Result<V, String> {
    if !raw.is_empty() && !raw.ends_with('\n') {
        return fail("JOURNAL_INCOMPLETE",
            "Intent journal has an incomplete crash tail; preserve it for explicit recovery");
    }
    let mut state = empty();
    if let Some(s) = scope { id(&json!(s), "scope")?; }
    if !raw.is_empty() {
        for line in raw[..raw.len() - 1].split('\n') {
            let record: V = serde_json::from_str(line)
                .map_err(|_| "JOURNAL_CORRUPT".to_string())?;
            exact_keys(&record, &["schemaVersion", "sequence", "previous", "at",
                "ownerId", "runId", "scope", "event", "hash"], "journal record")?;
            let mut body = record.clone();
            let record_hash = record["hash"].as_str().unwrap_or("").to_string();
            if let V::Object(m) = &mut body { m.shift_remove("hash"); }
            if body["schemaVersion"].as_i64() != Some(VERSION)
                || body["sequence"].as_i64() != Some(state["sequence"].as_i64().unwrap_or(0) + 1)
                || body["previous"].as_str().unwrap_or("") != state["lastHash"].as_str().unwrap_or("")
                || record_hash != hash(&body)? {
                return fail("JOURNAL_CORRUPT", "Intent journal sequence or hash mismatch");
            }
            instant(&body["at"])?;
            id(&body["ownerId"], "owner")?;
            id(&body["runId"], "run")?;
            id(&body["scope"], "scope")?;
            if (scope.is_some() && body["scope"].as_str() != scope)
                || (!state["scope"].is_null()
                    && body["scope"].as_str() != state["scope"].as_str()) {
                return fail("SCOPE_MISMATCH",
                    "Intent journal belongs to another execution scope");
            }
            state["scope"] = body["scope"].clone();
            apply_event(&mut state, &body["event"], body["at"].as_str().unwrap_or(""))?;
            state["sequence"] = body["sequence"].clone();
            state["lastHash"] = json!(record_hash);
        }
    }
    if state["scope"].is_null() {
        if let Some(s) = scope { state["scope"] = json!(s); }
    }
    Ok(state)
}

pub struct IntentLedger {
    pub dir: PathBuf,
    pub file: PathBuf,
    pub projection_file: PathBuf,
    state: V,
    lease: Option<OwnerLease>,
    closed: bool,
    poisoned: bool,
    identity: (String, String, String),
}
impl IntentLedger {
    /// new({dir, ownerId, runId, scope, now}) — isStopped becomes a caller-side
    /// check: JS requires the callback; pass is_stopped results at call sites.
    pub fn open(dir: &Path, owner_id: &str, run_id: &str, scope: &str,
            now_ms: f64) -> Result<Self, String> {
        let resolved = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
        let file = resolved.join("execution-journal.jsonl");
        let projection_file = resolved.join("execution-state.json");
        let mut lease = acquire_owner_lease(&resolved, owner_id, run_id, scope, now_ms)?;
        let raw = match std::fs::read_to_string(&file) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => { let _ = lease.release(); return Err(e.to_string()); }
        };
        let state = match replay_journal(&raw, Some(scope)) {
            Ok(s) => s,
            Err(e) => { let _ = lease.release(); return Err(e); }
        };
        if let Err(e) = atomic_json(&projection_file, &state) {
            let _ = lease.release();
            return Err(e);
        }
        Ok(IntentLedger { dir: resolved, file, projection_file, state,
            lease: Some(lease), closed: false, poisoned: false,
            identity: (owner_id.to_string(), run_id.to_string(), scope.to_string()) })
    }
    fn check(&self) -> Result<(), String> {
        if self.closed { return fail("LEDGER_CLOSED", "Intent ledger is closed"); }
        if self.poisoned {
            return fail("DURABILITY_FAILURE",
                "Intent ledger requires restart and reconciliation");
        }
        self.lease.as_ref().map(|l| l.assert_ownership()).transpose()?;
        Ok(())
    }
    fn append(&mut self, event: &V, now_ms: f64) -> Result<(), String> {
        self.check()?;
        let at = baal_service_runtime::contract::ms_to_iso(now_ms);
        let mut next = self.state.clone();
        apply_event(&mut next, event, &at)?;
        let body = json!({"schemaVersion": VERSION,
            "sequence": next["sequence"].as_i64().unwrap_or(0) + 1,
            "previous": next["lastHash"], "at": at,
            "ownerId": self.identity.0, "runId": self.identity.1,
            "scope": self.identity.2, "event": event});
        let digest = hash(&body)?;
        let mut record = body.clone();
        record["hash"] = json!(digest);
        let append_res = (|| -> Result<(), String> {
            let mut o = std::fs::OpenOptions::new();
            o.append(true).create(true);
            #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; o.mode(0o600); }
            let mut fd = o.open(&self.file).map_err(|e| e.to_string())?;
            write_all(&mut fd, &format!("{}\n", canonical_json(&record)?))?;
            fd.sync_all().map_err(|e| e.to_string())?;
            drop(fd);
            sync_directory(&self.dir)?;
            Ok(())
        })();
        if let Err(e) = append_res {
            self.poisoned = true;
            return Err(e);
        }
        next["sequence"] = body["sequence"].clone();
        next["lastHash"] = json!(digest);
        next["scope"] = json!(self.identity.2);
        self.state = next;
        if let Err(e) = atomic_json(&self.projection_file, &self.state) {
            self.poisoned = true;
            return Err(e);
        }
        Ok(())
    }
    /// prepareIntent(input) — caller wraps with the JS isStopped gate where the
    /// original call site had one.
    pub fn prepare_intent(&mut self, input: &V, now_ms: f64) -> Result<V, String> {
        self.check()?;
        let intent = normalize_intent(input)?;
        let existing = &self.state["intents"][intent["intentId"].as_str().unwrap_or("")];
        if !existing.is_null() {
            if canonical_json(&immutable(existing))? != canonical_json(&intent)? {
                return fail("INTENT_CONFLICT", "Existing intent terms cannot change");
            }
            return Ok(clone(existing));
        }
        let dup = self.state["intents"].as_object().map(|m| m.values()
            .any(|x| x["idempotencyKey"] == intent["idempotencyKey"])).unwrap_or(false);
        if dup {
            return fail("INTENT_CONFLICT",
                "Idempotency key already belongs to another intent");
        }
        self.append(&json!({"type": "prepared", "intent": intent}), now_ms)?;
        self.get_intent(intent["intentId"].as_str().unwrap_or(""))
    }
    pub fn mark_dispatching(&mut self, intent_id: &str, retry: bool,
            is_stopped: &dyn Fn() -> bool, now_ms: f64) -> Result<V, String> {
        self.check()?;
        if is_stopped() { return fail("STOPPED", "Execution STOP is active"); }
        let key = id(&json!(intent_id), "intent ID")?;
        if self.state["intents"][&key].is_null() {
            return fail("UNKNOWN_INTENT",
                "Intent must be durably prepared before dispatch");
        }
        let intent = &self.state["intents"][&key];
        let status = intent["status"].as_str().unwrap_or("");
        if (!retry && status != "prepared") || (retry && status != "uncertain") {
            return fail("INVALID_TRANSITION",
                "Retry must be explicit and preserve the original prepared intent");
        }
        let attempt = intent["attempts"].as_i64().unwrap_or(0) + 1;
        self.append(&json!({"type": "dispatching", "intentId": key,
            "attempt": attempt}), now_ms)?;
        // A STOP observed during the durability barrier leaves an unresolved
        // dispatch record, which is safer than returning permission to send.
        if is_stopped() {
            return fail("STOPPED", "Execution STOP became active before dispatch");
        }
        self.get_intent(&key)
    }
    pub fn record_outcome(&mut self, intent_id: &str, input: &V,
            now_ms: f64) -> Result<V, String> {
        self.check()?;
        let key = id(&json!(intent_id), "intent ID")?;
        if self.state["intents"][&key].is_null() {
            return fail("UNKNOWN_INTENT", "Unknown prepared intent");
        }
        let outcome = normalize_outcome(input)?;
        let intent = &self.state["intents"][&key];
        if intent["status"].as_str() == outcome["status"].as_str()
            && canonical_json(&intent["outcome"])? == canonical_json(&outcome)? {
            return Ok(clone(intent));
        }
        self.append(&json!({"type": "outcome", "intentId": key,
            "outcome": outcome}), now_ms)?;
        self.get_intent(&key)
    }
    pub fn get_intent(&self, intent_id: &str) -> Result<V, String> {
        self.check()?;
        let key = id(&json!(intent_id), "intent ID")?;
        Ok(if self.state["intents"][&key].is_null() { V::Null }
            else { clone(&self.state["intents"][&key]) })
    }
    pub fn snapshot(&self) -> Result<V, String> {
        self.check()?;
        Ok(clone(&self.state))
    }
    pub fn close(&mut self) {
        if self.closed { return; }
        if let Some(mut l) = self.lease.take() { let _ = l.release(); }
        self.closed = true;
    }
}
pub fn open_ledger(dir: &Path, owner_id: &str, run_id: &str, scope: &str,
        now_ms: f64) -> Result<IntentLedger, String> {
    IntentLedger::open(dir, owner_id, run_id, scope, now_ms)
}
