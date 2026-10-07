//! Port of execution/baal-owned-hedge-shared-budget-v2.mjs.
//! Source-verified budget for the separately migrated owned-counterpart writer.

use std::cell::RefCell;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use serde_json::{json, Map, Value as V};

use baal_service_runtime::fixed::{amount, instant, money};
use baal_service_runtime::journal::Journal;
use baal_service_runtime::json_text::sha256 as sha;
use baal_service_runtime::ledger::OwnerLease;

use crate::core::payload_hash;

fn need(v: bool, code: &str) -> Result<(), String> {
    if v {
        Ok(())
    } else {
        Err(format!("OWNED_HEDGE_BUDGET_{code}"))
    }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn sget<'a>(v: &'a V, k: &str) -> Option<&'a str> {
    get(v, k).as_str()
}
fn sha_bytes(b: &[u8]) -> String {
    sha(b)
}
fn hex(v: &V) -> bool {
    v.as_str()
        .is_some_and(|s| s.len() == 64 && s.bytes().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()))
}
fn same(a: &V, b: &V) -> Result<bool, String> {
    Ok(payload_hash(a)? == payload_hash(b)?)
}
fn fmt_f64_js(x: f64) -> String {
    if x.fract() == 0.0 && x.is_finite() {
        format!("{}", x as i64)
    } else {
        format!("{x}")
    }
}
fn stamp(m: &fs::Metadata) -> Result<String, String> {
    use std::os::unix::fs::MetadataExt;
    let bt = m
        .created()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(0.0);
    Ok(format!("{}:{}", m.ino(), fmt_f64_js(bt)))
}
fn is_safe_int(v: &V) -> bool {
    v.as_f64().is_some_and(|x| x.is_finite() && x.fract() == 0.0 && x.abs() <= 9007199254740991.0)
}
fn num_amount(v: &V) -> Result<i128, String> {
    match v {
        V::String(s) => amount(s),
        V::Number(n) => amount(&n.to_string()),
        _ => amount(&v.to_string()),
    }
}
fn i64n(v: &V) -> i64 {
    v.as_i64().unwrap_or(0)
}

/// combinedRisk(g,record,allocation,{allowFundingShortfall})
fn combined_risk(g: &V, record: &V, allocation: &V, allow_funding_shortfall: bool) -> Result<V, String> {
    let mut out = Map::new();
    out.insert("owned".into(), get(record, "owned").clone());
    out.insert("reservations".into(), get(record, "reservations").clone());
    out.insert("loans".into(), json!([]));
    let mut ids: HashSet<String> = HashSet::new();
    let run_id = sget(record, "runId").unwrap_or("");
    for (kind, field) in [
        ("owned", "nominalCost"),
        ("reservations", "maximumNominalCost"),
        ("loans", "fullNominalPrincipal"),
    ] {
        let out_arr = get(&V::Object(out.clone()), kind)
            .as_array()
            .cloned()
            .unwrap_or_default();
        need(
            get(&V::Object(out.clone()), kind).is_array() && get(g, kind).is_array(),
            "EXPOSURE_ARRAY",
        )?;
        for r in &out_arr {
            need(
                get(r, "exposureId").as_str().is_some()
                    && !ids.contains(get(r, "exposureId").as_str().unwrap_or("")),
                "OWN_EXPOSURE_ID",
            )?;
            ids.insert(get(r, "exposureId").as_str().unwrap_or("").to_string());
        }
        let mut merged = out_arr.clone();
        for r in get(g, kind).as_array().cloned().unwrap_or_default() {
            need(
                r.is_object()
                    && get(&r, "exposureId").as_str().is_some_and(|s| !s.is_empty())
                    && get(&r, "marketId").is_string()
                    && get(&r, "eventId").as_str().is_some_and(|s| !s.is_empty())
                    && num_amount(get(&r, field)).map(|x| x >= 0).unwrap_or(false),
                "EXTERNAL_EXPOSURE",
            )?;
            let id = get(&r, "exposureId").as_str().unwrap_or("").to_string();
            if ids.contains(&id) {
                let mut any_same = false;
                for v in &out_arr {
                    if same(v, &r)? {
                        any_same = true;
                        break;
                    }
                }
                need(any_same, "CONFLICTING_DUPLICATE")?;
                continue;
            }
            need(
                !id.starts_with(&format!("scaled:{run_id}:"))
                    && !id.starts_with(&format!("hedge:{run_id}:")),
                "UNVERIFIED_OWN_SHADOW",
            )?;
            ids.insert(id);
            merged.push(r);
        }
        out.insert(kind.into(), V::Array(merged));
    }
    let mut nominal: i128 = 0;
    let mut reserved: i128 = 0;
    let mut markets: Map<String, V> = Map::new();
    let mut events: Map<String, V> = Map::new();
    // marketId -> eventId uniqueness mapping (first-write wins like JS Map.set).
    let mut mapping: Vec<(String, String)> = Vec::new();
    for (kind, field) in [
        ("owned", "nominalCost"),
        ("reservations", "maximumNominalCost"),
        ("loans", "fullNominalPrincipal"),
    ] {
        for r in get(&V::Object(out.clone()), kind)
            .as_array()
            .cloned()
            .unwrap_or_default()
        {
            let n = num_amount(get(&r, field))?;
            need(
                n >= 0
                    && get(&r, "marketId").is_string()
                    && get(&r, "eventId").as_str().is_some_and(|s| !s.is_empty()),
                "NOMINAL_EXPOSURE",
            )?;
            let mid = get(&r, "marketId").as_str().unwrap_or("").to_string();
            let eid = get(&r, "eventId").as_str().unwrap_or("").to_string();
            need(
                !mapping.iter().any(|(m, _)| m == &mid)
                    || mapping.iter().find(|(m, _)| m == &mid).map(|(_, e)| e.as_str()) == Some(eid.as_str()),
                "EVENT_SCOPE_CONFLICT",
            )?;
            if !mapping.iter().any(|(m, _)| m == &mid) {
                mapping.push((mid.clone(), eid.clone()));
            }
            nominal += n;
            if kind == "reservations" {
                reserved += n;
            }
            let mv = markets.get(&mid).and_then(|x| num_amount(x).ok()).unwrap_or(0) + n;
            markets.insert(mid, json!(money(mv)));
            let ev = events.get(&eid).and_then(|x| num_amount(x).ok()).unwrap_or(0) + n;
            events.insert(eid, json!(money(ev)));
        }
    }
    let max_m = num_amount(get(allocation, "maximumMarketCost"))?;
    let max_e = num_amount(get(allocation, "maximumEventCost"))?;
    need(
        nominal <= num_amount(get(allocation, "capital"))?
            && markets.values().all(|v| num_amount(v).unwrap_or(i128::MAX) <= max_m)
            && events.values().all(|v| num_amount(v).unwrap_or(i128::MAX) <= max_e)
            && markets
                .values()
                .filter(|v| num_amount(v).unwrap_or(0) > 0)
                .count()
                <= i64n(get(allocation, "maximumActiveMarkets")) as usize,
        "FULL_NOMINAL_LIMIT",
    )?;
    let cash = num_amount(get(g, "cashLower"))?;
    need(
        cash >= 0 && (allow_funding_shortfall || reserved <= cash),
        "FULL_CASH_RESERVATION",
    )?;
    out.insert("nominalExposure".into(), json!(money(nominal)));
    out.insert("reservedCash".into(), json!(money(reserved)));
    out.insert("fundingSatisfied".into(), json!(reserved <= cash));
    out.insert("markets".into(), V::Object(markets));
    out.insert("events".into(), V::Object(events));
    out.insert("loansDiscounted".into(), json!(false));
    Ok(V::Object(out))
}

