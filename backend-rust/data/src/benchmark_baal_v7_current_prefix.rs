// data/benchmark-baal-v7-current-prefix.mjs — OFFLINE ONLY: actual sealed
// history, hypothetical fresh responses, zero network. Raw copies stay in a
// new child of the already restricted paper runtime.
//
// Ports the script body as `run(exit_only)` mirroring the --exit flag. All
// orchestration, checks and the output record match the JS exactly. The JS
// mock fetchFn returns a `Response`; here it returns
// scoped_rest_receipts_v5::FetchOutcome (text body + status 200), and the
// ReadClient's `Fetch` adapter drains the scoped response reader into the
// client's FetchOutcome — equivalent wire bytes.
//
// JS `authorizeDispatch(s)` receives the dispatch spec *with* its before
// guard functions; the ported ReadClient safe-view strips them, so the
// dispatch wrapper stashes the guard data-pointers in `last_guards` and
// `authorize_dispatch` passes them to `host.authorizes(spec, bd_ptr, bn_ptr)`
// — the same identity comparison the JS performs on function references.
//
// Threading note: the benchmark is single-threaded like the JS it ports, but
// the producer/ReadClient closures are typed `Send + Sync`. `Share` wraps the
// Rc state honestly — nothing ever crosses a thread boundary — and
// `BorrowedGuard` similarly carries the `&Guard` borrows into the synchronous
// dispatch window where the blocking client invokes them.
use baal_execution::core::payload_hash;
use baal_paper::fixed::{big_amount, big_price, money};
use baal_service_runtime::client::{
    Clocks, DispatchSpec, Fetch, FetchOutcome as ClientOutcome, FetchRequest, ReadClient,
    ReadClientOptions,
};
use baal_service_runtime::contract::ms_to_iso;
use baal_service_runtime::json_text::sha256;
use baal_service_runtime::ledger::acquire_owner_lease;
use num_bigint::BigInt;
use serde_json::{json, Map, Value as V};
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use uuid::Uuid;

use crate::baal_combined_custody_host_v6::{
    baal_combined_custody_v6_identity, create_baal_combined_custody_v6_descriptor,
};
use crate::baal_combined_custody_host_v7::{
    baal_combined_custody_v7_identity, open_baal_combined_custody_host_v7, CustodyHostDeps,
    CustodyHostV7,
};
use crate::baal_scaled_controller_legacy_replay_v1::{
    baal_scaled_no_controller_identity, replay_baal_scaled_controller_v1,
};
use crate::owned_no_fresh_none_hedge_evidence_bridge_v1::{Guard, OwnedNoneBridgeApi};
use crate::scoped_rest_receipts_v5::{
    create_scoped_rest_receipts_v5, FetchOutcome, ScopedRestReceiptsV5,
};

