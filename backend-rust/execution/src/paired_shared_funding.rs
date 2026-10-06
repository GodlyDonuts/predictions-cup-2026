// execution/paired-shared-funding.mjs — one durable pair slot in the host's
// fixed global registry. This is a budget gate, not an account owner, cash
// receipt, dispatcher or trading activation.
use crate::core::{canonical_payload, decimal_micros, format_micros, payload_hash, validate_execution_state};
use baal_service_runtime::journal::Journal;
use baal_service_runtime::json_text::{js_int, js_stringify, sha256};
use baal_service_runtime::ledger::acquire_owner_lease;
use serde_json::{json, Value as V};
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

const TID: &str = "bda92870-621e-47b0-bc3c-3602c5c26f55";

fn need(x: bool, c: &str) -> Result<(), String> {
    if x { Ok(()) } else { Err(format!("PAIRED_FUNDING_{c}")) }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn hex(x: &V) -> bool {
    x.as_str().is_some_and(|s| {
        s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}
// at(x): Date.parse(x) + canonical-ISO round-trip — PAIRED_FUNDING_CLOCK.
fn at(x: &V) -> Result<i64, String> {
    let s = x.as_str();
    need(
        s.is_some()
            && baal_service_runtime::fixed::instant(s.unwrap()).is_ok()
            && baal_service_runtime::contract::ms_to_iso(
                baal_service_runtime::fixed::instant(s.unwrap()).unwrap() as f64,
            ) == s.unwrap(),
        "CLOCK",
    )?;
    Ok(baal_service_runtime::fixed::instant(s.unwrap()).unwrap())
}
fn a(v: &V) -> i128 {
    decimal_micros(v, "reject").unwrap_or(0)
}
fn m(v: i128) -> V {
    json!(format_micros(v))
}
fn a_sum(xs: impl Iterator<Item = i128>) -> i128 {
    xs.fold(0, |n, x| n + x)
}

const SLOT_KEYS_ALL: [&str; 19] = [
    "schemaVersion", "kind", "ownerId", "runId", "tournamentId", "activationAuthorityHash",
    "planHash", "captureHash", "registryDir", "controllerDir", "startedAt", "entryExpiresAt",
    "allocatedCapital", "maxOrderCost", "maxAggregateCost", "maxCumulativeBuyCost",
    "maxManagedMarkets", "quantity", "maxPairs",
];

fn activation_check(x: &V, dir: &Path, controller_dir: &Path) -> Result<(), String> {
    let mut keys: Vec<String> = x
        .as_object()
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();
    keys.sort();
    let mut expected: Vec<String> = SLOT_KEYS_ALL.iter().map(|s| s.to_string()).collect();
    expected.sort();
    need(
        !x.is_null()
            && keys == expected
            && get(x, "schemaVersion") == &json!(1)
            && get(x, "kind") == &json!("one_pair_shared_funding")
            && get(x, "ownerId") == &json!("Codex")
            && get(x, "tournamentId") == &json!(TID)
            && get(x, "runId").as_str().is_some_and(|s| {
                !s.is_empty()
                    && s.len() <= 120
                    && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"_.:-".contains(&b))
            })
            && hex(get(x, "activationAuthorityHash"))
            && hex(get(x, "planHash"))
            && hex(get(x, "captureHash")),
        "ACTIVATION",
    )?;
    need(
        get(x, "registryDir").as_str().is_some_and(|s| s.starts_with('/'))
            && get(x, "controllerDir").as_str().is_some_and(|s| s.starts_with('/'))
            && Path::new(get(x, "registryDir").as_str().unwrap()) == dir
            && Path::new(get(x, "controllerDir").as_str().unwrap()) == controller_dir,
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
    )
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

fn core_view(input: &V) -> Result<CoreView, String> {
    let s = validate_execution_state(input)?;
    need(
        get(&s, "tournamentId") == &json!(TID)
            && a(get(get(&s, "policy"), "maxAggregateCost")) <= 10_000_000
            && a(get(get(&s, "policy"), "maxCumulativeBuyCost")) <= 25_000_000
            && a(get(get(&s, "policy"), "maxOrderCost")) <= 1_000_000
            && js_int(get(get(&s, "policy"), "maxManagedMarkets")).is_some_and(|n| n <= 3),
        "OTHER_CORE_POLICY",
    )?;
    let intents: Vec<V> = get(&s, "intents")
        .as_object()
        .map(|o| o.values().cloned().collect())
        .unwrap_or_default();
    let owned_map: Vec<V> = get(&s, "owned")
        .as_object()
        .map(|o| o.values().cloned().collect())
        .unwrap_or_default();
    let owned = a_sum(owned_map.iter().map(|p| a(get(p, "cost"))));
    let spent = a_sum(
        intents
            .iter()
            .filter(|i| get(i, "action").as_str() == Some("buy"))
            .flat_map(|i| {
                get(i, "reconciledFills").as_array().cloned().unwrap_or_default()
            })
            .map(|f| a(get(&f, "price")) * (js_int(get(&f, "quantity")).unwrap_or(0) as i128)),
    );
    let pending: Vec<V> = intents
        .iter()
        .filter(|i| get(i, "status").as_str() != Some("closed"))
        .cloned()
        .collect();
    let reserve = a_sum(pending.iter().filter(|i| get(i, "action").as_str() == Some("buy")).map(|i| {
        let filled: i128 = get(i, "reconciledFills")
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|f| js_int(get(f, "quantity")).unwrap_or(0) as i128)
            .sum();
        a(get(i, "price")) * (js_int(get(i, "quantity")).unwrap_or(0) as i128 - filled)
    }));
    Ok(CoreView {
        hash: payload_hash(&s)?,
        s,
        intents,
        pending,
        owned,
        spent,
        reserve,
    })
}

fn core_forward(old: &V, next: &V) -> Result<(), String> {
    if old.is_null() {
        return Ok(());
    }
    need(
        payload_hash(get(old, "policy"))? == payload_hash(get(next, "policy"))?,
        "OTHER_POLICY_CHANGED",
    )?;
    if let Some(old_intents) = get(old, "intents").as_object() {
        for (id, i) in old_intents {
            let n = get(next, "intents").get(id).cloned().unwrap_or(V::Null);
            need(
                !n.is_null()
                    && get(&n, "payloadHash") == get(i, "payloadHash")
                    && get(&n, "bodyJson") == get(i, "bodyJson"),
                "OTHER_INTENT_RESET",
            )?;
            let fills = get(i, "reconciledFills").as_array().cloned().unwrap_or_default();
            let next_fills = get(&n, "reconciledFills").as_array().cloned().unwrap_or_default();
            need(
                fills.iter().all(|f| {
                    next_fills.iter().any(|g| {
                        canonical_payload(g).unwrap_or_default() == canonical_payload(f).unwrap_or_default()
                    })
                }) && (get(i, "status").as_str() != Some("closed")
                    || get(&n, "status").as_str() == Some("closed")),
                "OTHER_FILL_RESET",
            )?;
        }
    }
    Ok(())
}

struct FileIdentity {
    ino: u64,
    birthtime_ms: f64,
}
fn file_identity(path: &Path) -> Result<FileIdentity, String> {
    use std::os::unix::fs::MetadataExt;
    let meta = fs::metadata(path).map_err(|e| e.to_string())?;
    let birthtime = meta
        .created()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(0.0);
    Ok(FileIdentity { ino: meta.ino(), birthtime_ms: birthtime })
}

struct PairSource {
    f: V,
    s: V,
    owned: Vec<V>,
    ended: bool,
    actor_anchor: V,
    source_hash: String,
}

struct FundingInner {
    dir: PathBuf,
    controller_dir: PathBuf,
    file: PathBuf,
    actor_file: PathBuf,
    activation: V,
    activation_hash: String,
    plan: V,
    markets: Vec<V>,
    journal: Journal,
    state: Option<V>,
    closed: bool,
    failed: bool,
    size: u64,
    bytes_hash: String,
    identity: FileIdentity,
    lease: baal_service_runtime::ledger::OwnerLease,
    get_account_lease: Rc<dyn Fn() -> V>,
    get_other_state: Rc<dyn Fn() -> V>,
    get_paired_funding: Rc<dyn Fn() -> V>,
    get_paired_status: Rc<dyn Fn() -> V>,
    can_operate: Rc<dyn Fn() -> bool>,
    now: Rc<dyn Fn() -> f64>,
}

impl FundingInner {
    fn basic(&self) -> Result<String, String> {
        need(!self.closed && !self.failed && (self.can_operate)(), "STOPPED_OR_FAILED")?;
        self.lease.assert_ownership().map_err(|e| e.message)?;
        let l = (self.get_account_lease)();
        need(
            get(&l, "ownerId") == &json!("Codex")
                && get(&l, "tournamentId") == &json!(TID)
                && hex(get(&l, "leaseId")),
            "ACTUAL_ACCOUNT_LEASE",
        )?;
        Ok(payload_hash(&json!({
            "ownerId": get(&l, "ownerId"),
            "tournamentId": get(&l, "tournamentId"),
            "leaseId": get(&l, "leaseId"),
        }))?)
    }

    fn source(&self) -> Result<PairSource, String> {
        let f = (self.get_paired_funding)().clone();
        let s = (self.get_paired_status)().clone();
        let actor_bytes = fs::read(&self.actor_file).map_err(|e| e.to_string())?;
        let actual_actor = Journal::open(&self.actor_file)?;
        need(
            js_int(get(&s, "sequence")).is_some_and(|x| x > 0)
                && hex(get(&s, "hash"))
                && actual_actor.sequence == js_int(get(&s, "sequence")).unwrap() as u64
                && actual_actor.previous == get(&s, "hash").as_str().unwrap_or("")
                && get(get(&get(&actual_actor.last.clone().unwrap_or(V::Null), "state"), "plan"), "planHash")
                    == get(&self.activation, "planHash"),
            "ACTUAL_ACTOR_JOURNAL",
        )?;
        if let Some(anchor) = self.state.as_ref().and_then(|st| {
            let v = st.get("actorAnchor")?;
            if v.is_null() { None } else { Some(v.clone()) }
        }) {
            need(
                actor_bytes.len() as i64 >= js_int(get(&anchor, "bytes")).unwrap_or(0)
                    && sha256(&actor_bytes[..js_int(get(&anchor, "bytes")).unwrap_or(0) as usize])
                        == get(&anchor, "sha256").as_str().unwrap_or(""),
                "ACTOR_PREFIX_RESET",
            )?;
        }
        let actor_anchor = json!({
            "bytes": actor_bytes.len(),
            "sha256": sha256(&actor_bytes),
            "sequence": get(&s, "sequence"),
            "hash": get(&s, "hash"),
        });
        need(
            get(&s, "mode").as_str()
                == Some(if get(&self.plan, "schemaVersion") == &json!(2) {
                    "paired_rest_lifecycle_v2"
                } else {
                    "paired_rest_lifecycle_v1"
                })
                && get(&s, "planHash") == get(&self.activation, "planHash")
                && payload_hash(get(&s, "funding"))? == payload_hash(&f)?
                && get(&f, "ownerId") == &json!("Codex")
                && get(&f, "runId") == get(&self.activation, "runId")
                && get(&f, "tournamentId") == &json!(TID)
                && get(&f, "planHash") == get(&self.activation, "planHash")
                && get(&f, "allocatedCapital") == &json!("2.000000")
                && get(&f, "maxCumulativeBuy") == &json!("2.000000")
                && get(&s, "orders").as_array().is_some_and(|a| a.len() <= 2)
                && get(&s, "owned").as_array().is_some_and(|a| a.len() <= 2),
            "ACTUAL_PAIRED_SOURCE",
        )?;
        let mut funding_body = f.clone();
        let reservation_hash = funding_body
            .as_object_mut()
            .and_then(|m| m.remove("reservationHash"));
        need(
            reservation_hash.as_ref().and_then(|h| h.as_str())
                == Some(payload_hash(&funding_body)?.as_str())
                && a(get(&f, "reservedCash")) >= 0
                && a(get(&f, "ownedPrincipal")) >= 0
                && a(get(&f, "cumulativeBuy")) == a(get(&f, "ownedPrincipal"))
                && a(get(&f, "cumulativeBuy")) <= a(get(&self.plan, "pairCostCeiling")),
            "PAIR_ACCOUNTING",
        )?;
        let mut owned: Vec<V> = Vec::new();
        for p in get(&s, "owned").as_array().cloned().unwrap_or_default() {
            need(
                self.markets.iter().any(|x| {
                    get(x, "marketId") == get(&p, "marketId") && get(x, "exchangeId") == get(&p, "exchangeId")
                }) && get(&p, "quantity") == &json!(1)
                    && get(&p, "side").as_str() == Some("no")
                    && get(&p, "ownerId") == &json!("Codex")
                    && get(&p, "runId") == get(&self.activation, "runId")
                    && get(&s, "orders").as_array().is_some_and(|orders| {
                        orders.iter().any(|o| {
                            get(o, "intentId") == get(&p, "intentId")
                                && get(o, "orderId") == get(&p, "orderId")
                                && get(o, "phase").as_str() == Some("reconciled")
                                && hex(get(o, "proofHash"))
                        })
                    }),
                "OWNED_PROVENANCE",
            )?;
            need(
                !owned.iter().any(|x| get(x, "exchangeId") == get(&p, "exchangeId")),
                "DUPLICATE_OWNED",
            )?;
            owned.push(json!({
                "marketId": get(&p, "marketId"),
                "exchangeId": get(&p, "exchangeId"),
                "quantity": 1,
                "entryPrice": get(&p, "entryPrice"),
                "orderId": get(&p, "orderId"),
                "intentId": get(&p, "intentId"),
            }));
        }
        need(
            a_sum(owned.iter().map(|p| a(get(p, "entryPrice")))) == a(get(&f, "ownedPrincipal")),
            "OWNED_PRINCIPAL",
        )?;
        let ended = matches!(get(&s, "phase").as_str(), Some("paired" | "orphaned" | "unfilled"))
            && get(&s, "orders").as_array().is_some_and(|o| {
                !o.is_empty() && o.iter().all(|x| get(x, "phase").as_str() == Some("reconciled"))
            });
        need(
            if ended {
                a(get(&f, "reservedCash")) == 0
            } else {
                a(get(&f, "reservedCash")) + a(get(&f, "cumulativeBuy")) == a(get(&self.plan, "pairCostCeiling"))
            },
            "FULL_UNKNOWN_RESERVE",
        )?;
        if let Some(state) = &self.state {
            let state_orders = get(state, "pairedOrders").as_array().cloned().unwrap_or_default();
            let cur_orders = get(&s, "orders").as_array().cloned().unwrap_or_default();
            need(
                cur_orders.len() >= state_orders.len()
                    && state_orders.iter().all(|o| {
                        cur_orders.iter().any(|n| {
                            get(n, "intentId") == get(o, "intentId")
                                && get(n, "bodyHash") == get(o, "bodyHash")
                                && (o.get("orderId").is_none_or(|v| v.is_null())
                                    || get(n, "orderId") == get(o, "orderId"))
                                && (get(o, "phase").as_str() != Some("reconciled")
                                    || get(n, "phase").as_str() == Some("reconciled"))
                        })
                    }),
                "PAIR_ORDER_RESET",
            )?;
            need(
                a(get(&f, "ownedPrincipal")) >= a(get(get(state, "funding"), "ownedPrincipal"))
                    && a(get(&f, "cumulativeBuy")) >= a(get(get(state, "funding"), "cumulativeBuy")),
                "PAIR_COST_RESET",
            )?;
            need(
                get(state, "owned").as_array().cloned().unwrap_or_default().iter().all(|p| {
                    owned.iter().any(|n| {
                        canonical_payload(n).unwrap_or_default() == canonical_payload(p).unwrap_or_default()
                    })
                }),
                "PAIR_OWNERSHIP_RESET",
            )?;
            need(
                get(state, "ended") != &json!(true) || ended,
                "PAIR_REOPENED",
            )?;
        }
        Ok(PairSource {
            source_hash: payload_hash(&json!({
                "sequence": get(&s, "sequence"),
                "hash": get(&s, "hash"),
                "funding": f,
                "orders": get(&s, "orders"),
                "owned": get(&s, "owned"),
            }))?,
            f,
            s,
            owned,
            ended,
            actor_anchor,
        })
    }
}

fn limits(core: &CoreView, pair: &PairSource, plan: &V, markets: &[V]) -> Result<V, String> {
    need(
        !core.intents.iter().any(|i| {
            pair.owned.iter().any(|p| get(p, "exchangeId") == get(i, "exchangeId"))
                && get(i, "status").as_str() != Some("closed")
        }),
        "CROSS_NET_PENDING",
    )?;
    let aggregate = core.owned + core.reserve + a(get(&pair.f, "ownedPrincipal")) + a(get(&pair.f, "reservedCash"));
    let cumulative = core.spent + core.reserve + a(get(&pair.f, "cumulativeBuy")) + a(get(&pair.f, "reservedCash"));
    let mut managed: BTreeSet<String> = BTreeSet::new();
    for p in get(&core.s, "owned").as_object().map(|o| o.values().cloned().collect::<Vec<_>>()).unwrap_or_default() {
        managed.insert(get(&p, "marketId").as_str().unwrap_or("").to_string());
    }
    for i in core.pending.iter().filter(|i| get(i, "action").as_str() == Some("buy")) {
        managed.insert(get(i, "marketId").as_str().unwrap_or("").to_string());
    }
    let legs: Vec<V> = if pair.ended { pair.owned.clone() } else { markets.to_vec() };
    for p in legs {
        managed.insert(get(&p, "marketId").as_str().unwrap_or("").to_string());
    }
    need(
        aggregate <= 10_000_000 && cumulative <= 25_000_000 && managed.len() <= 3,
        "GLOBAL_CAP",
    )?;
    let _ = plan;
    Ok(json!({
        "aggregate": format_micros(aggregate),
        "cumulative": format_micros(cumulative),
        "managedMarketIds": managed.iter().cloned().collect::<Vec<String>>(),
    }))
}

pub struct PairedSharedFunding {
    inner: Rc<RefCell<FundingInner>>,
}

pub struct PairedSharedFundingDeps {
    pub get_account_lease: Rc<dyn Fn() -> V>,
    pub get_other_state: Rc<dyn Fn() -> V>,
    pub get_paired_funding: Rc<dyn Fn() -> V>,
    pub get_paired_status: Rc<dyn Fn() -> V>,
    pub can_operate: Rc<dyn Fn() -> bool>,
    pub now: Rc<dyn Fn() -> f64>,
}

// openPairedSharedFunding({dir,activation,controllerDir,getAccountLease,getOtherState,getPairedFunding,getPairedStatus,canOperate,now})
pub fn open_paired_shared_funding(
    dir: &Path,
    activation: &V,
    controller_dir: &Path,
    deps: PairedSharedFundingDeps,
) -> Result<PairedSharedFunding, String> {
    need(
        dir.is_absolute() && controller_dir.is_absolute(),
        "DEPENDENCIES",
    )?;
    activation_check(activation, dir, controller_dir)?;
    let activation = activation.clone();
    let activation_hash = payload_hash(&activation)?;
    // Read only the already initialized actor's immutable opening. This is not
    // another actor session and never takes the shared account lease.
    let actor_file = controller_dir.join("paired-rest-journal.jsonl");
    let actor = Journal::open(&actor_file)?;
    need(
        get(&get(&get(&actor.last.clone().unwrap_or(V::Null), "state"), "plan"), "planHash")
            == get(&activation, "planHash")
            && get(&get(&get(&actor.last.clone().unwrap_or(V::Null), "state"), "plan"), "captureHash")
                == get(&activation, "captureHash"),
        "ACTOR_OPENING",
    )?;
    let plan = get(&get(&actor.last.clone().unwrap_or(V::Null), "state"), "plan").clone();
    let markets: Vec<V> = get(&plan, "legs")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|x| json!({"marketId": get(x, "marketId"), "exchangeId": get(x, "exchangeId")}))
        .collect();
    need(
        get(get(&plan, "scope"), "ownerId") == get(&activation, "ownerId")
            && get(get(&plan, "scope"), "runId") == get(&activation, "runId")
            && get(get(&plan, "scope"), "tournamentId") == &json!(TID)
            && markets.len() == 2
            && markets.iter().map(|x| get(x, "exchangeId").as_str().unwrap_or("")).collect::<BTreeSet<_>>().len() == 2
            && markets.iter().map(|x| get(x, "marketId").as_str().unwrap_or("")).collect::<BTreeSet<_>>().len() == 2
            && a(get(&plan, "pairCostCeiling")) <= 990_000,
        "PAIR_BINDING",
    )?;
    let lease = acquire_owner_lease(
        dir,
        "Codex",
        get(&activation, "runId").as_str().unwrap_or(""),
        "shared-pair-funding-registry",
        (deps.now)(),
    )
    .map_err(|e| e.message)?;
    let file = dir.join("paired-shared-funding.jsonl");

    let open_result: Result<(Journal, Option<V>, u64, String, FileIdentity), String> = (|| {
        if !file.exists() {
            // fs.openSync(file,'wx',0o600): create-new, then close.
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&file)
                .map_err(|e| e.to_string())?;
        }
        let raw = fs::read(&file).map_err(|e| e.to_string())?;
        let journal = Journal::open(&file)?;
        need(raw.is_empty() || *raw.last().unwrap() == 10, "PARTIAL_JOURNAL")?;
        let mut state: Option<V> = None;
        for line in String::from_utf8_lossy(&raw).split('\n').filter(|l| !l.is_empty()) {
            let row: V = serde_json::from_str(line).map_err(|_| "ONE_PAIR_SLOT".to_string())?;
            need(
                get(&row, "state").get("activationHash") == Some(&json!(activation_hash))
                    && get(&row, "state").get("controllerDir") == Some(&json!(controller_dir.to_string_lossy().to_string()))
                    && get(&row, "state").get("planHash") == Some(get(&activation, "planHash")),
                "ONE_PAIR_SLOT",
            )?;
            if let Some(prev) = &state {
                need(
                    a(&get(&get(&row, "state"), "funding").get("cumulativeBuy").cloned().unwrap_or(V::Null))
                        >= a(&get(&get(prev, "funding"), "cumulativeBuy").clone())
                        && a(&get(&get(&row, "state"), "funding").get("ownedPrincipal").cloned().unwrap_or(V::Null))
                            >= a(&get(&get(prev, "funding"), "ownedPrincipal").clone()),
                    "JOURNAL_COST_RESET",
                )?;
                core_forward(get(prev, "otherCore"), &get(&row, "state").get("otherCore").cloned().unwrap_or(V::Null))?;
            }
            state = Some(get(&row, "state").clone());
        }
        let size = raw.len() as u64;
        let bytes_hash = sha256(&raw);
        let identity = file_identity(&file)?;
        Ok((journal, state, size, bytes_hash, identity))
    })();
    let (journal, state, size, bytes_hash, identity) = match open_result {
        Ok(v) => v,
        Err(e) => {
            let mut lease = lease;
            let _ = lease.release();
            return Err(e);
        }
    };
    let inner = Rc::new(RefCell::new(FundingInner {
        dir: dir.to_path_buf(),
        controller_dir: controller_dir.to_path_buf(),
        file,
        actor_file,
        activation,
        activation_hash,
        plan,
        markets,
        journal,
        state,
        closed: false,
        failed: false,
        size,
        bytes_hash,
        identity,
        lease,
        get_account_lease: deps.get_account_lease,
        get_other_state: deps.get_other_state,
        get_paired_funding: deps.get_paired_funding,
        get_paired_status: deps.get_paired_status,
        can_operate: deps.can_operate,
        now: deps.now,
    }));
    Ok(PairedSharedFunding { inner })
}