fn own_record(r: &V) -> Result<(), String> {
    let mut body = r.as_object().cloned().unwrap_or_default();
    let reservation_hash = sget(r, "reservationHash").unwrap_or("").to_string();
    body.shift_remove("reservationHash");
    need(
        hex(&json!(reservation_hash))
            && payload_hash(&V::Object(body))? == reservation_hash
            && sget(r, "kind") == Some("baal_scaled_no_full_nominal_reservation_v1")
            && get(r, "owned").is_array()
            && get(r, "reservations").is_array()
            && get(r, "unresolvedIntentIds").is_array(),
        "RESERVATION_SCHEMA",
    )?;
    let reserved: i128 = get(r, "reservations")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .try_fold(0i128, |n, x| {
            amount(sget(x, "maximumNominalCost").unwrap_or("")).map(|x| n + x)
        })?;
    need(
        sget(r, "cashReserved") == Some(&*money(reserved)),
        "RESERVATION_CASH",
    )?;
    Ok(())
}

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
                        .filter(|o| sget(get(o, "body"), "action") == Some("buy"))
                        .count()
                        <= 1
                })
                .unwrap_or(false)
            && get(state, "currentInventory").is_object()
            && get(state, "assignments").is_array()
            && get(state, "pairRetirements").is_array(),
        "ORIGINAL_INVENTORY_OR_SINGLE_HEDGE",
    )?;
    let mut spent: i128 = get(get(seed, "state"), "orders")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .try_fold(0i128, |n, o| -> Result<i128, String> {
            if sget(get(o, "body"), "action") == Some("buy") {
                let c = get(get(o, "proof"), "scopedPrincipalCost");
                amount(&c.as_str().map(|s| s.to_string()).unwrap_or("0.000000".into()))
                    .map(|x| n + x)
            } else {
                Ok(n)
            }
        })?;
    for o in get(state, "orders").as_array().cloned().unwrap_or_default() {
        let price_v = get(get(&o, "body"), "price");
        let p = num_amount(&price_v)?;
        let qty = get(get(&o, "body"), "quantity");
        need(
            sget(get(&o, "body"), "side") == Some("no")
                && ["buy", "sell"].contains(&sget(get(&o, "body"), "action").unwrap_or(""))
                && is_safe_int(qty)
                && qty.as_i64().unwrap_or(0) >= 1
                && qty.as_i64().unwrap_or(0) <= 100
                && p > 0
                && p < 1_000_000
                && p * qty.as_i64().unwrap_or(0) as i128 <= 25_000_000
                && sget(get(&o, "body"), "tournamentId")
                    == sget(get(state, "scope"), "tournamentId"),
            "ORDER_BOUND",
        )?;
        if sget(get(&o, "body"), "action") == Some("buy") {
            need(
                qty.as_i64().unwrap_or(0) == 1 && p <= 995_000,
                "ONE_SHARE_HEDGE_BOUND",
            )?;
        }
        if !get(&o, "proof").is_null() {
            need(
                sget(&o, "phase") == Some("reconciled")
                    && get(get(&o, "proof"), "scopedTradeVerified") == &V::Bool(true)
                    && get(get(&o, "proof"), "action") == get(get(&o, "body"), "action")
                    && sget(get(&o, "proof"), "ownerId")
                        == sget(get(state, "scope"), "ownerId")
                    && sget(get(&o, "proof"), "runId") == sget(get(state, "scope"), "runId")
                    && sget(get(&o, "proof"), "tournamentId")
                        == sget(get(state, "scope"), "tournamentId"),
                "ATTRIBUTED_CURRENT_PROOF",
            )?;
            if sget(get(&o, "body"), "action") == Some("buy") {
                spent += amount(sget(get(&o, "proof"), "scopedPrincipalCost").unwrap_or(""))?;
            }
        }
    }
    let mut realized: i128 = 0;
    for (ex, slot) in get(state, "currentInventory")
        .as_object()
        .cloned()
        .unwrap_or_default()
    {
        let p = get(&slot, "proof");
        need(
            get(p, "scopedTradeVerified") == &V::Bool(true)
                && sget(p, "exchangeId") == Some(ex.as_str())
                && sget(p, "ownerId") == sget(get(state, "scope"), "ownerId")
                && sget(p, "runId") == sget(get(state, "scope"), "runId")
                && sget(p, "tournamentId") == sget(get(state, "scope"), "tournamentId")
                && hex(get(p, "verificationHash"))
                && get(p, "remainingLots").is_array()
                && is_safe_int(get(p, "remainingOwnedQuantity"))
                && get(p, "remainingOwnedQuantity").as_i64().unwrap_or(-1) >= 0,
            "CURRENT_ATTRIBUTED_INVENTORY",
        )?;
        let lots = get(p, "remainingLots").as_array().cloned().unwrap_or_default();
        let qty_sum: i64 = lots
            .iter()
            .map(|l| get(l, "quantity").as_i64().unwrap_or(0))
            .sum();
        let cost_sum: i128 = lots.iter().try_fold(0i128, |n, l| {
            amount(sget(l, "entryPrice").unwrap_or(""))
                .map(|x| n + x * get(l, "quantity").as_i64().unwrap_or(0) as i128)
        })?;
        need(
            get(p, "remainingOwnedQuantity").as_i64().unwrap_or(-1) == qty_sum
                && amount(sget(p, "remainingOwnedCost").unwrap_or(""))? == cost_sum,
            "CURRENT_FIFO_COST",
        )?;
        need(
            !get(get(state, "originalInventory"), &ex).is_null()
                || get(state, "orders").as_array().is_some_and(|os| {
                    os.iter().any(|o| {
                        sget(get(o, "body"), "action") == Some("buy")
                            && sget(get(o, "body"), "exchangeId") == Some(ex.as_str())
                            && !get(o, "proof").is_null()
                    })
                }),
            "NO_FOREIGN_CURRENT_INVENTORY",
        )?;
        realized += amount(sget(p, "cumulativeRealizedPnl").unwrap_or(""))?;
    }
    let record = crate::baal_neutral_owned_counterpart_pilot_v2::project_risk(state)?;
    own_record(&record)?;
    need(
        sget(&record, "allocationHash")
            == Some(&*payload_hash(get(get(seed, "state"), "allocation"))?)
            && sget(&record, "seedHash") == Some(&*payload_hash(seed)?),
        "SEEDED_RESERVATION",
    )?;
    let unknown = get(state, "orders").as_array().is_some_and(|os| {
        os.iter().any(|o| {
            get(o, "proof").is_null()
                && sget(o, "phase") != Some("abandoned")
                && (["uncertain", "semantic_mismatch"]
                    .contains(&sget(o, "phase").unwrap_or(""))
                    || get(o, "semanticMismatch") == &V::Bool(true)
                    || (get(o, "orderId").is_null()
                        && get(o, "fetchStarts")
                            .as_array()
                            .is_some_and(|x| !x.is_empty())))
        })
    });
    let orders: Vec<V> = get(state, "orders")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|o| {
            json!({
                "intentId": get(o,"intentId"),
                "bodyHash": get(o,"bodyHash"),
                "bodyJson": get(o,"bodyJson"),
                "phase": get(o,"phase"),
                "orderId": get(o,"orderId"),
                "proofHash": if get(get(o,"proof"),"verificationHash").is_null() {V::Null} else {get(get(o,"proof"),"verificationHash").clone()},
                "fetchStarts": if get(o,"fetchStarts").is_null() {V::Array(vec![])} else {get(o,"fetchStarts").clone()},
            })
        })
        .collect();
    Ok(json!({
        "record": record,
        "spent": money(spent),
        "realizedPnl": money(realized),
        "currentInventoryHash": payload_hash(get(state,"currentInventory"))?,
        "assignmentHistoryHash": payload_hash(get(state,"assignments"))?,
        "pairRetirementsHash": payload_hash(get(state,"pairRetirements"))?,
        "orders": orders,
        "unknown": unknown,
    }))
}

