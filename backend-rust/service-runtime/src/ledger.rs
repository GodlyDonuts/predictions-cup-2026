use crate::atomic::{atomic_json_canonical, read_json};
use crate::json_text::{canonical_json, sha256};
use serde_json::{json, Map, Value as V};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

const VERSION: i64 = 1;

#[derive(Debug)]
pub struct LedgerError {
    pub code: &'static str,
    pub message: String,
}
impl std::fmt::Display for LedgerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}
impl std::error::Error for LedgerError {}
fn fail<T>(code: &'static str, message: impl Into<String>) -> Result<T, LedgerError> {
    Err(LedgerError { code, message: message.into() })
}
pub type LResult<T> = Result<T, LedgerError>;

fn identity_ok(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 160
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"_.:-".contains(&b))
        && !["__proto__", "constructor", "prototype"].contains(&s)
}
fn id(v: &V, label: &str) -> LResult<String> {
    match v.as_str() {
        Some(s) if identity_ok(s) => Ok(s.to_string()),
        _ => fail("INVALID_INPUT", format!("Invalid {label}")),
    }
}
fn id_str(v: &str, label: &str) -> LResult<String> {
    if identity_ok(v) { Ok(v.to_string()) } else { fail("INVALID_INPUT", format!("Invalid {label}")) }
}
fn object(v: &V) -> bool {
    v.is_object()
}
fn exact_keys(v: &V, allowed: &[&str], label: &str) -> LResult<()> {
    if !object(v) {
        return fail("INVALID_INPUT", format!("Invalid {label}"));
    }
    if v.as_object().unwrap().keys().any(|k| !allowed.contains(&k.as_str())) {
        return fail("INVALID_INPUT", format!("Invalid {label}"));
    }
    Ok(())
}
// instant(): Date.parse finite and new Date(v).toISOString() === v — i.e. the
// exact millisecond ISO spelling JS produces.
fn instant(v: &V) -> LResult<String> {
    match v.as_str() {
        Some(s) => {
            let ok = chrono::DateTime::parse_from_rfc3339(s)
                .ok()
                .and_then(|d| {
                    chrono::DateTime::from_timestamp_millis(d.timestamp_millis())
                        .map(|x| x.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
                })
                == Some(s.to_string());
            if ok { Ok(s.to_string()) } else { fail("INVALID_INPUT", "Invalid timestamp") }
        }
        None => fail("INVALID_INPUT", "Invalid timestamp"),
    }
}
fn canonical(v: &V) -> LResult<String> {
    canonical_json(v).map_err(|m| LedgerError { code: "INVALID_INPUT", message: m })
}
fn hash(v: &V) -> LResult<String> {
    Ok(sha256(canonical(v)?.as_bytes()))
}
pub fn payload_hash(body: &V) -> LResult<String> {
    hash(body)
}
fn now_iso(now_ms: f64) -> String {
    chrono::DateTime::from_timestamp_millis(now_ms as i64)
        .map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .unwrap_or_default()
}
fn sync_directory(dir: &Path) -> LResult<()> {
    let f = fs::File::open(dir).map_err(|e| LedgerError { code: "IO", message: e.to_string() })?;
    f.sync_all().map_err(|e| LedgerError { code: "IO", message: e.to_string() })
}
fn atomic_json_ledger(file: &Path, value: &V) -> LResult<()> {
    atomic_json_canonical(file, value)
        .map_err(|m| LedgerError { code: "DURABILITY_FAILURE", message: m })
}

// --- Owner lease -------------------------------------------------------------

pub struct OwnerLease {
    pub file: PathBuf,
    pub identity: V,
    dir: PathBuf,
    released: bool,
}
pub fn acquire_owner_lease(dir: &Path, owner_id: &str, run_id: &str, scope: &str, now_ms: f64) -> LResult<OwnerLease> {
    id_str(owner_id, "owner")?;
    id_str(run_id, "run")?;
    id_str(scope, "scope")?;
    fs::create_dir_all(dir).map_err(|e| LedgerError { code: "IO", message: e.to_string() })?;
    let file = dir.join("execution-owner.lock");
    let record = json!({
        "schemaVersion": VERSION,
        "ownerId": owner_id,
        "runId": run_id,
        "scope": scope,
        "pid": std::process::id(),
        "token": format!("{}-{}", std::process::id(), uuid_counter()),
        "createdAt": now_iso(now_ms),
    });
    let mut f = match fs::OpenOptions::new().write(true).create_new(true).open(&file) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            return fail("OWNER_LOCKED", "Execution owner lock already exists; explicit recovery is required")
        }
        Err(e) => return Err(LedgerError { code: "IO", message: e.to_string() }),
    };
    let text = format!("{}\n", canonical(&record)?);
    if let Err(e) = f.write_all(text.as_bytes()).and_then(|_| f.sync_all()) {
        let _ = fs::remove_file(&file);
        return Err(LedgerError { code: "IO", message: e.to_string() });
    }
    drop(f);
    sync_directory(dir)?;
    Ok(OwnerLease { file, identity: record, dir: dir.to_path_buf(), released: false })
}
fn uuid_counter() -> u128 {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static C: AtomicU64 = AtomicU64::new(0);
    let n = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    (n ^ C.fetch_add(1, Ordering::Relaxed) as u128) & ((1u128 << 96) - 1)
}
impl OwnerLease {
    pub fn assert_ownership(&self) -> LResult<()> {
        if self.released {
            return fail("OWNER_RELEASED", "Execution owner lease has been released");
        }
        let current = match read_json(&self.file) {
            Ok(Some(v)) => v,
            _ => return fail("OWNER_CHANGED", "Execution owner lease is missing or invalid"),
        };
        if canonical(&current)? != canonical(&self.identity)? {
            return fail("OWNER_CHANGED", "Execution owner lease identity changed");
        }
        Ok(())
    }
    pub fn release(&mut self) -> LResult<()> {
        if self.released {
            return Ok(());
        }
        self.assert_ownership()?;
        fs::remove_file(&self.file).map_err(|e| LedgerError { code: "IO", message: e.to_string() })?;
        sync_directory(&self.dir)?;
        self.released = true;
        Ok(())
    }
}