impl PairedSharedFunding {
    // append(next,event)
    fn append(&self, next: &V, event: &V) -> Result<(), String> {
        let mut g = self.inner.borrow_mut();
        g.basic()?;
        let previous = fs::read(&g.file).map_err(|e| e.to_string())?;
        let stat = file_identity(&g.file)?;
        need(
            stat.ino == g.identity.ino
                && stat.birthtime_ms == g.identity.birthtime_ms
                && previous.len() as u64 == g.size
                && sha256(&previous) == g.bytes_hash,
            "JOURNAL_CHANGED",
        )?;
        let commit_result = g.journal.commit(next, event);
        match commit_result {
            Ok(_) => {
                g.basic()?;
                let after = fs::read(&g.file).map_err(|e| e.to_string())?;
                need(
                    after[..g.size as usize] == previous[..]
                        && after.len() as u64 > g.size,
                    "JOURNAL_CHANGED",
                )?;
                g.state = Some(next.clone());
                g.size = after.len() as u64;
                g.bytes_hash = sha256(&after);
                Ok(())
            }
            Err(e) => {
                g.failed = true;
                Err(e)
            }
        }
    }

    // installFundingReservation(record)
    pub fn install_funding_reservation(&self, record: &V) -> Result<V, String> {
        let lease_hash = self.inner.borrow().basic()?;
        let pair = self.inner.borrow().source()?;
        need(
            payload_hash(record)? == payload_hash(&pair.f)?,
            "RECORD_NOT_ACTUAL_CONTROLLER",
        )?;
        let core = core_view(&(self.inner.borrow().get_other_state)())?;
        {
            let g = self.inner.borrow();
            core_forward(
                g.state.as_ref().and_then(|s| s.get("otherCore")).unwrap_or(&V::Null),
                &core.s,
            )?;
        }
        {
            let g = self.inner.borrow();
            if g.state.is_none() {
                need(
                    (g.now)() >= at(get(&g.activation, "startedAt"))? as f64
                        && (g.now)() < at(get(&g.activation, "entryExpiresAt"))? as f64,
                    "INITIAL_WINDOW",
                )?;
                need(
                    core.pending.is_empty()
                        && !core.intents.iter().any(|i| get(i, "acceptanceUncertain") == &json!(true)),
                    "INITIAL_CORE_UNKNOWN",
                )?;
                need(
                    !get(&core.s, "owned").as_object().map(|o| {
                        o.keys().any(|id| g.markets.iter().any(|m| get(m, "exchangeId").as_str() == Some(id.as_str())))
                    }).unwrap_or(false),
                    "EXISTING_PAIR_HOLDING",
                )?;
            } else if !pair.ended {
                need(core.pending.is_empty(), "EXCLUSIVE_CORE_ACTIVE")?;
            }
        }
        let (capacity, activation_hash, plan_hash, capture_hash, next_at) = {
            let g = self.inner.borrow();
            let capacity = limits(&core, &pair, &g.plan, &g.markets)?;
            (
                capacity,
                g.activation_hash.clone(),
                get(&g.activation, "planHash").clone(),
                get(&g.activation, "captureHash").clone(),
                baal_service_runtime::contract::ms_to_iso((g.now)()),
            )
        };
        let g = self.inner.borrow();
        let next = json!({
            "schemaVersion": 1,
            "activationHash": activation_hash,
            "controllerDir": g.controller_dir.to_string_lossy(),
            "planHash": plan_hash,
            "captureHash": capture_hash,
            "funding": pair.f,
            "owned": pair.owned,
            "pairedOrders": get(&pair.s, "orders"),
            "actorAnchor": pair.actor_anchor,
            "ended": pair.ended,
            "exclusiveActive": !pair.ended,
            "otherCore": core.s,
            "pairedSourceHash": pair.source_hash,
            "accountLeaseHash": lease_hash,
            "capacity": capacity,
            "at": next_at,
        });
        let had_state = g.state.is_some();
        drop(g);
        self.append(
            &next,
            &json!({
                "kind": if had_state { "funding_update" } else { "one_pair_claim" },
                "activationHash": activation_hash,
                "pairedSourceHash": pair.source_hash,
            }),
        )?;
        Ok(json!({
            "reservationHash": get(&pair.f, "reservationHash"),
            "amount": get(&pair.f, "reservedCash"),
            "globalLedgerHash": self.inner.borrow().journal.previous,
            "confirmedAt": get(&next, "at"),
        }))
    }

