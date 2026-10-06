// live-paper/production-metadata.mjs — scoped public-contract metadata reader.
// One requested public-contract read replaces an auxiliary history/tape read.
// This reader owns no credentials, queue, timer or order permission.
use baal_execution::core::payload_hash;
use baal_service_runtime::atomic::atomic_json_canonical;
use baal_service_runtime::json_text::{js_int, js_stringify, js_truthy};
use serde_json::{json, Value as V};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

fn need(ok: bool, code: &str) -> Result<(), String> { if !ok { return Err(format!("PRODUCTION_METADATA_{}", code)); } Ok(()) }
fn date_parse(s: &str) -> Option<f64> { baal_service_runtime::contract::parse_iso_ms(s) }
fn iso_or_err(ms: f64) -> Result<String, String> {
    if !ms.is_finite() { return Err("Invalid time value".into()); }
    let s = baal_service_runtime::contract::ms_to_iso(ms);
    if s.is_empty() { Err("Invalid time value".into()) } else { Ok(s) }
}
// JS String(v) for concatenation coercions.
fn js_str(v: &V) -> String {
    match v {
        V::Null => "null".into(),
        V::Bool(b) => b.to_string(),
        V::String(s) => s.clone(),
        other => js_stringify(other),
    }
}
// /^[a-z][a-z0-9-]{1,63}$/
fn name_ok(s: &str) -> bool {
    let b = s.as_bytes();
    s.len() >= 2 && s.len() <= 64 && b[0].is_ascii_lowercase()
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

pub struct PendingRead {
    pub request: V,
    pub dir: PathBuf,
    pub market_id: V,
    pub kind: &'static str, // "nodes" | "book"
}

pub struct ProductionMetadata {
    runtime_dir: PathBuf,
    request_file: PathBuf,
    universe: V,
    dispatch: Arc<dyn Fn(&V) -> Result<V, String> + Send + Sync>,
    can_operate: Arc<dyn Fn() -> bool + Send + Sync>,
    now: Arc<dyn Fn() -> f64 + Send + Sync>,
    // JS WeakMap<fence, endpoint>; the port keys fences by numeric id, which is
    // what beforeDispatch/beforeNetwork carry in the dispatch spec.
    fences: Mutex<HashMap<usize, String>>,
}

/// openProductionMetadata({runtimeDir, universe, dispatch, canOperate=()=>true, now=Date.now})
pub fn open_production_metadata(
    runtime_dir: &Path,
    universe: &V,
    dispatch: Arc<dyn Fn(&V) -> Result<V, String> + Send + Sync>,
    can_operate: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
    now: Option<Arc<dyn Fn() -> f64 + Send + Sync>>,
) -> ProductionMetadata {
    ProductionMetadata {
        runtime_dir: runtime_dir.to_path_buf(),
        request_file: runtime_dir.join("production-metadata-request.json"),
        universe: universe.clone(),
        dispatch,
        can_operate: can_operate.unwrap_or_else(|| Arc::new(|| true)),
        now: now.unwrap_or_else(|| Arc::new(|| {
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as f64).unwrap_or(0.0)
        })),
        fences: Mutex::new(HashMap::new()),
    }
}

