// execution/baal-owned-hedge-shared-budget-v2.mjs — source-verified budget for
// the separately migrated owned-counterpart writer. No network client, trading
// window or automatic legacy-writer handback.
use baal_execution::core::payload_hash;
use baal_service_runtime::fixed::{amount, instant, money};
use baal_service_runtime::journal::Journal;
use baal_service_runtime::json_text::{js_int, js_stringify, sha256};
use baal_service_runtime::ledger::{acquire_owner_lease, OwnerLease};
use serde_json::{json, Map, Value as V};
use std::collections::HashSet;
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use indexmap::IndexMap;

fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(format!("OWNED_HEDGE_BUDGET_{code}"))
    }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.as_object().and_then(|m| m.get(k)).unwrap_or(&V::Null)
}
fn sget<'a>(v: &'a V, k: &str) -> Option<&'a str> {
    get(v, k).as_str()
}
fn copy(v: &V) -> V {
    v.clone()
}
fn sha(b: &[u8]) -> String {
    sha256(b)
}
fn sha_s(s: &str) -> String {
    sha256(s.as_bytes())
}
fn hex(v: &V) -> bool {
    v.as_str().is_some_and(|s| {
        s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}
fn same(a: &V, b: &V) -> Result<bool, String> {
    Ok(payload_hash(a)? == payload_hash(b)?)
}
fn stat_identity(st: &std::fs::Metadata) -> String {
    let ms = st
        .created()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(0.0);
    format!("{}:{}", st.ino(), js_stringify(&json!(ms)))
}
fn js_num_string(v: &V) -> String {
    v.as_str()
        .map(String::from)
        .or_else(|| v.as_i64().map(|x| x.to_string()))
        .or_else(|| v.as_u64().map(|x| x.to_string()))
        .or_else(|| v.as_f64().map(|x| js_stringify(&json!(x))))
        .unwrap_or_default()
}
fn fatal(e: &str) -> String {
    format!("{e}\u{0}fatal\u{0}durableRecordingFailed")
}

// combinedRisk(g,record,allocation,{allowFundingShortfall})
fn combined_risk(g: &V, record: &V, allocation: &V, allow_funding_shortfall: bool) -> Result<V, String> {
    let mut out = Map::new();
    out.insert("owned".into(), copy(get(record, "owned")));
    out.insert("reservations".into(), copy(get(record, "reservations")));
    out.insert("loans".into(), json!([]));
    let mut ids: HashSet<String> = HashSet::new();
    for (kind, field) in [
        ("owned", "nominalCost"),
        ("reservations", "maximumNominalCost"),
        ("loans", "fullNominalPrincipal"),
    ] {
        let arr = get(&V::Object(out.clone()), kind).clone();
        need(
            arr.is_array() && get(g, kind).is_array(),
            "EXPOSURE_ARRAY",
        )?;
        let mut rows: Vec<V> = arr.as_array().cloned().unwrap_or_default();
        for r in &rows {
            need(
                get(r, "exposureId").as_str().is_some() && !ids.contains(sget(r, "exposureId").unwrap_or("")),
                "OWN_EXPOSURE_ID",
            )?;
            ids.insert(sget(r, "exposureId").unwrap_or("").to_string());
        }
        for r in get(g, kind).as_array().cloned().unwrap_or_default() {
            need(
                !r.is_null()
                    && sget(&r, "exposureId").is_some_and(|s| !s.is_empty())
                    && sget(&r, "marketId").is_some()
                    && sget(&r, "eventId").is_some_and(|s| !s.is_empty())
                    && amount(sget(&r, field).unwrap_or(""))? >= 0,
                "EXTERNAL_EXPOSURE",
            )?;
            let eid = sget(&r, "exposureId").unwrap_or("");
            if ids.contains(eid) {
                need(
                    rows.iter().try_fold(false, |acc, v| same(v, &r).map(|x| acc || x))?,
                    "CONFLICTING_DUPLICATE",
                )?;
                continue;
            }
            let run_id = sget(record, "runId").unwrap_or("");
            need(
                !eid.starts_with(&format!("scaled:{run_id}:"))
                    && !eid.starts_with(&format!("hedge:{run_id}:")),
                "UNVERIFIED_OWN_SHADOW",
            )?;
            ids.insert(eid.to_string());
            rows.push(copy(&r));
        }
        out.insert(kind.into(), json!(rows));
    }
    let mut nominal: i128 = 0;
    let mut reserved: i128 = 0;
    let mut markets: IndexMap<String, i128> = IndexMap::new();
    let mut events: IndexMap<String, i128> = IndexMap::new();
    let mut mapping: IndexMap<String, String> = IndexMap::new();
    for (kind, field) in [
        ("owned", "nominalCost"),
        ("reservations", "maximumNominalCost"),
        ("loans", "fullNominalPrincipal"),
    ] {
        for r in get(&V::Object(out.clone()), kind).as_array().cloned().unwrap_or_default() {
            let n = amount(sget(&r, field).unwrap_or(""))?;
            need(
                n >= 0 && sget(&r, "marketId").is_some() && sget(&r, "eventId").is_some_and(|s| !s.is_empty()),
                "NOMINAL_EXPOSURE",
            )?;
            let m = sget(&r, "marketId").unwrap_or("").to_string();
            let e = sget(&r, "eventId").unwrap_or("").to_string();
            need(
                !mapping.contains_key(&m) || mapping.get(&m).map(|x| x.as_str()) == Some(e.as_str()),
                "EVENT_SCOPE_CONFLICT",
            )?;
            mapping.insert(m.clone(), e.clone());
            nominal += n;
            if kind == "reservations" {
                reserved += n;
            }
            *markets.entry(m).or_insert(0) += n;
            *events.entry(e).or_insert(0) += n;
        }
    }
    let cap = amount(sget(allocation, "capital").unwrap_or(""))?;
    let mkt_max = amount(sget(allocation, "maximumMarketCost").unwrap_or(""))?;
    let evt_max = amount(sget(allocation, "maximumEventCost").unwrap_or(""))?;
    let max_markets = get(allocation, "maximumActiveMarkets").as_f64().unwrap_or(f64::NAN);
    need(
        nominal <= cap
            && markets.values().all(|v| *v <= mkt_max)
            && events.values().all(|v| *v <= evt_max)
            && (markets.values().filter(|v| **v > 0).count() as f64) <= max_markets,
        "FULL_NOMINAL_LIMIT",
    )?;
    let cash = amount(sget(g, "cashLower").unwrap_or(""))?;
    need(
        cash >= 0 && (allow_funding_shortfall || reserved <= cash),
        "FULL_CASH_RESERVATION",
    )?;
    let mut out2 = out.clone();
    out2.insert("nominalExposure".into(), json!(money(nominal)));
    out2.insert("reservedCash".into(), json!(money(reserved)));
    out2.insert("fundingSatisfied".into(), V::Bool(reserved <= cash));
    let mut mo = Map::new();
    for (k, v) in &markets {
        mo.insert(k.clone(), json!(money(*v)));
    }
    out2.insert("markets".into(), V::Object(mo));
    let mut eo = Map::new();
    for (k, v) in &events {
        eo.insert(k.clone(), json!(money(*v)));
    }
    out2.insert("events".into(), V::Object(eo));
    out2.insert("loansDiscounted".into(), V::Bool(false));
    Ok(V::Object(out2))
}

// ownRecord(r)
fn own_record(r: &V) -> Result<V, String> {
    let reservation_hash = sget(r, "reservationHash").unwrap_or("").to_string();
    let mut body = r.as_object().cloned().unwrap_or_default();
    body.remove("reservationHash");
    need(
        hex(get(r, "reservationHash"))
            && payload_hash(&V::Object(body))? == reservation_hash
            && get(r, "kind").as_str() == Some("baal_scaled_no_full_nominal_reservation_v1")
            && get(r, "owned").is_array()
            && get(r, "reservations").is_array()
            && get(r, "unresolvedIntentIds").is_array(),
        "RESERVATION_SCHEMA",
    )?;
    let cash_reserved: i128 = get(r, "reservations")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|x| amount(sget(x, "maximumNominalCost").unwrap_or("")))
        .collect::<Result<Vec<i128>, _>>()?
        .iter()
        .sum();
    need(
        get(r, "cashReserved").as_str() == Some(money(cash_reserved).as_str()),
        "RESERVATION_CASH",
    )?;
    Ok(copy(r))
}

// projected(state,seed)
fn projected(state: &V, seed: &V) -> Result<V, String> {
    need(
        same(get(state, "scope"), get(get(seed, "state"), "scope"))?
            && same(get(state, "allocation"), get(get(seed, "state"), "allocation"))?
            && same(
                get(state, "originalInventory"),
                get(get(seed, "state"), "inventory"),
            )?
            && same(get(state, "originalOrders"), get(get(seed, "state"), "orders"))?
            && get(state, "orders").is_array()
            && get(state, "orders")
                .as_array()
                .map(|os| {
                    os.iter()
                        .filter(|o| get(get(o, "body"), "action").as_str() == Some("buy"))
                        .count()
                        <= 1
                })
                .unwrap_or(false)
            && !get(state, "currentInventory").is_null()
            && get(state, "assignments").is_array()
            && get(state, "pairRetirements").is_array(),
        "ORIGINAL_INVENTORY_OR_SINGLE_HEDGE",
    )?;
    let mut spent: i128 = get(get(seed, "state"), "orders")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|o| {
            if get(get(o, "body"), "action").as_str() == Some("buy") {
                amount(sget(get(o, "proof"), "scopedPrincipalCost").unwrap_or("0.000000"))
            } else {
                Ok(0)
            }
        })
        .collect::<Result<Vec<i128>, _>>()?
        .iter()
        .sum();
    for o in get(state, "orders").as_array().cloned().unwrap_or_default() {
        need(
            get(get(&o, "body"), "side").as_str() == Some("no")
                && ["buy", "sell"].contains(&get(get(&o, "body"), "action").as_str().unwrap_or(""))
                && js_int(get(get(&o, "body"), "quantity")).is_some_and(|q| (1..=100).contains(&q))
                && amount(&js_num_string(get(get(&o, "body"), "price")))
                    .map(|x| x > 0)
                    .unwrap_or(false)
                && amount(&js_num_string(get(get(&o, "body"), "price")))
                    .map(|x| x < 1_000_000)
                    .unwrap_or(false)
                && amount(&js_num_string(get(get(&o, "body"), "price")))
                    .map(|x| x * js_int(get(get(&o, "body"), "quantity")).unwrap_or(0) as i128 <= 25_000_000)
                    .unwrap_or(false)
                && get(get(&o, "body"), "tournamentId")
                    == get(get(state, "scope"), "tournamentId"),
            "ORDER_BOUND",
        )?;
        if get(get(&o, "body"), "action").as_str() == Some("buy") {
            need(
                num_eq(get(get(&o, "body"), "quantity"), 1.0)
                    && amount(&js_num_string(get(get(&o, "body"), "price")))
                        .map(|x| x <= 995_000)
                        .unwrap_or(false),
                "ONE_SHARE_HEDGE_BOUND",
            )?;
        }
        if !get(&o, "proof").is_null() {
            need(
                get(&o, "phase").as_str() == Some("reconciled")
                    && get(get(&o, "proof"), "scopedTradeVerified") == &V::Bool(true)
                    && get(get(&o, "proof"), "action") == get(get(&o, "body"), "action")
                    && get(get(&o, "proof"), "ownerId") == get(get(state, "scope"), "ownerId")
                    && get(get(&o, "proof"), "runId") == get(get(state, "scope"), "runId")
                    && get(get(&o, "proof"), "tournamentId")
                        == get(get(state, "scope"), "tournamentId"),
                "ATTRIBUTED_CURRENT_PROOF",
            )?;
            if get(get(&o, "body"), "action").as_str() == Some("buy") {
                spent += amount(sget(get(&o, "proof"), "scopedPrincipalCost").unwrap_or(""))?;
            }
        }
    }
    let mut realized: i128 = 0;
    if let V::Object(inv) = get(state, "currentInventory") {
        for (ex, slot) in inv {
            let p = get(slot, "proof");
            need(
                get(p, "scopedTradeVerified") == &V::Bool(true)
                    && get(p, "exchangeId").as_str() == Some(ex.as_str())
                    && get(p, "ownerId") == get(get(state, "scope"), "ownerId")
                    && get(p, "runId") == get(get(state, "scope"), "runId")
                    && get(p, "tournamentId") == get(get(state, "scope"), "tournamentId")
                    && hex(get(p, "verificationHash"))
                    && get(p, "remainingLots").is_array()
                    && js_int(get(p, "remainingOwnedQuantity")).is_some_and(|q| q >= 0),
                "CURRENT_ATTRIBUTED_INVENTORY",
            )?;
            let lot_sum: i64 = get(p, "remainingLots")
                .as_array()
                .map(|ls| {
                    ls.iter()
                        .map(|l| js_int(get(l, "quantity")).unwrap_or(0))
                        .sum()
                })
                .unwrap_or(0);
            let lot_cost: i128 = get(p, "remainingLots")
                .as_array()
                .cloned()
                .unwrap_or_default()
                .iter()
                .map(|l| {
                    Ok::<i128, String>(
                        amount(sget(l, "entryPrice").unwrap_or(""))?
                            * js_int(get(l, "quantity")).unwrap_or(0) as i128,
                    )
                })
                .collect::<Result<Vec<i128>, _>>()?
                .iter()
                .sum();
            need(
                js_int(get(p, "remainingOwnedQuantity")) == Some(lot_sum)
                    && amount(sget(p, "remainingOwnedCost").unwrap_or(""))? == lot_cost,
                "CURRENT_FIFO_COST",
            )?;
            need(
                !get(get(state, "originalInventory"), ex).is_null()
                    || get(state, "orders").as_array().is_some_and(|os| {
                        os.iter().any(|o| {
                            get(get(o, "body"), "action").as_str() == Some("buy")
                                && get(get(o, "body"), "exchangeId").as_str() == Some(ex.as_str())
                                && !get(o, "proof").is_null()
                        })
                    }),
                "NO_FOREIGN_CURRENT_INVENTORY",
            )?;
            realized += amount(sget(p, "cumulativeRealizedPnl").unwrap_or(""))?;
        }
    }
    let record = own_record(&project_risk_shim(state)?)?;
    need(
        get(&record, "allocationHash").as_str()
            == Some(payload_hash(get(get(seed, "state"), "allocation"))?.as_str())
            && get(&record, "seedHash").as_str() == Some(payload_hash(seed)?.as_str()),
        "SEEDED_RESERVATION",
    )?;
    let unknown = get(state, "orders")
        .as_array()
        .map(|os| {
            os.iter().any(|o| {
                get(o, "proof").is_null()
                    && get(o, "phase").as_str() != Some("abandoned")
                    && (["uncertain", "semantic_mismatch"]
                        .contains(&get(o, "phase").as_str().unwrap_or(""))
                        || get(o, "semanticMismatch") == &V::Bool(true)
                        || (get(o, "orderId").is_null()
                            && !get(o, "fetchStarts")
                                .as_array()
                                .map(|f| f.is_empty())
                                .unwrap_or(true)))
            })
        })
        .unwrap_or(false);
    Ok(json!({
        "record": record,
        "spent": money(spent),
        "realizedPnl": money(realized),
        "currentInventoryHash": payload_hash(get(state, "currentInventory"))?,
        "assignmentHistoryHash": payload_hash(get(state, "assignments"))?,
        "pairRetirementsHash": payload_hash(get(state, "pairRetirements"))?,
        "orders": get(state, "orders").as_array().cloned().unwrap_or_default().iter().map(|o| json!({
            "intentId": get(o, "intentId"),
            "bodyHash": get(o, "bodyHash"),
            "bodyJson": get(o, "bodyJson"),
            "phase": get(o, "phase"),
            "orderId": get(o, "orderId"),
            "proofHash": get(get(o, "proof"), "verificationHash"),
            "fetchStarts": copy(&{
                let f = get(o, "fetchStarts");
                if f.is_array() { f.clone() } else { json!([]) }
            }),
        })).collect::<Vec<V>>(),
        "unknown": unknown,
    }))
}