fn get<'a>(v: &'a V, k: &str) -> &'a V {
    match v {
        V::Object(m) => m.get(k).unwrap_or(&V::Null),
        _ => &V::Null,
    }
}
fn sha(b: &[u8]) -> String {
    sha256(b)
}
fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(format!("OFFLINE_CURRENT_PREFIX_{code}"))
    }
}
fn read_json(p: &Path) -> Result<V, String> {
    serde_json::from_slice(&std::fs::read(p).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}
fn write_600(p: &Path, body: &[u8]) -> Result<(), String> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true).mode(0o600);
    let mut f = o.open(p).map_err(|e| e.to_string())?;
    std::io::Write::write_all(&mut f, body).map_err(|e| e.to_string())
}
fn mkdir_700(p: &Path) -> Result<(), String> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(p)
        .map_err(|e| e.to_string())
}
fn parse_ts(v: &V) -> i64 {
    v.as_str()
        .and_then(|s| baal_paper::fixed::instant(s).ok())
        .unwrap_or(0)
}
/// JS String(x) for the mock's numeric coercion sites.
fn num_string(v: &V) -> String {
    match v {
        V::Null => "undefined".into(),
        V::Bool(b) => b.to_string(),
        V::Number(n) => {
            if let Some(i) = n.as_i64() {
                i.to_string()
            } else {
                n.as_f64().unwrap_or(0.0).to_string()
            }
        }
        V::String(s) => s.clone(),
        _ => "NaN".into(),
    }
}
/// `u.pathname.slice('/api/v1'.length) + u.search` — drops the first 7 chars
/// of the path and appends the raw query (with `?`).
fn endpoint_of(url: &str) -> String {
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let start = rest.find('/').unwrap_or(0);
    let pq = &rest[start..];
    let (path, search) = match pq.find('?') {
        Some(i) => (&pq[..i], &pq[i..]),
        None => (pq, ""),
    };
    let sliced = if path.len() > 7 { &path[7..] } else { "" };
    format!("{sliced}{search}")
}
fn query_get(endpoint: &str, key: &str) -> Option<String> {
    let q = endpoint.split_once('?')?.1;
    for pair in q.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            if k == key {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// Single-threaded share for Rc state used inside Send+Sync-typed closures.
struct Share<T>(Rc<T>);
unsafe impl<T> Send for Share<T> {}
unsafe impl<T> Sync for Share<T> {}
impl<T> Share<T> {
    fn get(&self) -> &T {
        &self.0
    }
}

struct BorrowedGuard(*const Guard<'static>);
unsafe impl Send for BorrowedGuard {}
unsafe impl Sync for BorrowedGuard {}
impl BorrowedGuard {
    /// Reborrow the caller-scoped guard inside the synchronous dispatch call.
    fn guard(&self) -> &Guard<'static> {
        unsafe { &*self.0 }
    }
}

struct ProducerFetch {
    inner: Arc<ScopedRestReceiptsV5>,
}
impl Fetch for ProducerFetch {
    fn fetch(&self, url: &str, req: &FetchRequest) -> Result<ClientOutcome, String> {
        let mut options = Map::new();
        options.insert("method".into(), json!(req.method));
        if let Some(b) = &req.body {
            options.insert("body".into(), json!(b));
        }
        let mut r = self.inner.fetch(url, &V::Object(options))?;
        let status = get(&r.value, "status").as_u64().unwrap_or(200) as u16;
        let body = r.text()?;
        Ok(ClientOutcome {
            status,
            retry_after: None,
            body,
        })
    }
}

/// run — the script body. `exit_only` mirrors `process.argv.includes('--exit')`.
pub fn run(exit_only: bool) -> Result<V, String> {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let source_runtime = match std::env::var_os("SIG_PAPER_DIR") {
        Some(d) => PathBuf::from(d),
        None => PathBuf::from(
            std::env::var_os("LOCALAPPDATA").ok_or_else(|| "LOCALAPPDATA".to_string())?,
        )
        .join("SigTradingCup")
        .join("paper-runtime"),
    };
    let request = read_json(&source_runtime.join("baal-scaled-live-request.json"))?;
    let relative = Path::new("baal-scaled-live-runs")
        .join(get(&request, "runId").as_str().unwrap_or(""))
        .join("controller")
        .join("baal-scaled-no-journal.jsonl");
    let original_file = source_runtime.join(&relative);
    let original_bytes = std::fs::read(&original_file).map_err(|e| e.to_string())?;
    let seed = replay_baal_scaled_controller_v1(&json!({
        "bodyText": String::from_utf8_lossy(&original_bytes),
        "expectedCodeIdentity": baal_scaled_no_controller_identity()?,
    }))?;
    need(
        sha(&std::fs::read(&original_file).map_err(|e| e.to_string())?)
            == get(get(&seed, "anchor"), "sha256").as_str().unwrap_or(""),
        "SEAL_CHANGED",
    )?;
    let first_ex = "1070";
    let inventory = get(get(&seed, "state"), "inventory").clone();
    let owned = get(&inventory, first_ex).clone();
    need(
        get(get(&owned, "proof"), "remainingOwnedQuantity")
            .as_f64()
            .map(|q| q > 0.0)
            .unwrap_or(false),
        "FIRST_LOT_MUST_STILL_EXIST",
    )?;
    let legs = get(get(&owned, "package"), "legs")
        .as_array()
        .cloned()
        .unwrap_or_default();
    let first = legs
        .iter()
        .find(|x| get(x, "exchangeId").as_str() == Some(first_ex))
        .cloned()
        .unwrap_or(V::Null);
    let other = legs
        .iter()
        .find(|x| get(x, "exchangeId").as_str() != Some(first_ex))
        .cloned()
        .unwrap_or(V::Null);
    let other_ex = get(&other, "exchangeId").as_str().unwrap_or("").to_string();
    need(
        !get(
            get(&inventory, other_ex.as_str()),
            "proof",
        )
        .pipe("remainingOwnedQuantity")
        .as_f64()
        .map(|q| q > 0.0)
        .unwrap_or(false),
        "COUNTERPART_NOT_FLAT",
    )?;
    let mut roots = V::Null;
    let text = String::from_utf8_lossy(&original_bytes);
    for line in text.trim_end().split('\n') {
        let r: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
        let e = get(get(&r, "event"), "input");
        if get(e, "kind").as_str() == Some("prepared")
            && get(get(e, "evidence"), "rootReceipts")
                .as_array()
                .map(|a| {
                    a.iter()
                        .any(|x| get(x, "exchangeId").as_str() == Some(first_ex))
                })
                .unwrap_or(false)
        {
            roots = get(get(e, "evidence"), "rootReceipts").clone();
        }
    }
    let roots_arr = roots.as_array().cloned().unwrap_or_default();
    need(
        roots_arr.len() == 2
            && roots_arr.iter().all(|x| {
                legs.iter().any(|l| {
                    get(l, "exchangeId") == get(x, "exchangeId")
                        && get(l, "marketId") == get(x, "marketId")
                })
            }),
        "ROOT_SOURCES",
    )?;
    let mut inv_sorted: Vec<&V> = inventory
        .as_object()
        .map(|m| m.values().collect())
        .unwrap_or_default();
    inv_sorted.sort_by_key(|s| std::cmp::Reverse(parse_ts(get(get(*s, "proof"), "readAt"))));
    let latest = inv_sorted.first().copied().cloned().unwrap_or(V::Null);
    let cash: V = serde_json::from_str(
        get(get(get(&latest, "after"), "cash"), "bodyText")
            .as_str()
            .unwrap_or("null"),
    )
    .map_err(|e| e.to_string())?;
    let positions: V = serde_json::from_str(
        get(get(get(&latest, "after"), "positions"), "bodyText")
            .as_str()
            .unwrap_or("null"),
    )
    .map_err(|e| e.to_string())?;
    let remaining = get(get(&owned, "proof"), "remainingOwnedQuantity")
        .as_f64()
        .unwrap_or(0.0);
    need(
        get(&positions, "positions")
            .as_array()
            .map(|a| {
                a.iter().any(|p| {
                    get(p, "exchangeId").as_str() == Some(first_ex)
                        && get(p, "quantity").as_f64() == Some(-remaining)
                })
            })
            .unwrap_or(false),
        "ACTUAL_LATEST_POSITION",
    )?;
    let scoped: V = serde_json::from_str(
        get(
            get(&owned, "after")
                .pipe("scopedOrdersPages")
                .as_array()
                .and_then(|a| a.first())
                .unwrap_or(&V::Null),
            "bodyText",
        )
        .as_str()
        .unwrap_or("null"),
    )
    .map_err(|e| e.to_string())?;
    let private_dir = source_runtime.join(format!("offline-custody-v6-{}", Uuid::new_v4()));
    need(
        private_dir.parent().map(|p| p.to_path_buf()) == Some(source_runtime.clone()),
        "PRIVATE_PATH",
    )?;
    mkdir_700(&private_dir)?;
    for f in [
        "baal-scaled-live-request.json",
        "baal-scaled-host-upgrade.json",
        "baal-scaled-host-upgrade-v3.json",
        "baal-scaled-host-upgrade-v5.json",
        "baal-capital-allocation.json",
        "paired-live-authority.json",
        "baal-scaled-rootcatalog.json",
    ] {
        std::fs::copy(source_runtime.join(f), private_dir.join(f)).map_err(|e| e.to_string())?;
    }
    mkdir_700(&private_dir.join(&relative).parent().unwrap().to_path_buf())?;
    write_600(&private_dir.join(&relative), &original_bytes)?;
    let universe = baal_collector::breadth::validate_universe(
        &read_json(&workspace.join("live-paper").join("price-universe.json"))?,
        get(&request, "tournamentId").as_str().unwrap_or(""),
    )?;
    let base = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as f64
        + 1000.0;
    let start = Instant::now();
    let virtual_delay = Arc::new(Mutex::new(0.0f64));
    let host_cell: Rc<RefCell<Option<CustodyHostV7>>> = Rc::new(RefCell::new(None));
    let make_now = |vd: &Arc<Mutex<f64>>| {
        let vd = Arc::clone(vd);
        move || base + *vd.lock().unwrap() + start.elapsed().as_millis() as f64
    };
    let now: Rc<dyn Fn() -> f64> = Rc::new(make_now(&virtual_delay));
    let now_sync: Arc<dyn Fn() -> f64 + Send + Sync> = Arc::new(make_now(&virtual_delay));
    let iso = || ms_to_iso(now());
    let descriptor = create_baal_combined_custody_v6_descriptor(
        &workspace,
        &universe,
        &request,
        &read_json(&private_dir.join("baal-scaled-host-upgrade-v5.json"))?,
        &seed,
        first_ex,
        &format!("offline-current-prefix-{}", Uuid::new_v4()),
        &iso(),
        &ms_to_iso(now() + 3600000.0),
        &ms_to_iso(now() + 86400000.0),
        None,
    )?;
    write_600(
        &private_dir.join("baal-combined-custody-request-v6.json"),
        format!(
            "{}\n",
            serde_json::to_string(&descriptor).map_err(|e| e.to_string())?
        )
        .as_bytes(),
    )?;
    write_600(
        &private_dir.join("baal-combined-read-upgrade-v7.json"),
        format!(
            "{}\n",
            serde_json::to_string(&json!({
                "schemaVersion": 1,
                "kind": "baal_combined_read_upgrade_v7",
                "ownerId": get(&descriptor, "ownerId"),
                "runId": get(&descriptor, "runId"),
                "tournamentId": get(&descriptor, "tournamentId"),
                "originalDescriptorHash": payload_hash(&descriptor)?,
                "codeIdentity": baal_combined_custody_v7_identity(&workspace)?,
                "recordedAt": iso(),
            }))
            .map_err(|e| e.to_string())?
        )
        .as_bytes(),
    )?;
    let core_lease = Rc::new(RefCell::new(
        acquire_owner_lease(
            &private_dir.join("offline-actual-core-lease"),
            "Codex",
            "offline-core",
            get(&request, "tournamentId").as_str().unwrap_or(""),
            now(),
        )
        .map_err(|e| e.message)?,
    ));
    let lease_identity = payload_hash(&core_lease.borrow().identity)?;
    let core = baal_execution::core::create_execution_state(
        get(&request, "tournamentId").as_str().unwrap_or(""),
        None,
    )?;
    let page = |data: V| -> V {
        json!({
            "data": data,
            "pagination": {"limit": 200, "hasMore": false, "nextCursor": null},
            "coverage": {"complete": true, "projectedThroughSequence": 99999999},
        })
    };
    let calls: Arc<Mutex<Vec<V>>> = Arc::new(Mutex::new(Vec::new()));
    let market_ids: Vec<V> = legs.iter().map(|x| get(x, "marketId").clone()).collect();
    let entry_price_str = get(
        get(&owned, "proof")
            .pipe("remainingLots")
            .as_array()
            .and_then(|a| a.first())
            .unwrap_or(&V::Null),
        "entryPrice",
    )
    .as_str()
    .unwrap_or("0")
    .to_string();
    let fetch_fn = {
        let calls = Arc::clone(&calls);
        let vd = Arc::clone(&virtual_delay);
        let now = Arc::clone(&now_sync);
        let roots = roots.clone();
        let cash = cash.clone();
        let positions = positions.clone();
        let scoped = scoped.clone();
        let first_ex = first_ex.to_string();
        let other_ex = other_ex.clone();
        let tournament_slug = get(&request, "tournamentSlug")
            .as_str()
            .unwrap_or("")
            .to_string();
        let tournament_id = get(&request, "tournamentId")
            .as_str()
            .unwrap_or("")
            .to_string();
        let entry_price_str = entry_price_str.clone();
        Arc::new(
            move |url: &str, options: &V| -> Result<FetchOutcome, String> {
                let endpoint = endpoint_of(url);
                calls.lock().unwrap().push(json!({
                    "method": options.get("method").cloned().unwrap_or(json!("GET")),
                    "endpoint": endpoint,
                    "at": ms_to_iso(now()),
                }));
                let method = options.get("method").and_then(V::as_str).unwrap_or("GET");
                let d: V = if method == "POST" {
                    let b: V = serde_json::from_str(
                        options.get("body").and_then(V::as_str).unwrap_or("null"),
                    )
                    .map_err(|e| e.to_string())?;
                    need(
                        if exit_only {
                            get(&b, "action").as_str() == Some("sell")
                                && get(&b, "quantity")
                                    .as_f64()
                                    .map(|q| q > 0.0 && q <= 100.0)
                                    .unwrap_or(false)
                                && get(&b, "exchangeId").as_str() == Some(first_ex.as_str())
                        } else {
                            get(&b, "action").as_str() == Some("buy")
                                && get(&b, "quantity").as_f64() == Some(1.0)
                                && get(&b, "exchangeId").as_str() == Some(other_ex.as_str())
                        },
                        "ONLY_HYPOTHETICAL_BOUND_ORDER",
                    )?;
                    let unit = big_price(&num_string(get(&b, "price")))?;
                    let total = money(
                        &(&unit * BigInt::from(get(&b, "quantity").as_i64().unwrap_or(0))),
                    );
                    json!({
                        "orderId": 990000001i64,
                        "exchangeId": get(&b, "exchangeId"),
                        "side": "no",
                        "action": get(&b, "action"),
                        "quantity": get(&b, "quantity"),
                        "quantityTraded": get(&b, "quantity"),
                        "remainingQuantity": 0,
                        "open": false,
                        "price": get(&b, "price"),
                        "fillPrice": get(&b, "price"),
                        "totalCost": total.parse::<f64>().unwrap_or(0.0),
                        "all": V::Null,
                    })
                } else if endpoint.contains("/nodes?") {
                    let target = roots
                        .as_array()
                        .map(|a| a.as_slice())
                        .unwrap_or(&[])
                        .iter()
                        .find(|r| {
                            endpoint.contains(&format!(
                                "/{}/",
                                get(r, "marketId").as_str().unwrap_or("")
                            ))
                        })
                        .cloned()
                        .unwrap_or(V::Null);
                    serde_json::from_str(
                        get(&target, "receipt")
                            .pipe("bodyText")
                            .as_str()
                            .unwrap_or("null"),
                    )
                    .map_err(|e| e.to_string())?
                } else if endpoint == format!("/tournaments/{tournament_slug}") {
                    cash.clone()
                } else if endpoint.ends_with("/portfolio/positions") {
                    positions.clone()
                } else if endpoint.starts_with("/orders?status=open") {
                    page(V::Array(vec![]))
                } else if endpoint.starts_with("/orders?status=all") {
                    if query_get(&endpoint, "exchangeId").as_deref() == Some(first_ex.as_str()) {
                        scoped.clone()
                    } else {
                        page(V::Array(vec![]))
                    }
                } else if endpoint.contains("/orderbook?") {
                    let limit = BigInt::from(990000) - big_amount(&entry_price_str)?;
                    let bid = if exit_only {
                        BigInt::from(995000) - (big_amount(&entry_price_str)? + BigInt::from(10000))
                    } else {
                        BigInt::from(1000000) - (&limit - BigInt::from(5000))
                    };
                    let bid_price: f64 = money(&bid).parse().unwrap_or(0.0);
                    let ask_price: f64 =
                        money(&(bid.clone() + BigInt::from(5000))).parse().unwrap_or(0.0);
                    json!({
                        "contexts": [{
                            "type": "tournament",
                            "tournament": {"id": tournament_id},
                            "orderbook": {"exchanges": [{
                                "exchangeId": if exit_only { &first_ex } else { &other_ex },
                                "option": "YES",
                                "asOf": {"sequence": 999999999i64, "at": ms_to_iso(now())},
                                "bids": [{"price": bid_price, "quantity": 300}],
                                "asks": [{"price": ask_price, "quantity": 300}],
                            }]},
                        }],
                    })
                } else if endpoint.starts_with("/relationships/graph?") {
                    json!({"nodes": [], "relationships": []})
                } else if endpoint.starts_with("/portfolio/collateral?") {
                    json!({"data": []})
                } else {
                    return Err("OFFLINE_UNEXPECTED_ROUTE".to_string());
                };
                *vd.lock().unwrap() += 100.0;
                Ok(FetchOutcome::text(
                    json!({"status": 200}),
                    serde_json::to_string(&d).map_err(|e| e.to_string())?,
                ))
            },
        )
    };
    let producer = Arc::new(create_scoped_rest_receipts_v5(
        fetch_fn,
        &json!({
            "tournamentId": get(&request, "tournamentId"),
            "tournamentSlug": get(&request, "tournamentSlug"),
            "marketIds": V::Array(market_ids.clone()),
            "allowedOwnedGraphMarketIds": V::Array(market_ids),
        }),
        Arc::clone(&now_sync),
        {
            let host_cell = Share(Rc::clone(&host_cell));
            Arc::new(move |e: &V| {
                match host_cell.get().borrow().as_ref() {
                    Some(h) => h.on_fetch_start(e).map(|_| ()),
                    None => Ok(()),
                }
            })
        },
        {
            let host_cell = Share(Rc::clone(&host_cell));
            Some(Arc::new(move |e: &V| {
                match host_cell.get().borrow().as_ref() {
                    Some(h) => h.on_before_fetch(e),
                    None => Ok(V::Null),
                }
            }))
        },
        64,
        64 * 1024 * 1024,
    )?);
    // JS api.next/api.cooldown/api.manualReview are public fields on the
    // ReadClient; the custody host mutates them through the shared api object.
    let api_fields = Rc::new(RefCell::new(OwnedNoneBridgeApi {
        dispatch: Rc::new(|_, _, _| Ok(V::Null)),
        manual_review: false,
        cooldown: 0.0,
    }));
    let api_next = Rc::new(Cell::new(0.0f64));
    let last_guards: Rc<Cell<(usize, usize)>> = Rc::new(Cell::new((0, 0)));
    let api = Rc::new(ReadClient::new(
        "OFFLINE-NONCREDENTIAL",
        ReadClientOptions {
            fetch: Some(Arc::new(ProducerFetch {
                inner: Arc::clone(&producer),
            })),
            network_budget: Some(json!({
                "schemaVersion": 2,
                "ownerId": "Codex",
                "enabled": true,
                "intervalMs": 1000,
                "writeIntervalMs": 2500,
                "soleAccountClient": true,
                "formerWriterStopCommentId": 5947535773i64,
            })),
            monitor_event_loop: false,
            clocks: Clocks {
                now: Arc::clone(&now_sync),
                sleep: {
                    let vd = Arc::clone(&virtual_delay);
                    Arc::new(move |ms: f64| {
                        *vd.lock().unwrap() += ms;
                    })
                },
            },
            authorize_dispatch: {
                let host_cell = Share(Rc::clone(&host_cell));
                let last_guards = Share(Rc::clone(&last_guards));
                Some(Arc::new(move |s: &V| {
                    match host_cell.get().borrow().as_ref() {
                        Some(h) => {
                            let (bd, bn) = last_guards.get().get();
                            h.authorizes(s, bd, bn)
                        }
                        None => false,
                    }
                }))
            },
            ..Default::default()
        },
    )?);
    // The dispatch surface the host and bridges see: api.dispatch(spec) drives
    // the same ReadClient and, as in JS, the guard functions ride on the spec.
    api_fields.borrow_mut().dispatch = {
        let api = Rc::clone(&api);
        let last_guards = Rc::clone(&last_guards);
        Rc::new(
            move |spec: &V, bd: Option<&Guard>, bn: Option<&Guard>| -> Result<V, String> {
                let bd_ptr = bd
                    .map(|g| g as *const Guard as *const () as usize)
                    .unwrap_or(0);
                let bn_ptr = bn
                    .map(|g| g as *const Guard as *const () as usize)
                    .unwrap_or(0);
                last_guards.set((bd_ptr, bn_ptr));
                let before_dispatch = bd.map(|g| {
                    let raw = BorrowedGuard(unsafe {
                        std::mem::transmute::<*const Guard<'_>, *const Guard<'static>>(g)
                    });
                    Arc::new(move || -> Result<(), String> {
                        raw.guard()(&V::Null, &V::Null).map(|_| ())
                    }) as Arc<dyn Fn() -> Result<(), String> + Send + Sync>
                });
                let before_network = bn.map(|g| {
                    let raw = BorrowedGuard(unsafe {
                        std::mem::transmute::<*const Guard<'_>, *const Guard<'static>>(g)
                    });
                    Arc::new(move || -> Result<(), String> {
                        raw.guard()(&V::Null, &V::Null).map(|_| ())
                    }) as Arc<dyn Fn() -> Result<(), String> + Send + Sync>
                });
                let spec = DispatchSpec {
                    method: get(spec, "method").as_str().unwrap_or("GET").to_string(),
                    endpoint: get(spec, "endpoint").as_str().unwrap_or("").to_string(),
                    body: spec.get("body").cloned(),
                    body_json: get(spec, "bodyJson").as_str().map(|s| s.to_string()),
                    before_dispatch,
                    before_network,
                };
                api.dispatch(spec).map_err(|e| format!("\u{0}{e}"))
            },
        )
    };
    let on_admission = {
        let api_next = Rc::clone(&api_next);
        let api_fields = Rc::clone(&api_fields);
        Rc::new(move |v: &V| {
            if v.is_null() {
                api_fields.borrow_mut().manual_review = true;
            } else {
                api_next.set(v.as_f64().unwrap_or(0.0));
            }
        })
    };
    let run_result = (|| -> Result<V, String> {
        let host = open_baal_combined_custody_host_v7(
            &private_dir,
            &workspace,
            &universe,
            CustodyHostDeps {
                api: Rc::clone(&api_fields),
                api_next: Rc::clone(&api_next),
                on_admission: on_admission.clone(),
                receipt_of: {
                    let producer = Arc::clone(&producer);
                    Rc::new(move |r: &V, meta: &V| producer.receipt_of(r, meta))
                },
                get_account_lease: {
                    let identity = lease_identity.clone();
                    let tournament_id = get(&request, "tournamentId")
                        .as_str()
                        .unwrap_or("")
                        .to_string();
                    Rc::new(move || {
                        Ok(json!({
                            "ownerId": "Codex",
                            "tournamentId": tournament_id,
                            "leaseId": identity,
                        }))
                    })
                },
                get_core_owner_record: {
                    let core_lease = Rc::clone(&core_lease);
                    Rc::new(move || {
                        core_lease.borrow().assert_ownership().map_err(|e| e.message)?;
                        Ok(core_lease.borrow().identity.clone())
                    })
                },
                get_other_state: {
                    let core = core.clone();
                    Rc::new(move || Ok(core.clone()))
                },
                get_legacy_risk: Rc::new(|| {
                    Ok(json!({
                        "complete": true,
                        "sourceHash": "a".repeat(64),
                        "currentOwnedQuantity": 0,
                        "currentReservedCash": "0.000000",
                        "unresolvedIntentIds": [],
                    }))
                }),
                get_pair_read_hints: {
                    let tournament_id = get(&request, "tournamentId")
                        .as_str()
                        .unwrap_or("")
                        .to_string();
                    let universe_hash = get(&universe, "hash").clone();
                    let first_market = get(&first, "marketId").clone();
                    let entry_price_str = entry_price_str.clone();
                    let first_ex = first_ex.to_string();
                    let now = Rc::clone(&now);
                    Rc::new(move || {
                        if !exit_only {
                            return Ok(V::Null);
                        }
                        let entry = big_amount(&entry_price_str)?;
                        Ok(json!({
                            "tournamentId": tournament_id,
                            "universeHash": universe_hash,
                            "quoteCaptures": [{
                                "marketId": first_market,
                                "exchangeId": first_ex,
                                "option": "YES",
                                "tournamentId": tournament_id,
                                "bestBid": money(&(BigInt::from(985000) - &entry)).parse::<f64>().unwrap_or(0.0),
                                "bestAsk": money(&(BigInt::from(990000) - &entry)).parse::<f64>().unwrap_or(0.0),
                                "crossed": false,
                                "sourceHash": "e".repeat(64),
                                "requestStartedAt": ms_to_iso(now() - 200.0),
                                "receivedAt": ms_to_iso(now() - 100.0),
                            }],
                        }))
                    })
                },
                can_operate: Rc::new(|| true),
                has_read_headroom: Rc::new(|_| Ok(true)),
                now: Rc::clone(&now),
            },
        )?;
        *host_cell.borrow_mut() = host;
        let s = host_cell.borrow().as_ref().map(|h| h.step()).transpose()?;
        let s = s.unwrap_or(V::Null);
        let controller = get(&s, "controller").clone();
        let calls_b = calls.lock().unwrap();
        let get_calls = calls_b
            .iter()
            .filter(|c| get(c, "method").as_str() == Some("GET"))
            .count();
        let post_calls = calls_b
            .iter()
            .filter(|c| get(c, "method").as_str() == Some("POST"))
            .count();
        drop(calls_b);
        let mut realized = BigInt::from(0);
        let mut shares = 0.0f64;
        if let Some(m) = inventory.as_object() {
            for (_, x) in m {
                let v = get(get(x, "proof"), "cumulativeRealizedPnl");
                realized += big_amount(&num_string(v)).unwrap_or_else(|_| BigInt::from(0));
                shares += get(get(x, "proof"), "remainingOwnedQuantity")
                    .as_f64()
                    .unwrap_or(0.0);
            }
        }
        Ok(json!({
            "schemaVersion": 1,
            "mode": "offline_current_sealed_prefix_hypothetical_packet_cpu_and_queue",
            "observedAt": ms_to_iso(std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as f64),
            "scenario": if exit_only { "priority_owned_sell" } else { "one_share_hedge" },
            "sourceAnchor": get(&seed, "anchor"),
            "sourceCodeHash": payload_hash(&baal_combined_custody_v6_identity(&workspace)?)?,
            "originalRealizedPnl": money(&realized).parse::<f64>().unwrap_or(0.0),
            "actualCurrentSeedShares": shares,
            "action": get(&s, "lastAction"),
            "reason": get(&s, "lastFailure"),
            "dispatch": get(&s, "lastDispatchTiming"),
            "rejection": get(&s, "lastRejectedTiming"),
            "stages": get(&s, "stageTimings"),
            "riskTimings": get(&s, "recentLocalTimings"),
            "coordinatorTimings": get(&controller, "recentLocalTimings"),
            "lastGuardFailure": get(&controller, "lastGuardFailure"),
            "simulatedGetCalls": get_calls,
            "simulatedPostCalls": post_calls,
            "actualNetworkCalls": 0,
            "actualAccountEvidence": false,
            "actualExecutionAuthority": false,
            "actualFillOrProfitClaim": false,
            "originalSourceUnchanged":
                sha(&std::fs::read(&original_file).map_err(|e| e.to_string())?)
                    == get(get(&seed, "anchor"), "sha256").as_str().unwrap_or(""),
        }))
    })();
    // finally { host?.close(); api.close(); producer.close(); coreLease.release(); }
    if let Some(h) = host_cell.borrow().as_ref() {
        let _ = h.close();
    }
    api.close();
    producer.close();
    let _ = core_lease.borrow_mut().release();
    let output = run_result?;
    write_600(
        &private_dir.join("curated-offline-timing.json"),
        format!(
            "{}\n",
            serde_json::to_string_pretty(&output).map_err(|e| e.to_string())?
        )
        .as_bytes(),
    )?;
    println!("{}", serde_json::to_string(&output).map_err(|e| e.to_string())?);
    Ok(output)
}

trait PipeGet {
    fn pipe(&self, k: &str) -> &V;
}
impl PipeGet for V {
    fn pipe(&self, k: &str) -> &V {
        get(self, k)
    }
}
