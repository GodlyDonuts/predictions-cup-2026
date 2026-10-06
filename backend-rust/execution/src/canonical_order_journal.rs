// execution/canonical-order-journal.mjs — owns one accounting journal only.
// The account owner lease and network permit remain the caller's
// responsibility; this class has no network capability.
//
// `source_dir` replaces import.meta.url: it must point at the execution/
// directory containing the ported .mjs sources so codeIdentity is the same
// sha256-of-source bytes the JS implementation writes.
use crate::canonical_fifo::project_canonical_fifo;
use crate::core::{canonical_payload, decimal_micros, payload_hash};
use baal_service_runtime::json_text::{js_int, js_truthy, sha256};
use serde_json::{json, Map, Value as V};
use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

fn need(x: bool, c: &str) -> Result<(), String> {
    if x { Ok(()) } else { Err(c.into()) }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn id(x: &V) -> bool {
    x.as_str().is_some_and(|s| {
        !s.is_empty() && !s.starts_with('0') && s.bytes().all(|b| b.is_ascii_digit())
            && s.parse::<i64>().is_ok_and(|n| n > 0 && n <= 9_007_199_254_740_991)
    })
}
fn token(x: &V) -> bool {
    x.as_str().is_some_and(|s| {
        !s.is_empty() && s.len() <= 160
            && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b':' | b'-'))
            && !["__proto__", "constructor", "prototype"].contains(&s)
    })
}
fn stamp(x: &V) -> bool {
    x.as_str().is_some_and(|s| {
        chrono::DateTime::parse_from_rfc3339(s).ok().is_some_and(|d| {
            d.to_rfc3339_opts(chrono::SecondsFormat::Millis, true) == s
        })
    })
}
fn parse_ms(s: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(s).unwrap().timestamp_millis()
}
fn keys(x: &V, allowed: &[&str]) -> bool {
    x.is_object() && x.as_object().unwrap().keys().all(|k| allowed.contains(&k.as_str()))
}
fn tick(x: &V) -> Result<i128, String> {
    need(x.is_string(), "CANONICAL_EXACT_LIMIT")?;
    let n = decimal_micros(x, "exact")?;
    need(n >= 5_000 && n <= 995_000 && n % 5_000 == 0, "CANONICAL_EXACT_LIMIT")?;
    Ok(n)
}
fn reason_code(x: &V) -> bool {
    x.as_str().is_some_and(|s| {
        !s.is_empty() && s.len() <= 81
            && s.as_bytes()[0].is_ascii_uppercase()
            && s.bytes().skip(1).all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
    })
}

// sources() — sha256 per source file, keyed by file name.
fn sources(source_dir: &Path) -> Result<V, String> {
    let mut out = Map::new();
    for name in [
        "canonical-order-journal.mjs",
        "canonical-fifo.mjs",
        "side-accounting.mjs",
        "api-tick-price.mjs",
        "core.mjs",
    ] {
        let bytes = fs::read(source_dir.join(name)).map_err(|e| e.to_string())?;
        out.insert(name.to_string(), json!(sha256(&bytes)));
    }
    Ok(V::Object(out))
}