// --- Intent normalization ----------------------------------------------------

const ORDER_FIELDS: [&str; 8] = [
    "idempotencyKey", "exchangeId", "side", "action", "quantity", "price",
    "expirationDate", "tournamentId",
];
const ATTRIBUTION_FIELDS: [&str; 11] = [
    "ownerId", "runId", "strategyId", "decisionId", "policyId", "marketId",
    "exchangeId", "parentIntentId", "orderId", "signalId", "modelHash",
];
const OUTCOME_FIELDS: [&str; 5] = ["status", "orderId", "reasonCode", "retryAllowed", "earliestRetryAt"];
const SEMANTIC_UNCERTAINTY: [&str; 6] = [
    "semantic_mismatch", "missing_order_id", "null_order_id", "conflict",
    "canonical_order_mismatch", "unattributable_order_id",
];
fn semantic_uncertainty(s: &str) -> bool {
    let l = s.to_lowercase();
    SEMANTIC_UNCERTAINTY.iter().any(|p| l.contains(p))
}

pub fn normalize_intent(input: &V) -> LResult<V> {
    exact_keys(
        input,
        &["intentId", "idempotencyKey", "method", "path", "body", "bodyJson", "payloadHash", "attribution"],
        "intent",
    )?;
    let intent_id = id(input.get("intentId").unwrap_or(&V::Null), "intent ID")?;
    let idempotency_key = id(input.get("idempotencyKey").unwrap_or(&V::Null), "idempotency key")?;
    if input.get("method").and_then(V::as_str) != Some("POST")
        || input.get("path").and_then(V::as_str) != Some("/orders")
    {
        return fail("INVALID_INPUT", "Only order-placement intents belong in this ledger");
    }
    let mut body = input.get("body").cloned().unwrap_or(V::Null);
    let has_body = input.get("body").is_some();
    if input.get("bodyJson").is_some() {
        let raw = input.get("bodyJson").unwrap();
        let Some(text) = raw.as_str() else {
            return fail("INVALID_INPUT", "Invalid frozen order body");
        };
        let parsed: V = serde_json::from_str(text)
            .map_err(|_| LedgerError { code: "INVALID_INPUT", message: "Invalid frozen order body".into() })?;
        if has_body && canonical(&body)? != canonical(&parsed)? {
            return fail("INTENT_CONFLICT", "Frozen body differs from order terms");
        }
        body = parsed;
    }
    exact_keys(&body, &ORDER_FIELDS, "order terms")?;
    for value in body.as_object().unwrap().values() {
        let scalar = matches!(value, V::Null | V::String(_) | V::Bool(_))
            || value.as_f64().is_some_and(|x| x.is_finite());
        if !scalar {
            return fail("INVALID_INPUT", "Order terms must be scalar JSON values");
        }
    }
    if body.get("idempotencyKey").and_then(V::as_str) != Some(idempotency_key.as_str()) {
        return fail("INTENT_CONFLICT", "Body and intent idempotency keys differ");
    }
    let body_json = canonical(&body)?;
    let digest = payload_hash(&body)?;
    if let Some(given) = input.get("bodyJson").and_then(V::as_str) {
        if given != body_json {
            return fail("INTENT_CONFLICT", "Frozen order body must use canonical JSON");
        }
    }
    if let Some(given) = input.get("payloadHash").and_then(V::as_str) {
        if given != digest {
            return fail("INTENT_CONFLICT", "Order payload hash differs");
        }
    }
    let attribution = input.get("attribution").cloned().unwrap_or_else(|| json!({}));
    exact_keys(&attribution, &ATTRIBUTION_FIELDS, "intent attribution")?;
    for (k, v) in attribution.as_object().unwrap() {
        id(v, k)?;
    }
    Ok(json!({
        "intentId": intent_id,
        "idempotencyKey": idempotency_key,
        "method": "POST",
        "path": "/orders",
        "bodyJson": body_json,
        "payloadHash": digest,
        "attribution": attribution,
    }))
}

