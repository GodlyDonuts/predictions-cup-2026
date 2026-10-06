// execution/paired-live-host.mjs — optional ONE-pair host inside the existing
// collector. Its request and human authority audit are installed by the root
// task, not by agent/repository text.
use baal_execution::core::{decimal_micros, format_micros, payload_hash};
use baal_service_runtime::atomic::atomic_json;
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::json_text::js_stringify;
use baal_execution::transport::OrderTransportError;
use serde_json::{json, Value as V};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

const TID: &str = "bda92870-621e-47b0-bc3c-3602c5c26f55";
fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(format!("PAIRED_HOST_{code}")) }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V { v.get(k).unwrap_or(&V::Null) }
fn iso(n: i64) -> String { ms_to_iso(n as f64) }
fn parse_ms(s: &str) -> Option<i64> { parse_iso_ms(s).map(|x| x as i64) }
fn read(p: &Path) -> Result<V, String> {
    serde_json::from_str(&std::fs::read_to_string(p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn durable_json(file: &Path, value: &V) -> Result<(), String> {
    atomic_json(file, value)
}
pub fn paired_live_identity(workspace: &Path) -> V {
    json!(baal_collector::frozen_paper_graph::collect_frozen_paper_files(
        workspace,
        &["execution/paired-live-host.mjs".to_string(), "execution/scoped-rest-receipts.mjs".to_string()],
    ).unwrap_or_default())
}

/// The pre-existing serialized read/write client the host is bound to.
pub trait HostApi {
    fn dispatch(&self, spec: &V) -> Result<V, OrderTransportError>;
    fn manual_review(&self) -> bool;
    fn set_manual_review(&self, v: bool);
    fn cooldown(&self) -> f64;
    fn set_cooldown(&self, v: f64);
    fn next(&self) -> f64;
    fn set_next(&self, v: f64);
    fn on_admission(&self, next: Option<f64>);
}

type GetV = Rc<dyn Fn() -> Result<V, String>>;

pub struct PairedLiveHost {
    runtime_dir: PathBuf,
    workspace: PathBuf,
    api: Rc<dyn HostApi>,
    receipt_of: Rc<dyn Fn(&V, &V) -> Result<V, String>>,
    get_account_lease: GetV,
    get_other_state: GetV,
    can_operate: Rc<dyn Fn() -> bool>,
    has_read_headroom: Rc<dyn Fn(usize) -> bool>,
    now: Rc<dyn Fn() -> i64>,
    file: PathBuf,
    authority_file: PathBuf,
    request: V,
    request_hash: String,
    authority_hash: String,
    code: V,
    dir: PathBuf,
    controller_dir: PathBuf,
    registry_dir: PathBuf,
    actor_file: PathBuf,
    controller: RefCell<Option<Rc<crate::paired_rest_controller::PairedRestController>>>,
    registry: RefCell<Option<Rc<crate::paired_shared_funding::PairedSharedFunding>>>,
    bridge: RefCell<Option<Rc<crate::paired_rest_evidence_bridge::PairedRestEvidenceBridge>>>,
    cycle: RefCell<Option<Rc<crate::paired_rest_cycle::PairedRestCycle>>>,
    last_funding: RefCell<V>,
    activation: RefCell<V>,
    closed: RefCell<bool>,
    last_failure: RefCell<V>,
}

impl PairedLiveHost {
    #[allow(clippy::too_many_arguments)]
    pub fn open(
        runtime_dir: &Path,
        workspace: &Path,
        api: Rc<dyn HostApi>,
        receipt_of: Rc<dyn Fn(&V, &V) -> Result<V, String>>,
        get_account_lease: GetV,
        get_other_state: GetV,
        can_operate: Rc<dyn Fn() -> bool>,
        has_read_headroom: Rc<dyn Fn(usize) -> bool>,
        now: Rc<dyn Fn() -> i64>,
    ) -> Result<Option<Rc<Self>>, String> {
        let file = runtime_dir.join("paired-live-request.json");
        if !file.exists() { return Ok(None); }
        let request = read(&file)?;
        let request_hash = payload_hash(&request)?;
        let authority_file = runtime_dir.join("paired-live-authority.json");
        let authority = read(&authority_file)?;
        let authority_hash = payload_hash(&authority)?;
        need(
            get(&request, "schemaVersion") == &json!(1)
                && get(&request, "enabled") == &json!(true)
                && get(&request, "ownerId") == &json!("Codex")
                && get(&request, "tournamentId") == &json!(TID)
                && get(&request, "tournamentSlug") == &json!("midterm-elections")
                && get(&request, "runId").as_str().is_some_and(|s| {
                    s.len() >= "sig-pilot-paired-".len() + 1
                        && s.len() <= "sig-pilot-paired-".len() + 60
                        && s.starts_with("sig-pilot-paired-")
                        && s.bytes().skip("sig-pilot-paired-".len())
                            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                }),
            "REQUEST",
        )?;
        need(
            get(&authority, "kind") == &json!("direct_human_authorized_bot_update")
                && get(&authority, "ownerId") == &json!("Codex")
                && get(&authority, "threadId") == &json!("01a0f8cc-b9bb-77d2-9f4c-2844396f1b79")
                && get(&authority, "exploratory") == &json!(true)
                && get(&authority, "profitabilityEstablished") == &json!(false)
                && get(&request, "authorityHash").as_str() == Some(authority_hash.as_str()),
            "AUTHORITY",
        )?;
        need(
            get(&request, "markets").as_array().map(|ms| {
                ms.len() == 2
                    && ms.iter().map(|m| js_stringify(m.get("marketId").unwrap_or(&V::Null))).collect::<std::collections::BTreeSet<_>>().len() == 2
                    && ms.iter().map(|m| js_stringify(m.get("exchangeId").unwrap_or(&V::Null))).collect::<std::collections::BTreeSet<_>>().len() == 2
                    && ms.iter().all(|m| {
                        ["marketId", "exchangeId"].iter().all(|k| {
                            m.get(k).and_then(|v| v.as_str()).is_some_and(|s| {
                                s.bytes().all(|b| b.is_ascii_digit()) && !s.starts_with('0')
                                    && s.parse::<i64>().map(|n| n <= i64::MAX && n >= 1).unwrap_or(false)
                            })
                        })
                    })
            }).unwrap_or(false),
            "MARKETS",
        )?;
        let started = get(&request, "startedAt").as_str().and_then(parse_ms);
        let expires = get(&request, "entryExpiresAt").as_str().and_then(parse_ms);
        need(
            started.is_some() && expires.is_some() && expires.unwrap() > started.unwrap()
                && expires.unwrap() - started.unwrap() <= 3_600_000,
            "WINDOW",
        )?;
        let code = paired_live_identity(workspace);
        need(payload_hash(get(&request, "codeIdentity")).ok() == payload_hash(&code).ok(), "CODE")?;
        let run_id = get(&request, "runId").as_str().unwrap().to_string();
        let dir = runtime_dir.join("paired-live-runs").join(&run_id);
        let controller_dir = dir.join("controller");
        let registry_dir = dir.join("funding");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        { use std::os::unix::fs::PermissionsExt; std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).ok(); }
        let this = Rc::new(Self {
            runtime_dir: runtime_dir.to_path_buf(),
            workspace: workspace.to_path_buf(),
            api,
            receipt_of,
            get_account_lease,
            get_other_state,
            can_operate,
            has_read_headroom,
            now,
            file,
            authority_file,
            request,
            request_hash,
            authority_hash,
            code,
            dir: dir.clone(),
            controller_dir,
            registry_dir,
            actor_file: dir.join("controller").join("paired-rest-journal.jsonl"),
            controller: RefCell::new(None),
            registry: RefCell::new(None),
            bridge: RefCell::new(None),
            cycle: RefCell::new(None),
            last_funding: RefCell::new(V::Null),
            activation: RefCell::new(V::Null),
            closed: RefCell::new(false),
            last_failure: RefCell::new(V::Null),
        });
        if this.has_durable_actor() { this.initialize(None)?; }
        this.publish("host_loaded")?;
        Ok(Some(this))
    }
    fn allowed(&self) -> bool {
        (|| -> Result<(), String> {
            need(!*self.closed.borrow(), "CLOSED")?;
            need((self.can_operate)(), "STOPPED")?;
            need(
                payload_hash(&read(&self.file)?).ok() == Some(self.request_hash.clone())
                    && payload_hash(&read(&self.authority_file)?).ok() == Some(self.authority_hash.clone())
                    && payload_hash(&paired_live_identity(&self.workspace)).ok() == payload_hash(&self.code).ok(),
                "IDENTITY",
            )?;
            Ok(())
        })().is_ok()
    }
    fn entry_allowed(&self) -> bool {
        self.allowed()
            && (self.now)() >= parse_ms(get(&self.request, "startedAt").as_str().unwrap_or("")).unwrap_or(i64::MIN)
            && (self.now)() < parse_ms(get(&self.request, "entryExpiresAt").as_str().unwrap_or("")).unwrap_or(i64::MAX)
    }
    /// Persist server-directed retry delays through the SAME shared limiter
    /// file. In-flight 409 delay starts at response/error observation.
    fn record_backoff(&self, error: &OrderTransportError) -> Result<(), String> {
        let delay = error.retry_after_ms;
        if !(delay > 0.0) { return Ok(()); }
        let api = &self.api;
        let n = (self.now)() as f64;
        if !(delay.fract() == 0.0 && delay <= i64::MAX as f64)
            || !((n + delay).fract() == 0.0)
            || n + delay > 8.640e15
        {
            api.set_manual_review(true);
            api.on_admission(None);
        } else {
            api.set_cooldown(api.cooldown().max(n + delay));
            api.set_next(api.next().max(api.cooldown()));
            api.on_admission(Some(api.next()));
        }
        Ok(())
    }
    fn global_risk(&self) -> Result<V, String> {
        let core = (self.get_other_state)()?;
        let pair = self.controller.borrow().as_ref().map(|c| c.status());
        let mut unresolved: Vec<V> = get(&core, "intents").as_object().map(|o| {
            o.values().filter(|i| get(i, "status") != &json!("closed")).map(|i| get(i, "intentId").clone()).collect()
        }).unwrap_or_default();
        if let Some(p) = &pair {
            unresolved.extend(get(p, "orders").as_array().map(|os| {
                os.iter().filter(|i| get(i, "phase") != &json!("reconciled")).map(|i| get(i, "intentId").clone()).collect::<Vec<V>>()
            }).unwrap_or_default());
        }
        Ok(json!({
            "ownerId": "Codex",
            "runId": get(&self.request, "runId"),
            "tournamentId": TID,
            "complete": true,
            "soleCollector": true,
            "stopped": !self.allowed(),
            "at": iso((self.now)()),
            "unresolvedIntentIds": unresolved,
            "sourceHash": payload_hash(&json!({
                "core": core,
                "paired": pair,
                "accountLease": (self.get_account_lease)()?,
            }))?,
        }))
    }
    fn publish(&self, action: &str) -> Result<(), String> {
        let s = self.controller.borrow().as_ref().map(|c| c.status());
        let phase = s.as_ref().map(|x| get(x, "phase").clone()).unwrap_or(json!("awaiting_fresh_candidate"));
        durable_json(&self.runtime_dir.join("paired-live-stats.json"), &json!({
            "schemaVersion": 1,
            "observedAt": iso((self.now)()),
            "runId": get(&self.request, "runId"),
            "ownerId": "Codex",
            "mode": "official_one_pair_rest",
            "phase": phase,
            "action": action,
            "failed": s.as_ref().map(|x| get(x, "failed") == &json!(true)).unwrap_or(false),
            "lastFailure": self.last_failure.borrow().clone(),
            "orders": s.as_ref().map(|x| get(x, "orders").as_array().map(|os| os.iter().map(|o| json!({
                "intentId": get(o, "intentId"),
                "orderId": get(o, "orderId"),
                "marketId": get(o, "marketId"),
                "exchangeId": get(o, "exchangeId"),
                "phase": get(o, "phase"),
                "limit": get(get(o, "body"), "price"),
                "quantity": 1,
                "cancelPending": get(o, "cancelPending"),
                "proofHash": get(o, "proofHash"),
            })).collect::<Vec<V>>()).unwrap_or_default()).unwrap_or_default(),
            "owned": s.as_ref().map(|x| get(x, "owned").clone()).unwrap_or(json!([])),
            "funding": s.as_ref().map(|x| get(x, "funding").clone()).unwrap_or(V::Null),
            "realizedPnl": "0.000000",
            "ordinaryMinimumPayout": if s.as_ref().map(|x| get(x, "phase") == &json!("paired")).unwrap_or(false) {
                json!("1.000000")
            } else { V::Null },
            "ordinarySurplus": if s.as_ref().map(|x| get(x, "phase") == &json!("paired")).unwrap_or(false) {
                json!(format_micros(1_000_000 - decimal_micros(get(s.as_ref().unwrap(), "funding").get("ownedPrincipal").unwrap_or(&V::Null), "exact").unwrap_or(0)))
            } else { V::Null },
            "ordinaryResolutionOnly": true,
            "maximumExceptionalLoss": s.as_ref().map(|x| get(get(x, "funding"), "ownedPrincipal").clone()).unwrap_or(json!("0.000000")),
            "profitabilityEstablished": false,
            "additionalPersistentProcesses": 0,
            "authorityHash": self.authority_hash,
        }))
    }
    fn cleanup(&self) -> Result<(), String> {
        let mut error: Option<String> = None;
        for handle in [
            self.cycle.borrow_mut().take().map(|c| c as Rc<dyn Closeable>).map(|c| c),
            self.bridge.borrow_mut().take().map(|c| c as Rc<dyn Closeable>),
            self.registry.borrow_mut().take().map(|c| c as Rc<dyn Closeable>),
            self.controller.borrow_mut().take().map(|c| c as Rc<dyn Closeable>),
        ].into_iter().flatten() {
            if let Err(e) = handle.close() { if error.is_none() { error = Some(e); } }
        }
        *self.last_funding.borrow_mut() = V::Null;
        match error { Some(e) => Err(e), None => Ok(()) }
    }
    fn has_durable_actor(&self) -> bool {
        self.actor_file.exists()
            && std::fs::metadata(&self.actor_file).map(|m| m.len() > 0).unwrap_or(false)
    }
    pub fn blocks_legacy_writes(&self) -> bool {
        self.registry.borrow().as_ref().map(|r| r.exclusive_active()).unwrap_or_else(|| self.has_durable_actor())
    }
    fn scope(&self) -> V {
        json!({
            "ownerId": "Codex",
            "runId": get(&self.request, "runId"),
            "tournamentId": TID,
            "tournamentSlug": get(&self.request, "tournamentSlug"),
            "markets": get(&self.request, "markets"),
        })
    }
    fn initialize(self: &Rc<Self>, mut capture: Option<V>) -> Result<(), String> {
        let capture_file = self.dir.join("capture.json");
        let restoring = self.has_durable_actor();
        if restoring {
            need(capture_file.exists(), "CAPTURE_MISSING")?;
            capture = Some(read(&capture_file)?);
        } else {
            need(
                capture.as_ref().map(|c| {
                    get(c, "phase") == &json!("complete")
                        && payload_hash(get(c, "legs")).ok() == payload_hash(get(&self.request, "markets")).ok()
                }).unwrap_or(false),
                "CANDIDATE",
            )?;
            let plan = crate::paired_rest_controller::derive_paired_rest_plan(&json!({
                "capture": capture,
                "scope": self.scope(),
                "readAt": iso((self.now)()),
            }))?;
            need(
                parse_ms(get(&plan, "reviewExpiresAt").as_str().unwrap_or("")).unwrap_or(i64::MAX)
                    <= parse_ms(get(&self.request, "entryExpiresAt").as_str().unwrap_or("")).unwrap_or(i64::MIN),
                "REVIEW_OUTSIDE_REQUEST",
            )?;
        }
        // Reject stale/malformed first captures before pinning any journal.
        // Persist the immutable capture first so a crash cannot strand an
        // actor without it.
        let result = (|| -> Result<(), String> {
            if !restoring { durable_json(&capture_file, capture.as_ref().unwrap())?; }
            let api = self.api.clone();
            let this = self.clone();
            let this2 = self.clone();
            let this3 = self.clone();
            let controller = crate::paired_rest_controller::PairedRestController::open(
                &self.controller_dir,
                capture.as_ref(),
                &self.scope(),
                Rc::new(move |s: &V, _d, _n| api.dispatch(s)),
                Rc::new(move || this.allowed()),
                self.get_account_lease.clone(),
                Rc::new(move |r: V| {
                    let reg = this2.registry.borrow();
                    let r2 = reg.as_ref().unwrap().install_funding_reservation(&r)?;
                    *this2.last_funding.borrow_mut() = r2.clone();
                    Ok(r2)
                }),
                Rc::new(move |context: &V| {
                    let b = this3.bridge.borrow();
                    b.as_ref().unwrap().get_evidence(context)
                }),
                self.receipt_of.clone(),
                self.now.clone(),
            )?;
            let slot_file = self.dir.join("activation.json");
            let activation: V;
            if slot_file.exists() {
                activation = read(&slot_file)?;
            } else {
                let status = controller.status();
                activation = json!({
                    "schemaVersion": 1,
                    "kind": "one_pair_shared_funding",
                    "ownerId": "Codex",
                    "runId": get(&self.request, "runId"),
                    "tournamentId": TID,
                    "activationAuthorityHash": self.authority_hash,
                    "planHash": get(&status, "planHash"),
                    "captureHash": payload_hash(capture.as_ref().unwrap())?,
                    "registryDir": self.registry_dir,
                    "controllerDir": self.controller_dir,
                    "startedAt": iso((self.now)()),
                    "entryExpiresAt": iso(std::cmp::min(
                        parse_ms(get(&status, "reviewExpiresAt").as_str().unwrap_or("")).unwrap_or(i64::MAX),
                        parse_ms(get(&self.request, "entryExpiresAt").as_str().unwrap_or("")).unwrap_or(i64::MIN),
                    )),
                    "allocatedCapital": "2.000000",
                    "maxOrderCost": "1.000000",
                    "maxAggregateCost": "10.000000",
                    "maxCumulativeBuyCost": "25.000000",
                    "maxManagedMarkets": 3,
                    "quantity": 1,
                    "maxPairs": 1,
                });
                durable_json(&slot_file, &activation)?;
            }
            need(
                get(&activation, "activationAuthorityHash").as_str() == Some(self.authority_hash.as_str())
                    && get(&activation, "runId") == get(&self.request, "runId")
                    && get(&activation, "planHash") == get(&controller.status(), "planHash")
                    && get(&activation, "captureHash").as_str() == payload_hash(capture.as_ref().unwrap()).ok().as_deref()
                    && parse_ms(get(&activation, "startedAt").as_str().unwrap_or("")).unwrap_or(i64::MIN)
                        >= parse_ms(get(&self.request, "startedAt").as_str().unwrap_or("")).unwrap_or(i64::MAX)
                    && get(&activation, "entryExpiresAt") == get(&controller.status(), "reviewExpiresAt")
                    && parse_ms(get(&activation, "entryExpiresAt").as_str().unwrap_or("")).unwrap_or(i64::MAX)
                        <= parse_ms(get(&self.request, "entryExpiresAt").as_str().unwrap_or("")).unwrap_or(i64::MIN),
                "ACTIVATION_BINDING",
            )?;
            let this4 = self.clone();
            let this5 = self.clone();
            let registry = Rc::new(crate::paired_shared_funding::PairedSharedFunding::open(
                &self.registry_dir,
                &activation,
                &self.controller_dir,
                self.get_account_lease.clone(),
                self.get_other_state.clone(),
                Rc::new(move || this4.controller.borrow().as_ref().unwrap().funding_reservation()),
                Rc::new(move || Ok(this5.controller.borrow().as_ref().unwrap().status())),
                {
                    let t = self.clone();
                    Rc::new(move || t.allowed())
                },
                self.now.clone(),
            )?);
            *self.last_funding.borrow_mut() = registry.install_funding_reservation(&controller.funding_reservation()?)?;
            let this6 = self.clone();
            let this7 = self.clone();
            let this8 = self.clone();
            let this9 = self.clone();
            let api2 = self.api.clone();
            let bridge = Rc::new(crate::paired_rest_evidence_bridge::PairedRestEvidenceBridge::new(
                Rc::new(move |s: &V, _d, _n| api2.dispatch(s).map_err(|e| e.code)),
                self.receipt_of.clone(),
                &self.scope(),
                Rc::new(move || this6.global_risk()),
                Rc::new(move || Ok(this7.last_funding.borrow().clone())),
                Rc::new(move || {
                    let s = (this8.get_other_state)()?;
                    Ok(json!(format_micros(get(&s, "intents").as_object().map(|o| o.values()
                        .filter(|i| get(i, "status") != &json!("closed") && get(i, "action") == &json!("buy"))
                        .map(|i| {
                            decimal_micros(get(i, "price"), "exact").unwrap_or(0) * {
                                let fills: i128 = get(i, "reconciledFills").as_array().map(|fs| fs.iter().map(|f| baal_service_runtime::json_text::js_int(get(f, "quantity")).unwrap_or(0) as i128).sum::<i128>()).unwrap_or(0);
                                baal_service_runtime::json_text::js_int(get(i, "quantity")).unwrap_or(0) as i128 - fills
                            }
                        }).sum::<i128>()).unwrap_or(0))))
                }),
                Rc::new(move |id: &V, leg: &V| {
                    this9.controller.borrow().as_ref().map(|c| {
                        get(&c.status(), "orders").as_array().map(|os| os.iter().any(|o| {
                            get(o, "orderId") == id && get(o, "marketId") == get(leg, "marketId") && get(o, "exchangeId") == get(leg, "exchangeId")
                        })).unwrap_or(false)
                    }).unwrap_or(false)
                }),
                {
                    let t = self.clone();
                    Rc::new(move || t.allowed())
                },
                {
                let h = self.has_read_headroom.clone();
                Rc::new(move |n: i64| -> Result<bool, String> { Ok((h)(n as usize)) }) as std::rc::Rc<dyn Fn(i64) -> Result<bool, String>>
            },
                self.now.clone(),
                Some(json!({"preparation": 5, "reconciliation": 6, "cancel": 1})),
            )?);
            *self.activation.borrow_mut() = activation.clone();
            let this10 = self.clone();
            let api3 = self.api.clone();
            let this11 = self.clone();
            let this12 = self.clone();
            let cycle = Rc::new(crate::paired_rest_cycle::PairedRestCycle::new(
                controller.clone(),
                bridge.clone(),
                Rc::new(move || {
                    this10.entry_allowed()
                        && (this10.now)() < parse_ms(get(&this10.activation.borrow(), "entryExpiresAt").as_str().unwrap_or("")).unwrap_or(i64::MAX)
                }),
                {
                    let t = self.clone();
                    Rc::new(move || t.allowed())
                },
                Rc::new(move || {
                    api3.manual_review() != true
                        && api3.cooldown().is_finite() && api3.next().is_finite()
                        && api3.cooldown().max(api3.next()) <= (this11.now)() as f64
                }),
                self.now.clone(),
                90_000,
                Rc::new(move |r: &V| { let _ = this12.publish(get(r, "action").as_str().unwrap_or("status")); }),
            )?);
            *self.controller.borrow_mut() = Some(controller);
            *self.registry.borrow_mut() = Some(registry);
            *self.bridge.borrow_mut() = Some(bridge);
            *self.cycle.borrow_mut() = Some(cycle);
            Ok(())
        })();
        if let Err(error) = result {
            let durable_failed = self.controller.borrow().as_ref().map(|c| get(&c.status(), "failed") == &json!(true)).unwrap_or(false)
                || self.registry.borrow().as_ref().map(|r| get(&r.status(), "failed") == &json!(true)).unwrap_or(false)
                || ["EIO", "ENOSPC", "EROFS", "EPERM", "EACCES", "EBUSY"].iter().any(|c| error.contains(c));
            let _ = self.cleanup();
            if durable_failed {
                return Err(format!("{error} (fatal)"));
            }
            return Err(error);
        }
        Ok(())
    }
    pub fn authorizes(&self, spec: &V) -> bool {
        self.bridge.borrow().as_ref().map(|b| b.authorizes(spec)).unwrap_or(false)
            || self.controller.borrow().as_ref().map(|c| c.authorizes(spec)).unwrap_or(false)
    }
    pub fn on_fetch_start(&self, meta: &V) -> bool {
        self.controller.borrow().as_ref().and_then(|c| c.on_fetch_start(meta).ok()).unwrap_or(false)
    }
    pub fn allows_legacy(&self, spec: &V) -> bool {
        self.registry.borrow().as_ref().map(|r| r.allows_other(spec)).unwrap_or_else(|| !self.blocks_legacy_writes())
    }
    pub fn step(self: &Rc<Self>, capture: Option<&V>) -> Result<(), String> {
        if !self.allowed() { return Ok(()); }
        let result = (|| -> Result<(), String> {
            if self.controller.borrow().is_none() {
                if !self.entry_allowed()
                    || capture.map(|c| get(c, "phase") != &json!("complete")
                        || payload_hash(get(c, "legs")).ok() != payload_hash(get(&self.request, "markets")).ok()).unwrap_or(true)
                {
                    return Ok(());
                }
                self.initialize(capture.cloned())?;
            }
            *self.last_failure.borrow_mut() = V::Null;
            self.cycle.borrow().as_ref().unwrap().step()?;
            Ok(())
        })();
        if let Err(e) = result {
            let _ = self.record_backoff(&OrderTransportError {
                code: e.clone(), status: None, api_code: None, outcome: String::new(),
                may_have_executed: false, retry_same_request_only: false,
                retry_after_ms: 0.0, semantic_mismatch: false,
            });
            let code = if e.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_') {
                e.clone()
            } else { "PAIRED_HOST_OPERATION_UNAVAILABLE".into() };
            *self.last_failure.borrow_mut() = json!(code);
            let _ = self.publish("operation_blocked");
            return Err(e);
        }
        Ok(())
    }
    pub fn status(&self) -> V {
        json!({
            "runId": get(&self.request, "runId"),
            "initialized": self.controller.borrow().is_some(),
            "phase": self.controller.borrow().as_ref().map(|c| get(&c.status(), "phase").clone())
                .unwrap_or(json!("awaiting_fresh_candidate")),
            "lastFailure": self.last_failure.borrow().clone(),
            "blocksLegacyWrites": self.blocks_legacy_writes(),
            "orders": self.controller.borrow().as_ref()
                .map(|c| get(&c.status(), "orders").as_array().map(|a| a.len()).unwrap_or(0)).unwrap_or(0),
        })
    }
    pub fn close(&self) {
        if *self.closed.borrow() { return; }
        *self.closed.borrow_mut() = true;
        let _ = self.cleanup();
    }
}

impl Closeable for crate::paired_rest_cycle::PairedRestCycle { fn close(&self) -> Result<(), String> { crate::paired_rest_cycle::PairedRestCycle::close(self) } }
impl Closeable for crate::paired_rest_evidence_bridge::PairedRestEvidenceBridge { fn close(&self) -> Result<(), String> { crate::paired_rest_evidence_bridge::PairedRestEvidenceBridge::close(self) } }
impl Closeable for crate::paired_shared_funding::PairedSharedFunding { fn close(&self) -> Result<(), String> { crate::paired_shared_funding::PairedSharedFunding::close(self) } }
impl Closeable for crate::paired_rest_controller::PairedRestController { fn close(&self) -> Result<(), String> { crate::paired_rest_controller::PairedRestController::close(self) } }


pub trait Closeable { fn close(&self) -> Result<(), String>; }