pub struct OwnedHedgeBudgetDeps {
    pub assert_account_lease: Rc<dyn Fn() -> Result<V, String>>,
    pub assert_financial_owner_lease: Rc<dyn Fn() -> Result<V, String>>,
    pub get_coordinator_source: Rc<dyn Fn() -> Result<V, String>>,
    pub observe_global_risk: Rc<dyn Fn() -> Result<V, String>>,
    pub can_operate: Rc<dyn Fn() -> bool>,
    pub now: Rc<dyn Fn() -> f64>,
}

struct SourceCache {
    key: String,
    value: V,
}

struct BudgetInner {
    scope: V,
    allocation: V,
    allocation_hash: String,
    seed: V,
    seed_hash: String,
    activation_hash: String,
    migration: V,
    migration_hash: String,
    coordinator_code_identity_hash: String,
    activation: V,
    budget_installed_hash: String,
    actor_file: PathBuf,
    file: PathBuf,
    legacy_file: PathBuf,
    legacy_bytes: Vec<u8>,
    legacy_identity: String,
    workspace: PathBuf,
    lease: Option<OwnerLease>,
    journal: Option<Journal>,
    state: Option<V>,
    closed: bool,
    failed: bool,
    size: u64,
    file_hash: String,
    file_identity: String,
    source_cache: Option<SourceCache>,
    cache_hits: u64,
    cache_misses: u64,
    initial_financial_lease_hash: Option<String>,
    deps: OwnedHedgeBudgetDeps,
}

pub struct BaalOwnedHedgeSharedBudgetV2 {
    inner: Rc<RefCell<BudgetInner>>,
}