pub fn normalize_outcome(input: &V) -> LResult<V> {
    exact_keys(input, &OUTCOME_FIELDS, "intent outcome")?;
    let status = input.get("status").and_then(V::as_str).unwrap_or("");
    if !["uncertain", "acknowledged", "reconciled", "closed"].contains(&status) {
        return fail("INVALID_INPUT", "Invalid intent outcome status");
    }
    let mut out = Map::new();
    out.insert("status".into(), json!(status));
    let order_id = input.get("orderId").unwrap_or(&V::Null);
    if !order_id.is_null() {
        out.insert("orderId".into(), json!(id(order_id, "order ID")?));
    }
    if let Some(rc) = input.get("reasonCode") {
        let s = rc.as_str();
        let ok = s.is_some_and(|s| {
            !s.is_empty()
                && s.len() <= 80
                && s.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
                && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        });
        if !ok {
            return fail("INVALID_INPUT", "Invalid outcome reason code");
        }
        out.insert("reasonCode".into(), json!(s.unwrap()));
    }
    if let Some(ra) = input.get("retryAllowed") {
        if !ra.is_boolean() {
            return fail("INVALID_INPUT", "Invalid retry permission");
        }
    }
    if status == "uncertain" {
        let retry = input.get("retryAllowed").and_then(V::as_bool) == Some(true);
        out.insert("retryAllowed".into(), json!(retry));
        let reason = out.get("reasonCode").and_then(V::as_str).unwrap_or("");
        if semantic_uncertainty(reason) && retry {
            return fail("INVALID_INPUT", "Semantic uncertainty cannot authorize retry");
        }
        if let Some(era) = input.get("earliestRetryAt") {
            out.insert("earliestRetryAt".into(), json!(instant(era)?));
        }
    } else if input.get("retryAllowed").is_some() || input.get("earliestRetryAt").is_some() {
        return fail("INVALID_INPUT", "Only uncertain outcomes may authorize retries");
    }
    Ok(V::Object(out))
}

