// execution/paired-shared-funding.mjs — one durable pair slot in the host's
// fixed global registry. Budget gate, not an account owner, cash receipt,
// dispatcher or trading activation.
use baal_execution::core::{canonical_payload, decimal_micros, format_micros, payload_hash, validate_execution_state};
use baal_service_runtime::journal::Journal;
use baal_service_runtime::json_text::{js_int, js_stringify, sha256};
use baal_service_runtime::ledger::{acquire_owner_lease, OwnerLease};
use serde_json::{json, Value as V};
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;

const TID: &str = "bda92870-621e-47b0-bc3c-3602c5c26f55";
fn need(x: bool, c: &str) -> Result<(), String> {
    if x { Ok(()) } else { Err(format!("PAIRED_FUNDING_{c}")) }
}
fn hex(v: &V) -> bool {
    v.as_str().is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
}
fn copy(v: &V) -> V { v.clone() }
fn sha(b: &[u8]) -> String { sha256(b) }
fn get<'a>(v: &'a V, k: &str) -> &'a V { v.get(k).unwrap_or(&V::Null) }
fn a(v: &V) -> Result<i128, String> { decimal_micros(v, "exact") }
fn m(v: i128) -> String { format_micros(v) }
fn at(v: &V) -> Result<i64, String> {
    baal_service_runtime::contract::parse_iso_ms(v.as_str().unwrap_or("")).map(|x| x as i64).ok_or_else(|| "PAIRED_FUNDING_CLOCK".to_string())
}
const SLOT_KEYS: &[&str] = &[
    "schemaVersion", "kind", "ownerId", "runId", "tournamentId", "activationAuthorityHash",
    "planHash", "captureHash", "registryDir", "controllerDir", "startedAt", "entryExpiresAt",
    "allocatedCapital", "maxOrderCost", "maxAggregateCost", "maxCumulativeBuyCost",
    "maxManagedMarkets", "quantity", "maxPairs",
];
fn activation_check(x: &V, dir: &Path, controller_dir: &Path) -> Result<(), String> {
    let mut sorted: Vec<String> = x.as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
    sorted.sort();
    let mut expected: Vec<String> = SLOT_KEYS.iter().map(|s| s.to_string()).collect();
    expected.sort();
    need(
        x.is_object()
            && sorted == expected
            && get(x, "schemaVersion") == &json!(1)
            && get(x, "kind") == &json!("one_pair_shared_funding")
            && get(x, "ownerId") == &json!("Codex")
            && get(x, "tournamentId") == &json!(TID)
            && get(x, "runId").as_str().is_some_and(|s| {
                !s.is_empty() && s.len() <= 120
                    && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"_.:-".contains(&b))
            })
            && hex(get(x, "activationAuthorityHash"))
            && hex(get(x, "planHash"))
            && hex(get(x, "captureHash")),
        "ACTIVATION",
    )?;
    need(
        Path::new(get(x, "registryDir").as_str().unwrap_or("")).is_absolute()
            && Path::new(get(x, "controllerDir").as_str().unwrap_or("")).is_absolute()
            && get(x, "registryDir").as_str() == dir.to_str()
            && get(x, "controllerDir").as_str() == controller_dir.to_str(),
        "FIXED_DIRECTORIES",
    )?;
    need(
        at(get(x, "entryExpiresAt"))? > at(get(x, "startedAt"))?
            && at(get(x, "entryExpiresAt"))? - at(get(x, "startedAt"))? <= 3_600_000
            && get(x, "allocatedCapital") == &json!("2.000000")
            && get(x, "maxOrderCost") == &json!("1.000000")
            && get(x, "maxAggregateCost") == &json!("10.000000")
            && get(x, "maxCumulativeBuyCost") == &json!("25.000000")
            && get(x, "maxManagedMarkets") == &json!(3)
            && get(x, "quantity") == &json!(1)
            && get(x, "maxPairs") == &json!(1),
        "FIXED_CAPS",
    )?;
    Ok(())
}
struct CoreView {
    s: V,
    intents: Vec<V>,
    pending: Vec<V>,
    owned: i128,
    spent: i128,
    reserve: i128,
    hash: String,
}
fn a_sum(xs: Vec<i128>) -> i128 { xs.into_iter().sum() }
fn core_view(input: &V) -> Result<CoreView, String> {
    let s = validate_execution_state(input)?;
    need(
        get(&s, "tournamentId") == &json!(TID)
            && a(get(get(&s, "policy"), "maxAggregateCost"))? <= 10_000_000
            && a(get(get(&s, "policy"), "maxCumulativeBuyCost"))? <= 25_000_000
            && a(get(get(&s, "policy"), "maxOrderCost"))? <= 1_000_000
            && js_int(get(get(&s, "policy"), "maxManagedMarkets")).is_some_and(|x| x <= 3),
        "OTHER_CORE_POLICY",
    )?;
    let intents: Vec<V> = get(&s, "intents").as_object().map(|o| o.values().cloned().collect()).unwrap_or_default();
    let owned: i128 = a_sum(get(&s, "owned").as_object().map(|o| o.values().map(|p| a(get(p, "cost")).unwrap_or(0)).collect()).unwrap_or_default());
    let spent: i128 = a_sum(intents.iter().filter(|i| get(i, "action") == &json!("buy"))
        .flat_map(|i| get(i, "reconciledFills").as_array().cloned().unwrap_or_default().into_iter()
            .map(|f| a(get(&f, "price")).unwrap_or(0) * get(&f, "quantity").as_i64().unwrap_or(0) as i128).collect::<Vec<_>>()).collect());
    let pending: Vec<V> = intents.iter().filter(|i| get(i, "status") != &json!("closed")).cloned().collect();
    let reserve: i128 = a_sum(pending.iter().filter(|i| get(i, "action") == &json!("buy"))
        .map(|i| {
            let fills = get(i, "reconciledFills").as_array().map(|a| a.iter().map(|f| get(f, "quantity").as_i64().unwrap_or(0)).sum::<i64>()).unwrap_or(0);
            a(get(i, "price")).unwrap_or(0) * (get(i, "quantity").as_i64().unwrap_or(0) - fills) as i128
        }).collect());
    Ok(CoreView { hash: payload_hash(&s)?, s, intents, pending, owned, spent, reserve })
}
fn core_forward(old: Option<&V>, next: &V) -> Result<(), String> {
    let old = match old { Some(x) => x, None => return Ok(()) };
    need(payload_hash(get(old, "policy")).ok() == payload_hash(get(next, "policy")).ok(), "OTHER_POLICY_CHANGED")?;
    if let Some(im) = get(old, "intents").as_object() {
        for (id, i) in im {
            let n = get(next, "intents").as_object().and_then(|m| m.get(id)).cloned().unwrap_or(V::Null);
            need(
                !n.is_null() && get(&n, "payloadHash") == get(i, "payloadHash") && get(&n, "bodyJson") == get(i, "bodyJson"),
                "OTHER_INTENT_RESET",
            )?;
            need(
                get(i, "reconciledFills").as_array().map(|fs| fs.iter().all(|f| {
                    get(&n, "reconciledFills").as_array().map(|gs| gs.iter().any(|g| {
                        canonical_payload(g).ok() == canonical_payload(f).ok()
                    })).unwrap_or(false)
                })).unwrap_or(true)
                    && (get(i, "status") != &json!("closed") || get(&n, "status") == &json!("closed")),
                "OTHER_FILL_RESET",
            )?;
        }
    }
    Ok(())
}