    // allowsOther(spec)
    pub fn allows_other(&self, spec: &V) -> bool {
        if get(spec, "method").as_str() == Some("GET") {
            return true;
        }
        let run: Result<bool, String> = (|| {
            self.inner.borrow().basic()?;
            if self
                .inner
                .borrow()
                .state
                .as_ref()
                .is_some_and(|s| get(s, "exclusiveActive") == &json!(true))
            {
                return Ok(false);
            }
            let pair = {
                let g = self.inner.borrow();
                if g.state.is_some() {
                    g.source()?
                } else {
                    PairSource {
                        f: json!({
                            "ownedPrincipal": "0.000000",
                            "cumulativeBuy": "0.000000",
                            "reservedCash": "0.000000",
                        }),
                        s: V::Null,
                        owned: vec![],
                        ended: true,
                        actor_anchor: V::Null,
                        source_hash: String::new(),
                    }
                }
            };
            need(pair.ended, "PAIR_NOT_ENDED")?;
            let core = core_view(&(self.inner.borrow().get_other_state)())?;
            {
                let g = self.inner.borrow();
                core_forward(
                    g.state.as_ref().and_then(|s| s.get("otherCore")).unwrap_or(&V::Null),
                    &core.s,
                )?;
                limits(&core, &pair, &g.plan, &g.markets)?;
            }
            if get(spec, "method").as_str() == Some("DELETE") {
                let endpoint = get(spec, "endpoint").as_str().unwrap_or("");
                let order_id = endpoint
                    .strip_prefix("/orders/")
                    .filter(|t| !t.is_empty() && !t.starts_with('0') && t.bytes().all(|b| b.is_ascii_digit()));
                let i = order_id.and_then(|oid| {
                    core.intents.iter().find(|x| {
                        js_int(get(x, "orderId")).map(|n| n.to_string()) == Some(oid.to_string())
                            && get(x, "status").as_str() != Some("closed")
                    })
                });
                return Ok(i.is_some()
                    && get(i.unwrap(), "side").as_str() == Some("yes")
                    && spec.get("body").is_none()
                    && spec.get("bodyJson").is_none());
            }
            need(
                get(spec, "method").as_str() == Some("POST")
                    && get(spec, "endpoint").as_str() == Some("/orders")
                    && !get(spec, "body").is_null()
                    && get(spec, "bodyJson").as_str()
                        == Some(canonical_payload(get(spec, "body"))?.as_str()),
                "OTHER_REQUEST",
            )?;
            let i = core.intents.iter().find(|x| {
                get(x, "status").as_str() != Some("closed")
                    && get(x, "payloadHash").as_str() == Some(payload_hash(get(spec, "body")).unwrap_or_default().as_str())
                    && get(x, "bodyJson") == get(spec, "bodyJson")
            }).cloned();
            need(
                i.is_some()
                    && get(i.as_ref().unwrap(), "side").as_str() == Some("yes")
                    && matches!(
                        get(i.as_ref().unwrap(), "status").as_str(),
                        Some("prepared" | "dispatching" | "uncertain" | "acknowledged" | "reconciled")
                    )
                    && a(get(i.as_ref().unwrap(), "price")) * js_int(get(i.as_ref().unwrap(), "quantity")).unwrap_or(0) as i128 <= 1_000_000,
                "KNOWN_ORIGINAL_INTENT",
            )?;
            let i = i.unwrap();
            need(
                !pair.owned.iter().any(|p| get(p, "exchangeId") == get(&i, "exchangeId")),
                "NO_CROSS_NET",
            )?;
            if get(&i, "action").as_str() == Some("sell") {
                let covered = get(&core.s, "owned")
                    .get(get(&i, "exchangeId").as_str().unwrap_or(""))
                    .and_then(|v| js_int(get(v, "quantity")))
                    .unwrap_or(0)
                    >= js_int(get(&i, "quantity")).unwrap_or(0);
                need(covered, "COVERED_SELL")?;
            }
            {
                let g = self.inner.borrow();
                if g.state.is_some()
                    && payload_hash(g.state.as_ref().unwrap().get("otherCore").unwrap_or(&V::Null)).unwrap_or_default() != core.hash
                {
                    let capacity = limits(&core, &pair, &g.plan, &g.markets)?;
                    let mut next = g.state.clone().unwrap();
                    let at = baal_service_runtime::contract::ms_to_iso((g.now)());
                    next.as_object_mut().unwrap().insert("otherCore".into(), core.s.clone());
                    next.as_object_mut().unwrap().insert("capacity".into(), capacity);
                    next.as_object_mut().unwrap().insert("at".into(), json!(at));
                    let event = json!({"kind": "original_core_observed", "originalCoreHash": core.hash});
                    drop(g);
                    self.append(&next, &event)?;
                }
            }
            Ok(true)
        })();
        run.unwrap_or(false)
    }