// --- Replay ------------------------------------------------------------------

fn empty_state() -> V {
    json!({"schemaVersion": VERSION, "sequence": 0, "lastHash": "", "scope": V::Null, "intents": {}})
}
fn immutable_intent(intent: &V) -> V {
    json!({
        "intentId": intent.get("intentId").cloned().unwrap_or(V::Null),
        "idempotencyKey": intent.get("idempotencyKey").cloned().unwrap_or(V::Null),
        "method": intent.get("method").cloned().unwrap_or(V::Null),
        "path": intent.get("path").cloned().unwrap_or(V::Null),
        "bodyJson": intent.get("bodyJson").cloned().unwrap_or(V::Null),
        "payloadHash": intent.get("payloadHash").cloned().unwrap_or(V::Null),
        "attribution": intent.get("attribution").cloned().unwrap_or(V::Null),
    })
}
fn apply_event(state: &mut V, event: &V, at: &str) -> LResult<()> {
    if !object(event) {
        return fail("JOURNAL_CORRUPT", "Invalid intent journal event");
    }
    let etype = event.get("type").and_then(V::as_str).unwrap_or("");
    if etype == "prepared" {
        exact_keys(event, &["type", "intent"], "prepared event")?;
        let intent = normalize_intent(event.get("intent").unwrap_or(&V::Null))?;
        let tournament = serde_json::from_str::<V>(intent.get("bodyJson").unwrap().as_str().unwrap())
            .ok()
            .and_then(|b| b.get("tournamentId").cloned())
            .unwrap_or(V::Null);
        if tournament != *state.get("scope").unwrap_or(&V::Null) {
            return fail("SCOPE_MISMATCH", "Prepared order belongs to another execution scope");
        }
        let intents = state.get("intents").and_then(V::as_object).unwrap();
        let iid = intent.get("intentId").and_then(V::as_str).unwrap_or("");
        if intents.contains_key(iid)
            || intents
                .values()
                .any(|x| x.get("idempotencyKey") == intent.get("idempotencyKey"))
        {
            return fail("INTENT_CONFLICT", "Duplicate intent or idempotency key in journal");
        }
        let mut stored = intent.clone();
        {
            let m = stored.as_object_mut().unwrap();
            m.insert("body".into(), tournament_body(&intent));
            m.insert("status".into(), json!("prepared"));
            m.insert("attempts".into(), json!(0));
            m.insert("preparedAt".into(), json!(at));
            m.insert("updatedAt".into(), json!(at));
            m.insert("outcome".into(), V::Null);
        }
        state
            .get_mut("intents")
            .and_then(V::as_object_mut)
            .unwrap()
            .insert(iid.to_string(), stored);
        return Ok(());
    }
    if !["dispatching", "outcome"].contains(&etype) {
        return fail("JOURNAL_CORRUPT", "Unknown intent journal event");
    }
    let intent_id = id(event.get("intentId").unwrap_or(&V::Null), "intent ID")?;
    let intents = state.get_mut("intents").and_then(V::as_object_mut).unwrap();
    let Some(intent) = intents.get_mut(&intent_id) else {
        return fail("JOURNAL_CORRUPT", "Intent event precedes preparation");
    };
    if etype == "dispatching" {
        exact_keys(event, &["type", "intentId", "attempt"], "dispatch event")?;
        let status = intent.get("status").and_then(V::as_str).unwrap_or("");
        let retry_blocked = intent.get("retryBlocked").and_then(V::as_bool) == Some(true);
        let outcome = intent.get("outcome").cloned().unwrap_or(V::Null);
        let retry_allowed = outcome.get("retryAllowed").and_then(V::as_bool) == Some(true);
        let earliest = outcome.get("earliestRetryAt").and_then(V::as_str).map(str::to_string);
        let too_early = earliest.is_some_and(|e| {
            chrono::DateTime::parse_from_rfc3339(at)
                .map(|a| a.timestamp_millis())
                .ok()
                .zip(chrono::DateTime::parse_from_rfc3339(&e).ok().map(|x| x.timestamp_millis()))
                .is_some_and(|(a, e)| a < e)
        });
        if intent.get("orderId").is_some()
            || retry_blocked
            || !["prepared", "uncertain"].contains(&status)
            || (status == "uncertain" && (!retry_allowed || too_early))
        {
            return fail("INVALID_TRANSITION", "Intent requires reconciliation before dispatch");
        }
        let expected = intent.get("attempts").and_then(V::as_i64).unwrap_or(0) + 1;
        if event.get("attempt").and_then(V::as_i64) != Some(expected) {
            return fail("JOURNAL_CORRUPT", "Invalid dispatch attempt sequence");
        }
        let m = intent.as_object_mut().unwrap();
        m.insert("status".into(), json!("dispatching"));
        m.insert("attempts".into(), json!(expected));
        m.insert("updatedAt".into(), json!(at));
        m.insert("lastDispatchAt".into(), json!(at));
        return Ok(());
    }
    exact_keys(event, &["type", "intentId", "outcome"], "outcome event")?;
    let outcome = normalize_outcome(event.get("outcome").unwrap_or(&V::Null))?;
    let status = intent.get("status").and_then(V::as_str).unwrap_or("").to_string();
    let allowed: &[&str] = match status.as_str() {
        "prepared" => &["closed"],
        "dispatching" => &["uncertain", "acknowledged", "reconciled", "closed"],
        "uncertain" => &["uncertain", "acknowledged", "reconciled", "closed"],
        "acknowledged" => &["uncertain", "reconciled", "closed"],
        "reconciled" => &["reconciled", "closed"],
        _ => &[],
    };
    let next_status = outcome.get("status").and_then(V::as_str).unwrap_or("");
    if !allowed.contains(&next_status) {
        return fail("INVALID_TRANSITION", "Invalid intent status transition");
    }
    let retry_blocked = intent.get("retryBlocked").and_then(V::as_bool) == Some(true);
    if (intent.get("orderId").is_some() || retry_blocked)
        && outcome.get("retryAllowed").and_then(V::as_bool) == Some(true)
    {
        return fail(
            "INVALID_TRANSITION",
            "Attributed or semantically uncertain orders cannot authorize another placement",
        );
    }
    if let (Some(a), Some(b)) = (
        intent.get("orderId").and_then(V::as_str),
        outcome.get("orderId").and_then(V::as_str),
    ) {
        if a != b {
            return fail("INTENT_CONFLICT", "Official order attribution changed");
        }
    }
    if ["acknowledged", "reconciled"].contains(&next_status)
        && outcome.get("orderId").is_none()
        && intent.get("orderId").is_none()
    {
        return fail("INVALID_INPUT", "Acknowledged or reconciled intent requires an order ID");
    }
    if let Some(oid) = outcome.get("orderId") {
        intent.as_object_mut().unwrap().insert("orderId".into(), oid.clone());
    }
    if next_status == "uncertain"
        && semantic_uncertainty(outcome.get("reasonCode").and_then(V::as_str).unwrap_or(""))
    {
        intent.as_object_mut().unwrap().insert("retryBlocked".into(), json!(true));
    }
    let m = intent.as_object_mut().unwrap();
    m.insert("status".into(), json!(next_status));
    m.insert("outcome".into(), outcome);
    m.insert("updatedAt".into(), json!(at));
    Ok(())
}
fn tournament_body(intent: &V) -> V {
    serde_json::from_str(intent.get("bodyJson").and_then(V::as_str).unwrap_or("{}"))
        .unwrap_or(V::Null)
}