fn num_eq(v: &V, n: f64) -> bool {
    v.is_number() && v.as_f64() == Some(n)
}

// JS `o.proof?.scopedPrincipalCost ?? '0.000000'` — covered by unwrap_or above.
fn project_risk_shim(state: &V) -> Result<V, String> {
    crate::baal_neutral_owned_counterpart_pilot_v2::project_risk(state)
}


pub struct OwnedHedgeBudgetDeps {
    pub assert_account_lease: Rc<dyn Fn() -> Result<V, String>>,
    pub assert_financial_owner_lease: Rc<dyn Fn() -> Result<V, String>>,
    /// getCoordinatorSource — the coordinator's journalSource() hint.
    pub get_coordinator_source: Rc<dyn Fn() -> Result<V, String>>,
    /// observeGlobalRisk — fresh collector-level account observation.
    pub observe_global_risk: Rc<dyn Fn() -> Result<V, String>>,
    pub can_operate: Rc<dyn Fn() -> bool>,
    pub now: Rc<dyn Fn() -> f64>,
}

struct BudgetInner {
    dir: PathBuf,
    coordinator_dir: PathBuf,
    workspace: PathBuf,
    legacy_file: PathBuf,
    legacy_bytes: Vec<u8>,
    legacy_identity: String,
    seed: V,
    seed_hash: String,
    scope: V,
    allocation: V,
    allocation_hash: String,
    migration: V,
    migration_hash: String,
    activation: V,
    activation_hash: String,
    coordinator_code_identity_hash: String,
    budget_installed_hash: String,
    actor_file: PathBuf,
    file: PathBuf,
    lease: Option<OwnerLease>,
    journal: Option<Journal>,
    state: V,
    closed: bool,
    failed: bool,
    size: usize,
    file_hash: Option<String>,
    file_identity: Option<String>,
    source_cache: Option<V>,
    cache_hits: i64,
    cache_misses: i64,
    initial_financial_lease_hash: Option<String>,
    deps: OwnedHedgeBudgetDeps,
}