fn prepare(input: &V) -> Result<V, String> {
    need(
        keys(input, &["intentId", "idempotencyKey", "binding", "wallet"])
            && token(get(input, "intentId"))
            && token(get(input, "idempotencyKey")),
        "CANONICAL_PREPARATION",
    )?;
    let b = get(input, "binding");
    let w = get(input, "wallet");
    need(
        keys(b, &["ownerId", "exchangeId", "marketId", "tournamentId", "side", "action", "quantity", "limit", "createdAt", "expirationDate"])
            && token(get(b, "ownerId"))
            && id(get(b, "exchangeId"))
            && id(get(b, "marketId"))
            && get(b, "tournamentId").as_str().is_some_and(|s| !s.is_empty())
            && matches!(get(b, "side").as_str(), Some("yes" | "no"))
            && matches!(get(b, "action").as_str(), Some("buy" | "sell"))
            && js_int(get(b, "quantity")).is_some_and(|q| q > 0 && q <= 100)
            && stamp(get(b, "createdAt"))
            && stamp(get(b, "expirationDate"))
            && parse_ms(get(b, "expirationDate").as_str().unwrap()) > parse_ms(get(b, "createdAt").as_str().unwrap())
            && parse_ms(get(b, "expirationDate").as_str().unwrap()) - parse_ms(get(b, "createdAt").as_str().unwrap()) <= 120_000,
        "CANONICAL_BINDING",
    )?;
    tick(get(b, "limit"))?;
    need(
        keys(w, &["ownerId", "exchangeId", "marketId", "tournamentId", "cash", "realizedPnl", "accountSignedQuantity", "lots", "appliedOrderIds"])
            && get(w, "ownerId") == get(b, "ownerId")
            && get(w, "exchangeId") == get(b, "exchangeId")
            && get(w, "marketId") == get(b, "marketId")
            && get(w, "tournamentId") == get(b, "tournamentId")
            && get(w, "cash").is_string()
            && get(w, "realizedPnl").is_string()
            && js_int(get(w, "accountSignedQuantity")).is_some()
            && get(w, "lots").as_array().is_some_and(|l| l.len() <= 10_000),
        "CANONICAL_BASELINE",
    )?;
    decimal_micros(get(w, "cash"), "exact")?;
    decimal_micros(&json!(get(w, "realizedPnl").as_str().unwrap_or("").strip_prefix('-').unwrap_or(get(w, "realizedPnl").as_str().unwrap_or("")).to_string()), "exact")?;
    let mut qty: i64 = 0;
    let mut old: Option<String> = None;
    let mut clock = i64::MIN;
    let mut lot_ids: BTreeSet<String> = BTreeSet::new();
    for l in get(w, "lots").as_array().unwrap() {
        let lot_id = get(l, "lotId").as_str().unwrap_or("").to_string();
        need(
            keys(l, &["lotId", "side", "quantity", "entryPrice", "openedAt"])
                && token(get(l, "lotId"))
                && !lot_ids.contains(&lot_id)
                && matches!(get(l, "side").as_str(), Some("yes" | "no"))
                && js_int(get(l, "quantity")).is_some_and(|q| q > 0)
                && stamp(get(l, "openedAt"))
                && parse_ms(get(l, "openedAt").as_str().unwrap()) >= clock
                && parse_ms(get(l, "openedAt").as_str().unwrap()) <= parse_ms(get(b, "createdAt").as_str().unwrap()),
            "CANONICAL_BASELINE_LOT",
        )?;
        let lside = get(l, "side").as_str().unwrap().to_string();
        if old.is_none() {
            old = Some(lside.clone());
        }
        need(old.as_deref() == Some(lside.as_str()), "CANONICAL_MIXED_LOTS")?;
        lot_ids.insert(lot_id);
        tick(get(l, "entryPrice"))?;
        clock = parse_ms(get(l, "openedAt").as_str().unwrap());
        qty += (if lside == "yes" { 1 } else { -1 }) * js_int(get(l, "quantity")).unwrap();
    }
    need(
        qty == js_int(get(w, "accountSignedQuantity")).unwrap_or(i64::MIN),
        "CANONICAL_UNATTRIBUTED_BASELINE",
    )?;
    let applied: Vec<V> = if get(w, "appliedOrderIds").is_null() {
        Vec::new()
    } else {
        get(w, "appliedOrderIds").as_array().cloned().unwrap_or_default()
    };
    need(
        (get(w, "appliedOrderIds").is_null() || get(w, "appliedOrderIds").is_array())
            && applied.len() <= 10_000
            && applied.iter().all(id)
            && applied.iter().map(|a| a.as_str().unwrap_or("")).collect::<BTreeSet<_>>().len() == applied.len(),
        "CANONICAL_BASELINE_HISTORY",
    )?;
    let limit_number: f64 = get(b, "limit").as_str().unwrap_or("").parse().unwrap_or(f64::NAN);
    let body = json!({
        "idempotencyKey": get(input, "idempotencyKey"),
        "exchangeId": get(b, "exchangeId"),
        "side": get(b, "side"),
        "action": get(b, "action"),
        "quantity": get(b, "quantity"),
        "price": limit_number,
        "expirationDate": get(b, "expirationDate"),
        "tournamentId": get(b, "tournamentId"),
    });
    let mut state = input.clone();
    let so = state.as_object_mut().unwrap();
    so.insert("requestBody".into(), body.clone());
    so.insert("requestHash".into(), json!(payload_hash(&body)?));
    so.insert("baselineHash".into(), json!(payload_hash(w)?));
    so.insert("orderId".into(), V::Null);
    so.insert("phase".into(), json!("prepared"));
    so.insert("attempts".into(), json!(0));
    so.insert("evidence".into(), V::Null);
    so.insert("projection".into(), V::Null);
    so.insert("executionAuthority".into(), json!(false));
    so.insert("authoritativeReconciliation".into(), json!(false));
    Ok(state)
}