// replayJournal(raw, {scope}).
pub fn replay_journal(raw: &str, scope: Option<&str>) -> LResult<V> {
    let mut state = empty_state();
    if let Some(s) = scope {
        id_str(s, "scope")?;
    }
    if !raw.is_empty() && !raw.ends_with('\n') {
        return fail(
            "JOURNAL_INCOMPLETE",
            "Intent journal has an incomplete crash tail; preserve it for explicit recovery",
        );
    }
    let lines: Vec<&str> = if raw.is_empty() {
        Vec::new()
    } else {
        raw.strip_suffix('\n').unwrap().split('\n').collect()
    };
    for line in lines {
        let record: V = serde_json::from_str(line)
            .map_err(|_| LedgerError { code: "JOURNAL_CORRUPT", message: "Invalid intent journal record".into() })?;
        exact_keys(
            &record,
            &["schemaVersion", "sequence", "previous", "at", "ownerId", "runId", "scope", "event", "hash"],
            "journal record",
        )?;
        let mut body = record.clone();
        let record_hash = body
            .as_object_mut()
            .unwrap()
            .shift_remove("hash")
            .and_then(|h| h.as_str().map(str::to_string))
            .unwrap_or_default();
        let seq = state.get("sequence").and_then(V::as_i64).unwrap_or(0) + 1;
        let last_hash = state.get("lastHash").and_then(V::as_str).unwrap_or("").to_string();
        if body.get("schemaVersion").and_then(V::as_i64) != Some(VERSION)
            || crate::json_text::js_int(body.get("sequence").unwrap_or(&V::Null)) != Some(seq)
            || body.get("previous").and_then(V::as_str) != Some(last_hash.as_str())
            || hash(&body)? != record_hash
        {
            return fail("JOURNAL_CORRUPT", "Intent journal sequence or hash mismatch");
        }
        instant(body.get("at").unwrap_or(&V::Null))?;
        id(body.get("ownerId").unwrap_or(&V::Null), "owner")?;
        id(body.get("runId").unwrap_or(&V::Null), "run")?;
        id(body.get("scope").unwrap_or(&V::Null), "scope")?;
        let body_scope = body.get("scope").and_then(V::as_str).unwrap_or("");
        let state_scope = state.get("scope").and_then(V::as_str);
        if (scope.is_some() && Some(body_scope) != scope) || (state_scope.is_some() && Some(body_scope) != state_scope) {
            return fail("SCOPE_MISMATCH", "Intent journal belongs to another execution scope");
        }
        state.as_object_mut().unwrap().insert("scope".into(), json!(body_scope));
        apply_event(&mut state, body.get("event").unwrap_or(&V::Null), body.get("at").and_then(V::as_str).unwrap())?;
        state.as_object_mut().unwrap().insert("sequence".into(), json!(seq));
        state.as_object_mut().unwrap().insert("lastHash".into(), json!(record_hash));
    }
    if state.get("scope").unwrap_or(&V::Null).is_null() {
        if let Some(s) = scope {
            state.as_object_mut().unwrap().insert("scope".into(), json!(s));
        }
    }
    Ok(state)
}

