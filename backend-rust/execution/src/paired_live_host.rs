// execution/paired-live-host.mjs — optional ONE-pair host inside the existing
// collector. Its request and human authority audit are installed by the root
// task, not by agent/repository text.
use crate::core::{decimal_micros, payload_hash};
use crate::paired_rest_controller::{
    PairedRestController, PairedRestControllerDeps, WireDispatch, derive_paired_rest_plan,
    open_paired_rest_controller,
};
use crate::paired_rest_cycle::{PairedRestCycle, PairedRestCycleDeps, create_paired_rest_cycle};
use crate::paired_rest_evidence_bridge::{
    EvidenceBridgeDeps, PairedRestEvidenceBridge, create_paired_rest_evidence_bridge,
};
use crate::paired_shared_funding::{PairedSharedFunding, PairedSharedFundingDeps, open_paired_shared_funding};
use crate::paired_rest_controller::WireDispatchError;
use crate::transport::{Guard as TGuard, OrderTransportError};
use baal_service_runtime::atomic::atomic_json;
use crate::core::format_micros;
use baal_service_runtime::fixed::instant;
use baal_service_runtime::json_text::{js_stringify, sha256};
use serde_json::Value as V;
use serde_json::json;
use std::cell::RefCell;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;

const PREFIX: &str = "PAIRED_HOST_";
const TID: &str = "bda92870-621e-47b0-bc3c-3602c5c26f55";