fn advance(state: &V, kind: &str, payload: &V) -> Result<V, String> {
    if kind == "prepare" {
        need(state.is_null(), "CANONICAL_ALREADY_PREPARED")?;
        return prepare(payload);
    }
    need(!state.is_null(), "CANONICAL_NOT_PREPARED")?;
    let mut next = state.clone();
    match kind {
        "dispatching" => {
            need(
                get(state, "phase").as_str() == Some("prepared") && payload.is_null(),
                "CANONICAL_DISPATCH_PHASE",
            )?;
            let o = next.as_object_mut().unwrap();
            o.insert("phase".into(), json!("dispatching"));
            let attempts = js_int(get(state, "attempts")).unwrap_or(0) + 1;
            o.insert("attempts".into(), json!(attempts));
            Ok(next)
        }
        "retry" => {
            need(
                get(state, "phase").as_str() == Some("uncertain")
                    && keys(payload, &["requestHash", "idempotencyKey"])
                    && get(payload, "requestHash") == get(state, "requestHash")
                    && get(payload, "idempotencyKey") == get(state, "idempotencyKey"),
                "CANONICAL_RETRY_CHANGED_OR_PHASE",
            )?;
            let o = next.as_object_mut().unwrap();
            o.insert("phase".into(), json!("dispatching"));
            let attempts = js_int(get(state, "attempts")).unwrap_or(0) + 1;
            o.insert("attempts".into(), json!(attempts));
            Ok(next)
        }
        "uncertain" => {
            need(
                matches!(get(state, "phase").as_str(), Some("dispatching" | "uncertain"))
                    && keys(payload, &["reasonCode"])
                    && reason_code(get(payload, "reasonCode")),
                "CANONICAL_UNCERTAIN_PHASE",
            )?;
            let o = next.as_object_mut().unwrap();
            o.insert("phase".into(), json!("uncertain"));
            o.insert("reasonCode".into(), get(payload, "reasonCode").clone());
            Ok(next)
        }
        "bind" => {
            let applied: Vec<V> = if get(get(state, "wallet"), "appliedOrderIds").is_null() {
                Vec::new()
            } else {
                get(get(state, "wallet"), "appliedOrderIds").as_array().cloned().unwrap_or_default()
            };
            need(
                matches!(get(state, "phase").as_str(), Some("dispatching" | "uncertain"))
                    && keys(payload, &["orderId"])
                    && id(get(payload, "orderId"))
                    && !applied.iter().any(|a| a == get(payload, "orderId")),
                "CANONICAL_ORDER_BINDING",
            )?;
            let o = next.as_object_mut().unwrap();
            o.insert("orderId".into(), get(payload, "orderId").clone());
            o.insert("phase".into(), json!("bound"));
            Ok(next)
        }
        _ => {
            need(
                kind == "observe"
                    && matches!(get(state, "phase").as_str(), Some("bound" | "observed" | "terminal")),
                "CANONICAL_OBSERVATION_PHASE",
            )?;
            need(
                keys(payload, &["ack", "rows", "source", "readAt"])
                    && stamp(get(payload, "readAt"))
                    && keys(get(payload, "ack"), &["orderId", "exchangeId", "side", "action", "quantity", "price", "all", "quantityTraded", "remainingQuantity", "open", "totalCost", "terminalReasonCode"])
                    && get(payload, "rows").as_array().is_some_and(|rows| {
                        rows.iter().all(|f| {
                            keys(f, &["id", "orderId", "exchangeId", "marketId", "tournamentId", "side", "quantity", "price", "filledAt"])
                        })
                    }),
                "CANONICAL_OBSERVATION_SHAPE",
            )?;
            let prior = state.get("evidence").cloned().unwrap_or(V::Null);
            if !prior.is_null() {
                need(
                    get(payload, "source") == get(&prior, "source")
                        && parse_ms(get(payload, "readAt").as_str().unwrap())
                            >= parse_ms(get(&prior, "readAt").as_str().unwrap()),
                    "CANONICAL_OBSERVATION_CLOCK_OR_SOURCE",
                )?;
                for k in ["orderId", "exchangeId", "side", "action", "quantity", "price"] {
                    need(
                        canonical_payload(get(get(payload, "ack"), k))?
                            == canonical_payload(get(get(&prior, "ack"), k))?,
                        "CANONICAL_ACK_CHANGED",
                    )?;
                }
                need(
                    js_int(get(get(payload, "ack"), "quantityTraded")).unwrap_or(i64::MIN)
                        >= js_int(get(get(&prior, "ack"), "quantityTraded")).unwrap_or(i64::MAX)
                        && js_int(get(get(payload, "ack"), "remainingQuantity")).unwrap_or(i64::MAX)
                            <= js_int(get(get(&prior, "ack"), "remainingQuantity")).unwrap_or(i64::MIN),
                    "CANONICAL_FILL_REGRESSION",
                )?;
                for old in get(&prior, "rows").as_array().cloned().unwrap_or_default() {
                    need(
                        get(payload, "rows").as_array().cloned().unwrap_or_default().iter()
                            .any(|f| canonical_payload(f).ok() == canonical_payload(&old).ok()),
                        "CANONICAL_FILL_HISTORY_CHANGED",
                    )?;
                }
                if get(state, "phase").as_str() == Some("terminal") {
                    let mut mutated = payload.clone();
                    mutated.as_object_mut().unwrap().insert("readAt".into(), get(&prior, "readAt").clone());
                    need(
                        payload_hash(&mutated)? == payload_hash(&prior)?,
                        "CANONICAL_TERMINAL_CHANGED",
                    )?;
                }
            }
            let mut fifo_input = Map::new();
            fifo_input.insert("wallet".into(), get(state, "wallet").clone());
            let mut binding = get(state, "binding").as_object().cloned().unwrap_or_default();
            binding.insert("orderId".into(), get(state, "orderId").clone());
            fifo_input.insert("binding".into(), V::Object(binding));
            if let Some(po) = payload.as_object() {
                for (k, v) in po {
                    fifo_input.insert(k.clone(), v.clone());
                }
            }
            let projected = project_canonical_fifo(&V::Object(fifo_input))?;
            let o = next.as_object_mut().unwrap();
            o.insert("evidence".into(), payload.clone());
            o.insert("projection".into(), projected);
            let phase = if js_truthy(payload.get("ack").and_then(|a| a.get("open"))) { "observed" } else { "terminal" };
            o.insert("phase".into(), json!(phase));
            Ok(next)
        }
    }
}