impl BudgetInner {
    fn basic(&mut self) -> Result<V, String> {
        need(
            !self.closed && !self.failed && (self.deps.can_operate)(),
            "STOPPED",
        )?;
        if let Some(l) = &self.lease {
            l.assert_ownership().map_err(|e| e.code.to_string())?;
        }
        let a = (self.deps.assert_account_lease)()?;
        let f = (self.deps.assert_financial_owner_lease)()?;
        need(
            get(&a, "ownerId") == get(&self.scope, "ownerId")
                && get(&a, "runId") == get(&self.scope, "runId")
                && get(&a, "tournamentId") == get(&self.scope, "tournamentId")
                && sget(&a, "leaseId").is_some_and(|s| !s.is_empty()),
            "ACCOUNT_LEASE",
        )?;
        need(
            get(&f, "ownerId") == get(&self.scope, "ownerId")
                && get(&f, "runId") == get(&self.scope, "runId")
                && get(&f, "tournamentId") == get(&self.scope, "tournamentId")
                && sget(&f, "leaseId").is_some_and(|s| !s.is_empty())
                && get(&f, "implementationEpoch")
                    == get(&self.migration, "implementationEpoch"),
            "FINANCIAL_OWNER_LEASE",
        )?;
        let financial_hash = payload_hash(&f)?;
        match &self.initial_financial_lease_hash {
            Some(h) => need(financial_hash == *h, "FINANCIAL_OWNER_CHANGED")?,
            None => self.initial_financial_lease_hash = Some(financial_hash.clone()),
        }
        Ok(json!({
            "accountLeaseHash": payload_hash(&a)?,
            "financialOwnerLeaseHash": financial_hash,
        }))
    }