impl BaalOwnedHedgeSharedBudgetV2 {
    fn basic(&self) -> Result<V, String> {
        let g = self.inner.borrow();
        need(!g.closed && !g.failed && (g.deps.can_operate)(), "STOPPED")?;
        if let Some(l) = &g.lease {
            l.assert_ownership().map_err(|e| e.message)?;
        }
        let a = (g.deps.assert_account_lease)()?;
        let f = (g.deps.assert_financial_owner_lease)()?;
        need(
            sget(&a, "ownerId") == sget(&g.scope, "ownerId")
                && sget(&a, "runId") == sget(&g.scope, "runId")
                && sget(&a, "tournamentId") == sget(&g.scope, "tournamentId")
                && sget(&a, "leaseId").is_some_and(|s| !s.is_empty()),
            "ACCOUNT_LEASE",
        )?;
        need(
            sget(&f, "ownerId") == sget(&g.scope, "ownerId")
                && sget(&f, "runId") == sget(&g.scope, "runId")
                && sget(&f, "tournamentId") == sget(&g.scope, "tournamentId")
                && sget(&f, "leaseId").is_some_and(|s| !s.is_empty())
                && get(&f, "implementationEpoch") == get(&g.migration, "implementationEpoch"),
            "FINANCIAL_OWNER_LEASE",
        )?;
        let financial_hash = payload_hash(&f)?;
        match &g.initial_financial_lease_hash {
            Some(h) => need(financial_hash == *h, "FINANCIAL_OWNER_CHANGED")?,
            None => {
                drop(g);
                self.inner.borrow_mut().initial_financial_lease_hash = Some(financial_hash.clone());
            }
        }
        Ok(json!({
            "accountLeaseHash": payload_hash(&a)?,
            "financialOwnerLeaseHash": financial_hash,
        }))
    }
    fn observed(&self) -> Result<V, String> {
        let g = self.inner.borrow();
        let gv = (g.deps.observe_global_risk)()?;
        let now = (g.deps.now)();
        need(
            sget(&gv, "ownerId") == sget(&g.scope, "ownerId")
                && sget(&gv, "runId") == sget(&g.scope, "runId")
                && sget(&gv, "tournamentId") == sget(&g.scope, "tournamentId")
                && get(&gv, "complete") == &V::Bool(true)
                && get(&gv, "eventScopeComplete") == &V::Bool(true)
                && get(&gv, "soleCollector") == &V::Bool(true)
                && get(&gv, "stopped") == &V::Bool(false)
                && get(&gv, "unknownAcceptance").is_boolean()
                && hex(get(&gv, "sourceHash"))
                && get(&gv, "unresolvedIntentIds")
                    .as_array()
                    .is_some_and(|ids| ids.iter().all(|id| id.is_string()))
                && instant(sget(&gv, "at").unwrap_or(""))? <= now as i64
                && now as i64 - instant(sget(&gv, "at").unwrap_or(""))? <= 20000,
            "FRESH_GLOBAL_OBSERVATION",
        )?;
        Ok(gv)
    }
    fn source(&self) -> Result<V, String> {
        self.basic()?;
        let (
            workspace,
            coordinator_code_identity_hash,
            legacy_file,
            legacy_identity,
            legacy_bytes,
            seed,
            seed_hash,
            activation,
            activation_hash,
            migration,
            migration_hash,
            actor_file,
        ) = {
            let g = self.inner.borrow();
            (
                g.workspace.clone(),
                g.coordinator_code_identity_hash.clone(),
                g.legacy_file.clone(),
                g.legacy_identity.clone(),
                g.legacy_bytes.clone(),
                g.seed.clone(),
                g.seed_hash.clone(),
                g.activation.clone(),
                g.activation_hash.clone(),
                g.migration.clone(),
                g.migration_hash.clone(),
                g.actor_file.clone(),
            )
        };
        need(
            payload_hash(&crate::baal_neutral_owned_counterpart_pilot_v2::live_coordinator_identity(
                &workspace,
            )?)? == coordinator_code_identity_hash,
            "COORDINATOR_CODE_CHANGED",
        )?;
        let old_before = fs::metadata(&legacy_file).map_err(|e| e.to_string())?;
        let old = fs::read(&legacy_file).map_err(|e| e.to_string())?;
        let old_after = fs::metadata(&legacy_file).map_err(|e| e.to_string())?;
        need(
            stamp(&old_before)? == legacy_identity
                && stamp(&old_after)? == legacy_identity
                && old_after.len() == old.len() as u64
                && old.len() as u64 == get(get(&seed, "anchor"), "bytes").as_u64().unwrap_or(0)
                && old == legacy_bytes,
            "LEGACY_NOT_SEALED",
        )?;
        let hint = (self.inner.borrow().deps.get_coordinator_source)()?;
        need(
            sget(&hint, "journalFile")
                .map(|p| {
                    Path::new(p)
                        .canonicalize()
                        .or_else(|_| Ok::<PathBuf, std::io::Error>(PathBuf::from(p)))
                        .unwrap_or_default()
                })
                .map(|p| p == actor_file.canonicalize().unwrap_or(actor_file.clone()))
                .unwrap_or(false),
            "ACTUAL_SOURCE_PATH",
        )?;
        let before = fs::metadata(&actor_file).map_err(|e| e.to_string())?;
        let bytes = fs::read(&actor_file).map_err(|e| e.to_string())?;
        let hash = sha_bytes(&bytes);
        need(
            !bytes.is_empty() && bytes.len() <= 128 * 1024 * 1024 && bytes.last() == Some(&10),
            "COMPLETE_SOURCE",
        )?;
        {
            let g = self.inner.borrow();
            if let Some(state) = &g.state {
                let anchor = get(state, "actorAnchor");
                let abytes = get(anchor, "bytes").as_u64().unwrap_or(u64::MAX) as usize;
                need(
                    bytes.len() >= abytes && sha_bytes(&bytes[..abytes]) == sget(anchor, "sha256").unwrap_or(""),
                    "SOURCE_PREFIX_RESET",
                )?;
            }
        }
        let key = payload_hash(&json!({
            "hash": hash,
            "size": bytes.len(),
            "identity": stamp(&before)?,
            "seedHash": seed_hash,
            "activationHash": activation_hash,
            "migrationHash": migration_hash,
            "coordinatorCodeIdentityHash": coordinator_code_identity_hash,
        }))?;
        let cached = {
            let g = self.inner.borrow();
            g.source_cache
                .as_ref()
                .filter(|c| c.key == key)
                .map(|c| c.value.clone())
        };
        let (result, p): (V, V) = if let Some(ref cached_v) = cached {
            self.inner.borrow_mut().cache_hits += 1;
            (
                get(cached_v, "result").clone(),
                get(cached_v, "p").clone(),
            )
        } else {
            self.inner.borrow_mut().cache_misses += 1;
            let result = crate::baal_neutral_owned_counterpart_pilot_v2::replay_live_hedge_journal(
                &json!({
                    "bodyText": String::from_utf8_lossy(&bytes),
                    "seed": seed,
                    "activation": activation,
                    "migration": migration,
                    "expectedCodeIdentity": crate::baal_neutral_owned_counterpart_pilot_v2::live_coordinator_identity(&workspace)?,
                }),
            )?;
            need(
                get(&result, "economicReplayPerformed") == &V::Bool(true),
                "ECONOMIC_REPLAY_REQUIRED",
            )?;
            let p = projected(get(&result, "state"), &seed)?;
            (result, p)
        };
        need(
            get(get(&result, "anchor"), "bytes").as_u64().unwrap_or(0) == bytes.len() as u64
                && sget(get(&result, "anchor"), "sha256") == Some(hash.as_str())
                && stamp(&fs::metadata(&actor_file).map_err(|e| e.to_string())?)?
                    == stamp(&before)?
                && fs::metadata(&actor_file).map_err(|e| e.to_string())?.len()
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
                == get(&migration, "implementationEpoch"),
            "EPOCH",
        )?;
        {
            let g = self.inner.borrow();
            if let Some(state) = &g.state {
                need(
                    same(get(&result, "rootPrefix"), get(state, "coordinatorRootPrefix"))?
                        && num_amount(get(&p, "spent"))?
                            >= num_amount(get(get(state, "projection"), "spent"))?
                        && get(&p, "orders").as_array().map(|x| x.len()).unwrap_or(0)
                            >= get(get(state, "projection"), "orders")
                                .as_array()
                                .map(|x| x.len())
                                .unwrap_or(0),
                    "EXPENSE_OR_ROOT_RESET",
                )?;
                for o in get(get(state, "projection"), "orders")
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                {
                    let n = get(&p, "orders").as_array().and_then(|ns| {
                        ns.iter()
                            .find(|n| get(n, "intentId") == get(&o, "intentId"))
                            .cloned()
                    });
                    need(
                        n.as_ref().is_some_and(|n| {
                            if get(n, "bodyHash") != get(&o, "bodyHash")
                                || get(n, "bodyJson") != get(&o, "bodyJson")
                                || (!get(&o, "orderId").is_null()
                                    && get(n, "orderId") != get(&o, "orderId"))
                                || (!get(&o, "proofHash").is_null()
                                    && get(n, "proofHash") != get(&o, "proofHash"))
                            {
                                return false;
                            }
                            for f in get(&o, "fetchStarts")
                                .as_array()
                                .cloned()
                                .unwrap_or_default()
                            {
                                let mut ok = false;
                                for g2 in get(n, "fetchStarts")
                                    .as_array()
                                    .cloned()
                                    .unwrap_or_default()
                                {
                                    if same(&f, &g2).unwrap_or(false) {
                                        ok = true;
                                        break;
                                    }
                                }
                                if !ok {
                                    return false;
                                }
                            }
                            true
                        }),
                        "ORIGINAL_HEDGE_RESET",
                    )?;
                }
            }
        }
        if cached.is_none() {
            let value = json!({
                "result": {
                    "anchor": get(&result,"anchor"),
                    "rootPrefix": get(&result,"rootPrefix"),
                    "economicReplayPerformed": true,
                },
                "p": p,
            });
            self.inner.borrow_mut().source_cache = Some(SourceCache { key, value });
        }
        Ok(json!({
            "record": get(&p,"record"),
            "spent": get(&p,"spent"),
            "realizedPnl": get(&p,"realizedPnl"),
            "currentInventoryHash": get(&p,"currentInventoryHash"),
            "assignmentHistoryHash": get(&p,"assignmentHistoryHash"),
            "pairRetirementsHash": get(&p,"pairRetirementsHash"),
            "orders": get(&p,"orders"),
            "unknown": get(&p,"unknown"),
            "anchor": get(&result,"anchor"),
            "opening": get(&result,"rootPrefix"),
        }))
    }
    fn validate_snapshot(&self, next: &V) -> Result<(), String> {
        let (actor_file, workspace, coordinator_code_identity_hash, seed, seed_hash, activation, activation_hash, migration, migration_hash, allocation, allocation_hash, budget_installed_hash, scope) = {
            let g = self.inner.borrow();
            (
                g.actor_file.clone(),
                g.workspace.clone(),
                g.coordinator_code_identity_hash.clone(),
                g.seed.clone(),
                g.seed_hash.clone(),
                g.activation.clone(),
                g.activation_hash.clone(),
                g.migration.clone(),
                g.migration_hash.clone(),
                g.allocation.clone(),
                g.allocation_hash.clone(),
                g.budget_installed_hash.clone(),
                g.scope.clone(),
            )
        };
        let before = fs::metadata(&actor_file).map_err(|e| e.to_string())?;
        let all = fs::read(&actor_file).map_err(|e| e.to_string())?;
        let a = get(next, "actorAnchor");
        need(
            is_safe_int(get(a, "bytes"))
                && get(a, "bytes").as_i64().unwrap_or(0) > 0
                && get(a, "bytes").as_u64().unwrap_or(u64::MAX) <= all.len() as u64,
            "HISTORICAL_SOURCE_BOUND",
        )?;
        let prefix = &all[..get(a, "bytes").as_u64().unwrap_or(0) as usize];
        let hash = sha_bytes(prefix);
        need(
            prefix.last() == Some(&10) && hash == sget(a, "sha256").unwrap_or(""),
            "HISTORICAL_SOURCE_HASH",
        )?;
        let code = crate::baal_neutral_owned_counterpart_pilot_v2::live_coordinator_identity(
            &workspace,
        )?;
        need(
            payload_hash(&code)? == coordinator_code_identity_hash,
            "COORDINATOR_CODE_CHANGED",
        )?;
        let key = payload_hash(&json!({
            "hash": hash,
            "size": prefix.len(),
            "identity": stamp(&before)?,
            "seedHash": seed_hash,
            "activationHash": activation_hash,
            "migrationHash": migration_hash,
            "coordinatorCodeIdentityHash": coordinator_code_identity_hash,
        }))?;
        let use_cache = {
            let g = self.inner.borrow();
            all.len() == prefix.len()
                && g.source_cache
                    .as_ref()
                    .is_some_and(|c| c.key == key && {
                        same(get(get(&c.value, "result"), "anchor"), a).unwrap_or(false)
                    })
        };
        let (replay, actual): (V, V) = if use_cache {
            let g = self.inner.borrow();
            let c = g.source_cache.as_ref().unwrap();
            (
                get(&c.value, "result").clone(),
                get(&c.value, "p").clone(),
            )
        } else {
            let replay = crate::baal_neutral_owned_counterpart_pilot_v2::replay_live_hedge_journal(
                &json!({
                    "bodyText": String::from_utf8_lossy(prefix),
                    "seed": seed,
                    "activation": activation,
                    "migration": migration,
                    "expectedCodeIdentity": code,
                }),
            )?;
            (replay.clone(), projected(get(&replay, "state"), &seed)?)
        };
        let after = fs::metadata(&actor_file).map_err(|e| e.to_string())?;
        need(
            stamp(&before)? == stamp(&after)? && after.len() == all.len() as u64,
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
                && sget(next, "mode") == Some("source_verified_owned_hedge_budget_v2")
                && sget(next, "seedHash") == Some(seed_hash.as_str())
                && sget(next, "migrationHash") == Some(migration_hash.as_str())
                && sget(next, "activationHash") == Some(activation_hash.as_str())
                && sget(next, "coordinatorCodeIdentityHash")
                    == Some(coordinator_code_identity_hash.as_str())
                && sget(next, "budgetInstalledHash") == Some(budget_installed_hash.as_str())
                && sget(next, "allocationHash") == Some(allocation_hash.as_str())
                && same(get(next, "scope"), &scope)?
                && same(get(next, "legacyAnchor"), get(&seed, "anchor"))?
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
                    &allocation,
                    false,
                )?,
                get(next, "risk"),
            )? && get(next, "cumulativeActualExpenses") == get(get(next, "projection"), "spent"),
            "BUDGET_ECONOMICS",
        )?;
        {
            let g = self.inner.borrow();
            if let Some(state) = &g.state {
                need(
                    get(a, "bytes").as_u64().unwrap_or(0)
                        >= get(get(state, "actorAnchor"), "bytes").as_u64().unwrap_or(0)
                        && get(a, "sequence").as_i64().unwrap_or(0)
                            >= get(get(state, "actorAnchor"), "sequence").as_i64().unwrap_or(0)
                        && num_amount(get(get(next, "projection"), "spent"))?
                            >= num_amount(get(get(state, "projection"), "spent"))?,
                    "BUDGET_RESET",
                )?;
            }
        }
        Ok(())
    }
    fn append(&self, next: &V) -> Result<(), String> {
        self.basic()?;
        let (file, old_size, old_hash, old_identity) = {
            let g = self.inner.borrow();
            (g.file.clone(), g.size, g.file_hash.clone(), g.file_identity.clone())
        };
        let old = fs::read(&file).map_err(|e| e.to_string())?;
        let metadata = fs::metadata(&file).map_err(|e| e.to_string())?;
        need(
            stamp(&metadata)? == old_identity
                && old.len() as u64 == old_size
                && sha_bytes(&old) == old_hash,
            "BUDGET_FILE_CHANGED",
        )?;
        self.validate_snapshot(next)?;
        let result = (|| -> Result<(), String> {
            {
                let mut g = self.inner.borrow_mut();
                let event = json!({
                    "kind": "installed_source_verified_hedge_reserve",
                    "seedHash": g.seed_hash,
                    "migrationHash": g.migration_hash,
                    "activationHash": g.activation_hash,
                });
                g.journal
                    .as_mut()
                    .unwrap()
                    .commit(next, &event)
                    .map_err(|e| e.to_string())?;
            }
            let after = fs::read(&file).map_err(|e| e.to_string())?;
            need(
                after[..old_size as usize] == old[..],
                "BUDGET_PREFIX_CHANGED",
            )?;
            {
                let mut g = self.inner.borrow_mut();
                g.state = Some(next.clone());
                g.size = after.len() as u64;
                g.file_hash = sha_bytes(&after);
            }
            self.basic()?;
            Ok(())
        })();
        match result {
            Ok(()) => Ok(()),
            Err(e) => {
                self.inner.borrow_mut().failed = true;
                Err(e)
            }
        }
    }
    /// installFundingReservation(record)
    pub fn install_funding_reservation(&self, record: &V) -> Result<V, String> {
        let leases = self.basic()?;
        let s = self.source()?;
        need(same(record, get(&s, "record"))?, "SOURCE_RESERVATION_REQUIRED")?;
        {
            let g = self.inner.borrow();
            if g.state.is_none() {
                need(
                    get(&s, "orders").as_array().map(|x| x.is_empty()).unwrap_or(false)
                        && get(&s, "unknown") != &V::Bool(true)
                        && get(get(&s, "record"), "unresolvedIntentIds")
                            .as_array()
                            .map(|x| x.is_empty())
                            .unwrap_or(false),
                    "MIGRATE_BEFORE_NEW_HEDGE",
                )?;
            }
        }
        let g = self.observed()?;
        let allocation = self.inner.borrow().allocation.clone();
        let risk = combined_risk(&g, get(&s, "record"), &allocation, false)?;
        let other: Vec<String> = get(&g, "unresolvedIntentIds")
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|x| x.as_str().map(|s| s.to_string()))
            .filter(|id| {
                !get(get(&s, "record"), "unresolvedIntentIds")
                    .as_array()
                    .map(|ids| ids.iter().any(|x| x.as_str() == Some(id.as_str())))
                    .unwrap_or(false)
            })
            .collect();
        need(other.is_empty(), "FOREIGN_UNKNOWN")?;
        let now = (self.inner.borrow().deps.now)();
        let at = baal_service_runtime::contract::ms_to_iso(now);
        let (scope, allocation_hash, budget_installed_hash, coordinator_code_identity_hash, seed_hash, migration_hash, activation_hash, legacy_anchor) = {
            let g = self.inner.borrow();
            (
                g.scope.clone(),
                g.allocation_hash.clone(),
                g.budget_installed_hash.clone(),
                g.coordinator_code_identity_hash.clone(),
                g.seed_hash.clone(),
                g.migration_hash.clone(),
                g.activation_hash.clone(),
                get(&g.seed, "anchor").clone(),
            )
        };
        let mut next = Map::new();
        next.insert("schemaVersion".into(), json!(1));
        next.insert(
            "mode".into(),
            json!("source_verified_owned_hedge_budget_v2"),
        );
        next.insert("scope".into(), scope);
        next.insert("allocationHash".into(), json!(allocation_hash));
        next.insert("budgetInstalledHash".into(), json!(budget_installed_hash));
        next.insert(
            "coordinatorCodeIdentityHash".into(),
            json!(coordinator_code_identity_hash),
        );
        next.insert("seedHash".into(), json!(seed_hash));
        next.insert("migrationHash".into(), json!(migration_hash));
        next.insert("activationHash".into(), json!(activation_hash));
        next.insert("legacyAnchor".into(), legacy_anchor);
        next.insert("coordinatorRootPrefix".into(), get(&s, "opening").clone());
        next.insert("actorAnchor".into(), get(&s, "anchor").clone());
        next.insert(
            "projection".into(),
            json!({
                "record": get(&s,"record"),
                "orders": get(&s,"orders"),
                "spent": get(&s,"spent"),
                "realizedPnl": get(&s,"realizedPnl"),
                "currentInventoryHash": get(&s,"currentInventoryHash"),
                "assignmentHistoryHash": get(&s,"assignmentHistoryHash"),
                "pairRetirementsHash": get(&s,"pairRetirementsHash"),
                "unknown": get(&s,"unknown"),
            }),
        );
        next.insert("risk".into(), risk);
        next.insert("externalObservation".into(), g);
        if let Some(m) = leases.as_object() {
            for (k, v) in m {
                next.insert(k.clone(), v.clone());
            }
        }
        next.insert("cumulativeActualExpenses".into(), get(&s, "spent").clone());
        next.insert("at".into(), json!(at));
        self.append(&V::Object(next))?;
        let prev = self
            .inner
            .borrow()
            .journal
            .as_ref()
            .map(|j| j.previous.clone())
            .unwrap_or_default();
        Ok(json!({
            "reservationHash": get(get(&s,"record"),"reservationHash"),
            "allocationHash": allocation_hash,
            "seedHash": seed_hash,
            "budgetInstalledHash": budget_installed_hash,
            "globalLedgerHash": prev,
            "confirmedAt": at,
        }))
    }
    /// globalRisk()
    pub fn global_risk(&self) -> Result<V, String> {
        self.basic()?;
        {
            let g = self.inner.borrow();
            need(g.state.is_some(), "NOT_INSTALLED")?;
        }
        let s = self.source()?;
        let g = self.observed()?;
        {
            let inner = self.inner.borrow();
            let state = inner.state.as_ref().unwrap();
            need(
                get(get(&s, "record"), "reservationHash")
                    == get(get(get(state, "projection"), "record"), "reservationHash"),
                "CURRENT_RESERVE_NOT_INSTALLED",
            )?;
        }
        let allocation = self.inner.borrow().allocation.clone();
        let r = combined_risk(&g, get(&s, "record"), &allocation, true)?;
        let (
            installed_cash_lower,
            allocation_hash,
            budget_installed_hash,
            seed_hash,
            epoch,
            prev,
        ) = {
            let inner = self.inner.borrow();
            let state = inner.state.as_ref().unwrap();
            (
                get(get(state, "externalObservation"), "cashLower").clone(),
                inner.allocation_hash.clone(),
                inner.budget_installed_hash.clone(),
                inner.seed_hash.clone(),
                get(&inner.migration, "implementationEpoch").clone(),
                inner.journal.as_ref().map(|j| j.previous.clone()).unwrap_or_default(),
            )
        };
        let mut out = g.as_object().cloned().unwrap_or_default();
        if let Some(m) = r.as_object() {
            for (k, v) in m {
                out.insert(k.clone(), v.clone());
            }
        }
        out.insert("installedCashLower".into(), installed_cash_lower);
        out.insert("allocationHash".into(), json!(allocation_hash));
        out.insert("budgetInstalledHash".into(), json!(budget_installed_hash));
        out.insert("seedHash".into(), json!(seed_hash));
        out.insert("implementationEpoch".into(), epoch);
        out.insert(
            "controllerReservationHash".into(),
            get(get(&s, "record"), "reservationHash").clone(),
        );
        out.insert("globalLedgerHash".into(), json!(prev.clone()));
        out.insert(
            "sourceHash".into(),
            json!(payload_hash(&json!({
                "observation": get(&g,"sourceHash"),
                "actor": get(&s,"anchor"),
                "reservation": get(get(&s,"record"),"reservationHash"),
                "globalLedgerHash": prev,
            }))?),
        );
        let any_unknown_acceptance = ["owned", "reservations", "loans"].iter().any(|k| {
            get(&r, k)
                .as_array()
                .map(|xs| xs.iter().any(|v| get(v, "unknownAcceptance") == &V::Bool(true)))
                .unwrap_or(false)
        });
        let s_unknown = get(&s, "unknown") == &V::Bool(true);
        let g_unknown = get(&g, "unknownAcceptance") == &V::Bool(true);
        out.insert(
            "unknownAcceptance".into(),
            json!(s_unknown || g_unknown || any_unknown_acceptance),
        );
        let mut seen = HashSet::new();
        let mut merged_ids: Vec<V> = Vec::new();
        for arr in [
            get(get(&s, "record"), "unresolvedIntentIds").clone(),
            get(&g, "unresolvedIntentIds").clone(),
        ] {
            for id in arr.as_array().cloned().unwrap_or_default() {
                if seen.insert(serde_json::to_string(&id).unwrap_or_default()) {
                    merged_ids.push(id);
                }
            }
        }
        out.insert("unresolvedIntentIds".into(), V::Array(merged_ids));
        out.insert("executionAuthority".into(), json!(false));
        out.insert("oldWriterHandbackAllowed".into(), json!(false));
        Ok(V::Object(out))
    }
    pub fn get_global_risk(&self) -> Result<V, String> {
        self.global_risk()
    }
    pub fn status(&self) -> V {
        let g = self.inner.borrow();
        let mut unknown = g
            .state
            .as_ref()
            .map(|s| get(get(s, "projection"), "unknown") == &V::Bool(true))
            .unwrap_or(false);
        let has_state = g.state.is_some();
        let closed = g.closed;
        drop(g);
        if has_state && !closed {
            match self.source() {
                Ok(s) => unknown = unknown || get(&s, "unknown") == &V::Bool(true),
                Err(_) => unknown = true,
            }
        }
        let g = self.inner.borrow();
        let realized_default: i128 = get(get(&g.seed, "state"), "inventory")
            .as_object()
            .cloned()
            .unwrap_or_default()
            .values()
            .try_fold(0i128, |n, x| {
                amount(sget(get(x, "proof"), "cumulativeRealizedPnl").unwrap_or(""))
                    .map(|v| n + v)
            })
            .unwrap_or(0);
        let spent_default: i128 = get(get(&g.seed, "state"), "orders")
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .try_fold(0i128, |n, o| {
                if sget(get(o, "body"), "action") == Some("buy") {
                    amount(
                        sget(get(o, "proof"), "scopedPrincipalCost")
                            .unwrap_or("0.000000"),
                    )
                    .map(|v| n + v)
                } else {
                    Ok(n)
                }
            })
            .unwrap_or(0);
        json!({
            "mode": "source_verified_owned_hedge_budget_v2",
            "claimed": has_state,
            "allocationHash": g.allocation_hash,
            "budgetInstalledHash": g.budget_installed_hash,
            "seedHash": g.seed_hash,
            "migrationHash": g.migration_hash,
            "coordinatorCodeIdentityHash": g.coordinator_code_identity_hash,
            "sequence": g.journal.as_ref().map(|j| j.sequence).unwrap_or(0),
            "globalLedgerHash": g.journal.as_ref().map(|j| j.previous.clone()).unwrap_or_default(),
            "controllerReservationHash": g.state.as_ref().map(|s| get(get(get(s,"projection"),"record"),"reservationHash").clone()).unwrap_or(V::Null),
            "realizedPnl": g.state.as_ref().map(|s| get(get(s,"projection"),"realizedPnl").clone()).unwrap_or(json!(money(realized_default))),
            "currentInventoryHash": g.state.as_ref().map(|s| get(get(s,"projection"),"currentInventoryHash").clone()).unwrap_or(json!(payload_hash(get(get(&g.seed,"state"),"inventory")).unwrap_or_default())),
            "pairRetirementsHash": g.state.as_ref().map(|s| get(get(s,"projection"),"pairRetirementsHash").clone()).unwrap_or(json!(payload_hash(&json!([])).unwrap_or_default())),
            "cumulativeActualExpenses": g.state.as_ref().map(|s| get(s,"cumulativeActualExpenses").clone()).unwrap_or(json!(money(spent_default))),
            "risk": g.state.as_ref().map(|s| get(s,"risk").clone()).unwrap_or(V::Null),
            "unknownAcceptance": unknown,
            "actorAnchor": g.state.as_ref().map(|s| get(s,"actorAnchor").clone()).unwrap_or(V::Null),
            "legacyAnchor": get(&g.seed,"anchor"),
            "cacheHits": g.cache_hits,
            "cacheMisses": g.cache_misses,
            "failed": g.failed,
            "closed": g.closed,
            "executionAuthority": false,
            "oldWriterHandbackAllowed": false,
            "additionalApiClients": 0,
        })
    }
    pub fn close(&self) -> Result<(), String> {
        let mut g = self.inner.borrow_mut();
        if g.closed {
            return Ok(());
        }
        g.closed = true;
        if let Some(l) = g.lease.as_mut() {
            l.release().map_err(|e| e.message)?;
        }
        Ok(())
    }
}