type GetV = Rc<dyn Fn() -> Result<V, String>>;
type Now = Rc<dyn Fn() -> i64>;

pub struct PairedSharedFunding {
    activation: V,
    activation_hash: String,
    controller_dir: PathBuf,
    actor_file: PathBuf,
    plan: V,
    markets: Vec<V>,
    get_account_lease: GetV,
    get_other_state: GetV,
    get_paired_funding: GetV,
    get_paired_status: GetV,
    can_operate: Rc<dyn Fn() -> bool>,
    now: Now,
    local: RefCell<OwnerLease>,
    file: PathBuf,
    journal: RefCell<Journal>,
    state: RefCell<Option<V>>,
    closed: RefCell<bool>,
    failed: RefCell<bool>,
    size: RefCell<usize>,
    bytes_hash: RefCell<String>,
    identity: RefCell<(u64, f64)>,
}

impl PairedSharedFunding {
    #[allow(clippy::too_many_arguments)]
    pub fn open(
        dir: &Path,
        activation: &V,
        controller_dir: &Path,
        get_account_lease: GetV,
        get_other_state: GetV,
        get_paired_funding: GetV,
        get_paired_status: GetV,
        can_operate: Rc<dyn Fn() -> bool>,
        now: Now,
    ) -> Result<Self, String> {
        need(dir.is_absolute() && controller_dir.is_absolute(), "DEPENDENCIES")?;
        activation_check(activation, dir, controller_dir)?;
        let activation = copy(activation);
        let activation_hash = payload_hash(&activation)?;
        // Read only the already initialized actor's immutable opening. Not
        // another actor session and never takes the shared account lease.
        let actor_file = controller_dir.join("paired-rest-journal.jsonl");
        let actor = Journal::open(&actor_file)?;
        need(
            get(get(get(actor.last.as_ref().unwrap_or(&V::Null), "state"), "plan"), "planHash") == get(&activation, "planHash")
                && get(get(get(actor.last.as_ref().unwrap_or(&V::Null), "state"), "plan"), "captureHash") == get(&activation, "captureHash"),
            "ACTOR_OPENING",
        )?;
        let plan = copy(get(get(actor.last.as_ref().unwrap(), "state"), "plan"));
        let markets: Vec<V> = get(&plan, "legs").as_array().map(|l| l.iter().map(|x| json!({
            "marketId": get(x, "marketId"), "exchangeId": get(x, "exchangeId"),
        })).collect()).unwrap_or_default();
        need(
            get(get(&plan, "scope"), "ownerId") == get(&activation, "ownerId")
                && get(get(&plan, "scope"), "runId") == get(&activation, "runId")
                && get(get(&plan, "scope"), "tournamentId") == &json!(TID)
                && markets.len() == 2
                && markets.iter().map(|x| get(x, "exchangeId").to_string()).collect::<BTreeSet<_>>().len() == 2
                && markets.iter().map(|x| get(x, "marketId").to_string()).collect::<BTreeSet<_>>().len() == 2
                && a(get(&plan, "pairCostCeiling"))? <= 990_000,
            "PAIR_BINDING",
        )?;
        let local = acquire_owner_lease(dir, "Codex", activation.get("runId").and_then(|x| x.as_str()).unwrap_or(""), "shared-pair-funding-registry", (now)() as f64)
            .map_err(|e| format!("PAIRED_FUNDING_{}", e.code))?;
        let file = dir.join("paired-shared-funding.jsonl");
        let this = Self {
            activation: activation.clone(),
            activation_hash,
            controller_dir: controller_dir.to_path_buf(),
            actor_file,
            plan,
            markets,
            get_account_lease,
            get_other_state,
            get_paired_funding,
            get_paired_status,
            can_operate,
            now,
            local: RefCell::new(local),
            file: file.clone(),
            journal: RefCell::new(Journal::open(&file)?),
            state: RefCell::new(None),
            closed: RefCell::new(false),
            failed: RefCell::new(false),
            size: RefCell::new(0),
            bytes_hash: RefCell::new(String::new()),
            identity: RefCell::new((0, 0.0)),
        };
        this.init()?;
        Ok(this)
    }
    fn basic(&self) -> Result<String, String> {
        need(!*self.closed.borrow() && !*self.failed.borrow() && (self.can_operate)(), "STOPPED_OR_FAILED")?;
        self.local.borrow().assert_ownership().map_err(|e| format!("PAIRED_FUNDING_{}", e.code))?;
        let l = (self.get_account_lease)()?;
        need(
            get(&l, "ownerId") == &json!("Codex") && get(&l, "tournamentId") == &json!(TID) && hex(get(&l, "leaseId")),
            "ACTUAL_ACCOUNT_LEASE",
        )?;
        payload_hash(&json!({
            "ownerId": get(&l, "ownerId"),
            "tournamentId": get(&l, "tournamentId"),
            "leaseId": get(&l, "leaseId"),
        }))
    }
    fn source(&self) -> Result<V, String> {
        let f = copy(&(self.get_paired_funding)()?);
        let s = copy(&(self.get_paired_status)()?);
        let actor_bytes = std::fs::read(&self.actor_file).map_err(|e| e.to_string())?;
        let actual_actor = Journal::open(&self.actor_file)?;
        need(
            js_int(get(&s, "sequence")).is_some_and(|x| x > 0)
                && hex(get(&s, "hash"))
                && actual_actor.sequence == js_int(get(&s, "sequence")).unwrap_or(-1) as u64
                && actual_actor.previous == get(&s, "hash").as_str().unwrap_or("")
                && get(get(get(actual_actor.last.as_ref().unwrap_or(&V::Null), "state"), "plan"), "planHash") == get(&self.activation, "planHash"),
            "ACTUAL_ACTOR_JOURNAL",
        )?;
        if let Some(anchor) = self.state.borrow().as_ref().map(|st| get(st, "actorAnchor")).filter(|x| !x.is_null()) {
            need(
                actor_bytes.len() >= js_int(get(&anchor, "bytes")).unwrap_or(i64::MAX) as usize
                    && sha(&actor_bytes[..js_int(get(&anchor, "bytes")).unwrap() as usize]) == get(&anchor, "sha256").as_str().unwrap_or(""),
                "ACTOR_PREFIX_RESET",
            )?;
        }
        let actor_anchor = json!({
            "bytes": actor_bytes.len(),
            "sha256": sha(&actor_bytes),
            "sequence": get(&s, "sequence"),
            "hash": get(&s, "hash"),
        });
        need(
            get(&s, "mode") == &json!(if get(&self.plan, "schemaVersion") == &json!(2) { "paired_rest_lifecycle_v2" } else { "paired_rest_lifecycle_v1" })
                && get(&s, "planHash") == get(&self.activation, "planHash")
                && payload_hash(get(&s, "funding")).ok() == payload_hash(&f).ok()
                && get(&f, "ownerId") == &json!("Codex")
                && get(&f, "runId") == get(&self.activation, "runId")
                && get(&f, "tournamentId") == &json!(TID)
                && get(&f, "planHash") == get(&self.activation, "planHash")
                && get(&f, "allocatedCapital") == &json!("2.000000")
                && get(&f, "maxCumulativeBuy") == &json!("2.000000")
                && get(&s, "orders").as_array().is_some_and(|o| o.len() <= 2)
                && get(&s, "owned").as_array().is_some_and(|o| o.len() <= 2),
            "ACTUAL_PAIRED_SOURCE",
        )?;
        let mut body = f.clone();
        let reservation_hash = body.as_object_mut().and_then(|m| m.shift_remove("reservationHash")).unwrap_or(V::Null);
        need(
            reservation_hash.as_str() == payload_hash(&body).ok().as_deref()
                && a(get(&f, "reservedCash"))? >= 0
                && a(get(&f, "ownedPrincipal"))? >= 0
                && a(get(&f, "cumulativeBuy"))? == a(get(&f, "ownedPrincipal"))?
                && a(get(&f, "cumulativeBuy"))? <= a(get(&self.plan, "pairCostCeiling"))?,
            "PAIR_ACCOUNTING",
        )?;
        let mut owned: Vec<V> = Vec::new();
        for p in get(&s, "owned").as_array().cloned().unwrap_or_default() {
            need(
                self.markets.iter().any(|x| get(x, "marketId") == get(&p, "marketId") && get(x, "exchangeId") == get(&p, "exchangeId"))
                    && get(&p, "quantity") == &json!(1)
                    && get(&p, "side") == &json!("no")
                    && get(&p, "ownerId") == &json!("Codex")
                    && get(&p, "runId") == get(&self.activation, "runId")
                    && get(&s, "orders").as_array().map(|os| os.iter().any(|o| {
                        get(o, "intentId") == get(&p, "intentId") && get(o, "orderId") == get(&p, "orderId")
                            && get(o, "phase") == &json!("reconciled") && hex(get(o, "proofHash"))
                    })).unwrap_or(false),
                "OWNED_PROVENANCE",
            )?;
            need(!owned.iter().any(|x| get(x, "exchangeId") == get(&p, "exchangeId")), "DUPLICATE_OWNED")?;
            owned.push(json!({
                "marketId": get(&p, "marketId"),
                "exchangeId": get(&p, "exchangeId"),
                "quantity": 1,
                "entryPrice": get(&p, "entryPrice"),
                "orderId": get(&p, "orderId"),
                "intentId": get(&p, "intentId"),
            }));
        }
        need(a_sum(owned.iter().map(|p| a(get(p, "entryPrice")).unwrap_or(0)).collect()) == a(get(&f, "ownedPrincipal"))?, "OWNED_PRINCIPAL")?;
        let ended = ["paired", "orphaned", "unfilled"].contains(&get(&s, "phase").as_str().unwrap_or(""))
            && get(&s, "orders").as_array().is_some_and(|o| !o.is_empty())
            && get(&s, "orders").as_array().map(|o| o.iter().all(|x| get(x, "phase") == &json!("reconciled"))).unwrap_or(false);
        need(
            if ended { a(get(&f, "reservedCash"))? == 0 }
            else { a(get(&f, "reservedCash"))? + a(get(&f, "cumulativeBuy"))? == a(get(&self.plan, "pairCostCeiling"))? },
            "FULL_UNKNOWN_RESERVE",
        )?;
        if let Some(state) = self.state.borrow().as_ref().cloned() {
            need(
                get(&s, "orders").as_array().map(|so| so.len() >= get(&state, "pairedOrders").as_array().map(|a| a.len()).unwrap_or(0)).unwrap_or(false)
                    && get(&state, "pairedOrders").as_array().map(|pos| pos.iter().all(|o| {
                        get(&s, "orders").as_array().map(|so| so.iter().any(|n| {
                            get(n, "intentId") == get(o, "intentId")
                                && get(n, "bodyHash") == get(o, "bodyHash")
                                && (get(o, "orderId").is_null() || get(n, "orderId") == get(o, "orderId"))
                                && (get(o, "phase") != &json!("reconciled") || get(n, "phase") == &json!("reconciled"))
                        })).unwrap_or(false)
                    })).unwrap_or(true),
                "PAIR_ORDER_RESET",
            )?;
            need(
                a(get(&f, "ownedPrincipal"))? >= a(get(get(&state, "funding"), "ownedPrincipal"))?
                    && a(get(&f, "cumulativeBuy"))? >= a(get(get(&state, "funding"), "cumulativeBuy"))?,
                "PAIR_COST_RESET",
            )?;
            need(
                get(&state, "owned").as_array().map(|ps| ps.iter().all(|p| {
                    owned.iter().any(|n| canonical_payload(n).ok() == canonical_payload(p).ok())
                })).unwrap_or(true),
                "PAIR_OWNERSHIP_RESET",
            )?;
            need(!baal_service_runtime::json_text::js_truthy(state.get("ended")) || ended, "PAIR_REOPENED")?;
        }
        let source_hash = payload_hash(&json!({
            "sequence": get(&s, "sequence"),
            "hash": get(&s, "hash"),
            "funding": f,
            "orders": get(&s, "orders"),
            "owned": get(&s, "owned"),
        }))?;
        Ok(json!({
            "f": f, "s": s, "owned": owned, "ended": ended,
            "actorAnchor": actor_anchor, "sourceHash": source_hash,
        }))
    }
    fn limits(&self, core: &CoreView, pair: &V) -> Result<V, String> {
        need(
            !core.intents.iter().any(|i| {
                get(pair, "owned").as_array().map(|os| os.iter().any(|p| get(p, "exchangeId") == get(i, "exchangeId"))).unwrap_or(false)
                    && get(i, "status") != &json!("closed")
            }),
            "CROSS_NET_PENDING",
        )?;
        let aggregate = core.owned + core.reserve + a(get(get(pair, "f"), "ownedPrincipal"))? + a(get(get(pair, "f"), "reservedCash"))?;
        let cumulative = core.spent + core.reserve + a(get(get(pair, "f"), "cumulativeBuy"))? + a(get(get(pair, "f"), "reservedCash"))?;
        let mut managed: BTreeSet<String> = BTreeSet::new();
        for p in get(&core.s, "owned").as_object().map(|o| o.values().cloned().collect::<Vec<_>>()).unwrap_or_default() {
            if let Some(v) = get(&p, "marketId").as_str() { managed.insert(v.to_string()); }
        }
        for i in core.pending.iter().filter(|i| get(i, "action") == &json!("buy")) {
            if let Some(v) = get(i, "marketId").as_str() { managed.insert(v.to_string()); }
        }
        let src: Vec<V> = if get(pair, "ended") == &json!(true) {
            get(pair, "owned").as_array().cloned().unwrap_or_default()
        } else { self.markets.clone() };
        for p in src { if let Some(v) = get(&p, "marketId").as_str() { managed.insert(v.to_string()); } }
        need(aggregate <= 10_000_000 && cumulative <= 25_000_000 && managed.len() <= 3, "GLOBAL_CAP")?;
        Ok(json!({
            "aggregate": m(aggregate),
            "cumulative": m(cumulative),
            "managedMarketIds": managed.into_iter().collect::<Vec<String>>(),
        }))
    }
    fn init(&self) -> Result<(), String> {
        if !self.file.exists() {
            let f = std::fs::OpenOptions::new().write(true).create_new(true).open(&self.file).map_err(|e| e.to_string())?;
            #[cfg(unix)]
            { use std::os::unix::fs::PermissionsExt; std::fs::set_permissions(&self.file, std::fs::Permissions::from_mode(0o600)).ok(); }
            drop(f);
        }
        let raw = std::fs::read(&self.file).map_err(|e| e.to_string())?;
        need(raw.is_empty() || raw.last() == Some(&10), "PARTIAL_JOURNAL")?;
        for line in String::from_utf8_lossy(&raw).split('\n').filter(|l| !l.is_empty()) {
            let row: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
            need(
                get(get(&row, "state"), "activationHash").as_str() == Some(self.activation_hash.as_str())
                    && get(get(&row, "state"), "controllerDir").as_str() == self.controller_dir.to_str()
                    && get(get(&row, "state"), "planHash") == get(&self.activation, "planHash"),
                "ONE_PAIR_SLOT",
            )?;
            if let Some(state) = self.state.borrow().as_ref().cloned() {
                need(
                    a(get(get(&row, "state"), "funding").get("cumulativeBuy").unwrap_or(&V::Null))? >= a(get(get(&state, "funding"), "cumulativeBuy"))?
                        && a(get(get(&row, "state"), "funding").get("ownedPrincipal").unwrap_or(&V::Null))? >= a(get(get(&state, "funding"), "ownedPrincipal"))?,
                    "JOURNAL_COST_RESET",
                )?;
                core_forward(state.get("otherCore"), get(get(&row, "state"), "otherCore"))?;
            }
            *self.state.borrow_mut() = Some(get(&row, "state").clone());
        }
        *self.size.borrow_mut() = raw.len();
        *self.bytes_hash.borrow_mut() = sha(&raw);
        let meta = std::fs::metadata(&self.file).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        { use std::os::unix::fs::MetadataExt; *self.identity.borrow_mut() = (meta.ino(), meta.mtime() as f64); }
        Ok(())
    }
    fn append(&self, next: &V, event: &V) -> Result<(), String> {
        self.basic()?;
        let previous = std::fs::read(&self.file).map_err(|e| e.to_string())?;
        let meta = std::fs::metadata(&self.file).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        { use std::os::unix::fs::MetadataExt; need(meta.ino() == self.identity.borrow().0, "JOURNAL_CHANGED")?; }
        need(previous.len() == *self.size.borrow() && sha(&previous) == *self.bytes_hash.borrow(), "JOURNAL_CHANGED")?;
        let result = (|| -> Result<(), String> {
            self.journal.borrow_mut().commit(next, event)?;
            self.basic()?;
            let after = std::fs::read(&self.file).map_err(|e| e.to_string())?;
            need(after[..*self.size.borrow()] == previous[..] && after.len() > *self.size.borrow(), "JOURNAL_CHANGED")?;
            *self.state.borrow_mut() = Some(next.clone());
            *self.size.borrow_mut() = after.len();
            *self.bytes_hash.borrow_mut() = sha(&after);
            Ok(())
        })();
        if result.is_err() { *self.failed.borrow_mut() = true; }
        result
    }
    pub fn install_funding_reservation(&self, record: &V) -> Result<V, String> {
        let lease_hash = self.basic()?;
        let pair = self.source()?;
        need(payload_hash(record).ok() == payload_hash(get(&pair, "f")).ok(), "RECORD_NOT_ACTUAL_CONTROLLER")?;
        let core = core_view(&(self.get_other_state)()?)?;
        core_forward(self.state.borrow().as_ref().and_then(|s| s.get("otherCore")), &core.s)?;
        if self.state.borrow().is_none() {
            need(
                (self.now)() >= at(get(&self.activation, "startedAt"))?
                    && (self.now)() < at(get(&self.activation, "entryExpiresAt"))?,
                "INITIAL_WINDOW",
            )?;
            need(
                core.pending.is_empty() && !core.intents.iter().any(|i| baal_service_runtime::json_text::js_truthy(i.get("acceptanceUncertain"))),
                "INITIAL_CORE_UNKNOWN",
            )?;
            need(
                !get(&core.s, "owned").as_object().map(|o| o.keys().any(|id| {
                    self.markets.iter().any(|m| get(m, "exchangeId").as_str() == Some(id.as_str()))
                })).unwrap_or(false),
                "EXISTING_PAIR_HOLDING",
            )?;
        } else if get(&pair, "ended") != &json!(true) {
            need(core.pending.is_empty(), "EXCLUSIVE_CORE_ACTIVE")?;
        }
        let capacity = self.limits(&core, &pair)?;
        let next = json!({
            "schemaVersion": 1,
            "activationHash": self.activation_hash,
            "controllerDir": self.controller_dir.to_string_lossy(),
            "planHash": get(&self.activation, "planHash"),
            "captureHash": get(&self.activation, "captureHash"),
            "funding": get(&pair, "f"),
            "owned": get(&pair, "owned"),
            "pairedOrders": get(get(&pair, "s"), "orders"),
            "actorAnchor": get(&pair, "actorAnchor"),
            "ended": get(&pair, "ended"),
            "exclusiveActive": get(&pair, "ended") != &json!(true),
            "otherCore": core.s,
            "pairedSourceHash": get(&pair, "sourceHash"),
            "accountLeaseHash": lease_hash,
            "capacity": capacity,
            "at": baal_service_runtime::contract::ms_to_iso((self.now)() as f64),
        });
        self.append(&next, &json!({
            "kind": if self.state.borrow().is_some() { "funding_update" } else { "one_pair_claim" },
            "activationHash": self.activation_hash,
            "pairedSourceHash": get(&pair, "sourceHash"),
        }))?;
        Ok(json!({
            "reservationHash": get(get(&pair, "f"), "reservationHash"),
            "amount": get(get(&pair, "f"), "reservedCash"),
            "globalLedgerHash": self.journal.borrow().previous.clone(),
            "confirmedAt": get(&next, "at"),
        }))
    }
    pub fn allows_other(&self, spec: &V) -> bool {
        if get(spec, "method") == &json!("GET") { return true; }
        (|| -> Result<bool, String> {
            self.basic()?;
            if self.state.borrow().as_ref().and_then(|s| s.get("exclusiveActive")) == Some(&json!(true)) { return Ok(false); }
            let pair = if self.state.borrow().is_some() {
                self.source()?
            } else {
                json!({"ended": true, "owned": [], "f": {"ownedPrincipal": "0.000000", "cumulativeBuy": "0.000000", "reservedCash": "0.000000"}})
            };
            need(get(&pair, "ended") == &json!(true), "PAIR_NOT_ENDED")?;
            let core = core_view(&(self.get_other_state)()?)?;
            core_forward(self.state.borrow().as_ref().and_then(|s| s.get("otherCore")), &core.s)?;
            let capacity = self.limits(&core, &pair)?;
            if get(spec, "method") == &json!("DELETE") {
                let ep = get(spec, "endpoint").as_str().unwrap_or("");
                let id_str = ep.strip_prefix("/orders/").filter(|s| {
                    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && !s.starts_with('0')
                });
                let i2 = core.intents.iter().find(|x| {
                    id_str.map(|id| {
                        baal_execution::js_key(get(x, "orderId")) == id
                    }).unwrap_or(false)
                        && get(x, "status") != &json!("closed")
                }).cloned().unwrap_or(V::Null);
                return Ok(!i2.is_null() && get(&i2, "side") == &json!("yes") && spec.get("body").is_none() && spec.get("bodyJson").is_none());
            }
            need(
                get(spec, "method") == &json!("POST") && get(spec, "endpoint") == &json!("/orders")
                    && spec.get("body").is_some()
                    && spec.get("bodyJson").and_then(|b| b.as_str()) == canonical_payload(spec.get("body").unwrap()).ok().as_deref(),
                "OTHER_REQUEST",
            )?;
            let body_hash = payload_hash(spec.get("body").unwrap()).unwrap_or_default();
            let i = core.intents.iter().find(|x| {
                get(x, "status") != &json!("closed")
                    && get(x, "payloadHash") == &json!(body_hash)
                    && get(x, "bodyJson").as_str() == spec.get("bodyJson").and_then(|b| b.as_str())
            }).cloned().unwrap_or(V::Null);
            need(
                !i.is_null()
                    && get(&i, "side") == &json!("yes")
                    && ["prepared", "dispatching", "uncertain", "acknowledged", "reconciled"].contains(&get(&i, "status").as_str().unwrap_or(""))
                    && a(get(&i, "price"))? * get(&i, "quantity").as_i64().unwrap_or(0) as i128 <= 1_000_000,
                "KNOWN_ORIGINAL_INTENT",
            )?;
            need(
                !get(&pair, "owned").as_array().map(|os| os.iter().any(|p| get(p, "exchangeId") == get(&i, "exchangeId"))).unwrap_or(false),
                "NO_CROSS_NET",
            )?;
            if get(&i, "action") == &json!("sell") {
                need(
                    get(get(&core.s, "owned"), get(&i, "exchangeId").as_str().unwrap_or("")).get("quantity").and_then(|q| q.as_i64()).unwrap_or(0)
                        >= get(&i, "quantity").as_i64().unwrap_or(0),
                    "COVERED_SELL",
                )?;
            }
            if self.state.borrow().is_some()
                && payload_hash(self.state.borrow().as_ref().unwrap().get("otherCore").unwrap()).ok() != Some(core.hash.clone())
            {
                let mut nxt = self.state.borrow().clone().unwrap();
                nxt.as_object_mut().unwrap().insert("otherCore".into(), core.s.clone());
                nxt.as_object_mut().unwrap().insert("capacity".into(), capacity);
                nxt.as_object_mut().unwrap().insert("at".into(), json!(baal_service_runtime::contract::ms_to_iso((self.now)() as f64)));
                self.append(&nxt, &json!({"kind": "original_core_observed", "originalCoreHash": core.hash}))?;
            }
            Ok(true)
        })().unwrap_or(false)
    }
    pub fn exclusive_active(&self) -> bool {
        if self.state.borrow().is_none() { return false; }
        (|| -> Result<bool, String> {
            self.basic()?;
            let p = self.source()?;
            Ok(baal_service_runtime::json_text::js_truthy(self.state.borrow().as_ref().unwrap().get("exclusiveActive"))
                || get(&p, "ended") != &json!(true))
        })().unwrap_or(true)
    }
    pub fn status(&self) -> V {
        let state = self.state.borrow().clone();
        let journal = self.journal.borrow();
        json!({
            "mode": "one_pair_shared_funding_v1",
            "activationHash": self.activation_hash,
            "planHash": get(&self.activation, "planHash"),
            "claimed": state.is_some(),
            "exclusiveActive": state.as_ref().map(|s| s.get("exclusiveActive").cloned().unwrap_or(json!(false))).unwrap_or(json!(false)),
            "ended": state.as_ref().map(|s| s.get("ended").cloned().unwrap_or(json!(false))).unwrap_or(json!(false)),
            "owned": state.as_ref().map(|s| get(s, "owned").clone()).unwrap_or_else(|| json!([])),
            "funding": state.as_ref().map(|s| get(s, "funding").clone()).unwrap_or(V::Null),
            "globalLedgerHash": journal.previous.clone(),
            "sequence": journal.sequence,
            "failed": *self.failed.borrow(),
            "closed": *self.closed.borrow(),
            "executionAuthority": false,
            "additionalClients": 0,
        })
    }
    pub fn close(&self) -> Result<(), String> {
        if *self.closed.borrow() { return Ok(()); }
        *self.closed.borrow_mut() = true;
        self.local.borrow_mut().release().map_err(|e| e.message)
    }
}