    fn observed(&self) -> Result<V, String> {
        let g = (self.deps.observe_global_risk)()?;
        need(
            get(&g, "ownerId") == get(&self.scope, "ownerId")
                && get(&g, "runId") == get(&self.scope, "runId")
                && get(&g, "tournamentId") == get(&self.scope, "tournamentId")
                && get(&g, "complete") == &V::Bool(true)
                && get(&g, "eventScopeComplete") == &V::Bool(true)
                && get(&g, "soleCollector") == &V::Bool(true)
                && get(&g, "stopped") == &V::Bool(false)
                && get(&g, "unknownAcceptance").is_boolean()
                && hex(get(&g, "sourceHash"))
                && get(&g, "unresolvedIntentIds").is_array()
                && get(&g, "unresolvedIntentIds")
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|id| id.is_string())
                && instant(sget(&g, "at").unwrap_or(""))? as f64 <= (self.deps.now)()
                && (self.deps.now)() - instant(sget(&g, "at").unwrap_or(""))? as f64 <= 20000.0,
            "FRESH_GLOBAL_OBSERVATION",
        )?;
        Ok(g)
    }

    fn source(&mut self) -> Result<V, String> {
        self.basic()?;
        need(
            payload_hash(&crate::baal_neutral_owned_counterpart_pilot_v2::live_coordinator_identity(
                &self.workspace,
            )?)?
                == self.coordinator_code_identity_hash,
            "COORDINATOR_CODE_CHANGED",
        )?;
        let old_before = std::fs::metadata(&self.legacy_file).map_err(|e| e.to_string())?;
        let old = std::fs::read(&self.legacy_file).map_err(|e| e.to_string())?;
        let old_after = std::fs::metadata(&self.legacy_file).map_err(|e| e.to_string())?;
        need(
            stat_identity(&old_before) == self.legacy_identity
                && stat_identity(&old_after) == self.legacy_identity
                && old_after.len() as usize == old.len()
                && old.len()
                    == js_int(get(get(&self.seed, "anchor"), "bytes")).map(|x| x as usize).unwrap_or(0)
                && old == self.legacy_bytes,
            "LEGACY_NOT_SEALED",
        )?;
        let hint = (self.deps.get_coordinator_source)()?;
        need(
            sget(&hint, "journalFile").is_some()
                && std::fs::canonicalize(sget(&hint, "journalFile").unwrap())
                    .map(|p| p == std::fs::canonicalize(&self.actor_file).unwrap_or_default())
                    .unwrap_or(false),
            "ACTUAL_SOURCE_PATH",
        )?;
        let before = std::fs::metadata(&self.actor_file).map_err(|e| e.to_string())?;
        let bytes = std::fs::read(&self.actor_file).map_err(|e| e.to_string())?;
        let hash = sha(&bytes);
        need(
            !bytes.is_empty()
                && bytes.len() <= 128 * 1024 * 1024
                && bytes.last() == Some(&10),
            "COMPLETE_SOURCE",
        )?;
        if !self.state.is_null() {
            need(
                bytes.len() as i64 >= js_int(get(get(&self.state, "actorAnchor"), "bytes")).unwrap_or(0)
                    && sha(&bytes[..js_int(get(get(&self.state, "actorAnchor"), "bytes")).unwrap_or(0) as usize])
                        == sget(get(&self.state, "actorAnchor"), "sha256").unwrap_or(""),
                "SOURCE_PREFIX_RESET",
            )?;
        }
        let key = payload_hash(&json!({
            "hash": hash,
            "size": bytes.len(),
            "identity": stat_identity(&before),
            "seedHash": self.seed_hash,
            "activationHash": self.activation_hash,
            "migrationHash": self.migration_hash,
            "coordinatorCodeIdentityHash": self.coordinator_code_identity_hash,
        }))?;
        let cached = self
            .source_cache
            .as_ref()
            .filter(|c| get(c, "key").as_str() == Some(key.as_str()))
            .cloned();
        let (result, p): (V, V);
        if let Some(ref c) = cached {
            self.cache_hits += 1;
            let val = get(&c, "value").clone();
            result = get(&val, "result").clone();
            p = get(&val, "p").clone();
        } else {
            self.cache_misses += 1;
            let sd = self.seed.clone();
            let act = self.activation.clone();
            let mig = self.migration.clone();
            let code = crate::baal_neutral_owned_counterpart_pilot_v2::live_coordinator_identity(
                &self.workspace,
            )?;
            result = crate::baal_neutral_owned_counterpart_pilot_v2::replay_live_hedge_journal(
                &json!({
                    "bodyText": String::from_utf8_lossy(&bytes),
                    "seed": sd,
                    "activation": act,
                    "migration": mig,
                    "expectedCodeIdentity": code,
                }),
            )?;
            need(
                get(&result, "economicReplayPerformed") == &V::Bool(true),
                "ECONOMIC_REPLAY_REQUIRED",
            )?;
            p = projected(get(&result, "state"), &self.seed)?;
        }
        need(
            get(get(&result, "anchor"), "bytes").as_i64().unwrap_or(-1)
                == bytes.len() as i64
                && get(get(&result, "anchor"), "sha256").as_str() == Some(hash.as_str())
                && stat_identity(&std::fs::metadata(&self.actor_file).map_err(|e| e.to_string())?)
                    == stat_identity(&before)
                && std::fs::metadata(&self.actor_file).map_err(|e| e.to_string())?.len()
                    == bytes.len() as u64,
            "SOURCE_CHANGED_DURING_READ",
        )?;
        need(
            get(&hint, "sequence") == get(get(&result, "anchor"), "sequence")
                && get(&hint, "hash") == get(get(&result, "anchor"), "hash")
                && same(get(&hint, "record"), get(&p, "record"))?
                && (get(&hint, "rootPrefix").is_null()
                    || same(get(&hint, "rootPrefix"), get(&result, "rootPrefix"))?)
                && (get(&hint, "headPrefix").is_null()
                    || same(get(&hint, "headPrefix"), get(&result, "anchor"))?),
            "CALLER_FRONTIER",
        )?;
        need(
            get(get(&p, "record"), "implementationEpoch")
                == get(&self.migration, "implementationEpoch"),
            "EPOCH",
        )?;
        if !self.state.is_null() {
            need(
                same(get(&result, "rootPrefix"), get(&self.state, "coordinatorRootPrefix"))?
                    && amount(sget(&p, "spent").unwrap_or(""))?
                        >= amount(sget(get(&self.state, "projection"), "spent").unwrap_or(""))?
                    && get(&p, "orders").as_array().map(|a| a.len()).unwrap_or(0)
                        >= get(get(&self.state, "projection"), "orders")
                            .as_array()
                            .map(|a| a.len())
                            .unwrap_or(0),
                "EXPENSE_OR_ROOT_RESET",
            )?;
            for o in get(get(&self.state, "projection"), "orders")
                .as_array()
                .cloned()
                .unwrap_or_default()
            {
                let n = get(&p, "orders")
                    .as_array()
                    .and_then(|os| {
                        os.iter()
                            .find(|n| get(n, "intentId") == get(&o, "intentId"))
                            .cloned()
                    })
                    .unwrap_or(V::Null);
                need(
                    !n.is_null()
                        && get(&n, "bodyHash") == get(&o, "bodyHash")
                        && get(&n, "bodyJson") == get(&o, "bodyJson")
                        && (get(&o, "orderId").is_null()
                            || get(&n, "orderId") == get(&o, "orderId"))
                        && (get(&o, "proofHash").is_null()
                            || get(&n, "proofHash") == get(&o, "proofHash"))
                        && get(&o, "fetchStarts").as_array().is_some_and(|fs| {
                            fs.iter().all(|f| {
                                get(&n, "fetchStarts").as_array().is_some_and(|ns| {
                                    ns.iter().any(|g2| same(f, g2).unwrap_or(false))
                                })
                            })
                        }),
                    "ORIGINAL_HEDGE_RESET",
                )?;
            }
        }
        if cached.is_none() {
            self.source_cache = Some(json!({
                "key": key,
                "value": {
                    "result": {
                        "anchor": get(&result, "anchor"),
                        "rootPrefix": get(&result, "rootPrefix"),
                        "economicReplayPerformed": true,
                    },
                    "p": p,
                },
            }));
        }
        let mut out = p.as_object().cloned().unwrap_or_default();
        out.insert("anchor".into(), get(&result, "anchor").clone());
        out.insert("opening".into(), get(&result, "rootPrefix").clone());
        Ok(V::Object(out))
    }

    fn validate_snapshot(&self, next: &V) -> Result<(), String> {
        let before = std::fs::metadata(&self.actor_file).map_err(|e| e.to_string())?;
        let all = std::fs::read(&self.actor_file).map_err(|e| e.to_string())?;
        let a = get(next, "actorAnchor");
        need(
            js_int(get(a, "bytes")).is_some()
                && js_int(get(a, "bytes")).unwrap() > 0
                && js_int(get(a, "bytes")).unwrap() as usize <= all.len(),
            "HISTORICAL_SOURCE_BOUND",
        )?;
        let prefix = &all[..js_int(get(a, "bytes")).unwrap() as usize];
        let hash = sha(prefix);
        need(
            prefix.last() == Some(&10) && hash == sget(a, "sha256").unwrap_or(""),
            "HISTORICAL_SOURCE_HASH",
        )?;
        let code = crate::baal_neutral_owned_counterpart_pilot_v2::live_coordinator_identity(
            &self.workspace,
        )?;
        need(
            payload_hash(&code)? == self.coordinator_code_identity_hash,
            "COORDINATOR_CODE_CHANGED",
        )?;
        let key = payload_hash(&json!({
            "hash": hash,
            "size": prefix.len(),
            "identity": stat_identity(&before),
            "seedHash": self.seed_hash,
            "activationHash": self.activation_hash,
            "migrationHash": self.migration_hash,
            "coordinatorCodeIdentityHash": self.coordinator_code_identity_hash,
        }))?;
        let (replay, actual): (V, V);
        if all.len() == prefix.len()
            && self
                .source_cache
                .as_ref()
                .map(|c| get(c, "key").as_str() == Some(key.as_str()))
                .unwrap_or(false)
            && same(
                get(
                    get(get(self.source_cache.as_ref().unwrap(), "value"), "result"),
                    "anchor",
                ),
                a,
            )?
        {
            let val = get(self.source_cache.as_ref().unwrap(), "value").clone();
            replay = get(&val, "result").clone();
            actual = get(&val, "p").clone();
        } else {
            replay = crate::baal_neutral_owned_counterpart_pilot_v2::replay_live_hedge_journal(
                &json!({
                    "bodyText": String::from_utf8_lossy(prefix),
                    "seed": self.seed,
                    "activation": self.activation,
                    "migration": self.migration,
                    "expectedCodeIdentity": code,
                }),
            )?;
            actual = projected(get(&replay, "state"), &self.seed)?;
        }
        let after = std::fs::metadata(&self.actor_file).map_err(|e| e.to_string())?;
        need(
            stat_identity(&before) == stat_identity(&after) && after.len() == all.len() as u64,
            "SOURCE_CHANGED_DURING_READ",
        )?;
        need(
            same(get(&replay, "anchor"), a)?
                && same(get(&replay, "rootPrefix"), get(next, "coordinatorRootPrefix"))?
                && same(&actual, get(next, "projection"))?,
            "HISTORICAL_ECONOMIC_SOURCE",
        )?;
        need(
            instant(sget(get(next, "externalObservation"), "at").unwrap_or(""))?
                <= instant(sget(next, "at").unwrap_or(""))?
                && instant(sget(next, "at").unwrap_or(""))?
                    - instant(sget(get(next, "externalObservation"), "at").unwrap_or(""))?
                    <= 20000,
            "HISTORICAL_OBSERVATION_CLOCK",
        )?;
        need(
            get(next, "schemaVersion") == &json!(1)
                && get(next, "mode").as_str() == Some("source_verified_owned_hedge_budget_v2")
                && get(next, "seedHash").as_str() == Some(self.seed_hash.as_str())
                && get(next, "migrationHash").as_str() == Some(self.migration_hash.as_str())
                && get(next, "activationHash").as_str() == Some(self.activation_hash.as_str())
                && get(next, "coordinatorCodeIdentityHash").as_str()
                    == Some(self.coordinator_code_identity_hash.as_str())
                && get(next, "budgetInstalledHash").as_str()
                    == Some(self.budget_installed_hash.as_str())
                && get(next, "allocationHash").as_str() == Some(self.allocation_hash.as_str())
                && same(get(next, "scope"), &self.scope)?
                && same(get(next, "legacyAnchor"), get(&self.seed, "anchor"))?
                && hex(get(next, "financialOwnerLeaseHash"))
                && hex(get(next, "accountLeaseHash")),
            "BUDGET_IDENTITY",
        )?;
        own_record(get(get(next, "projection"), "record"))?;
        need(
            same(
                &combined_risk(
                    get(next, "externalObservation"),
                    get(get(next, "projection"), "record"),
                    &self.allocation,
                    false,
                )?,
                get(next, "risk"),
            )? && get(next, "cumulativeActualExpenses") == get(get(next, "projection"), "spent"),
            "BUDGET_ECONOMICS",
        )?;
        if !self.state.is_null() {
            need(
                js_int(get(get(next, "actorAnchor"), "bytes")).unwrap_or(0)
                    >= js_int(get(get(&self.state, "actorAnchor"), "bytes")).unwrap_or(0)
                    && js_int(get(get(next, "actorAnchor"), "sequence")).unwrap_or(0)
                        >= js_int(get(get(&self.state, "actorAnchor"), "sequence")).unwrap_or(0)
                    && amount(sget(get(next, "projection"), "spent").unwrap_or(""))?
                        >= amount(sget(get(&self.state, "projection"), "spent").unwrap_or(""))?,
                "BUDGET_RESET",
            )?;
        }
        Ok(())
    }

    fn append(&mut self, next: &V) -> Result<(), String> {
        self.basic()?;
        let old = std::fs::read(&self.file).map_err(|e| e.to_string())?;
        let metadata = std::fs::metadata(&self.file).map_err(|e| e.to_string())?;
        need(
            Some(stat_identity(&metadata)) == self.file_identity
                && old.len() == self.size
                && Some(sha(&old)) == self.file_hash,
            "BUDGET_FILE_CHANGED",
        )?;
        self.validate_snapshot(next)?;
        let post: Result<(), String> = (|| {
            self.journal.as_mut().unwrap().commit(
                next,
                &json!({
                    "kind": "installed_source_verified_hedge_reserve",
                    "seedHash": self.seed_hash,
                    "migrationHash": self.migration_hash,
                    "activationHash": self.activation_hash,
                }),
            )?;
            let after = std::fs::read(&self.file).map_err(|e| e.to_string())?;
            need(after[..self.size] == old[..], "BUDGET_PREFIX_CHANGED")?;
            self.state = copy(next);
            self.size = after.len();
            self.file_hash = Some(sha(&after));
            self.basic()?;
            Ok(())
        })();
        match post {
            Ok(()) => Ok(()),
            Err(m) => {
                self.failed = true;
                Err(fatal(&m))
            }
        }
    }
}