/// openBaalOwnedHedgeSharedBudgetV2(options)
#[allow(clippy::too_many_arguments)]
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
) -> Result<BaalOwnedHedgeSharedBudgetV2, String> {
    need(
        dir.is_absolute()
            && coordinator_dir.is_absolute()
            && workspace.is_absolute()
            && legacy_file.is_absolute()
            && dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf())
                != coordinator_dir
                    .canonicalize()
                    .unwrap_or_else(|_| coordinator_dir.to_path_buf())
            && hex(&json!(coordinator_code_identity_hash)),
        "DEPENDENCIES",
    )?;
    let legacy_bytes = fs::read(legacy_file).map_err(|e| e.to_string())?;
    let legacy_identity = stamp(&fs::metadata(legacy_file).map_err(|e| e.to_string())?)?;
    let seed = crate::baal_scaled_controller_legacy_replay_v1::replay_baal_scaled_controller_v1(
        &json!({
            "bodyText": String::from_utf8_lossy(&legacy_bytes),
            "expectedCodeIdentity": legacy_code_identity,
            "expectedScope": legacy_scope,
            "expectedAllocation": legacy_allocation,
        }),
    )?;
    let seed_hash = payload_hash(&seed)?;
    let scope = get(get(&seed, "state"), "scope").clone();
    let mut allocation = Map::new();
    allocation.insert("maximumOrderCost".into(), json!("25.000000"));
    allocation.insert("maximumMarketCost".into(), json!("100.000000"));
    allocation.insert("maximumEventCost".into(), json!("200.000000"));
    allocation.insert("maximumActiveMarkets".into(), json!(20));
    if let Some(m) = get(get(&seed, "state"), "allocation").as_object() {
        for (k, v) in m {
            allocation.insert(k.clone(), v.clone());
        }
    }
    let allocation = V::Object(allocation);
    let allocation_hash = payload_hash(get(get(&seed, "state"), "allocation"))?;
    let migration_hash = payload_hash(migration)?;
    let activation_hash = payload_hash(activation)?;
    need(
        sget(&allocation, "ownerId") == Some("Codex")
            && sget(&allocation, "capital") == Some("1000.000000")
            && hex(get(&allocation, "humanAllocationSourceHash"))
            && num_amount(get(&allocation, "maximumOrderCost"))? > 0
            && num_amount(get(&allocation, "maximumOrderCost"))? <= 25_000_000
            && num_amount(get(&allocation, "maximumMarketCost"))? > 0
            && num_amount(get(&allocation, "maximumMarketCost"))? <= 100_000_000
            && num_amount(get(&allocation, "maximumEventCost"))? > 0
            && num_amount(get(&allocation, "maximumEventCost"))? <= 200_000_000
            && is_safe_int(get(&allocation, "maximumActiveMarkets"))
            && get(&allocation, "maximumActiveMarkets").as_i64().unwrap_or(0) > 0
            && get(&allocation, "maximumActiveMarkets").as_i64().unwrap_or(0) <= 20,
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
    let inner = Rc::new(RefCell::new(BudgetInner {
        scope,
        allocation,
        allocation_hash,
        seed,
        seed_hash,
        activation: activation.clone(),
        activation_hash,
        migration: migration.clone(),
        migration_hash,
        coordinator_code_identity_hash: coordinator_code_identity_hash.to_string(),
        budget_installed_hash,
        actor_file,
        file: file.clone(),
        legacy_file: legacy_file.to_path_buf(),
        legacy_bytes,
        legacy_identity,
        workspace: workspace.to_path_buf(),
        lease: None,
        journal: None,
        state: None,
        closed: false,
        failed: false,
        size: 0,
        file_hash: String::new(),
        file_identity: String::new(),
        source_cache: None,
        cache_hits: 0,
        cache_misses: 0,
        initial_financial_lease_hash: None,
        deps,
    }));
    let this = BaalOwnedHedgeSharedBudgetV2 {
        inner: inner.clone(),
    };
    let result = (|| -> Result<(), String> {
        this.basic()?;
        let (owner, run) = {
            let g = inner.borrow();
            (
                sget(&g.scope, "ownerId").unwrap_or("").to_string(),
                sget(&g.scope, "runId").unwrap_or("").to_string(),
            )
        };
        let lease = crate::ledger::acquire_owner_lease(
            dir,
            &owner,
            &run,
            "source-verified-owned-hedge-budget-v2",
            (inner.borrow().deps.now)(),
        )
        .map_err(|e| e.message)?;
        inner.borrow_mut().lease = Some(lease);
        if !file.exists() {
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&file)
                .map_err(|e| e.to_string())?;
        }
        let bytes = fs::read(&file).map_err(|e| e.to_string())?;
        need(
            bytes.len() <= 128 * 1024 * 1024
                && (bytes.is_empty() || bytes.last() == Some(&10)),
            "BUDGET_BOUND",
        )?;
        inner.borrow_mut().journal = Some(Journal::open(&file).map_err(|e| e.to_string())?);
        for line in String::from_utf8_lossy(&bytes)
            .split('\n')
            .filter(|l| !l.is_empty())
        {
            let row: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
            need(
                sget(get(&row, "event"), "kind")
                    == Some("installed_source_verified_hedge_reserve")
                    && sget(get(&row, "event"), "seedHash")
                        == Some(inner.borrow().seed_hash.as_str())
                    && sget(get(&row, "event"), "migrationHash")
                        == Some(inner.borrow().migration_hash.as_str())
                    && sget(get(&row, "event"), "activationHash")
                        == Some(inner.borrow().activation_hash.as_str()),
                "BUDGET_EVENT",
            )?;
            this.validate_snapshot(get(&row, "state"))?;
            inner.borrow_mut().state = Some(get(&row, "state").clone());
        }
        {
            let mut g = inner.borrow_mut();
            g.size = bytes.len() as u64;
            g.file_hash = sha_bytes(&bytes);
            g.file_identity = stamp(&fs::metadata(&file).map_err(|e| e.to_string())?)?;
        }
        if inner.borrow().state.is_some() {
            this.source()?;
        }
        Ok(())
    })();
    match result {
        Ok(()) => Ok(this),
        Err(e) => {
            let _ = inner
                .borrow_mut()
                .lease
                .as_mut()
                .map(|l| l.release());
            Err(e)
        }
    }
}