impl ProductionMetadata {
    /// authorizes(spec) — GET + no body + the same registered fence on both
    /// beforeDispatch and beforeNetwork, bound to the spec endpoint.
    pub fn authorizes(&self, spec: &V) -> bool {
        (self.can_operate)()
            && spec["method"].as_str() == Some("GET")
            && spec.get("body").is_none()
            && spec.get("bodyJson").is_none()
            && spec["beforeDispatch"] == spec["beforeNetwork"]
            && js_int(&spec["beforeNetwork"]).is_some_and(|f| {
                self.fences.lock().unwrap().get(&(f as usize)) == spec["endpoint"].as_str().map(String::from).as_ref()
            })
    }
    /// pending() — next missing nodes/book file for the scoped request.
    pub fn pending(&self) -> Result<Option<PendingRead>, String> {
        if !self.request_file.exists() {
            return Ok(None);
        }
        let request: V = serde_json::from_str(&fs::read_to_string(&self.request_file).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        need(js_int(&request["schemaVersion"]) == Some(1)
            && name_ok(&js_str(&request["name"])) // .test() coerces its argument
            && request["marketIds"].as_array().is_some_and(|m| !m.is_empty() && m.len() <= 3
                && m.iter().map(js_stringify).collect::<HashSet<_>>().len() == m.len()
                && m.iter().all(|id| self.universe["exchanges"].as_array()
                    .is_some_and(|xs| xs.iter().any(|x| x["marketId"] == *id))))
            && (request.get("includeBook").is_none() || request["includeBook"].is_boolean()),
            "REQUEST")?;
        // path.join requires a real string for a coerced non-string name.
        if !request["name"].is_string() {
            return Err("The \"paths[2]\" argument must be of type string".into());
        }
        let dir = self.runtime_dir.join("production-metadata").join(request["name"].as_str().unwrap());
        for market_id in request["marketIds"].as_array().cloned().unwrap_or_default() {
            let market_id = &market_id;
            if !dir.join(format!("{}.json", js_str(market_id))).exists() {
                return Ok(Some(PendingRead { request, dir, market_id: market_id.clone(), kind: "nodes" }));
            }
            if js_truthy(request.get("includeBook"))
                && !dir.join(format!("{}.book.json", js_str(market_id))).exists()
            {
                return Ok(Some(PendingRead { request, dir, market_id: market_id.clone(), kind: "book" }));
            }
        }
        Ok(None)
    }
    fn fence(&self, id: usize, endpoint: &str) -> Result<(), String> {
        need((self.can_operate)() && self.fences.lock().unwrap().get(&id).map(String::as_str) == Some(endpoint), "STOPPED")
    }
    /// consumeOne() — one serialized scoped read; synchronous dispatch port.
    pub fn consume_one(&self) -> Result<bool, String> {
        let Some(p) = self.pending()? else { return Ok(false); };
        if !(self.can_operate)() {
            return Ok(false);
        }
        let endpoint = format!("/markets/{}{}?tournamentId={}{}",
            js_str(&p.market_id),
            if p.kind == "nodes" { "/nodes" } else { "/orderbook" },
            js_str(&self.universe["tournamentId"]),
            if p.kind == "book" { "&depth=20" } else { "" });
        let fence_id = self.fences.lock().unwrap().len() + 1;
        self.fences.lock().unwrap().insert(fence_id, endpoint.clone());
        let outcome = (|| -> Result<bool, String> {
            let spec = json!({"method": "GET", "endpoint": endpoint.clone(),
                "beforeDispatch": fence_id, "beforeNetwork": fence_id});
            let r = (self.dispatch)(&spec)?;
            self.fence(fence_id, &endpoint)?;
            let started = date_parse(r["requestStartedAt"].as_str().unwrap_or(""));
            let received = date_parse(r["receivedAt"].as_str().unwrap_or(""));
            let now = (self.now)();
            need(js_int(&r["status"]) == Some(200)
                && r["endpoint"].as_str() == Some(endpoint.as_str())
                && started.is_some() && received.is_some()
                && started.unwrap() <= received.unwrap()
                && received.unwrap() <= now
                && now - received.unwrap() <= 20000.0, "RECEIPT")?;
            let d = &r["data"];
            need(d["contexts"].as_array().is_some_and(|c| c.len() == 1
                && c[0]["type"].as_str() == Some("tournament")
                && c[0]["tournament"]["id"] == self.universe["tournamentId"]), "SCOPE")?;
            let record = if p.kind == "nodes" {
                need(d["market_id"] == p.market_id
                    && d["root"]["node_type"].as_str() == Some("contract")
                    && d["root"]["contract_type"].as_str() == Some("Election Outcome"), "SCOPE")?;
                json!({"schemaVersion": 1, "marketId": p.market_id.clone(),
                    "requestStartedAt": r["requestStartedAt"].clone(), "receivedAt": r["receivedAt"].clone(),
                    "root": d["root"].clone(), "rootHash": payload_hash(&d["root"])?,
                    "responseHash": payload_hash(d)?, "competitionOrders": 0})
            } else {
                let book = crate::model::normalize_book(d, &js_str(&p.market_id),
                    &js_str(&self.universe["tournamentId"]),
                    r["receivedAt"].as_str().unwrap_or(""), r["requestStartedAt"].as_str().unwrap_or(""),
                    20000, &BTreeSet::new(), "authoritative_receipt_v1")?;
                need(self.universe["exchanges"].as_array().is_some_and(|xs| xs.iter()
                    .any(|x| x["marketId"] == p.market_id && x["exchangeId"] == book["exchangeId"])), "BOOK_EXCHANGE")?;
                json!({"schemaVersion": 1, "marketId": p.market_id.clone(), "book": book,
                    "responseHash": payload_hash(d)?, "executionAuthority": false, "competitionOrders": 0})
            };
            // mkdirSync(dir,{recursive:true,mode:0o700}) applies the mode to
            // each directory level it creates.
            #[cfg(unix)]
            for d in [p.dir.parent().unwrap_or(&p.dir), &p.dir] {
                if !d.exists() {
                    fs::create_dir_all(d).map_err(|e| e.to_string())?;
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(d, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
                }
            }
            #[cfg(not(unix))]
            fs::create_dir_all(&p.dir).map_err(|e| e.to_string())?;
            let f = p.dir.join(format!("{}{}.json", js_str(&p.market_id), if p.kind == "book" { ".book" } else { "" }));
            {
                let mut o = fs::OpenOptions::new();
                o.write(true).create_new(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    o.mode(0o600);
                }
                let mut file = o.open(&f).map_err(|e| e.to_string())?;
                file.write_all(format!("{}\n", js_stringify(&record)).as_bytes()).map_err(|e| e.to_string())?;
            }
            let fd = fs::OpenOptions::new().read(true).write(true).open(&f).map_err(|e| e.to_string())?;
            fd.sync_all().map_err(|e| e.to_string())?;
            drop(fd);
            let next = self.pending()?.map(|n| json!({"marketId": n.market_id.clone(), "kind": n.kind}));
            atomic_json_canonical(&self.runtime_dir.join("production-metadata-status.json"), &json!({
                "at": iso_or_err((self.now)())?,
                "name": p.request["name"].clone(),
                "completedMarketId": p.market_id.clone(),
                "completedKind": p.kind,
                "next": next.unwrap_or(V::Null),
                "executionAuthority": false}))?;
            Ok(true)
        })();
        self.fences.lock().unwrap().remove(&fence_id);
        outcome
    }
}