pub struct OwnedHedgeBudgetV2 {
    inner: std::cell::RefCell<BudgetInner>,
}

/// openBaalOwnedHedgeSharedBudgetV2({dir,coordinatorDir,workspace,legacyFile,
///   legacyCodeIdentity,legacyScope,legacyAllocation,activation,migration,
///   coordinatorCodeIdentityHash,...deps})
pub fn open_baal_owned_hedge_shared_budget_v2(
    dir: &Path,
    coordinator_dir: &Path,
    workspace: &Path,
    legacy_file: &Path,
    legacy_code_identity: &V,
    legacy_scope: &V,
    legacy_allocation: &V,
    activation: &V,
    migration: &V,
    coordinator_code_identity_hash: &str,
    deps: OwnedHedgeBudgetDeps,
) -> Result<OwnedHedgeBudgetV2, String> {
    need(
        dir.is_absolute()
            && coordinator_dir.is_absolute()
            && workspace.is_absolute()
            && legacy_file.is_absolute()
            && std::fs::canonicalize(dir).map(|d| d != std::fs::canonicalize(coordinator_dir).unwrap_or_default()).unwrap_or(false)
            && hex(&json!(coordinator_code_identity_hash)),
        "DEPENDENCIES",
    )?;
    let legacy_bytes = std::fs::read(legacy_file).map_err(|e| e.to_string())?;
    let legacy_identity =
        stat_identity(&std::fs::metadata(legacy_file).map_err(|e| e.to_string())?);
    let seed = crate::baal_scaled_controller_legacy_replay_v1::replay_baal_scaled_controller_v1(
        &json!({
            "bodyText": String::from_utf8_lossy(&legacy_bytes),
            "expectedCodeIdentity": legacy_code_identity,
            "expectedScope": legacy_scope,
            "expectedAllocation": legacy_allocation,
        }),
    )?;
    let seed_hash = payload_hash(&seed)?;
    let scope = copy(get(get(&seed, "state"), "scope"));
    let mut allocation = Map::new();
    allocation.insert("maximumOrderCost".into(), json!("25.000000"));
    allocation.insert("maximumMarketCost".into(), json!("100.000000"));
    allocation.insert("maximumEventCost".into(), json!("200.000000"));
    allocation.insert("maximumActiveMarkets".into(), json!(20));
    if let V::Object(a) = get(get(&seed, "state"), "allocation") {
        for (k, v) in a {
            allocation.insert(k.clone(), v.clone());
        }
    }
    let allocation = V::Object(allocation);
    let allocation_hash = payload_hash(get(get(&seed, "state"), "allocation"))?;
    let migration_hash = payload_hash(migration)?;
    let activation_hash = payload_hash(activation)?;
    need(
        get(&allocation, "ownerId").as_str() == Some("Codex")
            && get(&allocation, "capital").as_str() == Some("1000.000000")
            && hex(get(&allocation, "humanAllocationSourceHash"))
            && amount(sget(&allocation, "maximumOrderCost").unwrap_or("")).map(|x| x > 0 && x <= 25_000_000).unwrap_or(false)
            && amount(sget(&allocation, "maximumMarketCost").unwrap_or("")).map(|x| x > 0 && x <= 100_000_000).unwrap_or(false)
            && amount(sget(&allocation, "maximumEventCost").unwrap_or("")).map(|x| x > 0 && x <= 200_000_000).unwrap_or(false)
            && js_int(get(&allocation, "maximumActiveMarkets")).is_some_and(|x| x > 0 && x <= 20),
        "ALLOCATION",
    )?;
    let budget_installed_hash = payload_hash(&json!({
        "kind": "source_verified_owned_hedge_budget_v2",
        "seedHash": seed_hash,
        "migrationHash": migration_hash,
        "activationHash": activation_hash,
        "coordinatorCodeIdentityHash": coordinator_code_identity_hash,
    }))?;
    let actor_file = coordinator_dir.join("neutral-owned-counterpart-pilot-v2.jsonl");
    let file = dir.join("owned-hedge-shared-budget-v2.jsonl");
    let mut inner = BudgetInner {
        dir: dir.to_path_buf(),
        coordinator_dir: coordinator_dir.to_path_buf(),
        workspace: workspace.to_path_buf(),
        legacy_file: legacy_file.to_path_buf(),
        legacy_bytes,
        legacy_identity,
        seed,
        seed_hash,
        scope,
        allocation,
        allocation_hash,
        migration: migration.clone(),
        migration_hash,
        activation: activation.clone(),
        activation_hash,
        coordinator_code_identity_hash: coordinator_code_identity_hash.to_string(),
        budget_installed_hash,
        actor_file,
        file: file.clone(),
        lease: None,
        journal: None,
        state: V::Null,
        closed: false,
        failed: false,
        size: 0,
        file_hash: None,
        file_identity: None,
        source_cache: None,
        cache_hits: 0,
        cache_misses: 0,
        initial_financial_lease_hash: None,
        deps,
    };
    let boot: Result<(), String> = (|| {
        inner.basic()?;
        inner.lease = Some(
            acquire_owner_lease(
                dir,
                sget(&inner.scope, "ownerId").unwrap_or(""),
                sget(&inner.scope, "runId").unwrap_or(""),
                "source-verified-owned-hedge-budget-v2",
                (inner.deps.now)(),
            )
            .map_err(|e| e.code.to_string())?,
        );
        if !file.exists() {
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&file)
                .map(|f| drop(f))
                .map_err(|e| e.to_string())?;
        }
        let bytes = std::fs::read(&file).map_err(|e| e.to_string())?;
        need(
            bytes.len() <= 128 * 1024 * 1024 && (bytes.is_empty() || bytes.last() == Some(&10)),
            "BUDGET_BOUND",
        )?;
        inner.journal = Some(Journal::open(&file).map_err(|e| e.to_string())?);
        let text = String::from_utf8_lossy(&bytes).to_string();
        for line in text.split('\n').filter(|l| !l.is_empty()) {
            let row: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
            need(
                get(get(&row, "event"), "kind").as_str()
                    == Some("installed_source_verified_hedge_reserve")
                    && get(get(&row, "event"), "seedHash").as_str()
                        == Some(inner.seed_hash.as_str())
                    && get(get(&row, "event"), "migrationHash").as_str()
                        == Some(inner.migration_hash.as_str())
                    && get(get(&row, "event"), "activationHash").as_str()
                        == Some(inner.activation_hash.as_str()),
                "BUDGET_EVENT",
            )?;
            inner.validate_snapshot(get(&row, "state"))?;
            inner.state = copy(get(&row, "state"));
        }
        inner.size = bytes.len();
        inner.file_hash = Some(sha(&bytes));
        inner.file_identity =
            Some(stat_identity(&std::fs::metadata(&file).map_err(|e| e.to_string())?));
        if !inner.state.is_null() {
            inner.source()?;
        }
        Ok(())
    })();
    if let Err(e) = boot {
        if let Some(l) = inner.lease.as_mut() {
            let _ = l.release();
        }
        return Err(e);
    }
    Ok(OwnedHedgeBudgetV2 {
        inner: std::cell::RefCell::new(inner),
    })
}

