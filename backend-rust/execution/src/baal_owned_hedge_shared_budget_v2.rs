// execution/baal-owned-hedge-shared-budget-v2.mjs — source-verified budget for
// the separately migrated owned-counterpart writer. No network client, trading
// window or automatic legacy-writer handback.
//
// Port notes: `stamp()` (ino:birthtimeMs) is used for file-identity pins on
// unix metadata; `sourceCache` memoizes replay by exact-byte key. Errors carry
// OWNED_HEDGE_BUDGET_* codes via bare Strings (JS used Object.assign Error
// .code — same surfaced string). `e.fatal`/`e.durableRecordingFailed` flags
// cannot ride on String errors (noted divergence, same as siblings).
use crate::baal_neutral_owned_counterpart_pilot_v2::{
    live_coordinator_identity, project_risk, replay_live_hedge_journal,
};
use crate::baal_scaled_controller_legacy_replay_v1::replay_baal_scaled_controller_v1;
use crate::core::payload_hash;
use baal_service_runtime::fixed::{amount, instant, money};
use baal_service_runtime::journal::Journal;
use baal_service_runtime::json_text::sha256;
use baal_service_runtime::ledger::{acquire_owner_lease, OwnerLease};
use serde_json::{json, Map, Value as V};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(format!("OWNED_HEDGE_BUDGET_{code}"))
    }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn same(a: &V, b: &V) -> Result<bool, String> {
    Ok(payload_hash(a)? == payload_hash(b)?)
}
fn hex(v: &V) -> bool {
    v.as_str().is_some_and(|s| {
        s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}
fn sha(b: &[u8]) -> String {
    sha256(b)
}
fn stamp(m: &fs::Metadata) -> String {
    let ino = stat_ino(m);
    format!("{ino}:{:.3}", created_ms(m))
}
fn created_ms(st: &fs::Metadata) -> f64 {
    st.created()
        .ok()
        .map(|c| {
            c.duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64()
                * 1000.0
        })
        .unwrap_or(0.0)
}
#[cfg(unix)]
fn stat_ino(st: &fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    st.ino()
}
#[cfg(not(unix))]
fn stat_ino(_st: &fs::Metadata) -> u64 {
    0
}
fn sync(v: Result<V, String>) -> Result<V, String> {
    v
}
fn instant_v(v: &V) -> Result<i64, String> {
    instant(v.as_str().unwrap_or(""))
}

/// combinedRisk(g, record, allocation, {allowFundingShortfall=false})
pub fn combined_risk(
    g: &V,
    record: &V,
    allocation: &V,
    allow_funding_shortfall: bool,
) -> Result<V, String> {
    let mut out = Map::new();
    out.insert("owned".into(), get(record, "owned").clone());
    out.insert("reservations".into(), get(record, "reservations").clone());
    out.insert("loans".into(), V::Array(vec![]));
    let run_id = get(record, "runId").as_str().unwrap_or("");
    let mut ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut out_owned = out["owned"].as_array().cloned().unwrap_or_default();
    let mut out_reservations = out["reservations"].as_array().cloned().unwrap_or_default();
    let mut out_loans: Vec<V> = vec![];
    for (kind, field) in [
        ("owned", "nominalCost"),
        ("reservations", "maximumNominalCost"),
        ("loans", "fullNominalPrincipal"),
    ] {
        let rows: &mut Vec<V> = match kind {
            "owned" => &mut out_owned,
            "reservations" => &mut out_reservations,
            _ => &mut out_loans,
        };
        need(
            get(record, kind).is_array() && get(g, kind).is_array(),
            "EXPOSURE_ARRAY",
        )?;
        for r in rows.iter() {
            need(
                get(r, "exposureId").as_str().is_some_and(|s| !s.is_empty())
                    && !ids.contains(get(r, "exposureId").as_str().unwrap_or("")),
                "OWN_EXPOSURE_ID",
            )?;
            ids.insert(get(r, "exposureId").as_str().unwrap_or("").to_string());
        }
        for r in get(g, kind).as_array().cloned().unwrap_or_default() {
            need(
                get(&r, "exposureId").as_str().is_some_and(|s| !s.is_empty())
                    && get(&r, "marketId").is_string()
                    && get(&r, "eventId").as_str().is_some_and(|s| !s.is_empty())
                    && amount(get(&r, field).as_str().unwrap_or(""))
                        .map(|n| n >= 0)
                        .unwrap_or(false),
                "EXTERNAL_EXPOSURE",
            )?;
            let eid = get(&r, "exposureId").as_str().unwrap_or("");
            if ids.contains(eid) {
                need(
                    rows.iter().any(|v| same(v, &r).unwrap_or(false)),
                    "CONFLICTING_DUPLICATE",
                )?;
                continue;
            }
            need(
                !eid.starts_with(&format!("scaled:{run_id}:"))
                    && !eid.starts_with(&format!("hedge:{run_id}:")),
                "UNVERIFIED_OWN_SHADOW",
            )?;
            ids.insert(eid.to_string());
            rows.push(r);
        }
    }
    let mut nominal: i128 = 0;
    let mut reserved: i128 = 0;
    let mut markets: BTreeMap<String, i128> = BTreeMap::new();
    let mut events: BTreeMap<String, i128> = BTreeMap::new();
    let mut mapping: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for (kind, field) in [
        ("owned", "nominalCost"),
        ("reservations", "maximumNominalCost"),
        ("loans", "fullNominalPrincipal"),
    ] {
        let rows: &Vec<V> = match kind {
            "owned" => &out_owned,
            "reservations" => &out_reservations,
            _ => &out_loans,
        };
        for r in rows {
            let n = amount(get(r, field).as_str().unwrap_or(""))?;
            need(
                n >= 0
                    && get(r, "marketId").is_string()
                    && get(r, "eventId").as_str().is_some_and(|s| !s.is_empty()),
                "NOMINAL_EXPOSURE",
            )?;
            let mid = get(r, "marketId").as_str().unwrap_or("").to_string();
            let eid = get(r, "eventId").as_str().unwrap_or("").to_string();
            need(
                !mapping.contains_key(&mid) || mapping[&mid] == eid,
                "EVENT_SCOPE_CONFLICT",
            )?;
            mapping.insert(mid.clone(), eid.clone());
            nominal += n;
            if kind == "reservations" {
                reserved += n;
            }
            *markets.entry(mid).or_insert(0) += n;
            *events.entry(eid).or_insert(0) += n;
        }
    }
    need(
        nominal <= amount(get(allocation, "capital").as_str().unwrap_or(""))?
            && markets.values().all(|v| {
                *v <= amount(get(allocation, "maximumMarketCost").as_str().unwrap_or(""))
                    .unwrap_or(i128::MIN)
            })
            && events.values().all(|v| {
                *v <= amount(get(allocation, "maximumEventCost").as_str().unwrap_or(""))
                    .unwrap_or(i128::MIN)
            })
            && markets.values().filter(|v| **v > 0).count()
                <= get(allocation, "maximumActiveMarkets").as_i64().unwrap_or(0) as usize,
        "FULL_NOMINAL_LIMIT",
    )?;
    let cash = amount(get(g, "cashLower").as_str().unwrap_or(""))?;
    need(
        cash >= 0 && (allow_funding_shortfall || reserved <= cash),
        "FULL_CASH_RESERVATION",
    )?;
    out.insert("owned".into(), V::Array(out_owned));
    out.insert("reservations".into(), V::Array(out_reservations));
    out.insert("loans".into(), V::Array(out_loans));
    out.insert("nominalExposure".into(), json!(money(nominal)));
    out.insert("reservedCash".into(), json!(money(reserved)));
    out.insert("fundingSatisfied".into(), json!(reserved <= cash));
    out.insert(
        "markets".into(),
        V::Object(
            markets
                .into_iter()
                .map(|(k, v)| (k, json!(money(v))))
                .collect(),
        ),
    );
    out.insert(
        "events".into(),
        V::Object(
            events
                .into_iter()
                .map(|(k, v)| (k, json!(money(v))))
                .collect(),
        ),
    );
    out.insert("loansDiscounted".into(), json!(false));
    Ok(V::Object(out))
}

/// ownRecord(r)
pub fn own_record(r: &V) -> Result<V, String> {
    let mut body = r.as_object().cloned().unwrap_or_default();
    body.shift_remove("reservationHash");
    let body = V::Object(body);
    need(
        hex(get(r, "reservationHash"))
            && payload_hash(&body)? == get(r, "reservationHash").as_str().unwrap_or("")
            && get(r, "kind").as_str() == Some("baal_scaled_no_full_nominal_reservation_v1")
            && get(r, "owned").is_array()
            && get(r, "reservations").is_array()
            && get(r, "unresolvedIntentIds").is_array(),
        "RESERVATION_SCHEMA",
    )?;
    let total: i128 = get(r, "reservations")
        .as_array()
        .map(|a| {
            a.iter()
                .map(|x| amount(get(x, "maximumNominalCost").as_str().unwrap_or("")).unwrap_or(0))
                .sum()
        })
        .unwrap_or(0);
    need(
        get(r, "cashReserved").as_str() == Some(money(total).as_str()),
        "RESERVATION_CASH",
    )?;
    Ok(r.clone())
}

/// projected(state, seed)
pub fn projected(state: &V, seed: &V) -> Result<V, String> {
    need(
        same(get(state, "scope"), get(&get(seed, "state"), "scope"))?
            && same(
                get(state, "allocation"),
                get(&get(seed, "state"), "allocation"),
            )?
            && same(
                get(state, "originalInventory"),
                get(&get(seed, "state"), "inventory"),
            )?
            && same(
                get(state, "originalOrders"),
                get(&get(seed, "state"), "orders"),
            )?
            && get(state, "orders").is_array()
            && get(state, "orders")
                .as_array()
                .map(|a| {
                    a.iter()
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
    let mut spent: i128 = get(&get(seed, "state"), "orders")
        .as_array()
        .map(|a| {
            a.iter()
                .map(|o| {
                    if get(get(o, "body"), "action").as_str() == Some("buy") {
                        amount(
                            get(get(o, "proof"), "scopedPrincipalCost")
                                .as_str()
                                .unwrap_or("0.000000"),
                        )
                        .unwrap_or(0)
                    } else {
                        0
                    }
                })
                .sum()
        })
        .unwrap_or(0);
    for o in get(state, "orders").as_array().cloned().unwrap_or_default() {
        need(
            get(&get(&o, "body"), "side").as_str() == Some("no")
                && ["buy", "sell"].contains(&get(&get(&o, "body"), "action").as_str().unwrap_or(""))
                && baal_service_runtime::json_text::js_int(get(&get(&o, "body"), "quantity"))
                    .is_some_and(|q| (1..=100).contains(&q))
                && amount(
                    &get(&get(&o, "body"), "price")
                        .as_f64()
                        .map(|f| f.to_string())
                        .or_else(|| get(&get(&o, "body"), "price").as_str().map(|s| s.to_string()))
                        .unwrap_or_default(),
                )
                .map(|p| p > 0 && p < 1_000_000)
                .unwrap_or(false)
                && amount(
                    &get(&get(&o, "body"), "price")
                        .as_f64()
                        .map(|f| f.to_string())
                        .or_else(|| get(&get(&o, "body"), "price").as_str().map(|s| s.to_string()))
                        .unwrap_or_default(),
                )
                .unwrap_or(i128::MAX)
                    * baal_service_runtime::json_text::js_int(get(&get(&o, "body"), "quantity"))
                        .unwrap_or(i64::MAX) as i128
                    <= 25_000_000
                && get(&get(&o, "body"), "tournamentId")
                    == get(get(state, "scope"), "tournamentId"),
            "ORDER_BOUND",
        )?;
        if get(&get(&o, "body"), "action").as_str() == Some("buy") {
            need(
                baal_service_runtime::json_text::js_int(get(&get(&o, "body"), "quantity"))
                    == Some(1)
                    && amount(
                        &get(&get(&o, "body"), "price")
                            .as_f64()
                            .map(|f| f.to_string())
                            .or_else(|| {
                                get(&get(&o, "body"), "price")
                                    .as_str()
                                    .map(|s| s.to_string())
                            })
                            .unwrap_or_default(),
                    )
                    .map(|p| p <= 995_000)
                    .unwrap_or(false),
                "ONE_SHARE_HEDGE_BOUND",
            )?;
        }
        if !get(&o, "proof").is_null() {
            need(
                get(&o, "phase").as_str() == Some("reconciled")
                    && get(&get(&o, "proof"), "scopedTradeVerified") == &json!(true)
                    && get(&get(&o, "proof"), "action") == get(&get(&o, "body"), "action")
                    && get(&get(&o, "proof"), "ownerId")
                        == get(get(state, "scope"), "ownerId")
                    && get(&get(&o, "proof"), "runId") == get(get(state, "scope"), "runId")
                    && get(&get(&o, "proof"), "tournamentId")
                        == get(get(state, "scope"), "tournamentId"),
                "ATTRIBUTED_CURRENT_PROOF",
            )?;
            if get(&get(&o, "body"), "action").as_str() == Some("buy") {
                spent += amount(
                    get(&get(&o, "proof"), "scopedPrincipalCost")
                        .as_str()
                        .unwrap_or(""),
                )?;
            }
        }
    }
    let mut realized: i128 = 0;
    if let Some(m) = get(state, "currentInventory").as_object() {
        for (ex, slot) in m {
            let p = get(slot, "proof");
            need(
                get(p, "scopedTradeVerified") == &json!(true)
                    && get(p, "exchangeId").as_str() == Some(ex.as_str())
                    && get(p, "ownerId") == get(get(state, "scope"), "ownerId")
                    && get(p, "runId") == get(get(state, "scope"), "runId")
                    && get(p, "tournamentId") == get(get(state, "scope"), "tournamentId")
                    && hex(get(p, "verificationHash"))
                    && get(p, "remainingLots").is_array()
                    && baal_service_runtime::json_text::js_int(get(p, "remainingOwnedQuantity"))
                        .is_some_and(|q| q >= 0),
                "CURRENT_ATTRIBUTED_INVENTORY",
            )?;
            need(
                baal_service_runtime::json_text::js_int(get(p, "remainingOwnedQuantity"))
                    == Some(
                        get(p, "remainingLots")
                            .as_array()
                            .map(|a| {
                                a.iter()
                                    .map(|l| {
                                        baal_service_runtime::json_text::js_int(get(l, "quantity"))
                                            .unwrap_or(0)
                                    })
                                    .sum()
                            })
                            .unwrap_or(0),
                    )
                    && amount(get(p, "remainingOwnedCost").as_str().unwrap_or(""))
                        .map(|c| {
                            c == get(p, "remainingLots")
                                .as_array()
                                .map(|a| {
                                    a.iter()
                                        .map(|l| {
                                            amount(get(l, "entryPrice").as_str().unwrap_or(""))
                                                .unwrap_or(0)
                                                * baal_service_runtime::json_text::js_int(
                                                    get(l, "quantity"),
                                                )
                                                .unwrap_or(0)
                                                    as i128
                                        })
                                        .sum::<i128>()
                                })
                                .unwrap_or(0)
                        })
                        .unwrap_or(false),
                "CURRENT_FIFO_COST",
            )?;
            need(
                !get(get(state, "originalInventory"), ex).is_null()
                    || get(state, "orders").as_array().is_some_and(|a| {
                        a.iter().any(|o| {
                            get(get(o, "body"), "action").as_str() == Some("buy")
                                && get(get(o, "body"), "exchangeId").as_str() == Some(ex.as_str())
                                && !get(o, "proof").is_null()
                        })
                    }),
                "NO_FOREIGN_CURRENT_INVENTORY",
            )?;
            realized +=
                amount(get(p, "cumulativeRealizedPnl").as_str().unwrap_or("")).unwrap_or(0);
        }
    }
    let record = own_record(&project_risk(state)?)?;
    need(
        get(&record, "allocationHash").as_str()
            == Some(payload_hash(get(&get(seed, "state"), "allocation"))?.as_str())
            && get(&record, "seedHash").as_str() == Some(payload_hash(seed)?.as_str()),
        "SEEDED_RESERVATION",
    )?;
    let unknown = get(state, "orders").as_array().is_some_and(|a| {
        a.iter().any(|o| {
            get(o, "proof").is_null()
                && get(o, "phase").as_str() != Some("abandoned")
                && (["uncertain", "semantic_mismatch"]
                    .contains(&get(o, "phase").as_str().unwrap_or(""))
                    || get(o, "semanticMismatch") == &json!(true)
                    || (get(o, "orderId").is_null()
                        && get(o, "fetchStarts").as_array().is_some_and(|f| !f.is_empty())))
        })
    });
    let orders: Vec<V> = get(state, "orders")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|o| {
            json!({
                "intentId": get(o, "intentId"),
                "bodyHash": get(o, "bodyHash"),
                "bodyJson": get(o, "bodyJson"),
                "phase": get(o, "phase"),
                "orderId": get(o, "orderId"),
                "proofHash": get(get(o, "proof"), "verificationHash"),
                "fetchStarts": get(o, "fetchStarts"),
            })
        })
        .collect();
    Ok(json!({
        "record": record,
        "spent": money(spent),
        "realizedPnl": money(realized),
        "currentInventoryHash": payload_hash(get(state, "currentInventory"))?,
        "assignmentHistoryHash": payload_hash(get(state, "assignments"))?,
        "pairRetirementsHash": payload_hash(get(state, "pairRetirements"))?,
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

struct BudgetInner {
    deps: OwnedHedgeBudgetDeps,
    seed: V,
    seed_hash: String,
    scope: V,
    allocation: V,
    allocation_hash: String,
    migration_hash: String,
    activation_hash: String,
    coordinator_code_identity_hash: String,
    budget_installed_hash: String,
    actor_file: PathBuf,
    workspace: PathBuf,
    legacy_file: PathBuf,
    legacy_bytes: Vec<u8>,
    legacy_identity: String,
    migration: V,
    activation: V,
    file: PathBuf,
    lease: Option<OwnerLease>,
    journal: Option<Journal>,
    state: Option<V>,
    closed: bool,
    failed: bool,
    size: u64,
    file_hash: String,
    file_identity: String,
    source_cache: Option<(String, V)>,
    cache_hits: u64,
    cache_misses: u64,
    initial_financial_lease_hash: Option<String>,
}

pub struct BaalOwnedHedgeSharedBudget {
    inner: Rc<RefCell<BudgetInner>>,
}

fn basic(inner: &Rc<RefCell<BudgetInner>>) -> Result<V, String> {
    {
        let b = inner.borrow();
        need(!b.closed && !b.failed && (b.deps.can_operate)(), "STOPPED")?;
        if let Some(l) = b.lease.as_ref() {
            l.assert_ownership().map_err(|e| e.message)?;
        }
    }
    let a = {
        let b = inner.borrow();
        sync((b.deps.assert_account_lease)())?
    };
    let f = {
        let b = inner.borrow();
        sync((b.deps.assert_financial_owner_lease)())?
    };
    let (scope, migration) = {
        let b = inner.borrow();
        (b.scope.clone(), b.migration.clone())
    };
    need(
        get(&a, "ownerId") == get(&scope, "ownerId")
            && get(&a, "runId") == get(&scope, "runId")
            && get(&a, "tournamentId") == get(&scope, "tournamentId")
            && get(&a, "leaseId").as_str().is_some_and(|s| !s.is_empty()),
        "ACCOUNT_LEASE",
    )?;
    need(
        get(&f, "ownerId") == get(&scope, "ownerId")
            && get(&f, "runId") == get(&scope, "runId")
            && get(&f, "tournamentId") == get(&scope, "tournamentId")
            && get(&f, "leaseId").as_str().is_some_and(|s| !s.is_empty())
            && get(&f, "implementationEpoch") == get(&migration, "implementationEpoch"),
        "FINANCIAL_OWNER_LEASE",
    )?;
    let financial_hash = payload_hash(&f)?;
    {
        let mut b = inner.borrow_mut();
        if let Some(init) = &b.initial_financial_lease_hash {
            need(&financial_hash == init, "FINANCIAL_OWNER_CHANGED")?;
        } else {
            b.initial_financial_lease_hash = Some(financial_hash.clone());
        }
    }
    Ok(json!({
        "accountLeaseHash": payload_hash(&a)?,
        "financialOwnerLeaseHash": financial_hash,
    }))
}

fn observed(inner: &Rc<RefCell<BudgetInner>>) -> Result<V, String> {
    let g = {
        let b = inner.borrow();
        sync((b.deps.observe_global_risk)())?
    };
    let (scope, n) = {
        let b = inner.borrow();
        (b.scope.clone(), (b.deps.now)() as i64)
    };
    need(
        get(&g, "ownerId") == get(&scope, "ownerId")
            && get(&g, "runId") == get(&scope, "runId")
            && get(&g, "tournamentId") == get(&scope, "tournamentId")
            && get(&g, "complete") == &json!(true)
            && get(&g, "eventScopeComplete") == &json!(true)
            && get(&g, "soleCollector") == &json!(true)
            && get(&g, "stopped") == &json!(false)
            && get(&g, "unknownAcceptance").is_boolean()
            && hex(get(&g, "sourceHash"))
            && get(&g, "unresolvedIntentIds").is_array()
            && get(&g, "unresolvedIntentIds")
                .as_array()
                .unwrap()
                .iter()
                .all(|id| id.is_string())
            && instant_v(get(&g, "at")).map(|t| t <= n && n - t <= 20_000).unwrap_or(false),
        "FRESH_GLOBAL_OBSERVATION",
    )?;
    Ok(g)
}


/// source() — reads + replays the coordinator's actor journal with exact-byte
/// memoization keyed on (hash,size,file-identity,seed/activation/migration,code).
fn source(inner: &Rc<RefCell<BudgetInner>>) -> Result<V, String> {
    basic(inner)?;
    let (workspace, legacy_file, legacy_bytes, legacy_identity, actor_file, seed,
         activation, migration, seed_hash, activation_hash, migration_hash, code_hash) = {
        let b = inner.borrow();
        (
            b.workspace.clone(), b.legacy_file.clone(), b.legacy_bytes.clone(),
            b.legacy_identity.clone(), b.actor_file.clone(), b.seed.clone(),
            b.activation.clone(), b.migration.clone(), b.seed_hash.clone(),
            b.activation_hash.clone(), b.migration_hash.clone(),
            b.coordinator_code_identity_hash.clone(),
        )
    };
    need(
        payload_hash(&live_coordinator_identity(&workspace)?)? == code_hash,
        "COORDINATOR_CODE_CHANGED",
    )?;
    let old_before = fs::metadata(&legacy_file).map_err(|e| e.to_string())?;
    let old = fs::read(&legacy_file).map_err(|e| e.to_string())?;
    let old_after = fs::metadata(&legacy_file).map_err(|e| e.to_string())?;
    need(
        stamp(&old_before) == legacy_identity
            && stamp(&old_after) == legacy_identity
            && old_after.len() == old.len() as u64
            && old.len() == get(get(&seed, "anchor"), "bytes").as_u64().unwrap_or(0) as usize
            && old == legacy_bytes,
        "LEGACY_NOT_SEALED",
    )?;
    let hint = {
        let b = inner.borrow();
        sync((b.deps.get_coordinator_source)())?
    };
    need(
        get(&hint, "journalFile").as_str().is_some_and(|f| {
            PathBuf::from(f).canonicalize().unwrap_or_else(|_| PathBuf::from(f))
                == actor_file.canonicalize().unwrap_or_else(|_| actor_file.clone())
        }),
        "ACTUAL_SOURCE_PATH",
    )?;
    let before = fs::metadata(&actor_file).map_err(|e| e.to_string())?;
    let bytes = fs::read(&actor_file).map_err(|e| e.to_string())?;
    let hash = sha(&bytes);
    need(
        !bytes.is_empty() && bytes.len() <= 134_217_728 && bytes.last() == Some(&10),
        "COMPLETE_SOURCE",
    )?;
    {
        let b = inner.borrow();
        if let Some(state) = &b.state {
            let anchor = get(state, "actorAnchor");
            let nbytes = get(anchor, "bytes").as_u64().unwrap_or(0) as usize;
            need(
                bytes.len() >= nbytes
                    && sha(&bytes[..nbytes]) == get(anchor, "sha256").as_str().unwrap_or(""),
                "SOURCE_PREFIX_RESET",
            )?;
        }
    }
    let key = payload_hash(&json!({
        "hash": hash,
        "size": bytes.len(),
        "identity": stamp(&before),
        "seedHash": seed_hash,
        "activationHash": activation_hash,
        "migrationHash": migration_hash,
        "coordinatorCodeIdentityHash": code_hash,
    }))?;
    let (result, p): (V, V) = {
        let cached = inner.borrow().source_cache.clone();
        if cached.as_ref().map(|(k, _)| k == &key).unwrap_or(false) {
            inner.borrow_mut().cache_hits += 1;
            let cv = cached.unwrap().1;
            (get(&cv, "result").clone(), get(&cv, "p").clone())
        } else {
            inner.borrow_mut().cache_misses += 1;
            let result = replay_live_hedge_journal(
                std::str::from_utf8(&bytes).map_err(|e| e.to_string())?,
                &seed,
                &activation,
                &migration,
                &live_coordinator_identity(&workspace)?,
            )?;
            need(
                get(&result, "economicReplayPerformed") == &json!(true),
                "ECONOMIC_REPLAY_REQUIRED",
            )?;
            let p = projected(get(&result, "state"), &seed)?;
            (result, p)
        }
    };
    need(
        get(get(&result, "anchor"), "bytes").as_u64() == Some(bytes.len() as u64)
            && get(get(&result, "anchor"), "sha256").as_str() == Some(hash.as_str())
            && stamp(&fs::metadata(&actor_file).map_err(|e| e.to_string())?) == stamp(&before)
            && fs::metadata(&actor_file).map_err(|e| e.to_string())?.len() == bytes.len() as u64,
        "SOURCE_CHANGED_DURING_READ",
    )?;
    need(
        get(&hint, "sequence").as_u64() == get(get(&result, "anchor"), "sequence").as_u64()
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
        let b = inner.borrow();
        if let Some(state) = &b.state {
            need(
                same(get(&result, "rootPrefix"), get(state, "coordinatorRootPrefix"))?
                    && amount(get(&p, "spent").as_str().unwrap_or(""))?
                        >= amount(
                            get(get(state, "projection"), "spent")
                                .as_str()
                                .unwrap_or(""),
                        )?
                    && get(&p, "orders").as_array().map(|a| a.len()).unwrap_or(0)
                        >= get(get(state, "projection"), "orders")
                            .as_array()
                            .map(|a| a.len())
                            .unwrap_or(0),
                "EXPENSE_OR_ROOT_RESET",
            )?;
            for o in get(get(state, "projection"), "orders")
                .as_array()
                .cloned()
                .unwrap_or_default()
            {
                let n = get(&p, "orders").as_array().and_then(|a| {
                    a.iter()
                        .find(|n| get(n, "intentId") == get(&o, "intentId"))
                        .cloned()
                });
                need(n.is_some(), "ORIGINAL_HEDGE_RESET")?;
                let n = n.unwrap();
                need(
                    get(&n, "bodyHash") == get(&o, "bodyHash")
                        && get(&n, "bodyJson") == get(&o, "bodyJson")
                        && (get(&o, "orderId").is_null()
                            || get(&n, "orderId") == get(&o, "orderId"))
                        && (get(&o, "proofHash").is_null()
                            || get(&n, "proofHash") == get(&o, "proofHash"))
                        && get(&o, "fetchStarts").as_array().is_some_and(|fs| {
                            fs.iter().all(|f| {
                                get(&n, "fetchStarts").as_array().is_some_and(|gs| {
                                    gs.iter().any(|g| same(f, g).unwrap_or(false))
                                })
                            })
                        }),
                    "ORIGINAL_HEDGE_RESET",
                )?;
            }
        }
    }
    {
        let mut b = inner.borrow_mut();
        if b.source_cache.as_ref().map(|(k, _)| k != &key).unwrap_or(true) {
            b.source_cache = Some((
                key,
                json!({
                    "result": {
                        "anchor": get(&result, "anchor"),
                        "rootPrefix": get(&result, "rootPrefix"),
                        "economicReplayPerformed": true,
                    },
                    "p": p,
                }),
            ));
        }
    }
    let mut out = p.as_object().cloned().unwrap_or_default();
    out.insert("anchor".into(), get(&result, "anchor").clone());
    out.insert("opening".into(), get(&result, "rootPrefix").clone());
    Ok(V::Object(out))
}

/// validateSnapshot(next)
fn validate_snapshot(inner: &Rc<RefCell<BudgetInner>>, next: &V) -> Result<(), String> {
    let (actor_file, workspace, code_hash, seed, activation, migration, seed_hash,
         activation_hash, migration_hash, budget_installed_hash, allocation_hash,
         allocation, scope, legacy_anchor) = {
        let b = inner.borrow();
        (
            b.actor_file.clone(), b.workspace.clone(),
            b.coordinator_code_identity_hash.clone(), b.seed.clone(), b.activation.clone(),
            b.migration.clone(), b.seed_hash.clone(), b.activation_hash.clone(),
            b.migration_hash.clone(), b.budget_installed_hash.clone(),
            b.allocation_hash.clone(), b.allocation.clone(), b.scope.clone(),
            get(&b.seed, "anchor").clone(),
        )
    };
    let before = fs::metadata(&actor_file).map_err(|e| e.to_string())?;
    let all = fs::read(&actor_file).map_err(|e| e.to_string())?;
    let a = get(next, "actorAnchor");
    need(
        get(a, "bytes").as_u64().map(|b| b > 0 && b <= all.len() as u64).unwrap_or(false),
        "HISTORICAL_SOURCE_BOUND",
    )?;
    let prefix = &all[..a.get("bytes").and_then(V::as_u64).unwrap_or(0) as usize];
    let hash = sha(prefix);
    need(
        prefix.last() == Some(&10) && hash == get(a, "sha256").as_str().unwrap_or(""),
        "HISTORICAL_SOURCE_HASH",
    )?;
    let code = live_coordinator_identity(&workspace)?;
    need(payload_hash(&code)? == code_hash, "COORDINATOR_CODE_CHANGED")?;
    let key = payload_hash(&json!({
        "hash": hash,
        "size": prefix.len(),
        "identity": stamp(&before),
        "seedHash": seed_hash,
        "activationHash": activation_hash,
        "migrationHash": migration_hash,
        "coordinatorCodeIdentityHash": code_hash,
    }))?;
    let (replay, actual): (V, V) = {
        let cached = inner.borrow().source_cache.clone();
        if all.len() == prefix.len()
            && cached.as_ref().map(|(k, _)| k == &key).unwrap_or(false)
            && same(
                get(&cached.as_ref().unwrap().1, "result").get("anchor").unwrap(),
                a,
            )?
        {
            let cv = cached.unwrap().1;
            (get(&cv, "result").clone(), get(&cv, "p").clone())
        } else {
            let replay = replay_live_hedge_journal(
                std::str::from_utf8(prefix).map_err(|e| e.to_string())?,
                &seed,
                &activation,
                &migration,
                &code,
            )?;
            let actual = projected(get(&replay, "state"), &seed)?;
            (replay, actual)
        }
    };
    let after = fs::metadata(&actor_file).map_err(|e| e.to_string())?;
    need(
        stamp(&before) == stamp(&after) && after.len() == all.len() as u64,
        "SOURCE_CHANGED_DURING_READ",
    )?;
    need(
        same(get(&replay, "anchor"), a)?
            && same(get(&replay, "rootPrefix"), get(next, "coordinatorRootPrefix"))?
            && same(&actual, get(next, "projection"))?,
        "HISTORICAL_ECONOMIC_SOURCE",
    )?;
    need(
        instant_v(get(get(next, "externalObservation"), "at"))?
            <= instant_v(get(next, "at"))?
            && instant_v(get(next, "at"))?
                - instant_v(get(get(next, "externalObservation"), "at"))?
                <= 20_000,
        "HISTORICAL_OBSERVATION_CLOCK",
    )?;
    need(
        get(next, "schemaVersion") == &json!(1)
            && get(next, "mode").as_str() == Some("source_verified_owned_hedge_budget_v2")
            && get(next, "seedHash").as_str() == Some(seed_hash.as_str())
            && get(next, "migrationHash").as_str() == Some(migration_hash.as_str())
            && get(next, "activationHash").as_str() == Some(activation_hash.as_str())
            && get(next, "coordinatorCodeIdentityHash").as_str() == Some(code_hash.as_str())
            && get(next, "budgetInstalledHash").as_str()
                == Some(budget_installed_hash.as_str())
            && get(next, "allocationHash").as_str() == Some(allocation_hash.as_str())
            && same(get(next, "scope"), &scope)?
            && same(get(next, "legacyAnchor"), &legacy_anchor)?
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
        )? && get(next, "cumulativeActualExpenses")
            == get(get(next, "projection"), "spent"),
        "BUDGET_ECONOMICS",
    )?;
    let state = inner.borrow().state.clone();
    if let Some(state) = state {
        need(
            get(get(next, "actorAnchor"), "bytes").as_u64().unwrap_or(0)
                >= get(get(&state, "actorAnchor"), "bytes").as_u64().unwrap_or(0)
                && get(get(next, "actorAnchor"), "sequence").as_u64().unwrap_or(0)
                    >= get(get(&state, "actorAnchor"), "sequence").as_u64().unwrap_or(0)
                && amount(
                    get(get(next, "projection"), "spent").as_str().unwrap_or(""),
                )? >= amount(
                    get(get(&state, "projection"), "spent").as_str().unwrap_or(""),
                )?,
            "BUDGET_RESET",
        )?;
    }
    Ok(())
}

/// append(next)
fn append(inner: &Rc<RefCell<BudgetInner>>, next: &V) -> Result<(), String> {
    basic(inner)?;
    let (file, size, file_hash, file_identity) = {
        let b = inner.borrow();
        (b.file.clone(), b.size, b.file_hash.clone(), b.file_identity.clone())
    };
    let old = fs::read(&file).map_err(|e| e.to_string())?;
    let metadata = fs::metadata(&file).map_err(|e| e.to_string())?;
    need(
        stamp(&metadata) == file_identity
            && old.len() as u64 == size
            && sha(&old) == file_hash,
        "BUDGET_FILE_CHANGED",
    )?;
    validate_snapshot(inner, next)?;
    let (seed_hash, migration_hash, activation_hash) = {
        let b = inner.borrow();
        (b.seed_hash.clone(), b.migration_hash.clone(), b.activation_hash.clone())
    };
    let r = (|| -> Result<(), String> {
        {
            let mut b = inner.borrow_mut();
            let j = b.journal.as_mut().ok_or("OWNED_HEDGE_BUDGET_NOT_OPEN")?;
            j.commit(
                next,
                &json!({
                    "kind": "installed_source_verified_hedge_reserve",
                    "seedHash": seed_hash,
                    "migrationHash": migration_hash,
                    "activationHash": activation_hash,
                }),
            )?;
        }
        let after = fs::read(&file).map_err(|e| e.to_string())?;
        need(&after[..size as usize] == &old[..], "BUDGET_PREFIX_CHANGED")?;
        {
            let mut b = inner.borrow_mut();
            b.state = Some(next.clone());
            b.size = after.len() as u64;
            b.file_hash = sha(&after);
        }
        basic(inner)?;
        Ok(())
    })();
    match r {
        Ok(()) => Ok(()),
        Err(e) => {
            inner.borrow_mut().failed = true;
            Err(e)
        }
    }
}

impl BaalOwnedHedgeSharedBudget {
    /// openBaalOwnedHedgeSharedBudgetV2(options)
    #[allow(clippy::too_many_arguments)]
    pub fn open(
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
    ) -> Result<Self, String> {
        need(
            dir.is_absolute()
                && coordinator_dir.is_absolute()
                && workspace.is_absolute()
                && legacy_file.is_absolute()
                && dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf())
                    != coordinator_dir
                        .canonicalize()
                        .unwrap_or_else(|_| coordinator_dir.to_path_buf())
                && hex(&V::String(coordinator_code_identity_hash.to_string())),
            "DEPENDENCIES",
        )?;
        let legacy_bytes = fs::read(legacy_file).map_err(|e| e.to_string())?;
        let legacy_identity =
            stamp(&fs::metadata(legacy_file).map_err(|e| e.to_string())?);
        let seed = replay_baal_scaled_controller_v1(&json!({
            "bodyText": std::str::from_utf8(&legacy_bytes).map_err(|e| e.to_string())?,
            "expectedCodeIdentity": legacy_code_identity,
            "expectedScope": legacy_scope,
            "expectedAllocation": legacy_allocation,
        }))?;
        let seed_hash = payload_hash(&seed)?;
        let scope = get(&get(&seed, "state"), "scope").clone();
        let mut allocation = Map::new();
        allocation.insert("maximumOrderCost".into(), json!("25.000000"));
        allocation.insert("maximumMarketCost".into(), json!("100.000000"));
        allocation.insert("maximumEventCost".into(), json!("200.000000"));
        allocation.insert("maximumActiveMarkets".into(), json!(20));
        for (k, v) in get(&get(&seed, "state"), "allocation")
            .as_object()
            .cloned()
            .unwrap_or_default()
        {
            allocation.insert(k, v);
        }
        let allocation = V::Object(allocation);
        let allocation_hash = payload_hash(get(&get(&seed, "state"), "allocation"))?;
        let migration_hash = payload_hash(migration)?;
        let activation_hash = payload_hash(activation)?;
        need(
            get(&allocation, "ownerId").as_str() == Some("Codex")
                && get(&allocation, "capital").as_str() == Some("1000.000000")
                && hex(get(&allocation, "humanAllocationSourceHash"))
                && amount(get(&allocation, "maximumOrderCost").as_str().unwrap_or(""))
                    .map(|n| n > 0 && n <= 25_000_000)
                    .unwrap_or(false)
                && amount(get(&allocation, "maximumMarketCost").as_str().unwrap_or(""))
                    .map(|n| n > 0 && n <= 100_000_000)
                    .unwrap_or(false)
                && amount(get(&allocation, "maximumEventCost").as_str().unwrap_or(""))
                    .map(|n| n > 0 && n <= 200_000_000)
                    .unwrap_or(false)
                && get(&allocation, "maximumActiveMarkets")
                    .as_i64()
                    .is_some_and(|n| (1..=20).contains(&n)),
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
            deps,
            seed: seed.clone(),
            seed_hash: seed_hash.clone(),
            scope,
            allocation,
            allocation_hash,
            migration_hash: migration_hash.clone(),
            activation_hash: activation_hash.clone(),
            coordinator_code_identity_hash: coordinator_code_identity_hash.to_string(),
            budget_installed_hash: budget_installed_hash.clone(),
            actor_file: actor_file.clone(),
            workspace: workspace.to_path_buf(),
            legacy_file: legacy_file.to_path_buf(),
            legacy_bytes,
            legacy_identity,
            migration: migration.clone(),
            activation: activation.clone(),
            file: file.clone(),
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
        }));
        if let Err(e) = (|| -> Result<(), String> {
            basic(&inner)?;
            {
                let mut b = inner.borrow_mut();
                b.lease = Some(
                    acquire_owner_lease(
                        dir,
                        get(&b.scope, "ownerId").as_str().unwrap_or(""),
                        get(&b.scope, "runId").as_str().unwrap_or(""),
                        "source-verified-owned-hedge-budget-v2",
                        (b.deps.now)(),
                    )
                    .map_err(|e| e.message)?,
                );
            }
            if !file.exists() {
                fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&file)
                    .map_err(|e| e.to_string())?;
            }
            let bytes = fs::read(&file).map_err(|e| e.to_string())?;
            need(
                bytes.len() <= 134_217_728
                    && (bytes.is_empty() || bytes.last() == Some(&10)),
                "BUDGET_BOUND",
            )?;
            inner.borrow_mut().journal = Some(Journal::open(&file)?);
            for line in std::str::from_utf8(&bytes)
                .map_err(|e| e.to_string())?
                .split('\n')
                .filter(|l| !l.is_empty())
            {
                let row: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
                need(
                    get(get(&row, "event"), "kind").as_str()
                        == Some("installed_source_verified_hedge_reserve")
                        && get(get(&row, "event"), "seedHash").as_str()
                            == Some(seed_hash.as_str())
                        && get(get(&row, "event"), "migrationHash").as_str()
                            == Some(migration_hash.as_str())
                        && get(get(&row, "event"), "activationHash").as_str()
                            == Some(activation_hash.as_str()),
                    "BUDGET_EVENT",
                )?;
                validate_snapshot(&inner, get(&row, "state"))?;
                inner.borrow_mut().state = Some(get(&row, "state").clone());
            }
            {
                let mut b = inner.borrow_mut();
                b.size = bytes.len() as u64;
                b.file_hash = sha(&bytes);
                b.file_identity = stamp(&fs::metadata(&file).map_err(|e| e.to_string())?);
            }
            if inner.borrow().state.is_some() {
                source(&inner)?;
            }
            Ok(())
        })() {
            let mut b = inner.borrow_mut();
            if let Some(l) = b.lease.as_mut() {
                let _ = l.release();
            }
            return Err(e);
        }
        Ok(Self { inner })
    }

    /// installFundingReservation(record)
    pub fn install_funding_reservation(&self, record: &V) -> Result<V, String> {
        let leases = basic(&self.inner)?;
        let s = source(&self.inner)?;
        need(same(record, get(&s, "record"))?, "SOURCE_RESERVATION_REQUIRED")?;
        if self.inner.borrow().state.is_none() {
            need(
                get(&s, "orders").as_array().is_some_and(|a| a.is_empty())
                    && get(&s, "unknown") == &json!(false)
                    && get(get(&s, "record"), "unresolvedIntentIds")
                        .as_array()
                        .is_some_and(|a| a.is_empty()),
                "MIGRATE_BEFORE_NEW_HEDGE",
            )?;
        }
        let g = observed(&self.inner)?;
        let allocation = self.inner.borrow().allocation.clone();
        let risk = combined_risk(&g, get(&s, "record"), &allocation, false)?;
        let own_unresolved = get(get(&s, "record"), "unresolvedIntentIds")
            .as_array()
            .cloned()
            .unwrap_or_default();
        let other: Vec<V> = get(&g, "unresolvedIntentIds")
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|id| !own_unresolved.contains(id))
            .collect();
        need(other.is_empty(), "FOREIGN_UNKNOWN")?;
        let at = {
            let b = self.inner.borrow();
            baal_service_runtime::contract::ms_to_iso((b.deps.now)())
        };
        let mut next = Map::new();
        next.insert("schemaVersion".into(), json!(1));
        next.insert("mode".into(), json!("source_verified_owned_hedge_budget_v2"));
        next.insert("scope".into(), self.inner.borrow().scope.clone());
        next.insert(
            "allocationHash".into(),
            json!(self.inner.borrow().allocation_hash),
        );
        next.insert(
            "budgetInstalledHash".into(),
            json!(self.inner.borrow().budget_installed_hash),
        );
        next.insert(
            "coordinatorCodeIdentityHash".into(),
            json!(self.inner.borrow().coordinator_code_identity_hash),
        );
        next.insert("seedHash".into(), json!(self.inner.borrow().seed_hash));
        next.insert(
            "migrationHash".into(),
            json!(self.inner.borrow().migration_hash),
        );
        next.insert(
            "activationHash".into(),
            json!(self.inner.borrow().activation_hash),
        );
        next.insert(
            "legacyAnchor".into(),
            get(&self.inner.borrow().seed, "anchor").clone(),
        );
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
        next.insert("externalObservation".into(), g);
        for (k, v) in leases.as_object().cloned().unwrap_or_default() {
            next.insert(k, v);
        }
        next.insert("cumulativeActualExpenses".into(), get(&s, "spent").clone());
        next.insert("at".into(), json!(at));
        let next = V::Object(next);
        append(&self.inner, &next)?;
        let prev = self
            .inner
            .borrow()
            .journal
            .as_ref()
            .map(|j| j.previous.clone())
            .unwrap_or_default();
        Ok(json!({
            "reservationHash": get(get(&s, "record"), "reservationHash"),
            "allocationHash": self.inner.borrow().allocation_hash,
            "seedHash": self.inner.borrow().seed_hash,
            "budgetInstalledHash": self.inner.borrow().budget_installed_hash,
            "globalLedgerHash": prev,
            "confirmedAt": at,
        }))
    }

    /// globalRisk()
    pub fn global_risk(&self) -> Result<V, String> {
        basic(&self.inner)?;
        let state = self.inner.borrow().state.clone();
        let state = state.ok_or("OWNED_HEDGE_BUDGET_NOT_INSTALLED")?;
        let s = source(&self.inner)?;
        let g = observed(&self.inner)?;
        need(
            get(get(&s, "record"), "reservationHash")
                == get(get(get(&state, "projection"), "record"), "reservationHash"),
            "CURRENT_RESERVE_NOT_INSTALLED",
        )?;
        let allocation = self.inner.borrow().allocation.clone();
        let r = combined_risk(&g, get(&s, "record"), &allocation, true)?;
        let mut out = g.as_object().cloned().unwrap_or_default();
        for (k, v) in r.as_object().cloned().unwrap_or_default() {
            out.insert(k, v);
        }
        let jprev = self
            .inner
            .borrow()
            .journal
            .as_ref()
            .map(|j| j.previous.clone())
            .unwrap_or_default();
        out.insert(
            "installedCashLower".into(),
            get(get(&state, "externalObservation"), "cashLower").clone(),
        );
        out.insert("allocationHash".into(), json!(self.inner.borrow().allocation_hash));
        out.insert(
            "budgetInstalledHash".into(),
            json!(self.inner.borrow().budget_installed_hash),
        );
        out.insert("seedHash".into(), json!(self.inner.borrow().seed_hash));
        out.insert(
            "implementationEpoch".into(),
            get(&self.inner.borrow().migration, "implementationEpoch").clone(),
        );
        out.insert(
            "controllerReservationHash".into(),
            get(get(&s, "record"), "reservationHash").clone(),
        );
        out.insert("globalLedgerHash".into(), json!(jprev.clone()));
        out.insert(
            "sourceHash".into(),
            json!(payload_hash(&json!({
                "observation": get(&g, "sourceHash"),
                "actor": get(&s, "anchor"),
                "reservation": get(get(&s, "record"), "reservationHash"),
                "globalLedgerHash": jprev,
            }))?),
        );
        let unresolved_exposure = {
            let mut found = false;
            for k in ["owned", "reservations", "loans"] {
                if get(&r, k).as_array().is_some_and(|a| {
                    a.iter().any(|v| get(v, "unknownAcceptance") == &json!(true))
                }) {
                    found = true;
                }
            }
            found
        };
        out.insert(
            "unknownAcceptance".into(),
            json!(
                get(&s, "unknown") == &json!(true)
                    || get(&g, "unknownAcceptance") == &json!(true)
                    || unresolved_exposure
            ),
        );
        let mut seen = std::collections::HashSet::new();
        let mut merged: Vec<V> = Vec::new();
        for id in get(get(&s, "record"), "unresolvedIntentIds")
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .chain(
                get(&g, "unresolvedIntentIds")
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .into_iter(),
            )
        {
            let key = id.as_str().unwrap_or("").to_string();
            if seen.insert(key) {
                merged.push(id);
            }
        }
        out.insert("unresolvedIntentIds".into(), V::Array(merged));
        out.insert("executionAuthority".into(), json!(false));
        out.insert("oldWriterHandbackAllowed".into(), json!(false));
        Ok(V::Object(out))
    }

    /// status()
    pub fn status(&self) -> V {
        let b = self.inner.borrow();
        let state = b.state.clone().unwrap_or(V::Null);
        let seed = b.seed.clone();
        let seed_state = get(&seed, "state");
        let mut unknown = get(&get(&state, "projection"), "unknown")
            .as_bool()
            .unwrap_or(false);
        if !state.is_null() && !b.closed {
            let inner2 = self.inner.clone();
            drop(b);
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                source(&inner2).map(|s| get(&s, "unknown") == &json!(true))
            })) {
                Ok(Ok(u)) => unknown = unknown || u,
                _ => unknown = true,
            }
            return Self::status_with(&self.inner, &state, unknown);
        }
        Self::status_with(&self.inner, &state, unknown)
    }

    fn status_with(inner: &Rc<RefCell<BudgetInner>>, state: &V, unknown: bool) -> V {
        let b = inner.borrow();
        let seed_state = get(&b.seed, "state");
        let (jseq, jprev) = b
            .journal
            .as_ref()
            .map(|j| (j.sequence, j.previous.clone()))
            .unwrap_or((0, String::new()));
        let realized_fallback = get(seed_state, "inventory")
            .as_object()
            .map(|m| {
                money(m.values().map(|x| {
                    amount(get(get(x, "proof"), "cumulativeRealizedPnl").as_str().unwrap_or(""))
                        .unwrap_or(0)
                }).sum())
            })
            .unwrap_or_else(|| "0.000000".to_string());
        let spent_fallback = get(seed_state, "orders")
            .as_array()
            .map(|a| {
                money(a.iter().map(|o| {
                    if get(get(o, "body"), "action").as_str() == Some("buy") {
                        amount(
                            get(get(o, "proof"), "scopedPrincipalCost")
                                .as_str()
                                .unwrap_or("0.000000"),
                        )
                        .unwrap_or(0)
                    } else {
                        0
                    }
                }).sum::<i128>())
            })
            .unwrap_or_else(|| "0.000000".to_string());
        json!({
            "mode": "source_verified_owned_hedge_budget_v2",
            "claimed": !state.is_null(),
            "allocationHash": b.allocation_hash,
            "budgetInstalledHash": b.budget_installed_hash,
            "seedHash": b.seed_hash,
            "migrationHash": b.migration_hash,
            "coordinatorCodeIdentityHash": b.coordinator_code_identity_hash,
            "sequence": jseq,
            "globalLedgerHash": jprev,
            "controllerReservationHash": get(&get(&get(state, "projection"), "record"), "reservationHash"),
            "realizedPnl": if !get(&get(state, "projection"), "realizedPnl").is_null() {
                get(&get(state, "projection"), "realizedPnl").clone()
            } else { json!(realized_fallback) },
            "currentInventoryHash": if !get(&get(state, "projection"), "currentInventoryHash").is_null() {
                get(&get(state, "projection"), "currentInventoryHash").clone()
            } else {
                json!(payload_hash(get(seed_state, "inventory")).unwrap_or_default())
            },
            "pairRetirementsHash": if !get(&get(state, "projection"), "pairRetirementsHash").is_null() {
                get(&get(state, "projection"), "pairRetirementsHash").clone()
            } else {
                json!(payload_hash(&V::Array(vec![])).unwrap_or_default())
            },
            "cumulativeActualExpenses": if !get(state, "cumulativeActualExpenses").is_null() {
                get(state, "cumulativeActualExpenses").clone()
            } else { json!(spent_fallback) },
            "risk": get(state, "risk"),
            "unknownAcceptance": unknown,
            "actorAnchor": get(state, "actorAnchor"),
            "legacyAnchor": get(&b.seed, "anchor"),
            "cacheHits": b.cache_hits,
            "cacheMisses": b.cache_misses,
            "failed": b.failed,
            "closed": b.closed,
            "executionAuthority": false,
            "oldWriterHandbackAllowed": false,
            "additionalApiClients": 0,
        })
    }

    /// close()
    pub fn close(&self) {
        let mut b = self.inner.borrow_mut();
        if b.closed {
            return;
        }
        b.closed = true;
        if let Some(l) = b.lease.as_mut() {
            let _ = l.release();
        }
    }
}