    // exclusiveActive()
    pub fn exclusive_active(&self) -> bool {
        let g = self.inner.borrow();
        if g.state.is_none() {
            return false;
        }
        let r: Result<bool, String> = (|| {
            g.basic()?;
            let p = g.source()?;
            Ok(get(g.state.as_ref().unwrap(), "exclusiveActive") == &json!(true) || !p.ended)
        })();
        r.unwrap_or(true)
    }

    // status()
    pub fn status(&self) -> V {
        let g = self.inner.borrow();
        json!({
            "mode": "one_pair_shared_funding_v1",
            "activationHash": g.activation_hash,
            "planHash": get(&g.activation, "planHash"),
            "claimed": g.state.is_some(),
            "exclusiveActive": g.state.as_ref().map(|s| get(s, "exclusiveActive").clone()).unwrap_or(json!(false)),
            "ended": g.state.as_ref().map(|s| get(s, "ended").clone()).unwrap_or(json!(false)),
            "owned": g.state.as_ref().and_then(|s| s.get("owned").cloned()).unwrap_or(V::Array(vec![])),
            "funding": g.state.as_ref().and_then(|s| s.get("funding").cloned()).unwrap_or(V::Null),
            "globalLedgerHash": g.journal.previous,
            "sequence": g.journal.sequence,
            "failed": g.failed,
            "closed": g.closed,
            "executionAuthority": false,
            "additionalClients": 0,
        })
    }

    // close()
    pub fn close(&self) {
        let mut g = self.inner.borrow_mut();
        if g.closed {
            return;
        }
        g.closed = true;
        let _ = g.lease.release();
    }
}