impl OwnedHedgeBudgetV2 {
    /// installFundingReservation(record)
    pub fn install_funding_reservation(&self, record: &V) -> Result<V, String> {
        let mut g = self.inner.borrow_mut();
        let leases = g.basic()?;
        let s = g.source()?;
        need(same(record, get(&s, "record"))?, "SOURCE_RESERVATION_REQUIRED")?;
        if g.state.is_null() {
            need(
                get(&s, "orders").as_array().is_some_and(|a| a.is_empty())
                    && get(&s, "unknown") == &V::Bool(false)
                    && get(get(&s, "record"), "unresolvedIntentIds")
                        .as_array()
                        .is_some_and(|a| a.is_empty()),
                "MIGRATE_BEFORE_NEW_HEDGE",
            )?;
        }
        let obs = g.observed()?;
        let risk = combined_risk(&obs, get(&s, "record"), &g.allocation, false)?;
        let unresolved: HashSet<String> = get(get(&s, "record"), "unresolvedIntentIds")
            .as_array()
            .map(|a| a.iter().map(|x| js_stringify(x)).collect())
            .unwrap_or_default();
        need(
            get(&obs, "unresolvedIntentIds")
                .as_array()
                .map(|a| a.iter().all(|id| unresolved.contains(&js_stringify(id))))
                .unwrap_or(false),
            "FOREIGN_UNKNOWN",
        )?;
        let at = baal_service_runtime::contract::ms_to_iso((g.deps.now)());
        let state = &g.state; // keep borrow discipline: build `next` first.
        let _ = state;
        let mut next = Map::new();
        next.insert("schemaVersion".into(), json!(1));
        next.insert("mode".into(), json!("source_verified_owned_hedge_budget_v2"));
        next.insert("scope".into(), copy(&g.scope));
        next.insert("allocationHash".into(), json!(g.allocation_hash));
        next.insert("budgetInstalledHash".into(), json!(g.budget_installed_hash));
        next.insert(
            "coordinatorCodeIdentityHash".into(),
            json!(g.coordinator_code_identity_hash),
        );
        next.insert("seedHash".into(), json!(g.seed_hash));
        next.insert("migrationHash".into(), json!(g.migration_hash));
        next.insert("activationHash".into(), json!(g.activation_hash));
        next.insert("legacyAnchor".into(), copy(get(&g.seed, "anchor")));
        next.insert("coordinatorRootPrefix".into(), get(&s, "opening").clone());
        next.insert("actorAnchor".into(), get(&s, "anchor").clone());
        next.insert(
            "projection".into(),
            json!({
                "record": get(&s, "record"),
                "orders": get(&s, "orders"),
                "spent": get(&s, "spent"),
                "realizedPnl": get(&s, "realizedPnl"),
                "currentInventoryHash": get(&s, "currentInventoryHash"),
                "assignmentHistoryHash": get(&s, "assignmentHistoryHash"),
                "pairRetirementsHash": get(&s, "pairRetirementsHash"),
                "unknown": get(&s, "unknown"),
            }),
        );
        next.insert("risk".into(), risk);
        next.insert("externalObservation".into(), obs);
        if let V::Object(lm) = &leases {
            for (k, v) in lm {
                next.insert(k.clone(), v.clone());
            }
        }
        next.insert("cumulativeActualExpenses".into(), get(&s, "spent").clone());
        next.insert("at".into(), json!(at.clone()));
        g.append(&V::Object(next))?;
        let prev = g
            .journal
            .as_ref()
            .map(|j| j.previous.clone())
            .unwrap_or_default();
        Ok(json!({
            "reservationHash": get(get(&s, "record"), "reservationHash"),
            "allocationHash": g.allocation_hash,
            "seedHash": g.seed_hash,
            "budgetInstalledHash": g.budget_installed_hash,
            "globalLedgerHash": prev,
            "confirmedAt": at,
        }))
    }