fn birthtime_ms(meta: &fs::Metadata) -> (i64, i64) {
    // statx btime on Linux (what Node's statSync birthtimeMs reads); falls back
    // to the filesystem creation time where available.
    let t = meta
        .created()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .unwrap_or_default();
    (t.as_secs() as i64, t.subsec_nanos() as i64)
}

// openCanonicalOrderJournal({dir,readOnly,canCommit})
pub struct CanonicalOrderJournal {
    file: PathBuf,
    lock: PathBuf,
    source_dir: PathBuf,
    identity: V,
    read_only: bool,
    can_commit: Box<dyn Fn() -> Result<bool, String>>,
    lock_open: bool,
    lock_ino: u64,
    lock_birth: (i64, i64),
    lock_bytes: Vec<u8>,
    state: V,
    sequence: u64,
    previous: V,
    size: u64,
    bytes_hash: String,
    file_ino: u64,
    file_birth: (i64, i64),
    failed: bool,
    closed: bool,
}

impl CanonicalOrderJournal {
    pub fn open(
        dir: &Path,
        source_dir: &Path,
        read_only: bool,
        can_commit: Box<dyn Fn() -> Result<bool, String>>,
    ) -> Result<CanonicalOrderJournal, String> {
        need(dir.is_absolute(), "CANONICAL_JOURNAL_DIRECTORY")?;
        let identity = sources(source_dir)?;
        let file = dir.join("canonical-order.jsonl");
        let lock = dir.join("canonical-order.lock");
        let mut lock_open = false;
        let mut lock_ino = 0u64;
        let mut lock_birth = (0i64, 0i64);
        let mut lock_bytes = Vec::new();
        if !read_only {
            need(can_commit()?, "CANONICAL_JOURNAL_STOP")?;
            if !dir.exists() {
                fs::create_dir_all(dir).map_err(|e| e.to_string())?;
                fs::set_permissions(dir, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
            } else {
                fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            let mut lock_fd = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&lock)
                .map_err(|e| e.to_string())?;
            lock_bytes = serde_json::to_vec(&json!({
                "pid": std::process::id(),
                "token": uuid::Uuid::new_v4().to_string(),
            }))
            .map_err(|e| e.to_string())?;
            match (|| -> Result<(), String> {
                lock_fd.write_all(&lock_bytes).map_err(|e| e.to_string())?;
                lock_fd.sync_all().map_err(|e| e.to_string())?;
                let meta = lock_fd.metadata().map_err(|e| e.to_string())?;
                lock_ino = meta.ino();
                lock_birth = birthtime_ms(&meta);
                Ok(())
            })() {
                Ok(()) => lock_open = true,
                Err(e) => {
                    drop(lock_fd);
                    return Err(e);
                }
            }
        }
        let result = (|| {
            if !file.exists() {
                need(!read_only, "CANONICAL_JOURNAL_MISSING")?;
                fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(&file)
                    .map_err(|e| e.to_string())?;
            }
            let bytes = fs::read(&file).map_err(|e| e.to_string())?;
            need(bytes.is_empty() || bytes.last() == Some(&10), "CANONICAL_JOURNAL_PARTIAL")?;
            let mut state = V::Null;
            let mut sequence = 0u64;
            let mut previous = V::Null;
            for line in String::from_utf8_lossy(&bytes).split('\n').filter(|s| !s.is_empty()) {
                let row: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
                let mut body = row.as_object().cloned().unwrap_or_default();
                let hash = body.shift_remove("hash").unwrap_or(V::Null);
                sequence += 1;
                need(
                    keys(&V::Object(body.clone()), &["schemaVersion", "sequence", "previous", "codeIdentity", "kind", "payload", "stateHash"])
                        && body.get("schemaVersion") == Some(&json!(1))
                        && js_int(body.get("sequence").unwrap_or(&V::Null)) == Some(sequence as i64)
                        && body.get("previous") == Some(&previous)
                        && payload_hash(&V::Object(body.clone()))? == hash.as_str().unwrap_or("")
                        && payload_hash(body.get("codeIdentity").unwrap_or(&V::Null))? == payload_hash(&identity)?,
                    "CANONICAL_JOURNAL_CHAIN_OR_CODE",
                )?;
                state = advance(&state, body.get("kind").and_then(V::as_str).unwrap_or(""), body.get("payload").unwrap_or(&V::Null))?;
                need(payload_hash(&state)? == body.get("stateHash").and_then(V::as_str).unwrap_or(""), "CANONICAL_JOURNAL_REPLAY")?;
                previous = hash;
            }
            let size = bytes.len() as u64;
            let bytes_hash = sha256(&bytes);
            let stat = fs::metadata(&file).map_err(|e| e.to_string())?;
            Ok(CanonicalOrderJournal {
                file: file.clone(),
                lock: lock.clone(),
                source_dir: source_dir.to_path_buf(),
                identity,
                read_only,
                can_commit,
                lock_open,
                lock_ino,
                lock_birth,
                lock_bytes: lock_bytes.clone(),
                state,
                sequence,
                previous,
                size,
                bytes_hash,
                file_ino: stat.ino(),
                file_birth: birthtime_ms(&stat),
                failed: false,
                closed: false,
            })
        })();
        if let Err(e) = result {
            let mut j = CanonicalOrderJournal {
                file: file.clone(), lock: lock.clone(), source_dir: source_dir.to_path_buf(),
                identity: V::Null, read_only, can_commit: Box::new(|| Ok(true)),
                lock_open, lock_ino, lock_birth, lock_bytes,
                state: V::Null, sequence: 0, previous: V::Null, size: 0,
                bytes_hash: String::new(), file_ino: 0, file_birth: (0, 0),
                failed: false, closed: false,
            };
            j.release()?;
            return Err(e);
        }
        result
    }

    fn live(&self) -> Result<Vec<u8>, String> {
        need(!self.read_only && !self.closed && !self.failed, "CANONICAL_JOURNAL_UNWRITABLE")?;
        need((self.can_commit)()?, "CANONICAL_JOURNAL_STOP")?;
        let current = fs::metadata(&self.file).map_err(|e| e.to_string())?;
        let prior_bytes = fs::read(&self.file).map_err(|e| e.to_string())?;
        let ls = fs::metadata(&self.lock).map_err(|e| e.to_string())?;
        need(
            ls.ino() == self.lock_ino
                && birthtime_ms(&ls) == self.lock_birth
                && fs::read(&self.lock).map_err(|e| e.to_string())? == self.lock_bytes,
            "CANONICAL_LOCK_CHANGED",
        )?;
        need(
            current.ino() == self.file_ino
                && birthtime_ms(&current) == self.file_birth
                && current.size() == self.size
                && sha256(&prior_bytes) == self.bytes_hash
                && payload_hash(&sources(&self.source_dir)?)? == payload_hash(&self.identity)?,
            "CANONICAL_JOURNAL_CHANGED",
        )?;
        Ok(prior_bytes)
    }

    fn commit(&mut self, kind: &str, payload: &V) -> Result<V, String> {
        let prior_bytes = self.live()?;
        let next = advance(&self.state, kind, payload)?;
        let body = json!({
            "schemaVersion": 1,
            "sequence": self.sequence + 1,
            "previous": self.previous,
            "codeIdentity": self.identity,
            "kind": kind,
            "payload": payload,
            "stateHash": payload_hash(&next)?,
        });
        let hash = payload_hash(&body)?;
        let mut body_obj = body.as_object().cloned().unwrap();
        body_obj.insert("hash".into(), json!(hash));
        let data = format!("{}\n", canonical_payload(&V::Object(body_obj))?).into_bytes();
        need(self.size + data.len() as u64 <= 64 * 1024 * 1024, "CANONICAL_JOURNAL_TOO_LARGE")?;
        need((self.can_commit)()?, "CANONICAL_JOURNAL_STOP")?;
        let mut combined = prior_bytes.clone();
        combined.extend_from_slice(&data);
        let expected_hash = sha256(&combined);
        let commit_result = (|journal: &mut Self| -> Result<(), String> {
            let mut fd = fs::OpenOptions::new().append(true).open(&journal.file).map_err(|e| e.to_string())?;
            let write_result = (|| -> Result<(), String> {
                fd.write_all(&data).map_err(|e| {
                    if e.kind() == std::io::ErrorKind::WriteZero { "CANONICAL_JOURNAL_WRITE".to_string() } else { e.to_string() }
                })?;
                fd.sync_all().map_err(|e| e.to_string())?;
                let after = fs::metadata(&journal.file).map_err(|e| e.to_string())?;
                need(
                    after.ino() == journal.file_ino
                        && birthtime_ms(&after) == journal.file_birth
                        && after.size() == journal.size + data.len() as u64
                        && sha256(&fs::read(&journal.file).map_err(|e| e.to_string())?) == expected_hash,
                    "CANONICAL_JOURNAL_CHANGED",
                )?;
                need((journal.can_commit)()?, "CANONICAL_JOURNAL_STOP")?;
                Ok(())
            })();
            drop(fd);
            write_result
        })(self);
        if let Err(e) = commit_result {
            self.failed = true;
            return Err(e);
        }
        self.size += data.len() as u64;
        self.bytes_hash = expected_hash;
        self.state = next;
        self.sequence += 1;
        self.previous = json!(hash);
        Ok(self.state.clone())
    }

    pub fn prepare(&mut self, input: &V) -> Result<V, String> {
        self.live()?;
        if !self.state.is_null() {
            // JS evaluates prepare(input) first: its error propagates before the
            // re-prepare of the stored preparation is computed.
            need(
                payload_hash(&prepare(input)?)?
                    == payload_hash(&prepare(&json!({
                        "intentId": get(&self.state, "intentId"),
                        "idempotencyKey": get(&self.state, "idempotencyKey"),
                        "binding": get(&self.state, "binding"),
                        "wallet": get(&self.state, "wallet"),
                    }))?)?,
                "CANONICAL_PREPARATION_CHANGED",
            )?;
            return Ok(self.state.clone());
        }
        self.commit("prepare", input)
    }

    pub fn mark_dispatch(&mut self) -> Result<V, String> {
        self.commit("dispatching", &V::Null)
    }

    pub fn uncertain(&mut self, reason_code: &V) -> Result<V, String> {
        self.commit("uncertain", &json!({"reasonCode": reason_code}))
    }

    pub fn retry(&mut self, proof: &V) -> Result<V, String> {
        self.commit("retry", proof)
    }

    pub fn bind(&mut self, order_id: &V) -> Result<V, String> {
        self.live()?;
        if js_truthy(self.state.get("orderId")) {
            need(get(&self.state, "orderId") == order_id, "CANONICAL_ORDER_ID_CHANGED")?;
            return Ok(self.state.clone());
        }
        self.commit("bind", &json!({"orderId": order_id}))
    }

    pub fn observe(&mut self, input: &V) -> Result<V, String> {
        self.live()?;
        if js_truthy(self.state.get("evidence"))
            && payload_hash(get(&self.state, "evidence"))? == payload_hash(input)?
        {
            return Ok(self.state.clone());
        }
        self.commit("observe", input)
    }

    pub fn status(&self) -> V {
        json!({
            "sequence": self.sequence,
            "hash": self.previous,
            "state": self.state,
            "readOnly": self.read_only,
            "closed": self.closed,
            "failed": self.failed,
            "unconfirmedPersistence": self.failed,
            "executionAuthority": false,
        })
    }

    fn release(&mut self) -> Result<(), String> {
        if self.lock_open {
            self.lock_open = false;
            let s = fs::metadata(&self.lock).map_err(|e| e.to_string())?;
            need(
                s.ino() == self.lock_ino
                    && birthtime_ms(&s) == self.lock_birth
                    && fs::read(&self.lock).map_err(|e| e.to_string())? == self.lock_bytes,
                "CANONICAL_LOCK_CHANGED",
            )?;
            fs::remove_file(&self.lock).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub fn close(&mut self) -> Result<(), String> {
        if !self.closed {
            self.closed = true;
            self.release()?;
        }
        Ok(())
    }
}
