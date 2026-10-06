// execution/paired-shared-funding.mjs — one durable pair slot in the host's
// fixed global registry. This is a budget gate, not an account owner, cash
// receipt, dispatcher or trading activation.
use crate::core::{
    canonical_payload, decimal_micros, format_micros, payload_hash, validate_execution_state,
};
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::journal::Journal;
use baal_service_runtime::json_text::{js_int, sha256};
use baal_service_runtime::ledger::{acquire_owner_lease, LedgerError, OwnerLease};
use serde_json::{json, Map, Value as V};
use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::fs;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;

const TID: &str = "bda92870-621e-47b0-bc3c-3602c5c26f55";
fn need(x: bool, c: &str) -> Result<(), String> {
    if x {
        Ok(())
    } else {
        Err(format!("PAIRED_FUNDING_{c}"))
    }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn hex(x: &V) -> bool {
    x.as_str().is_some_and(|s| {
        s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}
fn ledger_err(e: LedgerError) -> String {
    format!("{}: {}", e.code, e.message)
}
// at(x): Date.parse then a canonical-ISO roundtrip.
fn at(x: &V) -> Result<f64, String> {
    let s = x.as_str().unwrap_or("");
    let n = parse_iso_ms(s);
    need(n.is_some() && ms_to_iso(n.unwrap()) == s, "CLOCK")?;
    Ok(n.unwrap())
}
fn a(v: &V) -> Result<i128, String> {
    decimal_micros(v, "exact")
}
fn asum(xs: impl Iterator<Item = i128>) -> i128 {
    xs.sum()
}
fn normalize_path(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}
fn birthtime(meta: &fs::Metadata) -> (i64, i64) {
    let t = meta
        .created()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .unwrap_or_default();
    (t.as_secs() as i64, t.subsec_nanos() as i64)
}
fn run_id_ok(s: &str) -> bool {
    // /^[A-Za-z0-9_.:-]{1,120}$/
    !s.is_empty()
        && s.len() <= 120
        && s.bytes().all(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b':' | b'-')
        })
}

const SLOT_KEYS: [&str; 19] = [
    "schemaVersion",
    "kind",
    "ownerId",
    "runId",
    "tournamentId",
    "activationAuthorityHash",
    "planHash",
    "captureHash",
    "registryDir",
    "controllerDir",
    "startedAt",
    "entryExpiresAt",
    "allocatedCapital",
    "maxOrderCost",
    "maxAggregateCost",
    "maxCumulativeBuyCost",
    "maxManagedMarkets",
    "quantity",
    "maxPairs",
];
fn activation_check(x: &V, dir: &Path, controller_dir: &Path) -> Result<(), String> {
    let mut keys: Vec<&String> = x
        .as_object()
        .map(|m| m.keys().collect())
        .unwrap_or_default();
    keys.sort();
    let mut slot_sorted = SLOT_KEYS.to_vec();
    slot_sorted.sort();
    need(
        x.is_object()
            && keys.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("|")
                == slot_sorted.join("|")
            && js_int(&x["schemaVersion"]) == Some(1)
            && x["kind"] == "one_pair_shared_funding"
            && x["ownerId"] == "Codex"
            && x["tournamentId"] == TID
            && x["runId"].as_str().is_some_and(run_id_ok)
            && hex(&x["activationAuthorityHash"])
            && hex(&x["planHash"])
            && hex(&x["captureHash"]),
        "ACTIVATION",
    )?;
    need(
        get(x, "registryDir").as_str().is_some_and(|s| Path::new(s).is_absolute())
            && get(x, "controllerDir").as_str().is_some_and(|s| Path::new(s).is_absolute())
            && normalize_path(Path::new(get(x, "registryDir").as_str().unwrap_or("")))
                == normalize_path(dir)
            && normalize_path(Path::new(get(x, "controllerDir").as_str().unwrap_or("")))
                == normalize_path(controller_dir),
        "FIXED_DIRECTORIES",
    )?;
    need(
        at(get(x, "entryExpiresAt"))? > at(get(x, "startedAt"))?
            && at(get(x, "entryExpiresAt"))? - at(get(x, "startedAt"))? <= 3_600_000.0
            && x["allocatedCapital"] == "2.000000"
            && x["maxOrderCost"] == "1.000000"
            && x["maxAggregateCost"] == "10.000000"
            && x["maxCumulativeBuyCost"] == "25.000000"
            && x["maxManagedMarkets"] == json!(3)
            && js_int(&x["quantity"]) == Some(1)
            && js_int(&x["maxPairs"]) == Some(1),
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
fn core_view(input: &V) -> Result<CoreView, String> {
    let s = validate_execution_state(input)?;
    need(
        s["tournamentId"] == TID
            && a(&s["policy"]["maxAggregateCost"])? <= 10_000_000
            && a(&s["policy"]["maxCumulativeBuyCost"])? <= 25_000_000
            && a(&s["policy"]["maxOrderCost"])? <= 1_000_000
            && s["policy"]["maxManagedMarkets"].as_i64().unwrap_or(i64::MAX) <= 3,
        "OTHER_CORE_POLICY",
    )?;
    let intents: Vec<V> = get(&s, "intents")
        .as_object()
        .map(|m| m.values().cloned().collect())
        .unwrap_or_default();
    let owned = asum(
        get(&s, "owned")
            .as_object()
            .map(|m| m.values().map(|p| a(get(p, "cost")).unwrap_or(0)).collect::<Vec<_>>())
            .unwrap_or_default()
            .into_iter(),
    );
    let spent = asum(
        intents
            .iter()
            .filter(|i| i["action"] == "buy")
            .flat_map(|i| {
                get(i, "reconciledFills")
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|f| a(get(&f, "price")).unwrap_or(0) * get(&f, "quantity").as_i64().unwrap_or(0) as i128)
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
            .into_iter(),
    );
    let pending: Vec<V> = intents
        .iter()
        .filter(|i| i["status"] != "closed")
        .cloned()
        .collect();
    let reserve = asum(
        pending
            .iter()
            .filter(|i| i["action"] == "buy")
            .map(|i| {
                let fills: i64 = get(i, "reconciledFills")
                    .as_array()
                    .map(|fs| fs.iter().map(|f| f["quantity"].as_i64().unwrap_or(0)).sum())
                    .unwrap_or(0);
                a(get(i, "price")).unwrap_or(0)
                    * (i["quantity"].as_i64().unwrap_or(0) - fills) as i128
            })
            .collect::<Vec<_>>()
            .into_iter(),
    );
    let hash = payload_hash(&s)?;
    Ok(CoreView {
        s,
        intents,
        pending,
        owned,
        spent,
        reserve,
        hash,
    })
}
// coreForward(old, next) — `if (!old) return` uses JS truthiness.
fn core_forward(old: &V, next: &V) -> Result<(), String> {
    if !baal_service_runtime::json_text::js_truthy(Some(old)) {
        return Ok(());
    }
    need(
        payload_hash(get(old, "policy"))? == payload_hash(get(next, "policy"))?,
        "OTHER_POLICY_CHANGED",
    )?;
    if let V::Object(intents) = get(old, "intents") {
        for (id, i) in intents {
            let n = get(get(next, "intents"), id);
            need(
                !n.is_null()
                    && get(n, "payloadHash") == get(i, "payloadHash")
                    && get(n, "bodyJson") == get(i, "bodyJson"),
                "OTHER_INTENT_RESET",
            )?;
            let fills_ok = get(i, "reconciledFills")
                .as_array()
                .map(|fs| {
                    fs.iter().all(|f| {
                        get(n, "reconciledFills")
                            .as_array()
                            .map(|gs| {
                                gs.iter().any(|g| {
                                    canonical_payload(g).unwrap_or_default()
                                        == canonical_payload(f).unwrap_or_default()
                                })
                            })
                            .unwrap_or(false)
                    })
                })
                .unwrap_or(false);
            need(
                fills_ok && (get(i, "status") != "closed" || get(n, "status") == "closed"),
                "OTHER_FILL_RESET",
            )?;
        }
    }
    Ok(())
}

struct PairSource {
    f: V,
    s: V,
    owned: Vec<V>,
    ended: bool,
    actor_anchor: V,
    source_hash: V,
}

pub struct PairedSharedFundingOptions {
    pub dir: PathBuf,
    pub activation: V,
    pub controller_dir: PathBuf,
    pub get_account_lease: Rc<dyn Fn() -> Result<V, String>>,
    pub get_other_state: Rc<dyn Fn() -> Result<V, String>>,
    pub get_paired_funding: Rc<dyn Fn() -> Result<V, String>>,
    pub get_paired_status: Rc<dyn Fn() -> Result<V, String>>,
    pub can_operate: Rc<dyn Fn() -> bool>,
    pub now: Rc<dyn Fn() -> f64>,
}

struct Inner {
    state: V, // Null until claimed
    closed: bool,
    failed: bool,
    size: usize,
    bytes_hash: String,
    file_ino: u64,
    file_birth: (i64, i64),
    journal: Journal,
    lease: OwnerLease,
}
pub struct PairedSharedFunding {
    inner: RefCell<Inner>,
    opts: PairedSharedFundingOptions,
    activation: V,
    activation_hash: String,
    plan: V,
    markets: Vec<V>,
    file: PathBuf,
    actor_file: PathBuf,
}

/// openPairedSharedFunding({dir, activation, controllerDir, getAccountLease,
/// getOtherState, getPairedFunding, getPairedStatus, canOperate, now})
pub fn open_paired_shared_funding(
    opts: PairedSharedFundingOptions,
) -> Result<PairedSharedFunding, String> {
    need(
        opts.dir.is_absolute() && opts.controller_dir.is_absolute(),
        "DEPENDENCIES",
    )?;
    activation_check(&opts.activation, &opts.dir, &opts.controller_dir)?;
    let activation = opts.activation.clone();
    let activation_hash = payload_hash(&activation)?;
    // Read only the already initialized actor's immutable opening. This is not
    // another actor session and never takes the shared account lease.
    let actor_file = opts.controller_dir.join("paired-rest-journal.jsonl");
    let actor = Journal::open(&actor_file).map_err(|e| format!("PAIRED_FUNDING_{e}"))?;
    let actor_last = actor.last.clone().unwrap_or(V::Null);
    let plan = get(get(&actor_last, "state"), "plan").clone();
    need(
        get(&plan, "planHash") == get(&activation, "planHash")
            && get(&plan, "captureHash") == get(&activation, "captureHash"),
        "ACTOR_OPENING",
    )?;
    let markets: Vec<V> = get(&plan, "legs")
        .as_array()
        .map(|ls| {
            ls.iter()
                .map(|x| json!({"marketId": x["marketId"], "exchangeId": x["exchangeId"]}))
                .collect()
        })
        .unwrap_or_default();
    need(
        get(get(&plan, "scope"), "ownerId") == get(&activation, "ownerId")
            && get(get(&plan, "scope"), "runId") == get(&activation, "runId")
            && get(get(&plan, "scope"), "tournamentId") == TID
            && markets.len() == 2
            && markets
                .iter()
                .map(|x| get(x, "exchangeId").as_str().unwrap_or(""))
                .collect::<BTreeSet<_>>()
                .len()
                == 2
            && markets
                .iter()
                .map(|x| get(x, "marketId").as_str().unwrap_or(""))
                .collect::<BTreeSet<_>>()
                .len()
                == 2
            && a(get(&plan, "pairCostCeiling"))? <= 990_000,
        "PAIR_BINDING",
    )?;
    let mut local = acquire_owner_lease(
        &opts.dir,
        "Codex",
        get(&activation, "runId").as_str().unwrap_or(""),
        "shared-pair-funding-registry",
        (opts.now)(),
    )
    .map_err(ledger_err)?;
    let file = opts.dir.join("paired-shared-funding.jsonl");
    // try { ... } catch (e) { local.release(); throw e; } — everything past the
    // lease acquisition runs inside this closure; on failure the lease is
    // released before the error propagates.
    let replayed = (|| -> Result<(V, usize, String, u64, (i64, i64), Journal), String> {
        if !file.exists() {
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&file)
                .map_err(|e| e.to_string())?;
        }
        let raw = fs::read(&file).map_err(|e| e.to_string())?;
        let journal = Journal::open(&file).map_err(|e| format!("PAIRED_FUNDING_{e}"))?;
        need(raw.is_empty() || raw.last() == Some(&10), "PARTIAL_JOURNAL")?;
        let mut state = V::Null;
        for line in String::from_utf8_lossy(&raw).split('\n').filter(|l| !l.is_empty()) {
            let row: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
            need(
                get(get(&row, "state"), "activationHash").as_str()
                    == Some(activation_hash.as_str())
                    && get(get(&row, "state"), "controllerDir").as_str()
                        == Some(normalize_path(&opts.controller_dir).to_string_lossy().as_ref())
                    && get(get(&row, "state"), "planHash") == get(&activation, "planHash"),
                "ONE_PAIR_SLOT",
            )?;
            if !state.is_null() {
                need(
                    a(get(get(get(&row, "state"), "funding"), "cumulativeBuy"))?
                        >= a(get(get(&state, "funding"), "cumulativeBuy"))?
                        && a(get(get(get(&row, "state"), "funding"), "ownedPrincipal"))?
                            >= a(get(get(&state, "funding"), "ownedPrincipal"))?,
                    "JOURNAL_COST_RESET",
                )?;
                core_forward(get(&state, "otherCore"), get(get(&row, "state"), "otherCore"))?;
            }
            state = get(&row, "state").clone();
        }
        let meta = fs::metadata(&file).map_err(|e| e.to_string())?;
        Ok((
            state,
            raw.len(),
            sha256(&raw),
            meta.ino(),
            birthtime(&meta),
            journal,
        ))
    })();
    let (state, size, bytes_hash, file_ino, file_birth, journal) = match replayed {
        Ok(x) => x,
        Err(e) => {
            let _ = local.release();
            return Err(e);
        }
    };
    Ok(PairedSharedFunding {
        inner: RefCell::new(Inner {
            state,
            closed: false,
            failed: false,
            size,
            bytes_hash,
            file_ino,
            file_birth,
            journal,
            lease: local,
        }),
        opts,
        activation,
        activation_hash,
        plan,
        markets,
        file,
        actor_file,
    })
}

impl PairedSharedFunding {
    // basic() — stopped/failed guard + owner lease + actual account lease.
    fn basic(&self) -> Result<String, String> {
        {
            let i = self.inner.borrow();
            need(!i.closed && !i.failed && (self.opts.can_operate)(), "STOPPED_OR_FAILED")?;
        }
        self.inner.borrow().lease.assert_ownership().map_err(ledger_err)?;
        let l = (self.opts.get_account_lease)()?;
        need(
            get(&l, "ownerId") == "Codex" && get(&l, "tournamentId") == TID && hex(get(&l, "leaseId")),
            "ACTUAL_ACCOUNT_LEASE",
        )?;
        payload_hash(&json!({
            "ownerId": get(&l, "ownerId"),
            "tournamentId": get(&l, "tournamentId"),
            "leaseId": get(&l, "leaseId"),
        }))
    }

    // source() — re-read the actor journal + controller status and funding,
    // then verify pair accounting and carry-forward invariants.
    fn source(&self) -> Result<PairSource, String> {
        let f = (self.opts.get_paired_funding)()?.clone();
        let s = (self.opts.get_paired_status)()?.clone();
        let actor_bytes = fs::read(&self.actor_file).map_err(|e| e.to_string())?;
        let actual_actor = Journal::open(&self.actor_file).map_err(|e| format!("PAIRED_FUNDING_{e}"))?;
        need(
            get(&s, "sequence").as_i64().is_some_and(|n| n > 0)
                && hex(get(&s, "hash"))
                && json!(actual_actor.sequence as i64) == s["sequence"]
                && actual_actor.previous == get(&s, "hash").as_str().unwrap_or("")
                && get(get(get(&actual_actor.last.clone().unwrap_or(V::Null), "state"), "plan"), "planHash")
                    == get(&self.activation, "planHash"),
            "ACTUAL_ACTOR_JOURNAL",
        )?;
        {
            let i = self.inner.borrow();
            let anchor = get(&i.state, "actorAnchor");
            if !anchor.is_null() {
                need(
                    actor_bytes.len() >= anchor["bytes"].as_u64().unwrap_or(0) as usize
                        && sha256(&actor_bytes[..anchor["bytes"].as_u64().unwrap_or(0) as usize])
                            == anchor["sha256"].as_str().unwrap_or(""),
                    "ACTOR_PREFIX_RESET",
                )?;
            }
        }
        let actor_anchor = json!({
            "bytes": actor_bytes.len(),
            "sha256": sha256(&actor_bytes),
            "sequence": get(&s, "sequence"),
            "hash": get(&s, "hash"),
        });
        need(
            get(&s, "mode")
                == &json!(if self.plan["schemaVersion"] == json!(2) {
                    "paired_rest_lifecycle_v2"
                } else {
                    "paired_rest_lifecycle_v1"
                })
            && get(&s, "planHash") == get(&self.activation, "planHash")
            && payload_hash(get(&s, "funding"))? == payload_hash(&f)?
            && get(&f, "ownerId") == "Codex"
            && get(&f, "runId") == get(&self.activation, "runId")
            && get(&f, "tournamentId") == TID
            && get(&f, "planHash") == get(&self.activation, "planHash")
            && get(&f, "allocatedCapital") == "2.000000"
            && get(&f, "maxCumulativeBuy") == "2.000000"
            && get(&s, "orders").as_array().is_some_and(|o| o.len() <= 2)
            && get(&s, "owned").as_array().is_some_and(|o| o.len() <= 2),
            "ACTUAL_PAIRED_SOURCE",
        )?;
        // const {reservationHash, ...body} = f
        let mut body = f.clone();
        let reservation_hash = match &mut body {
            V::Object(m) => m.shift_remove("reservationHash"),
            _ => None,
        };
        need(
            reservation_hash.as_ref().and_then(|v| v.as_str())
                == Some(payload_hash(&body)?.as_str())
                && a(get(&f, "reservedCash"))? >= 0
                && a(get(&f, "ownedPrincipal"))? >= 0
                && a(get(&f, "cumulativeBuy"))? == a(get(&f, "ownedPrincipal"))?
                && a(get(&f, "cumulativeBuy"))? <= a(get(&self.plan, "pairCostCeiling"))?,
            "PAIR_ACCOUNTING",
        )?;
        let mut owned: Vec<V> = Vec::new();
        for p in get(&s, "owned").as_array().cloned().unwrap_or_default() {
            need(
                self.markets.iter().any(|x| {
                    get(x, "marketId") == get(&p, "marketId")
                        && get(x, "exchangeId") == get(&p, "exchangeId")
                }) && js_int(&p["quantity"]) == Some(1)
                    && p["side"] == "no"
                    && p["ownerId"] == "Codex"
                    && get(&p, "runId") == get(&self.activation, "runId")
                    && get(&s, "orders").as_array().is_some_and(|orders| {
                        orders.iter().any(|o| {
                            o["intentId"] == p["intentId"]
                                && o["orderId"] == p["orderId"]
                                && o["phase"] == "reconciled"
                                && hex(get(o, "proofHash"))
                        })
                    }),
                "OWNED_PROVENANCE",
            )?;
            need(
                !owned.iter().any(|x| x["exchangeId"] == p["exchangeId"]),
                "DUPLICATE_OWNED",
            )?;
            owned.push(json!({
                "marketId": p["marketId"],
                "exchangeId": p["exchangeId"],
                "quantity": 1,
                "entryPrice": p["entryPrice"],
                "orderId": p["orderId"],
                "intentId": p["intentId"],
            }));
        }
        need(
            asum(owned.iter().map(|p| a(get(p, "entryPrice")).unwrap_or(0)).collect::<Vec<_>>().into_iter())
                == a(get(&f, "ownedPrincipal"))?,
            "OWNED_PRINCIPAL",
        )?;
        let ended = ["paired", "orphaned", "unfilled"].contains(&s["phase"].as_str().unwrap_or(""))
            && get(&s, "orders").as_array().is_some_and(|o| !o.is_empty())
            && get(&s, "orders")
                .as_array()
                .map(|os| os.iter().all(|o| o["phase"] == "reconciled"))
                .unwrap_or(false);
        need(
            if ended {
                a(get(&f, "reservedCash"))? == 0
            } else {
                a(get(&f, "reservedCash"))? + a(get(&f, "cumulativeBuy"))?
                    == a(get(&self.plan, "pairCostCeiling"))?
            },
            "FULL_UNKNOWN_RESERVE",
        )?;
        {
            let i = self.inner.borrow();
            let state = &i.state;
            if !state.is_null() {
                let orders = get(&s, "orders").as_array().cloned().unwrap_or_default();
                need(
                    orders.len() >= get(state, "pairedOrders").as_array().map(|p| p.len()).unwrap_or(0)
                        && get(state, "pairedOrders").as_array().map(|ps| {
                            ps.iter().all(|o| {
                                orders.iter().any(|n| {
                                    n["intentId"] == o["intentId"]
                                        && n["bodyHash"] == o["bodyHash"]
                                        && (!baal_service_runtime::json_text::js_truthy(o.get("orderId"))
                                            || n["orderId"] == o["orderId"])
                                        && (o["phase"] != "reconciled"
                                            || n["phase"] == "reconciled")
                                })
                            })
                        }).unwrap_or(false),
                    "PAIR_ORDER_RESET",
                )?;
                need(
                    a(get(&f, "ownedPrincipal"))? >= a(get(get(state, "funding"), "ownedPrincipal"))?
                        && a(get(&f, "cumulativeBuy"))? >= a(get(get(state, "funding"), "cumulativeBuy"))?,
                    "PAIR_COST_RESET",
                )?;
                need(
                    get(state, "owned").as_array().map(|ps| {
                        ps.iter().all(|p| {
                            owned.iter().any(|n| {
                                canonical_payload(n).unwrap_or_default()
                                    == canonical_payload(p).unwrap_or_default()
                            })
                        })
                    }).unwrap_or(false),
                    "PAIR_OWNERSHIP_RESET",
                )?;
                need(
                    !get(state, "ended").as_bool().unwrap_or(false) || ended,
                    "PAIR_REOPENED",
                )?;
            }
        }
        Ok(PairSource {
            source_hash: json!(payload_hash(&json!({
                "sequence": get(&s, "sequence"),
                "hash": get(&s, "hash"),
                "funding": f,
                "orders": get(&s, "orders"),
                "owned": get(&s, "owned"),
            }))?),
            f,
            s,
            owned,
            ended,
            actor_anchor,
        })
    }

    // append(next, event) — verify the durable prefix then commit and verify.
    fn append(&self, next: &V, event: &V) -> Result<(), String> {
        self.basic()?;
        let (size, bytes_hash, ino, birth) = {
            let i = self.inner.borrow();
            (i.size, i.bytes_hash.clone(), i.file_ino, i.file_birth)
        };
        let previous = fs::read(&self.file).map_err(|e| e.to_string())?;
        let stat = fs::metadata(&self.file).map_err(|e| e.to_string())?;
        need(
            stat.ino() == ino
                && birthtime(&stat) == birth
                && previous.len() == size
                && sha256(&previous) == bytes_hash,
            "JOURNAL_CHANGED",
        )?;
        let write_result = (|| -> Result<(), String> {
            self.inner.borrow_mut().journal.commit(next, event)?;
            self.basic()?;
            let after = fs::read(&self.file).map_err(|e| e.to_string())?;
            need(
                after[..size] == previous[..] && after.len() > size,
                "JOURNAL_CHANGED",
            )?;
            let mut i = self.inner.borrow_mut();
            i.state = next.clone();
            i.size = after.len();
            i.bytes_hash = sha256(&after);
            Ok(())
        })();
        if let Err(e) = write_result {
            self.inner.borrow_mut().failed = true;
            return Err(e);
        }
        Ok(())
    }

    // limits(core, pair)
    fn limits(&self, core: &CoreView, pair_ended: bool, pair_owned: &[V], pair_f: &V) -> Result<V, String> {
        need(
            !core.intents.iter().any(|i| {
                pair_owned.iter().any(|p| p["exchangeId"] == i["exchangeId"])
                    && i["status"] != "closed"
            }),
            "CROSS_NET_PENDING",
        )?;
        let aggregate = core.owned + core.reserve + a(get(pair_f, "ownedPrincipal"))? + a(get(pair_f, "reservedCash"))?;
        let cumulative = core.spent + core.reserve + a(get(pair_f, "cumulativeBuy"))? + a(get(pair_f, "reservedCash"))?;
        let mut managed: BTreeSet<String> = BTreeSet::new();
        if let V::Object(owned) = get(&core.s, "owned") {
            for p in owned.values() {
                if let Some(m) = p["marketId"].as_str() {
                    managed.insert(m.to_string());
                }
            }
        }
        for i in &core.pending {
            if i["action"] == "buy" {
                if let Some(m) = i["marketId"].as_str() {
                    managed.insert(m.to_string());
                }
            }
        }
        let pair_markets: Vec<V> = if pair_ended {
            pair_owned.to_vec()
        } else {
            self.markets.clone()
        };
        for p in &pair_markets {
            if let Some(m) = p["marketId"].as_str() {
                managed.insert(m.to_string());
            }
        }
        need(
            aggregate <= 10_000_000 && cumulative <= 25_000_000 && managed.len() <= 3,
            "GLOBAL_CAP",
        )?;
        Ok(json!({
            "aggregate": format_micros(aggregate),
            "cumulative": format_micros(cumulative),
            "managedMarketIds": managed.into_iter().collect::<Vec<_>>(),
        }))
    }

    /// installFundingReservation(record)
    pub fn install_funding_reservation(&self, record: &V) -> Result<V, String> {
        let lease_hash = self.basic()?;
        let pair = self.source()?;
        need(
            payload_hash(record)? == payload_hash(&pair.f)?,
            "RECORD_NOT_ACTUAL_CONTROLLER",
        )?;
        let core = core_view(&(self.opts.get_other_state)()?)?;
        let prev_other_core = self.inner.borrow().state["otherCore"].clone();
        core_forward(&prev_other_core, &core.s)?;
        if self.inner.borrow().state.is_null() {
            need(
                (self.opts.now)() >= at(get(&self.activation, "startedAt"))?
                    && (self.opts.now)() < at(get(&self.activation, "entryExpiresAt"))?,
                "INITIAL_WINDOW",
            )?;
            need(
                core.pending.is_empty()
                    && !core.intents.iter().any(|i| {
                        i["acceptanceUncertain"].as_bool().unwrap_or(false)
                    }),
                "INITIAL_CORE_UNKNOWN",
            )?;
            need(
                !get(&core.s, "owned")
                    .as_object()
                    .map(|o| {
                        o.keys().any(|id| {
                            self.markets.iter().any(|m| m["exchangeId"].as_str() == Some(id.as_str()))
                        })
                    })
                    .unwrap_or(false),
                "EXISTING_PAIR_HOLDING",
            )?;
        } else if !pair.ended {
            need(core.pending.is_empty(), "EXCLUSIVE_CORE_ACTIVE")?;
        }
        let capacity = self.limits(&core, pair.ended, &pair.owned, &pair.f)?;
        let is_claim = self.inner.borrow().state.is_null();
        let next = json!({
            "schemaVersion": 1,
            "activationHash": self.activation_hash,
            "controllerDir": normalize_path(&self.opts.controller_dir).to_string_lossy(),
            "planHash": get(&self.activation, "planHash"),
            "captureHash": get(&self.activation, "captureHash"),
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
            "at": ms_to_iso((self.opts.now)()),
        });
        self.append(
            &next,
            &json!({
                "kind": if is_claim { "one_pair_claim" } else { "funding_update" },
                "activationHash": self.activation_hash,
                "pairedSourceHash": pair.source_hash,
            }),
        )?;
        let previous = self.inner.borrow().journal.previous.clone();
        Ok(json!({
            "reservationHash": get(&pair.f, "reservationHash"),
            "amount": get(&pair.f, "reservedCash"),
            "globalLedgerHash": previous,
            "confirmedAt": get(&next, "at"),
        }))
    }

    /// allowsOther(spec) — false while exclusive, then the other-account rules.
    /// Never throws: any failure denies the write.
    pub fn allows_other(&self, spec: &V) -> bool {
        if get(spec, "method").as_str() == Some("GET") {
            return true;
        }
        (|| -> Result<bool, String> {
            self.basic()?;
            if get(&self.inner.borrow().state, "exclusiveActive").as_bool().unwrap_or(false) {
                return Ok(false);
            }
            let have_state = !self.inner.borrow().state.is_null();
            let pair = if have_state {
                self.source()?
            } else {
                PairSource {
                    f: json!({
                        "ownedPrincipal": "0.000000",
                        "cumulativeBuy": "0.000000",
                        "reservedCash": "0.000000",
                    }),
                    s: V::Null,
                    owned: Vec::new(),
                    ended: true,
                    actor_anchor: V::Null,
                    source_hash: V::Null,
                }
            };
            need(pair.ended, "PAIR_NOT_ENDED")?;
            let core = core_view(&(self.opts.get_other_state)()?)?;
            let prev_other_core = self.inner.borrow().state["otherCore"].clone();
            core_forward(&prev_other_core, &core.s)?;
            self.limits(&core, pair.ended, &pair.owned, &pair.f)?;
            if get(spec, "method").as_str() == Some("DELETE") {
                // /^\/orders\/([1-9]\d*)$/
                let endpoint = get(spec, "endpoint").as_str().unwrap_or("");
                let oid = endpoint
                    .strip_prefix("/orders/")
                    .filter(|r| !r.is_empty() && !r.starts_with('0') && r.bytes().all(|b| b.is_ascii_digit()));
                let i = core.intents.iter().find(|x| {
                    crate::js_key(get(x, "orderId")) == oid.unwrap_or("\u{0}")
                        && x["status"] != "closed"
                });
                return Ok(i.is_some()
                    && i.unwrap()["side"] == "yes"
                    && spec.get("body").is_none()
                    && spec.get("bodyJson").is_none());
            }
            need(
                get(spec, "method").as_str() == Some("POST")
                    && get(spec, "endpoint").as_str() == Some("/orders")
                    && baal_service_runtime::json_text::js_truthy(spec.get("body"))
                    && get(spec, "bodyJson").as_str()
                        == Some(canonical_payload(get(spec, "body"))?.as_str()),
                "OTHER_REQUEST",
            )?;
            let spec_body_hash = payload_hash(get(spec, "body"))?;
            let i = core
                .intents
                .iter()
                .find(|x| {
                    x["status"] != "closed"
                        && x["payloadHash"].as_str() == Some(spec_body_hash.as_str())
                        && x["bodyJson"] == spec["bodyJson"]
                })
                .cloned();
            need(
                i.is_some()
                    && i.as_ref().unwrap()["side"] == "yes"
                    && [
                        "prepared",
                        "dispatching",
                        "uncertain",
                        "acknowledged",
                        "reconciled",
                    ]
                    .contains(&i.as_ref().unwrap()["status"].as_str().unwrap_or(""))
                    && a(get(i.as_ref().unwrap(), "price")).unwrap_or(i128::MAX)
                        * i.as_ref().unwrap()["quantity"].as_i64().unwrap_or(i64::MAX) as i128
                        <= 1_000_000,
                "KNOWN_ORIGINAL_INTENT",
            )?;
            let i = i.unwrap();
            need(
                !pair.owned.iter().any(|p| p["exchangeId"] == i["exchangeId"]),
                "NO_CROSS_NET",
            )?;
            if i["action"] == "sell" {
                let owned_q = get(get(&core.s, "owned"), i["exchangeId"].as_str().unwrap_or(""))
                    ["quantity"]
                    .as_i64()
                    .unwrap_or(i64::MIN);
                need(
                    owned_q >= i["quantity"].as_i64().unwrap_or(i64::MAX),
                    "COVERED_SELL",
                )?;
            }
            if have_state && payload_hash(&self.inner.borrow().state["otherCore"])? != core.hash {
                let state_next = {
                    let i_ref = self.inner.borrow();
                    let mut n = i_ref.state.clone();
                    n["otherCore"] = core.s.clone();
                    n["capacity"] = self.limits(&core, pair.ended, &pair.owned, &pair.f)?;
                    n["at"] = json!(ms_to_iso((self.opts.now)()));
                    n
                };
                self.append(
                    &state_next,
                    &json!({
                        "kind": "original_core_observed",
                        "originalCoreHash": core.hash,
                    }),
                )?;
            }
            Ok(true)
        })()
        .unwrap_or(false)
    }

    /// exclusiveActive()
    pub fn exclusive_active(&self) -> bool {
        if self.inner.borrow().state.is_null() {
            return false;
        }
        (|| -> Result<bool, String> {
            self.basic()?;
            let p = self.source()?;
            Ok(get(&self.inner.borrow().state, "exclusiveActive").as_bool().unwrap_or(false)
                || !p.ended)
        })()
        .unwrap_or(true)
    }

    /// status()
    pub fn status(&self) -> V {
        let i = self.inner.borrow();
        json!({
            "mode": "one_pair_shared_funding_v1",
            "activationHash": self.activation_hash,
            "planHash": get(&self.activation, "planHash"),
            "claimed": !i.state.is_null(),
            "exclusiveActive": get(&i.state, "exclusiveActive").as_bool().unwrap_or(false),
            "ended": get(&i.state, "ended").as_bool().unwrap_or(false),
            "owned": get(&i.state, "owned").as_array().cloned().unwrap_or_default(),
            "funding": get(&i.state, "funding"),
            "globalLedgerHash": i.journal.previous,
            "sequence": i.journal.sequence,
            "failed": i.failed,
            "closed": i.closed,
            "executionAuthority": false,
            "additionalClients": 0,
        })
    }

    /// close()
    pub fn close(&self) {
        let mut i = self.inner.borrow_mut();
        if i.closed {
            return;
        }
        i.closed = true;
        let _ = i.lease.release();
    }
}