fn need(ok: bool, code: &str) -> Result<(), String> {
    if !ok {
        return Err(format!("{PREFIX}{code}"));
    }
    Ok(())
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn iso(n: f64) -> String {
    baal_service_runtime::contract::ms_to_iso(n)
}
fn read(p: &Path) -> Result<V, String> {
    serde_json::from_str(&fs::read_to_string(p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn durable_json(file: &Path, value: &V) -> Result<(), String> {
    atomic_json(file, value).map_err(|e| format!("{e}:fatal:durableRecordingFailed"))
}
fn instant_v(v: &V) -> Result<i64, String> {
    instant(v.as_str().unwrap_or(""))
}
fn is_all_caps_code(s: &str) -> bool {
    !s.is_empty()
        && s.bytes().next().map(|c| c.is_ascii_uppercase()).unwrap_or(false)
        && s.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
}
fn run_id_ok(s: &str) -> bool {
    // /^sig-pilot-paired-[a-z0-9-]{1,60}$/
    s.strip_prefix("sig-pilot-paired-").is_some_and(|t| {
        !t.is_empty()
            && t.len() <= 60
            && t.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    })
}
fn id(x: &V) -> bool {
    x.as_str()
        .map(|s| {
            !s.is_empty()
                && s.bytes().all(|b| b.is_ascii_digit())
                && !s.starts_with('0')
                && s.parse::<i64>().is_ok()
        })
        .unwrap_or(false)
}

// pairedLiveIdentity(workspace)
pub fn paired_live_identity(workspace: &Path) -> Result<V, String> {
    crate::frozen::collect_frozen_paper_files(
        workspace,
        &["execution/paired-live-host.mjs", "execution/scoped-rest-receipts.mjs"],
    )
}

// The mutable shared api object the caller owns: {dispatch,manualReview,cooldown,next,onAdmission}.
pub struct PairedLiveApi {
    pub dispatch: Rc<WireDispatch>,
    pub manual_review: bool,
    pub cooldown: f64,
    pub next: f64,
    pub on_admission: Box<dyn FnMut(Option<f64>)>,
}

#[allow(clippy::type_complexity)]
pub struct PairedLiveHostDeps {
    pub receipt_of: Rc<dyn Fn(&V, &V) -> Result<V, String>>,
    pub get_account_lease: Rc<dyn Fn() -> V>,
    pub get_other_state: Rc<dyn Fn() -> V>,
    pub can_operate: Rc<dyn Fn() -> bool>,
    pub has_read_headroom: Rc<dyn Fn(i64) -> bool>,
    pub now: Rc<dyn Fn() -> f64>,
    pub workspace: PathBuf,
}

pub struct PairedLiveHost {
    inner: Rc<RefCell<HostInner>>,
}

struct HostInner {
    request: V,
    request_hash: String,
    file: PathBuf,
    authority_file: PathBuf,
    authority_hash: String,
    code: V,
    dir: PathBuf,
    controller_dir: PathBuf,
    registry_dir: PathBuf,
    runtime_dir: PathBuf,
    api: Rc<RefCell<PairedLiveApi>>,
    deps: PairedLiveHostDeps,
    controller: Option<Rc<PairedRestController>>,
    registry: Option<PairedSharedFunding>,
    bridge: Option<Rc<PairedRestEvidenceBridge>>,
    cycle: Option<Rc<PairedRestCycle>>,
    last_funding: Option<V>,
    closed: bool,
    last_failure: Option<String>,
}

fn guard_err(code: &str) -> OrderTransportError {
    OrderTransportError {
        code: code.into(),
        status: None,
        api_code: None,
        outcome: "blocked".into(),
        may_have_executed: false,
        retry_same_request_only: false,
        retry_after_ms: 0.0,
        semantic_mismatch: false,
    }
}

fn allowed(inner: &Rc<RefCell<HostInner>>) -> bool {
    (|| -> Result<bool, String> {
        let g = inner.borrow();
        if g.closed || !(g.deps.can_operate)() {
            return Ok(false);
        }
        let file = g.file.clone();
        let authority_file = g.authority_file.clone();
        let request_hash = g.request_hash.clone();
        let authority_hash = g.authority_hash.clone();
        let code_hash = payload_hash(&g.code)?;
        let workspace = g.deps.workspace.clone();
        drop(g);
        Ok(payload_hash(&read(&file)?)? == request_hash
            && payload_hash(&read(&authority_file)?)? == authority_hash
            && payload_hash(&paired_live_identity(&workspace)?)? == code_hash)
    })()
    .unwrap_or(false)
}

fn entry_allowed(inner: &Rc<RefCell<HostInner>>) -> bool {
    let (n, start, expires) = {
        let g = inner.borrow();
        (
            (g.deps.now)(),
            instant_v(get(&g.request, "startedAt")).unwrap_or(i64::MIN) as f64,
            instant_v(get(&g.request, "entryExpiresAt")).unwrap_or(i64::MIN) as f64,
        )
    };
    allowed(inner) && n >= start && n < expires
}

fn has_durable_actor(inner: &Rc<RefCell<HostInner>>) -> bool {
    let actor = inner.borrow().controller_dir.join("paired-rest-journal.jsonl");
    actor.exists() && fs::metadata(&actor).map(|m| m.len() > 0).unwrap_or(false)
}

fn blocks_legacy(inner: &Rc<RefCell<HostInner>>) -> bool {
    let g = inner.borrow();
    match &g.registry {
        Some(r) => r.exclusive_active(),
        None => has_durable_actor(inner),
    }
}

// globalRisk() — shared uncertainty fence seen by the bridge and reconciler.
fn global_risk(inner: &Rc<RefCell<HostInner>>) -> V {
    let g = inner.borrow();
    let core = (g.deps.get_other_state)();
    let pair = g.controller.as_ref().map(|c| c.status());
    let mut unresolved: Vec<V> = get(&core, "intents")
        .as_object()
        .map(|m| {
            m.values()
                .filter(|i| get(i, "status").as_str() != Some("closed"))
                .map(|i| get(i, "intentId").clone())
                .collect()
        })
        .unwrap_or_default();
    if let Some(p) = &pair {
        for i in get(&p, "orders").as_array().cloned().unwrap_or_default() {
            if get(&i, "phase").as_str() != Some("reconciled") {
                unresolved.push(get(&i, "intentId").clone());
            }
        }
    }
    json!({
        "ownerId": "Codex",
        "runId": get(&g.request, "runId"),
        "tournamentId": TID,
        "complete": true,
        "soleCollector": true,
        "stopped": !allowed(inner),
        "at": iso((g.deps.now)()),
        "unresolvedIntentIds": unresolved,
        "sourceHash": payload_hash(&json!({
            "core": core,
            "paired": pair,
            "accountLease": (g.deps.get_account_lease)(),
        })).unwrap_or_default(),
    })
}

fn publish(inner: &Rc<RefCell<HostInner>>, action: &str) -> Result<(), String> {
    let (s, run_id, stats_file, authority_hash, last_failure, n) = {
        let g = inner.borrow();
        (
            g.controller.as_ref().map(|c| c.status()),
            get(&g.request, "runId").as_str().unwrap_or("").to_string(),
            g.runtime_dir.join("paired-live-stats.json"),
            g.authority_hash.clone(),
            g.last_failure.clone(),
            (g.deps.now)(),
        )
    };
    let phase = s
        .as_ref()
        .map(|x| get(x, "phase").as_str().unwrap_or("awaiting_fresh_candidate").to_string())
        .unwrap_or_else(|| "awaiting_fresh_candidate".to_string());
    let orders: Vec<V> = s
        .as_ref()
        .map(|x| get(x, "orders").as_array().cloned().unwrap_or_default())
        .unwrap_or_default()
        .iter()
        .map(|o| {
            json!({
                "intentId": get(o, "intentId"),
                "orderId": get(o, "orderId"),
                "marketId": get(o, "marketId"),
                "exchangeId": get(o, "exchangeId"),
                "phase": get(o, "phase"),
                "limit": get(&get(o, "body"), "price"),
                "quantity": 1,
                "cancelPending": get(o, "cancelPending"),
                "proofHash": get(o, "proofHash"),
            })
        })
        .collect();
    let ordinary_surplus = if phase == "paired" {
        let owned_principal = get(&get(s.as_ref().unwrap(), "funding"), "ownedPrincipal");
        Some(format_micros(1_000_000 - decimal_micros(owned_principal, "exact").unwrap_or(0)))
    } else {
        None
    };
    let stats = json!({
        "schemaVersion": 1,
        "observedAt": iso(n),
        "runId": run_id,
        "ownerId": "Codex",
        "mode": "official_one_pair_rest",
        "phase": phase,
        "action": action,
        "failed": s.as_ref().map(|x| get(x, "failed").as_bool().unwrap_or(false)).unwrap_or(false),
        "lastFailure": last_failure.map(V::from).unwrap_or(V::Null),
        "orders": orders,
        "owned": s.as_ref().map(|x| get(x, "owned").clone()).unwrap_or(json!([])),
        "funding": s.as_ref().map(|x| get(x, "funding").clone()).unwrap_or(V::Null),
        "realizedPnl": "0.000000",
        "ordinaryMinimumPayout": if phase == "paired" { json!("1.000000") } else { V::Null },
        "ordinarySurplus": ordinary_surplus.map(|v| json!(v)).unwrap_or(V::Null),
        "ordinaryResolutionOnly": true,
        "maximumExceptionalLoss": s
            .as_ref()
            .and_then(|x| x.get("funding"))
            .map(|f| get(f, "ownedPrincipal").clone())
            .unwrap_or(json!("0.000000")),
        "profitabilityEstablished": false,
        "additionalPersistentProcesses": 0,
        "authorityHash": authority_hash,
    });
    durable_json(&stats_file, &stats)
}

fn cleanup(inner: &Rc<RefCell<HostInner>>) -> Result<(), String> {
    let mut error: Option<String> = None;
    let (cycle, bridge, registry, controller) = {
        let g = inner.borrow();
        (
            g.cycle.is_some(),
            g.bridge.is_some(),
            g.registry.is_some(),
            g.controller.is_some(),
        )
    };
    if cycle {
        if let Err(e) = inner.borrow().cycle.as_ref().unwrap().close() {
            if error.is_none() {
                error = Some(e);
            }
        }
    }
    if bridge {
        if let Err(e) = inner.borrow().bridge.as_ref().unwrap().close() {
            if error.is_none() {
                error = Some(e);
            }
        }
    }
    if registry {
        inner.borrow().registry.as_ref().unwrap().close();
    }
    if controller {
        if let Err(e) = inner.borrow().controller.as_ref().unwrap().close() {
            if error.is_none() {
                error = Some(e);
            }
        }
    }
    {
        let mut g = inner.borrow_mut();
        g.cycle = None;
        g.bridge = None;
        g.registry = None;
        g.controller = None;
        g.last_funding = None;
    }
    match error {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

fn is_durable_error(code: &str) -> bool {
    ["EIO", "ENOSPC", "EROFS", "EPERM", "EACCES", "EBUSY"].iter().any(|c| code.contains(c))
}

fn initialize(inner: &Rc<RefCell<HostInner>>, capture: &V) -> Result<(), String> {
    let (dir, controller_dir, registry_dir, runtime_dir, workspace, scope, request, authority_hash) = {
        let g = inner.borrow();
        let scope = json!({
            "ownerId": "Codex",
            "runId": get(&g.request, "runId"),
            "tournamentId": TID,
            "tournamentSlug": get(&g.request, "tournamentSlug"),
            "markets": get(&g.request, "markets"),
        });
        (
            g.dir.clone(),
            g.controller_dir.clone(),
            g.registry_dir.clone(),
            g.runtime_dir.clone(),
            g.deps.workspace.clone(),
            scope,
            g.request.clone(),
            g.authority_hash.clone(),
        )
    };
    let capture_file = dir.join("capture.json");
    let restoring = has_durable_actor(inner);
    let capture = if restoring {
        need(capture_file.exists(), "PAIRED_HOST_CAPTURE_MISSING")?;
        read(&capture_file)?
    } else {
        need(
            get(capture, "phase").as_str() == Some("complete")
                && payload_hash(get(capture, "legs"))? == payload_hash(get(&request, "markets"))?,
            "PAIRED_HOST_CANDIDATE",
        )?;
        let plan = derive_paired_rest_plan(&json!({
            "capture": capture,
            "scope": scope,
            "readAt": iso((inner.borrow().deps.now)()),
        }))?;
        need(
            instant_v(get(&plan, "reviewExpiresAt"))?
                <= instant_v(get(&request, "entryExpiresAt"))?,
            "PAIRED_HOST_REVIEW_OUTSIDE_REQUEST",
        )?;
        capture.clone()
    };
    let init_result = (|| -> Result<(), String> {
        if !restoring {
            durable_json(&capture_file, &capture)?;
        }
        let registry_cell: Rc<RefCell<Option<PairedSharedFunding>>> = Rc::new(RefCell::new(None));
        let bridge_cell: Rc<RefCell<Option<Rc<PairedRestEvidenceBridge>>>> = Rc::new(RefCell::new(None));
        let last_funding: Rc<RefCell<Option<V>>> = Rc::new(RefCell::new(None));
        let inner_for_allowed = inner.clone();
        let can_operate_controller: Rc<dyn Fn() -> bool> =
            Rc::new(move || allowed(&inner_for_allowed));
        let inner_for_evidence = inner.clone();
        let bridge_for_evidence = bridge_cell.clone();
        let registry_for_install = registry_cell.clone();
        let funding_for_install = last_funding.clone();
        let api_dispatch = {
            let g = inner.borrow();
            g.api.borrow().dispatch.clone()
        };
        let get_account_lease = inner.borrow().deps.get_account_lease.clone();
        let raw_receipt_of = inner.borrow().deps.receipt_of.clone();
        let now = inner.borrow().deps.now.clone();
        let controller = open_paired_rest_controller(
            &controller_dir,
            &capture,
            &scope,
            PairedRestControllerDeps {
                dispatch: api_dispatch,
                can_operate: can_operate_controller.clone(),
                assert_account_lease: get_account_lease.clone(),
                install_funding_reservation: Rc::new(move |r: &V| -> Result<V, String> {
                    let receipt = registry_for_install
                        .borrow()
                        .as_ref()
                        .ok_or("PAIRED_HOST_REGISTRY_UNAVAILABLE")?
                        .install_funding_reservation(r)?;
                    *funding_for_install.borrow_mut() = Some(receipt.clone());
                    Ok(receipt)
                }),
                get_evidence: Rc::new(move |context: &V| -> Result<V, String> {
                    let b = bridge_for_evidence.borrow();
                    b.as_ref()
                        .ok_or("PAIRED_HOST_BRIDGE_UNAVAILABLE".to_string())?
                        .get_evidence(context)
                }),
                raw_receipt_of,
                now: now.clone(),
                workspace_dir: workspace.clone(),
            },
        )?;
        let controller = Rc::new(controller);
        let slot_file = dir.join("activation.json");
        let activation = if slot_file.exists() {
            read(&slot_file)?
        } else {
            let status = controller.status();
            let activation = json!({
                "schemaVersion": 1,
                "kind": "one_pair_shared_funding",
                "ownerId": "Codex",
                "runId": get(&request, "runId"),
                "tournamentId": TID,
                "activationAuthorityHash": authority_hash,
                "planHash": get(&status, "planHash"),
                "captureHash": payload_hash(&capture)?,
                "registryDir": registry_dir,
                "controllerDir": controller_dir,
                "startedAt": iso(now()),
                "entryExpiresAt": iso(instant_v(get(&status, "reviewExpiresAt"))?
                    .min(instant_v(get(&request, "entryExpiresAt"))?) as f64),
                "allocatedCapital": "2.000000",
                "maxOrderCost": "1.000000",
                "maxAggregateCost": "10.000000",
                "maxCumulativeBuyCost": "25.000000",
                "maxManagedMarkets": 3,
                "quantity": 1,
                "maxPairs": 1,
            });
            durable_json(&slot_file, &activation)?;
            activation
        };
        let status = controller.status();
        need(
            get(&activation, "activationAuthorityHash").as_str() == Some(authority_hash.as_str())
                && get(&activation, "runId") == get(&request, "runId")
                && get(&activation, "planHash") == get(&status, "planHash")
                && get(&activation, "captureHash").as_str()
                    == Some(payload_hash(&capture)?.as_str())
                && instant_v(get(&activation, "startedAt"))? >= instant_v(get(&request, "startedAt"))?
                && get(&activation, "entryExpiresAt") == get(&status, "reviewExpiresAt")
                && instant_v(get(&activation, "entryExpiresAt"))?
                    <= instant_v(get(&request, "entryExpiresAt"))?,
            "PAIRED_HOST_ACTIVATION_BINDING",
        )?;
        let get_other_state = inner.borrow().deps.get_other_state.clone();
        let controller_for_funding = controller.clone();
        let controller_for_status = controller.clone();
        let registry = open_paired_shared_funding(
            &registry_dir,
            &activation,
            &controller_dir,
            PairedSharedFundingDeps {
                get_account_lease: get_account_lease.clone(),
                get_other_state: get_other_state.clone(),
                get_paired_funding: Rc::new(move || controller_for_funding.funding_reservation()),
                get_paired_status: Rc::new(move || controller_for_status.status()),
                can_operate: can_operate_controller.clone(),
                now: now.clone(),
            },
        )?;
        let first_funding = registry.install_funding_reservation(&controller.funding_reservation())?;
        *last_funding.borrow_mut() = Some(first_funding);
        let receipt_of = inner.borrow().deps.receipt_of.clone();
        let inner_for_risk = inner.clone();
        let funding_for_bridge = last_funding.clone();
        let get_other_state_b = get_other_state.clone();
        let controller_for_owns = controller.clone();
        let inner_for_can_read = inner.clone();
        let has_read_headroom = inner.borrow().deps.has_read_headroom.clone();
        let bridge = Rc::new(create_paired_rest_evidence_bridge(
            EvidenceBridgeDeps {
                dispatch: {
                    let api = inner.borrow().api.borrow().dispatch.clone();
                    Rc::new(
                        move |spec: &V,
                              bd: Option<&dyn Fn(&V, &V) -> Result<bool, String>>,
                              bn: Option<&dyn Fn(&V, &V) -> Result<bool, String>>|
                              -> Result<V, String> {
                            let bd_t = |p: &V, s: &V| -> Result<bool, OrderTransportError> {
                                bd.map(|g| g(p, s).map_err(|c| guard_err(&c))).unwrap_or(Ok(false))
                            };
                            let bn_t = |p: &V, s: &V| -> Result<bool, OrderTransportError> {
                                bn.map(|g| g(p, s).map_err(|c| guard_err(&c))).unwrap_or(Ok(false))
                            };
                            api(
                                spec,
                                bd.map(|_| &bd_t as &TGuard),
                                bn.map(|_| &bn_t as &TGuard),
                            )
                            .map_err(|e: WireDispatchError| e.message)
                        },
                    )
                },
                receipt_of,
                get_global_risk: Rc::new(move || global_risk(&inner_for_risk)),
                get_funding: Rc::new(move || funding_for_bridge.borrow().clone().unwrap_or(V::Null)),
                get_other_reserved: Rc::new(move || {
                    let s = get_other_state_b();
                    let total: i128 = get(&s, "intents")
                        .as_object()
                        .map(|m| {
                            m.values()
                                .filter(|i| {
                                    get(i, "status").as_str() != Some("closed")
                                        && get(i, "action").as_str() == Some("buy")
                                })
                                .map(|i| {
                                    let remaining = get(i, "quantity").as_i64().unwrap_or(0)
                                        - get(i, "reconciledFills")
                                            .as_array()
                                            .map(|fs| {
                                                fs.iter()
                                                    .map(|f| get(f, "quantity").as_i64().unwrap_or(0))
                                                    .sum::<i64>()
                                            })
                                            .unwrap_or(0);
                                    decimal_micros(get(i, "price"), "exact").unwrap_or(0)
                                        * remaining as i128
                                })
                                .sum()
                        })
                        .unwrap_or(0);
                    json!(format_micros(total))
                }),
                owns_order: Rc::new(move |oid: &V, leg: &V| -> bool {
                    controller_for_owns.status().get("orders").and_then(|x| x.as_array()).map(|os| {
                        os.iter().any(|o| {
                            get(o, "orderId") == oid
                                && get(o, "marketId") == get(leg, "marketId")
                                && get(o, "exchangeId") == get(leg, "exchangeId")
                        })
                    }).unwrap_or(false)
                }),
                can_read: Rc::new(move || allowed(&inner_for_can_read)),
                has_read_headroom,
                now: now.clone(),
            },
            &scope,
            Some(&json!({"preparation": 5, "reconciliation": 6, "cancel": 1})),
        )?);
        *bridge_cell.borrow_mut() = Some(bridge.clone());
        let inner_for_entry = inner.clone();
        let activation_expires = instant_v(get(&activation, "entryExpiresAt"))? as f64;
        let inner_for_recovery = inner.clone();
        let api_for_recovery = inner.borrow().api.clone();
        let inner_for_status = inner.clone();
        let cycle = create_paired_rest_cycle(
            controller.clone(),
            bridge.clone(),
            PairedRestCycleDeps {
                entry_allowed: Rc::new(move || {
                    entry_allowed(&inner_for_entry) && (inner_for_entry.borrow().deps.now)() < activation_expires
                }),
                can_operate: can_operate_controller.clone(),
                recovery_allowed: Rc::new(move || {
                    let api = api_for_recovery.borrow();
                    let n = (inner_for_recovery.borrow().deps.now)();
                    !api.manual_review
                        && api.cooldown.is_finite()
                        && api.next.is_finite()
                        && api.cooldown.max(api.next) <= n
                }),
                now: now.clone(),
                on_status: Rc::new(move |r: &V| {
                    let action = get(r, "action").as_str().unwrap_or("").to_string();
                    let _ = publish(&inner_for_status, &action);
                }),
            },
            None,
        )?;
        *registry_cell.borrow_mut() = Some(registry);
        {
            let mut g = inner.borrow_mut();
            g.controller = Some(controller);
            g.registry = registry_cell.borrow_mut().take();
            g.bridge = Some(bridge);
            g.cycle = Some(Rc::new(cycle));
            g.last_funding = last_funding.borrow().clone();
        }
        Ok(())
    })();
    match init_result {
        Ok(()) => Ok(()),
        Err(e) => {
            let durable_failed = {
                let g = inner.borrow();
                g.controller
                    .as_ref()
                    .map(|c| get(&c.status(), "failed").as_bool().unwrap_or(false))
                    .unwrap_or(false)
                    || g.registry
                        .as_ref()
                        .map(|r| get(&r.status(), "failed").as_bool().unwrap_or(false))
                        .unwrap_or(false)
                    || is_durable_error(&e)
            };
            let _ = cleanup(inner);
            if durable_failed {
                return Err(format!("{e}:fatal:durableRecordingFailed"));
            }
            Err(e)
        }
    }
}



// openPairedLiveHost({runtimeDir,workspace,api,receiptOf,getAccountLease,getOtherState,canOperate,hasReadHeadroom,now})
#[allow(clippy::too_many_arguments)]
pub fn open_paired_live_host(
    runtime_dir: &Path,
    api: Rc<RefCell<PairedLiveApi>>,
    deps: PairedLiveHostDeps,
) -> Result<Option<PairedLiveHost>, String> {
    let file = runtime_dir.join("paired-live-request.json");
    if !file.exists() {
        return Ok(None);
    }
    let request = read(&file)?;
    let request_hash = payload_hash(&request)?;
    let authority_file = runtime_dir.join("paired-live-authority.json");
    let authority = read(&authority_file)?;
    let authority_hash = payload_hash(&authority)?;
    need(
        get(&request, "schemaVersion") == &json!(1)
            && get(&request, "enabled") == &json!(true)
            && get(&request, "ownerId").as_str() == Some("Codex")
            && get(&request, "tournamentId").as_str() == Some(TID)
            && get(&request, "tournamentSlug").as_str() == Some("midterm-elections")
            && get(&request, "runId").as_str().map(run_id_ok).unwrap_or(false),
        "PAIRED_HOST_REQUEST",
    )?;
    need(
        get(&authority, "kind").as_str() == Some("direct_human_authorized_bot_update")
            && get(&authority, "ownerId").as_str() == Some("Codex")
            && get(&authority, "threadId").as_str() == Some("01a0f8cc-b9bb-77d2-9f4c-2844396f1b79")
            && get(&authority, "exploratory") == &json!(true)
            && get(&authority, "profitabilityEstablished") == &json!(false)
            && get(&request, "authorityHash").as_str() == Some(authority_hash.as_str()),
        "PAIRED_HOST_AUTHORITY",
    )?;
    let markets = get(&request, "markets").as_array().cloned().unwrap_or_default();
    need(
        markets.len() == 2
            && markets
                .iter()
                .map(|m| get(m, "marketId").as_str().unwrap_or(""))
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                == 2
            && markets
                .iter()
                .map(|m| get(m, "exchangeId").as_str().unwrap_or(""))
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                == 2
            && markets.iter().all(|m| id(get(m, "marketId")) && id(get(m, "exchangeId"))),
        "PAIRED_HOST_MARKETS",
    )?;
    let start = instant_v(get(&request, "startedAt"))?;
    let expires = instant_v(get(&request, "entryExpiresAt"))?;
    need(
        expires > start && expires - start <= 3600000,
        "PAIRED_HOST_WINDOW",
    )?;
    let code = paired_live_identity(&deps.workspace)?;
    need(
        payload_hash(get(&request, "codeIdentity"))? == payload_hash(&code)?,
        "PAIRED_HOST_CODE",
    )?;
    let dir = runtime_dir.join("paired-live-runs").join(get(&request, "runId").as_str().unwrap_or(""));
    let controller_dir = dir.join("controller");
    let registry_dir = dir.join("funding");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    let inner = Rc::new(RefCell::new(HostInner {
        request,
        request_hash,
        file,
        authority_file,
        authority_hash,
        code,
        dir,
        controller_dir,
        registry_dir,
        runtime_dir: runtime_dir.to_path_buf(),
        api,
        deps,
        controller: None,
        registry: None,
        bridge: None,
        cycle: None,
        last_funding: None,
        closed: false,
        last_failure: None,
    }));
    let open_result = (|| -> Result<(), String> {
        if has_durable_actor(&inner) {
            initialize(&inner, &V::Null)?;
        }
        publish(&inner, "host_loaded")
    })();
    if let Err(e) = open_result {
        let _ = cleanup(&inner);
        return Err(e);
    }
    Ok(Some(PairedLiveHost { inner }))
}

impl PairedLiveHost {
    // authorizes(spec) — GET side: the bridge-issued guard on the spec.
    pub fn authorizes(
        &self,
        spec: &V,
        before_dispatch: &dyn Fn(&V, &V) -> Result<bool, String>,
        before_network: &dyn Fn(&V, &V) -> Result<bool, String>,
    ) -> bool {
        let g = self.inner.borrow();
        g.bridge
            .as_ref()
            .map(|b| b.authorizes(spec, before_dispatch, before_network))
            .unwrap_or(false)
    }

    // authorizes(spec) — write side: the transport-issued guard on the spec.
    pub fn authorizes_write(
        &self,
        spec: &V,
        before_dispatch: Option<&TGuard<'_>>,
        before_network: Option<&TGuard<'_>>,
    ) -> bool {
        let g = self.inner.borrow();
        g.controller
            .as_ref()
            .map(|c| c.authorizes(spec, before_dispatch, before_network))
            .unwrap_or(false)
    }

    pub fn on_fetch_start(&self, event: &V) -> bool {
        self.inner
            .borrow()
            .controller
            .as_ref()
            .and_then(|c| c.on_fetch_start(event).ok())
            .unwrap_or(false)
    }

    pub fn blocks_legacy_writes(&self) -> bool {
        blocks_legacy(&self.inner)
    }

    pub fn allows_legacy(&self, spec: &V) -> bool {
        let g = self.inner.borrow();
        g.registry
            .as_ref()
            .map(|r| r.allows_other(spec))
            .unwrap_or_else(|| !blocks_legacy(&self.inner))
    }

    pub fn step(&self, capture: &V) -> Result<(), String> {
        if !allowed(&self.inner) {
            return Ok(());
        }
        let result = (|| -> Result<(), String> {
            {
                let has_controller = self.inner.borrow().controller.is_some();
                if !has_controller {
                    if !entry_allowed(&self.inner)
                        || get(capture, "phase").as_str() != Some("complete")
                        || payload_hash(get(capture, "legs"))?
                            != payload_hash(get(&self.inner.borrow().request, "markets"))?
                    {
                        return Ok(());
                    }
                    initialize(&self.inner, capture)?;
                }
            }
            self.inner.borrow_mut().last_failure = None;
            let cycle = self.inner.borrow().cycle.as_ref().map(|c| c.clone());
            if let Some(c) = cycle {
                c.step()?;
            }
            Ok(())
        })();
        match result {
            Ok(()) => Ok(()),
            Err(e) => {
                // recordBackoff: the ported error channel carries no
                // retryAfterMs, so there is nothing to persist here.
                let failure = if is_all_caps_code(&e) {
                    e.clone()
                } else {
                    format!("{PREFIX}OPERATION_UNAVAILABLE")
                };
                self.inner.borrow_mut().last_failure = Some(failure);
                publish(&self.inner, "operation_blocked")?;
                if e.contains(":fatal:") || e.contains("durableRecordingFailed") {
                    return Err(e);
                }
                Ok(())
            }
        }
    }

    pub fn status(&self) -> V {
        let g = self.inner.borrow();
        let s = g.controller.as_ref().map(|c| c.status());
        json!({
            "runId": get(&g.request, "runId"),
            "initialized": g.controller.is_some(),
            "phase": s.as_ref().map(|x| get(x, "phase").clone()).unwrap_or(json!("awaiting_fresh_candidate")),
            "lastFailure": g.last_failure.clone().map(V::from).unwrap_or(V::Null),
            "blocksLegacyWrites": blocks_legacy(&self.inner),
            "orders": s.as_ref().map(|x| get(x, "orders").as_array().map(|a| a.len()).unwrap_or(0)).unwrap_or(0),
        })
    }

    pub fn close(&self) -> Result<(), String> {
        {
            let g = self.inner.borrow();
            if g.closed {
                return Ok(());
            }
        }
        self.inner.borrow_mut().closed = true;
        cleanup(&self.inner)
    }
}