    /// globalRisk() / getGlobalRisk()
    pub fn global_risk(&self) -> Result<V, String> {
        let mut g = self.inner.borrow_mut();
        g.basic()?;
        need(!g.state.is_null(), "NOT_INSTALLED")?;
        let s = g.source()?;
        let obs = g.observed()?;
        need(
            get(get(&s, "record"), "reservationHash")
                == get(get(get(&g.state, "projection"), "record"), "reservationHash"),
            "CURRENT_RESERVE_NOT_INSTALLED",
        )?;
        let r = combined_risk(&obs, get(&s, "record"), &g.allocation, true)?;
        let prev = g.journal.as_ref().map(|j| j.previous.clone()).unwrap_or_default();
        let mut out = obs.as_object().cloned().unwrap_or_default();
        if let V::Object(rm) = &r {
            for (k, v) in rm {
                out.insert(k.clone(), v.clone());
            }
        }
        out.insert(
            "installedCashLower".into(),
            get(get(&g.state, "externalObservation"), "cashLower").clone(),
        );
        out.insert("allocationHash".into(), json!(g.allocation_hash));
        out.insert("budgetInstalledHash".into(), json!(g.budget_installed_hash));
        out.insert("seedHash".into(), json!(g.seed_hash));
        out.insert("implementationEpoch".into(), get(&g.migration, "implementationEpoch").clone());
        out.insert(
            "controllerReservationHash".into(),
            get(get(&s, "record"), "reservationHash").clone(),
        );
        out.insert("globalLedgerHash".into(), json!(prev.clone()));
        out.insert(
            "sourceHash".into(),
            json!(payload_hash(&json!({
                "observation": get(&obs, "sourceHash"),
                "actor": get(&s, "anchor"),
                "reservation": get(get(&s, "record"), "reservationHash"),
                "globalLedgerHash": prev,
            }))?),
        );
        let r_unk = ["owned", "reservations", "loans"].iter().any(|k| {
            get(&r, k).as_array().is_some_and(|a| {
                a.iter().any(|v| get(v, "unknownAcceptance") == &V::Bool(true))
            })
        });
        out.insert(
            "unknownAcceptance".into(),
            V::Bool(
                get(&s, "unknown") == &V::Bool(true)
                    || get(&obs, "unknownAcceptance") == &V::Bool(true)
                    || r_unk,
            ),
        );
        let mut ids: Vec<V> = get(get(&s, "record"), "unresolvedIntentIds")
            .as_array()
            .cloned()
            .unwrap_or_default();
        let mut seen: HashSet<String> = ids.iter().map(js_stringify).collect();
        for id in get(&obs, "unresolvedIntentIds").as_array().cloned().unwrap_or_default() {
            if seen.insert(js_stringify(&id)) {
                ids.push(id);
            }
        }
        out.insert("unresolvedIntentIds".into(), json!(ids));
        out.insert("executionAuthority".into(), V::Bool(false));
        out.insert("oldWriterHandbackAllowed".into(), V::Bool(false));
        Ok(V::Object(out))
    }