// --- IntentLedger --------------------------------------------------------------

pub struct IntentLedger {
    pub dir: PathBuf,
    pub file: PathBuf,
    pub projection_file: PathBuf,
    state: V,
    lease: Option<OwnerLease>,
    closed: bool,
    poisoned: bool,
    is_stopped: Box<dyn Fn() -> bool>,
    now: Box<dyn Fn() -> f64>,
    identity: (String, String, String),
}
pub struct LedgerOptions {
    pub owner_id: String,
    pub run_id: String,
    pub scope: String,
    pub is_stopped: Box<dyn Fn() -> bool>,
    pub now: Box<dyn Fn() -> f64>,
}
impl IntentLedger {
    pub fn open(dir: &Path, options: LedgerOptions) -> LResult<IntentLedger> {
        let resolved = dir.to_path_buf();
        let file = resolved.join("execution-journal.jsonl");
        let projection_file = resolved.join("execution-state.json");
        let mut lease =
            acquire_owner_lease(dir, &options.owner_id, &options.run_id, &options.scope, (options.now)())?;
        let state = match (|| -> LResult<V> {
            let raw = match fs::read_to_string(&file) {
                Ok(x) => x,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
                Err(e) => return Err(LedgerError { code: "IO", message: e.to_string() }),
            };
            let s = replay_journal(&raw, Some(&options.scope))?;
            atomic_json_ledger(&projection_file, &s)?;
            Ok(s)
        })() {
            Ok(s) => s,
            Err(e) => {
                let _ = lease.release();
                return Err(e);
            }
        };
        Ok(IntentLedger {
            dir: resolved,
            file,
            projection_file,
            state,
            lease: Some(lease),
            closed: false,
            poisoned: false,
            is_stopped: options.is_stopped,
            now: options.now,
            identity: (options.owner_id, options.run_id, options.scope),
        })
    }
    fn check(&self) -> LResult<()> {
        if self.closed {
            return fail("LEDGER_CLOSED", "Intent ledger is closed");
        }
        if self.poisoned {
            return fail("DURABILITY_FAILURE", "Intent ledger requires restart and reconciliation");
        }
        self.lease.as_ref().unwrap().assert_ownership()
    }
    fn append(&mut self, event: &V) -> LResult<()> {
        self.check()?;
        let at = now_iso((self.now)());
        let mut next = self.state.clone();
        apply_event(&mut next, event, &at)?;
        let mut body = Map::new();
        body.insert("schemaVersion".into(), json!(VERSION));
        body.insert(
            "sequence".into(),
            json!(self.state.get("sequence").and_then(V::as_i64).unwrap_or(0) + 1),
        );
        body.insert(
            "previous".into(),
            json!(self.state.get("lastHash").and_then(V::as_str).unwrap_or("")),
        );
        body.insert("at".into(), json!(at));
        body.insert("ownerId".into(), json!(self.identity.0));
        body.insert("runId".into(), json!(self.identity.1));
        body.insert("scope".into(), json!(self.identity.2));
        body.insert("event".into(), event.clone());
        let body = V::Object(body);
        let digest = hash(&body)?;
        let mut record = body.clone();
        record.as_object_mut().unwrap().insert("hash".into(), json!(digest.clone()));
        let text = format!("{}\n", canonical(&record)?);
        let write_result = (|| -> LResult<()> {
            let mut fd = fs::OpenOptions::new().append(true).create(true).open(&self.file).map_err(
                |e| LedgerError { code: "IO", message: e.to_string() },
            )?;
            fd.write_all(text.as_bytes()).and_then(|_| fd.sync_all()).map_err(|e| {
                LedgerError { code: "DURABILITY_FAILURE", message: e.to_string() }
            })?;
            drop(fd);
            sync_directory(&self.dir)?;
            Ok(())
        })();
        if let Err(e) = write_result {
            self.poisoned = true;
            return Err(e);
        }
        next.as_object_mut().unwrap().insert(
            "sequence".into(),
            json!(body.get("sequence").unwrap().clone()),
        );
        next.as_object_mut().unwrap().insert("lastHash".into(), json!(digest));
        next.as_object_mut().unwrap().insert("scope".into(), json!(self.identity.2));
        self.state = next;
        if let Err(e) = atomic_json_ledger(&self.projection_file, &self.state) {
            self.poisoned = true;
            return Err(e);
        }
        Ok(())
    }
    pub fn prepare_intent(&mut self, input: &V) -> LResult<V> {
        self.check()?;
        let intent = normalize_intent(input)?;
        let iid = intent.get("intentId").and_then(V::as_str).unwrap_or("").to_string();
        if let Some(existing) = self
            .state
            .get("intents")
            .and_then(V::as_object)
            .and_then(|m| m.get(&iid))
        {
            if canonical(&immutable_intent(existing))? != canonical(&intent)? {
                return fail("INTENT_CONFLICT", "Existing intent terms cannot change");
            }
            return Ok(existing.clone());
        }
        if self
            .state
            .get("intents")
            .and_then(V::as_object)
            .map(|m| m.values().any(|x| x.get("idempotencyKey") == intent.get("idempotencyKey")))
            .unwrap_or(false)
        {
            return fail("INTENT_CONFLICT", "Idempotency key already belongs to another intent");
        }
        self.append(&json!({"type": "prepared", "intent": intent}))?;
        Ok(self.get_intent(&iid)?.unwrap_or(V::Null))
    }
    pub fn mark_dispatching(&mut self, intent_id: &str, retry: bool) -> LResult<V> {
        self.check()?;
        if (self.is_stopped)() {
            return fail("STOPPED", "Execution STOP is active");
        }
        let key = id_str(intent_id, "intent ID")?;
        let (status, attempts) = {
            let intents = self.state.get("intents").and_then(V::as_object);
            let Some(intent) = intents.and_then(|m| m.get(&key)) else {
                return fail("UNKNOWN_INTENT", "Intent must be durably prepared before dispatch");
            };
            (
                intent.get("status").and_then(V::as_str).unwrap_or("").to_string(),
                intent.get("attempts").and_then(V::as_i64).unwrap_or(0),
            )
        };
        if (!retry && status != "prepared") || (retry && status != "uncertain") {
            return fail("INVALID_TRANSITION", "Retry must be explicit and preserve the original prepared intent");
        }
        self.append(&json!({"type": "dispatching", "intentId": key, "attempt": attempts + 1}))?;
        // STOP during the durability barrier leaves the unresolved record on
        // purpose: safer than granting dispatch permission.
        if (self.is_stopped)() {
            return fail("STOPPED", "Execution STOP became active before dispatch");
        }
        Ok(self.get_intent(&key)?.unwrap_or(V::Null))
    }
    pub fn record_outcome(&mut self, intent_id: &str, input: &V) -> LResult<V> {
        self.check()?;
        let key = id_str(intent_id, "intent ID")?;
        let intents = self.state.get("intents").and_then(V::as_object);
        if intents.and_then(|m| m.get(&key)).is_none() {
            return fail("UNKNOWN_INTENT", "Unknown prepared intent");
        }
        let outcome = normalize_outcome(input)?;
        {
            let intents = self.state.get("intents").and_then(V::as_object);
            let intent = intents.and_then(|m| m.get(&key)).unwrap();
            if intent.get("status").and_then(V::as_str) == outcome.get("status").and_then(V::as_str)
                && canonical(intent.get("outcome").unwrap_or(&V::Null))? == canonical(&outcome)?
            {
                return Ok(intent.clone());
            }
        }
        self.append(&json!({"type": "outcome", "intentId": key, "outcome": outcome}))?;
        Ok(self.get_intent(&key)?.unwrap_or(V::Null))
    }
    pub fn get_intent(&self, intent_id: &str) -> LResult<Option<V>> {
        self.check()?;
        let key = id_str(intent_id, "intent ID")?;
        Ok(self
            .state
            .get("intents")
            .and_then(V::as_object)
            .and_then(|m| m.get(&key))
            .cloned())
    }
    pub fn snapshot(&self) -> LResult<V> {
        self.check()?;
        Ok(self.state.clone())
    }
    pub fn close(&mut self) -> LResult<()> {
        if self.closed {
            return Ok(());
        }
        if let Some(mut lease) = self.lease.take() {
            lease.release()?;
        }
        self.closed = true;
        Ok(())
    }
}

// atomic_json variant used by the ledger: canonical bytes + fsync + dir fsync
// + unique tmp file (execution/ledger.mjs atomicJson).
pub use crate::atomic::atomic_json_canonical as atomic_ledger_json;
#[allow(unused_imports)]
use crate::atomic::need as _need;