    pub fn status(&self) -> Result<V, String> {
        let mut g = self.inner.borrow_mut();
        let mut unknown = get(get(&g.state, "projection"), "unknown")
            .as_bool()
            .unwrap_or(false);
        if !g.state.is_null() && !g.closed {
            match g.source() {
                Ok(s) => {
                    if get(&s, "unknown") == &V::Bool(true) {
                        unknown = true;
                    }
                }
                Err(_) => unknown = true,
            }
        }
        let state = g.state.clone();
        let seed_state_inv = get(get(&g.seed, "state"), "inventory").clone();
        let default_realized: i128 = if let V::Object(m) = &seed_state_inv {
            m.values()
                .map(|x| amount(sget(get(x, "proof"), "cumulativeRealizedPnl").unwrap_or("")))
                .collect::<Result<Vec<i128>, _>>()?
                .iter()
                .sum()
        } else {
            0
        };
        let seed_orders = get(get(&g.seed, "state"), "orders").clone();
        let default_expenses: i128 = seed_orders
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|o| {
                if get(get(o, "body"), "action").as_str() == Some("buy") {
                    amount(sget(get(o, "proof"), "scopedPrincipalCost").unwrap_or("0.000000"))
                } else {
                    Ok(0)
                }
            })
            .collect::<Result<Vec<i128>, _>>()?
            .iter()
            .sum();
        let inv_hash_default = payload_hash(&seed_state_inv)?;
        Ok(json!({
            "mode": "source_verified_owned_hedge_budget_v2",
            "claimed": !g.state.is_null(),
            "allocationHash": g.allocation_hash,
            "budgetInstalledHash": g.budget_installed_hash,
            "seedHash": g.seed_hash,
            "migrationHash": g.migration_hash,
            "coordinatorCodeIdentityHash": g.coordinator_code_identity_hash,
            "sequence": g.journal.as_ref().map(|j| j.sequence).unwrap_or(0),
            "globalLedgerHash": g.journal.as_ref().map(|j| j.previous.clone()).unwrap_or_default(),
            "controllerReservationHash": if state.is_null() { V::Null } else { get(get(get(&state, "projection"), "record"), "reservationHash").clone() },
            "realizedPnl": if state.is_null() { json!(money(default_realized)) } else { get(get(&state, "projection"), "realizedPnl").clone() },
            "currentInventoryHash": if state.is_null() { json!(inv_hash_default) } else { get(get(&state, "projection"), "currentInventoryHash").clone() },
            "pairRetirementsHash": if state.is_null() { json!(payload_hash(&json!([])).unwrap_or_default()) } else { get(get(&state, "projection"), "pairRetirementsHash").clone() },
            "cumulativeActualExpenses": if state.is_null() { json!(money(default_expenses)) } else { get(&state, "cumulativeActualExpenses").clone() },
            "risk": copy(get(&state, "risk")),
            "unknownAcceptance": unknown,
            "actorAnchor": copy(get(&state, "actorAnchor")),
            "legacyAnchor": copy(get(&g.seed, "anchor")),
            "cacheHits": g.cache_hits,
            "cacheMisses": g.cache_misses,
            "failed": g.failed,
            "closed": g.closed,
            "executionAuthority": false,
            "oldWriterHandbackAllowed": false,
            "additionalApiClients": 0,
        }))
    }

    pub fn close(&self) -> Result<(), String> {
        let mut g = self.inner.borrow_mut();
        if g.closed {
            return Ok(());
        }
        g.closed = true;
        if let Some(l) = g.lease.as_mut() {
            l.release().map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}
