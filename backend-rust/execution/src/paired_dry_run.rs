// execution/paired-dry-run.mjs — a draft risk envelope, NOT an activation. No
// client, key, POST/DELETE permit, timer or runtime path is accepted by this
// module. The old portfolio's 25 cash units remain a technical accounting
// baseline, never allocated pilot capital.
//
// `workspace_dir` replaces the module-directory auto-discovery: it must be the
// repository root (the directory containing execution/, live-paper/, paper/,
// feed/) so codeIdentity() hashes the same .mjs sources the JS writes.
use crate::canonical_order_journal::CanonicalOrderJournal;
use crate::core::{canonical_payload, payload_hash};
use crate::dual_no_review_v1::validate_dual_no_review;
use crate::no_order_economics::verify_no_buy_economics;
use crate::paired_portfolio::{
    apply_reconciled_paired_order, create_paired_portfolio, reserve_paired_order,
};
use baal_service_runtime::contract::instant_ns;
use baal_service_runtime::fixed::{amount, instant, money, price, SCALE, TICK};
use baal_service_runtime::journal::Journal;
use baal_service_runtime::json_text::{js_int, js_truthy, sha256};
use baal_service_runtime::ledger::acquire_owner_lease;
use serde_json::{json, Map, Value as V};
use std::cell::Cell;
use std::collections::BTreeSet;
use std::fs;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;

fn need(x: bool, c: &str) -> Result<(), String> {
    if x { Ok(()) } else { Err(format!("PAIRED_DRY_{c}")) }
}
fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}
fn hex(x: &V) -> bool {
    x.as_str().is_some_and(|s| {
        s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}
fn token(x: &V) -> bool {
    x.as_str().is_some_and(|s| {
        !s.is_empty() && s.len() <= 120
            && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b':' | b'-'))
            && !["__proto__", "prototype", "constructor"].contains(&s)
    })
}
fn keys(x: &V, allowed: &[&str]) -> bool {
    x.is_object() && x.as_object().unwrap().keys().all(|k| allowed.contains(&k.as_str()))
}
fn instant_v(v: &V) -> Result<i64, String> {
    instant(v.as_str().unwrap_or(""))
}
fn price_v(v: &V) -> Result<i128, String> {
    price(v.as_str().unwrap_or(""))
}
fn amount_v(v: &V) -> Result<i128, String> {
    amount(v.as_str().unwrap_or(""))
}
fn slug(s: &str) -> bool {
    !s.is_empty()
        && s.split('-').all(|seg| !seg.is_empty() && seg.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()))
}
fn sequence_str(s: &str) -> bool {
    !s.is_empty() && (s == "0" || !s.starts_with('0')) && s.bytes().all(|b| b.is_ascii_digit())
}
fn ledger_err(e: baal_service_runtime::ledger::LedgerError) -> String {
    format!("{}: {}", e.code, e.message)
}

// PAIRED_DRY_RUN_POLICY (frozen object).
pub fn paired_dry_run_policy() -> V {
    json!({
        "schemaVersion": 1,
        "quantity": 1,
        "maxPairs": 1,
        "allocatedCapital": "2.000000",
        "maxOrderCost": "1.000000",
        "maxCumulativeBuy": "2.000000",
        "maxFirstLegOrphanPrincipal": "0.500000",
        "minimumOrdinaryMargin": "0.010000",
        "fee": "0.000000",
        "lifetimeMs": 30000,
        "maxAccountAgeMs": 20000,
        "maxBookAgeMs": 20000,
        "maxBookSkewMs": 10000,
        "depthFractionBps": 100,
    })
}

// PAIRED_LIVE_CONTRACT_GAPS (frozen array).
pub fn paired_live_contract_gaps() -> V {
    json!([
        "No activation, shared-account lease binding or write authorization is supplied by this package.",
        "The local full-pair reserve must be installed in the sole collector global funding ledger before any future live adapter can dispatch.",
        "Tournament myBalance is documented rounded to two decimals; numeric lexemes do not prove exact gross cash.",
        "A separately verified exact gross-cash producer and complete economic-activity window are required.",
        "REST NO fills are documented side-relative; the existing canonical FIFO accepts NO only from YES-denominated realtime_account_batch. No relabeling is performed.",
        "Two legs are sequential and non-atomic. An unfilled second leg leaves the first owned share at risk.",
        "Ordinary-election minimum payout is conditional; administrative/void outcomes have no proven unconditional pair floor.",
    ])
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

fn relative_path(base: &Path, file: &Path) -> String {
    let base_n = normalize_path(base);
    let file_n = normalize_path(file);
    let base_c: Vec<_> = base_n.components().collect();
    let file_c: Vec<_> = file_n.components().collect();
    let mut i = 0;
    while i < base_c.len() && i < file_c.len() && base_c[i] == file_c[i] {
        i += 1;
    }
    let mut parts: Vec<String> = vec!["..".to_string(); base_c.len() - i];
    parts.extend(file_c[i..].iter().map(|c| c.as_os_str().to_string_lossy().to_string()));
    parts.join("/")
}

fn is_js_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

// codeIdentity() — sha256 over every .mjs reachable through relative imports,
// keyed by path relative to the execution/ directory.
fn code_identity(workspace_dir: &Path) -> Result<V, String> {
    let dir = normalize_path(&workspace_dir.join("execution"));
    let entry = dir.join("paired-dry-run.mjs");
    let mut files: Vec<(PathBuf, String)> = Vec::new();
    let mut seen: BTreeSet<PathBuf> = BTreeSet::new();
    let mut stack = vec![entry];
    while let Some(file) = stack.pop() {
        let file = normalize_path(&file);
        if seen.contains(&file) {
            continue;
        }
        seen.insert(file.clone());
        let raw = fs::read(&file).map_err(|e| e.to_string())?;
        let source = String::from_utf8_lossy(&raw).to_string();
        files.push((file.clone(), sha256(&raw)));
        // (?:from\s*|import\s*)['"](\.[^'"]+)['"] — scanned byte-wise; the JS
        // regex has no word boundary, so substrings inside identifiers match.
        let bytes = source.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            let kw = if bytes[i..].starts_with(b"from") {
                Some(4)
            } else if bytes[i..].starts_with(b"import") {
                Some(6)
            } else {
                None
            };
            if let Some(n) = kw {
                let mut j = i + n;
                while j < bytes.len() && is_js_space(bytes[j]) {
                    j += 1;
                }
                if j < bytes.len() && (bytes[j] == b'\'' || bytes[j] == b'"') {
                    let quote = bytes[j];
                    let mut k = j + 1;
                    while k < bytes.len() && bytes[k] != quote && bytes[k] != b'\'' && bytes[k] != b'"' {
                        k += 1;
                    }
                    if k < bytes.len() && bytes[k] == quote {
                        let rel = &source[j + 1..k];
                        if rel.starts_with('.') && rel.len() > 1 {
                            stack.push(file.parent().unwrap_or(Path::new("/")).join(rel));
                        }
                        i = k + 1;
                        continue;
                    }
                }
            }
            i += 1;
        }
    }
    let mut pairs: Vec<(String, String)> = files
        .iter()
        .map(|(f, h)| (relative_path(&dir, f).replace('\\', "/"), h.clone()))
        .collect();
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    let mut out = Map::new();
    for (k, h) in pairs {
        out.insert(k, json!(h));
    }
    Ok(V::Object(out))
}

struct Quote {
    raw: i128,
    limit: i128,
    depth: i64,
    start: i64,
    end: i64,
}

fn quote(book: &V, leg: &V, read_at: &V) -> Result<Quote, String> {
    let now = instant_v(read_at)?;
    need(
        get(book, "marketId") == get(leg, "marketId")
            && get(book, "exchangeId") == get(leg, "exchangeId")
            && get(book, "instrument").as_str()
                == Some(format!("{}:YES", get(leg, "exchangeId").as_str().unwrap_or("")).as_str())
            && get(book, "freshnessPolicy").as_str() == Some("authoritative_receipt_v1"),
        "BOOK_SCOPE",
    )?;
    need(
        keys(book, &["marketId", "exchangeId", "instrument", "receivedAt", "requestStartedAt", "sourceAt", "sequence", "versionKey", "bids", "asks", "freshnessPolicy", "sourceEventAgeMs"]),
        "BOOK_SCHEMA",
    )?;
    let start = instant_v(get(book, "requestStartedAt"))?;
    let end = instant_v(get(book, "receivedAt"))?;
    need(start <= end && end <= now && now - start <= 20_000, "BOOK_STALE")?;
    need(
        get(book, "sequence").as_str().is_some_and(sequence_str)
            && get(book, "sourceAt") == &V::String(
                baal_service_runtime::contract::timestamp(get(book, "sourceAt")).map_err(|e| e.0)?,
            )
            && get(book, "versionKey").as_str()
                == Some(format!("{}:{}", get(book, "sequence").as_str().unwrap_or(""), get(book, "sourceAt").as_str().unwrap_or("")).as_str())
            && instant_ns(get(book, "sourceAt").as_str().unwrap_or("")).map_err(|e| e.0)?
                <= (end as i128 + 2_000) * 1_000_000,
        "BOOK_VERSION",
    )?;
    for (name, descending) in [("bids", true), ("asks", false)] {
        let rows = get(book, name);
        need(
            rows.as_array().is_some_and(|r| !r.is_empty() && r.len() <= 20),
            "BOOK_DEPTH",
        )?;
        let mut last: Option<i128> = None;
        for l in rows.as_array().unwrap() {
            let p = price_v(get(l, "price"))?;
            need(
                keys(l, &["price", "quantity"])
                    && js_int(get(l, "quantity")).is_some_and(|q| q > 0)
                    && (last.is_none() || if descending { p < last.unwrap() } else { p > last.unwrap() }),
                "BOOK_LEVEL",
            )?;
            last = Some(p);
        }
    }
    need(
        price_v(get(&get(book, "bids").as_array().unwrap()[0], "price"))?
            < price_v(get(&get(book, "asks").as_array().unwrap()[0], "price"))?,
        "BOOK_CROSSED",
    )?;
    let level = get(book, "bids").as_array().unwrap().iter().find(|l| {
        js_int(get(l, "quantity")).is_some_and(|q| q / 100 >= 1)
    });
    need(level.is_some(), "ONE_PERCENT_DEPTH")?;
    let level = level.unwrap();
    let raw = SCALE - price_v(get(level, "price"))?;
    let limit = raw + TICK;
    need(limit <= SCALE - TICK, "NO_LIMIT")?;
    Ok(Quote {
        raw,
        limit,
        depth: js_int(get(level, "quantity")).unwrap() / 100,
        start,
        end,
    })
}

// planPairedDryRun({capture,scope,readAt})
pub fn plan_paired_dry_run(input: &V) -> Result<V, String> {
    let capture = get(input, "capture");
    let scope = get(input, "scope");
    let read_at = get(input, "readAt");
    let now = instant_v(read_at)?;
    need(
        get(capture, "phase").as_str() == Some("complete")
            && get(capture, "executionAuthority") == &json!(false)
            && get(capture, "orders") == &json!(0)
            && get(capture, "fills") == &json!(0)
            && token(get(capture, "name")),
        "CAPTURE_COMPLETE",
    )?;
    validate_dual_no_review(get(capture, "review"))?;
    let review = get(capture, "review");
    need(
        get(scope, "ownerId").as_str() == Some("Codex")
            && token(get(scope, "runId"))
            && get(scope, "tournamentId") == get(review, "tournamentId")
            && get(scope, "tournamentSlug").as_str().is_some_and(slug)
            && payload_hash(get(scope, "markets"))? == payload_hash(get(capture, "legs"))?,
        "SCOPE",
    )?;
    need(
        get(capture, "books").as_array().is_some_and(|b| b.len() == 2)
            && get(capture, "roots").as_array().is_some_and(|r| r.len() == 2)
            && payload_hash(get(capture, "roots"))? == get(review, "evidenceHash").as_str().unwrap_or(""),
        "CAPTURE_HASH",
    )?;
    need(
        now >= instant_v(get(review, "recordedAt"))? && now < instant_v(get(review, "expiresAt"))?,
        "REVIEW_EXPIRED",
    )?;
    let legs = get(capture, "legs").as_array().cloned().unwrap_or_default();
    let books = get(capture, "books").as_array().cloned().unwrap_or_default();
    let roots = get(capture, "roots").as_array().cloned().unwrap_or_default();
    let review_legs = get(review, "legs").as_array().cloned().unwrap_or_default();
    let mut quotes = Vec::new();
    for i in 0..legs.len() {
        let l = &legs[i];
        let root = &roots[i];
        let r = &review_legs[i];
        let b = &books[i];
        need(
            get(l, "marketId") == get(r, "marketId")
                && get(l, "exchangeId") == get(r, "exchangeId")
                && get(root, "marketId") == get(l, "marketId")
                && get(root, "tournamentId") == get(scope, "tournamentId")
                && get(root, "rootHash").as_str() == Some(payload_hash(get(root, "root"))?.as_str())
                && get(root, "rootHash") == get(r, "rulesHash")
                && payload_hash(get(root, "root"))? == payload_hash(get(r, "root"))?
                && hex(get(root, "responseHash"))
                && hex(get(b, "responseHash")),
            "ROOT_BINDING",
        )?;
        need(
            instant_v(get(root, "requestStartedAt"))? <= instant_v(get(root, "receivedAt"))?
                && instant_v(get(root, "receivedAt"))? <= instant_v(get(review, "recordedAt"))?
                && now - instant_v(get(root, "receivedAt"))? <= 300_000,
            "ROOT_CLOCK",
        )?;
        need(
            get(b, "marketId") == get(l, "marketId")
                && get(b, "requestStartedAt") == get(get(b, "book"), "requestStartedAt")
                && get(b, "receivedAt") == get(get(b, "book"), "receivedAt"),
            "BOOK_WRAPPER",
        )?;
        let q = quote(get(b, "book"), l, read_at)?;
        need(q.start >= instant_v(get(review, "recordedAt"))?, "RULES_BEFORE_BOOK")?;
        quotes.push(q);
    }
    need(
        quotes.iter().map(|q| q.end).max().unwrap_or(0) - quotes.iter().map(|q| q.start).min().unwrap_or(0) <= 10_000,
        "BOOK_SKEW",
    )?;
    let total: i128 = quotes.iter().map(|q| q.limit).sum();
    need(
        total <= SCALE - amount_v(get(&paired_dry_run_policy(), "minimumOrdinaryMargin"))?,
        "ORDINARY_MARGIN",
    )?;
    let mut order: Vec<usize> = vec![0, 1];
    // JS comparator: equal limits -> Number(exchangeIdA)-Number(exchangeIdB);
    // a NaN difference is treated as 0 by Array#sort.
    order.sort_by(|a, b| {
        let la = quotes[*a].limit;
        let lb = quotes[*b].limit;
        if la == lb {
            let na = get(&legs[*a], "exchangeId").as_str().unwrap_or("").parse::<f64>().unwrap_or(f64::NAN);
            let nb = get(&legs[*b], "exchangeId").as_str().unwrap_or("").parse::<f64>().unwrap_or(f64::NAN);
            if na.is_nan() || nb.is_nan() || na == nb {
                std::cmp::Ordering::Equal
            } else {
                na.partial_cmp(&nb).unwrap()
            }
        } else {
            la.cmp(&lb)
        }
    });
    need(
        quotes[order[0]].limit <= amount_v(get(&paired_dry_run_policy(), "maxFirstLegOrphanPrincipal"))?,
        "ORPHAN_BUDGET",
    )?;
    let plan_legs: Vec<V> = order
        .iter()
        .map(|&i| {
            let mut leg = legs[i].as_object().cloned().unwrap_or_default();
            leg.insert("originalLimit".into(), json!(money(quotes[i].limit)));
            leg.insert("originalRawNoAsk".into(), json!(money(quotes[i].raw)));
            leg.insert("eligibleDepth".into(), json!(quotes[i].depth));
            leg.insert("book".into(), get(&books[i], "book").clone());
            V::Object(leg)
        })
        .collect();
    let mut body = Map::from_iter([
        ("schemaVersion".into(), json!(1)),
        ("mode".into(), json!("paired_no_dry_run_v1")),
        ("name".into(), get(capture, "name").clone()),
        ("scope".into(), scope.clone()),
        ("policy".into(), paired_dry_run_policy()),
        ("createdAt".into(), read_at.clone()),
        ("reviewExpiresAt".into(), get(review, "expiresAt").clone()),
        ("captureHash".into(), json!(payload_hash(capture)?)),
        ("reviewHash".into(), json!(payload_hash(review)?)),
        ("legs".into(), V::Array(plan_legs)),
        ("pairCostCeiling".into(), json!(money(total))),
        ("conditionalOrdinaryMinimumPayout".into(), json!("1.000000")),
        ("conditionalOrdinarySurplus".into(), json!(money(SCALE - total))),
        ("unconditionalPayoutFloor".into(), V::Null),
        ("exceptionLoss".into(), json!(money(-total))),
        ("priceBasis".into(), json!("side_relative_no_limit_from_yes_bid_complement")),
        ("executionAuthority".into(), json!(false)),
    ]);
    let ph = payload_hash(&V::Object(body.clone()))?;
    body.insert("planHash".into(), json!(ph));
    Ok(V::Object(body))
}

fn plan_check(plan: &V) -> Result<(), String> {
    let body_opt = plan.as_object().cloned();
    let mut body = body_opt.unwrap_or_default();
    let plan_hash = body.shift_remove("planHash").unwrap_or(V::Null);
    let body_v = V::Object(body.clone());
    need(
        hex(&plan_hash)
            && plan_hash.as_str() == Some(payload_hash(&body_v)?.as_str())
            && body_v.get("mode").and_then(V::as_str) == Some("paired_no_dry_run_v1")
            && payload_hash(body_v.get("policy").unwrap_or(&V::Null))?
                == payload_hash(&paired_dry_run_policy())?
            && body_v.get("executionAuthority") == Some(&json!(false))
            && body_v.get("legs").and_then(V::as_array).is_some_and(|l| l.len() == 2),
        "PLAN_IDENTITY",
    )?;
    need(
        keys(&body_v, &["schemaVersion", "mode", "name", "scope", "policy", "createdAt", "reviewExpiresAt", "captureHash", "reviewHash", "legs", "pairCostCeiling", "conditionalOrdinaryMinimumPayout", "conditionalOrdinarySurplus", "unconditionalPayoutFloor", "exceptionLoss", "priceBasis", "executionAuthority"])
            && get(&body_v, "schemaVersion") == &json!(1)
            && token(get(&body_v, "name"))
            && hex(get(&body_v, "captureHash"))
            && hex(get(&body_v, "reviewHash"))
            && instant_v(get(&body_v, "reviewExpiresAt"))? > instant_v(get(&body_v, "createdAt"))?,
        "PLAN_SCHEMA",
    )?;
    let scope = get(&body_v, "scope");
    need(
        keys(scope, &["ownerId", "runId", "tournamentId", "tournamentSlug", "markets"])
            && get(scope, "ownerId").as_str() == Some("Codex")
            && token(get(scope, "runId"))
            && get(scope, "markets").as_array().is_some_and(|m| m.len() == 2)
            && get(scope, "markets").as_array().is_some_and(|ms| ms.iter().all(|m| keys(m, &["marketId", "exchangeId"])))
            && {
                let mut markets = get(scope, "markets").as_array().cloned().unwrap_or_default();
                markets.sort_by(|a, b| {
                    js_int(get(a, "marketId")).unwrap_or(0).cmp(&js_int(get(b, "marketId")).unwrap_or(0))
                });
                let mut leg_keys: Vec<V> = get(&body_v, "legs").as_array().cloned().unwrap_or_default()
                    .iter()
                    .map(|l| json!({"marketId": get(l, "marketId"), "exchangeId": get(l, "exchangeId")}))
                    .collect();
                leg_keys.sort_by(|a, b| {
                    js_int(get(a, "marketId")).unwrap_or(0).cmp(&js_int(get(b, "marketId")).unwrap_or(0))
                });
                payload_hash(&V::Array(markets))? == payload_hash(&V::Array(leg_keys))?
            },
        "PLAN_SCOPE",
    )?;
    let mut quotes = Vec::new();
    for l in get(&body_v, "legs").as_array().cloned().unwrap_or_default() {
        need(
            keys(&l, &["marketId", "exchangeId", "originalLimit", "originalRawNoAsk", "eligibleDepth", "book"]),
            "PLAN_LEG_SCHEMA",
        )?;
        let q = quote(get(&l, "book"), &l, get(&body_v, "createdAt"))?;
        need(
            money(q.limit) == get(&l, "originalLimit").as_str().unwrap_or("")
                && money(q.raw) == get(&l, "originalRawNoAsk").as_str().unwrap_or("")
                && q.depth == js_int(get(&l, "eligibleDepth")).unwrap_or(i64::MIN),
            "PLAN_LEG_PRICE",
        )?;
        quotes.push(q);
    }
    let cost = quotes[0].limit + quotes[1].limit;
    need(
        cost == amount_v(get(&body_v, "pairCostCeiling"))?
            && cost <= 990_000
            && quotes[0].limit <= quotes[1].limit
            && quotes[0].limit <= 500_000
            && get(&body_v, "conditionalOrdinaryMinimumPayout") == &json!("1.000000")
            && body_v.get("unconditionalPayoutFloor") == Some(&V::Null)
            && get(&body_v, "conditionalOrdinarySurplus").as_str() == Some(money(SCALE - cost).as_str())
            && get(&body_v, "exceptionLoss").as_str() == Some(money(-cost).as_str())
            && get(&body_v, "priceBasis").as_str() == Some("side_relative_no_limit_from_yes_bid_complement"),
        "PLAN_BUDGET",
    )?;
    Ok(())
}

fn account_check(a: &V, plan: &V, read_at: &V) -> Result<(), String> {
    let now = instant_v(read_at)?;
    need(
        get(a, "tournamentId") == get(get(plan, "scope"), "tournamentId")
            && get(a, "complete") == &json!(true)
            && get(a, "openOrders").as_array().is_some_and(|o| o.is_empty())
            && hex(get(a, "positionsSourceHash")),
        "ACCOUNT_COMPLETE",
    )?;
    need(
        keys(a, &["tournamentId", "complete", "positionsSourceHash", "positions", "openOrders", "startedAt", "completedAt", "cashPrecision", "cashReceipt"])
            && get(a, "positions").as_array().is_some_and(|ps| {
                ps.iter().all(|p| keys(p, &["marketId", "exchangeId", "quantity"]))
            }),
        "ACCOUNT_SCHEMA",
    )?;
    need(
        instant_v(get(a, "startedAt"))? <= instant_v(get(a, "completedAt"))?
            && instant_v(get(a, "completedAt"))? <= now
            && now - instant_v(get(a, "startedAt"))? <= 20_000,
        "ACCOUNT_STALE",
    )?;
    // This is an explicit upstream proof contract. A hash/boolean is not an
    // independent audit of its producer; dry-run output never grants authority.
    let p = get(a, "cashPrecision");
    need(
        keys(p, &["basis", "scale", "rounded", "sourceHash"])
            && get(p, "basis").as_str() == Some("gross_cash")
            && get(p, "scale") == &json!(6)
            && get(p, "rounded") == &json!(false)
            && hex(get(p, "sourceHash"))
            && get(p, "sourceHash") == get(get(a, "cashReceipt"), "sourceBodyHash"),
        "EXACT_GROSS_CASH_PRODUCER_REQUIRED",
    )?;
    let cr = get(a, "cashReceipt");
    need(
        keys(cr, &["kind", "tournamentId", "joined", "method", "endpoint", "sourceBodyHash", "receivedAt", "fields"])
            && keys(get(cr, "fields"), &["myBalance"])
            && keys(get(get(cr, "fields"), "myBalance"), &["available", "decimal"]),
        "CASH_SCHEMA",
    )?;
    need(
        amount_v(get(get(get(cr, "fields"), "myBalance"), "decimal"))?
            >= amount_v(get(plan, "pairCostCeiling"))?,
        "FULL_PAIR_FUNDING",
    )?;
    Ok(())
}

fn global_check(g: &V, plan: &V, read_at: &V, own_intent: Option<&V>) -> Result<(), String> {
    let now = instant_v(read_at)?;
    need(
        get(g, "tournamentId") == get(get(plan, "scope"), "tournamentId")
            && get(g, "ownerId") == get(get(plan, "scope"), "ownerId")
            && get(g, "runId") == get(get(plan, "scope"), "runId")
            && get(g, "complete") == &json!(true)
            && get(g, "soleCollector") == &json!(true)
            && get(g, "stopped") == &json!(false)
            && hex(get(g, "sourceHash"))
            && get(g, "unresolvedIntentIds").as_array().is_some_and(|u| {
                u.iter().all(token) && u.iter().all(|x| own_intent.map(|o| x == o).unwrap_or(x.is_null()))
            }),
        "GLOBAL_UNKNOWN_OR_OWNER",
    )?;
    need(
        keys(g, &["tournamentId", "ownerId", "runId", "complete", "soleCollector", "stopped", "sourceHash", "unresolvedIntentIds", "at"]),
        "GLOBAL_SCHEMA",
    )?;
    need(
        instant_v(get(g, "at"))? <= now && now - instant_v(get(g, "at"))? <= 20_000,
        "GLOBAL_STALE",
    )?;
    Ok(())
}

fn next_terms(s: &V) -> Result<(V, String, usize), String> {
    let plan = get(s, "plan");
    let portfolio = get(s, "portfolio");
    need(
        !js_truthy(portfolio.get("active"))
            && get(portfolio, "completed").as_array().is_some_and(|c| c.len() < 2),
        "ACTIVE_OR_COMPLETE",
    )?;
    let legs = get(plan, "legs").as_array().cloned().unwrap_or_default();
    if get(portfolio, "completed").as_array().unwrap().is_empty() {
        return Ok((
            legs[0].clone(),
            get(&legs[0], "originalLimit").as_str().unwrap_or("").to_string(),
            0,
        ));
    }
    let first = get(get(portfolio, "owned"), get(&legs[0], "exchangeId").as_str().unwrap_or(""));
    let first_qty: i64 = get(first, "lots").as_array().cloned().unwrap_or_default().iter()
        .map(|l| js_int(get(l, "quantity")).unwrap_or(0))
        .sum();
    need(first_qty == 1, "FIRST_LEG_UNFILLED")?;
    let paid: i128 = get(first, "lots").as_array().cloned().unwrap_or_default().iter()
        .map(|l| amount_v(get(l, "entryPrice")).unwrap_or(0) * js_int(get(l, "quantity")).unwrap_or(0) as i128)
        .sum();
    let remaining = amount_v(get(plan, "pairCostCeiling"))? - paid;
    need(remaining >= TICK && remaining <= SCALE - TICK, "SECOND_REMAINING_CAP")?;
    Ok((legs[1].clone(), money(remaining), 1))
}

fn fence(s: &V, input: &V, prepared: bool) -> Result<(V, String, usize), String> {
    need(
        get(input, "tournamentId") == get(get(get(s, "plan"), "scope"), "tournamentId"),
        "BOOK_TOURNAMENT_SCOPE",
    )?;
    account_check(get(input, "account"), get(s, "plan"), get(input, "readAt"))?;
    let active = get(get(s, "portfolio"), "active");
    let binding = get(get(active, "preparation"), "binding");
    global_check(
        get(input, "globalRisk"),
        get(s, "plan"),
        get(input, "readAt"),
        if prepared { Some(get(get(active, "preparation"), "intentId")) } else { None },
    )?;
    need(
        instant_v(get(input, "readAt"))? < instant_v(get(get(s, "plan"), "reviewExpiresAt"))?,
        "REVIEW_EXPIRED",
    )?;
    let book = get(input, "book");
    let terms = if prepared {
        (binding.clone(), get(binding, "limit").as_str().unwrap_or("").to_string(), usize::MAX)
    } else {
        next_terms(s)?
    };
    let q = quote(book, &terms.0, get(input, "readAt"))?;
    let prior = get(get(s, "lastBooks"), get(book, "exchangeId").as_str().unwrap_or(""));
    need(
        get(book, "sequence").as_str().unwrap_or("").parse::<i128>().unwrap_or(-1)
            >= get(prior, "sequence").as_str().unwrap_or("").parse::<i128>().unwrap_or(i128::MAX)
            && instant_ns(get(book, "sourceAt").as_str().unwrap_or("")).map_err(|e| e.0)?
                >= instant_ns(get(prior, "sourceAt").as_str().unwrap_or("")).map_err(|e| e.0)?
            && instant_v(get(book, "requestStartedAt"))? >= instant_v(get(prior, "requestStartedAt"))?,
        "BOOK_ROLLBACK",
    )?;
    if get(book, "versionKey") == get(prior, "versionKey") {
        need(
            payload_hash(&json!([get(book, "bids"), get(book, "asks")]))?
                == payload_hash(&json!([get(prior, "bids"), get(prior, "asks")]))?,
            "BOOK_VERSION_CONFLICT",
        )?;
    }
    need(
        instant_v(get(get(input, "account"), "completedAt"))?
            >= instant_v(get(s, "accountCompletedAt"))?,
        "ACCOUNT_CLOCK_ROLLBACK",
    )?;
    need(
        instant_v(get(get(input, "account"), "completedAt"))? <= q.start,
        "ACCOUNT_NOT_KNOWN_BEFORE_BOOK",
    )?;
    need(q.raw <= price_v(&json!(terms.1))?, "SECOND_LEG_PRICE_EXCEEDS_CAP")?;
    if prepared {
        need(
            instant_v(get(input, "readAt"))? < instant_v(get(binding, "expirationDate"))?,
            "ORDER_EXPIRED",
        )?;
        // Core reserve performs exact external/own position agreement and cash tests.
        let mut clear = get(s, "portfolio").clone();
        clear.as_object_mut().unwrap().insert("active".into(), V::Null);
        let mut spec = binding.as_object().cloned().unwrap_or_default();
        spec.insert("intentId".into(), get(get(active, "preparation"), "intentId").clone());
        spec.insert("idempotencyKey".into(), get(get(active, "preparation"), "idempotencyKey").clone());
        spec.insert("createdAt".into(), get(input, "readAt").clone());
        reserve_paired_order(&clear, &V::Object(spec), get(input, "account"), get(input, "readAt"))?;
    }
    Ok(terms)
}

fn economics_snapshot(a: &V) -> V {
    let positions: Vec<V> = get(a, "positions").as_array().cloned().unwrap_or_default()
        .iter()
        .filter(|p| js_truthy(p.get("quantity")))
        .map(|p| {
            json!({
                "marketId": get(p, "marketId"),
                "exchangeId": get(p, "exchangeId"),
                "signedQuantity": get(p, "quantity"),
            })
        })
        .collect();
    json!({
        "tournamentId": get(a, "tournamentId"),
        "complete": get(a, "complete"),
        "cashBasis": get(get(a, "cashPrecision"), "basis"),
        "cash": get(get(get(get(a, "cashReceipt"), "fields"), "myBalance"), "decimal"),
        "positions": positions,
        "openOrders": get(a, "openOrders"),
        "startedAt": get(a, "startedAt"),
        "completedAt": get(a, "completedAt"),
    })
}

fn reconcile(s: &V, event: &V) -> Result<V, String> {
    account_check(get(event, "after"), get(s, "plan"), get(event, "readAt"))?;
    need(
        keys(get(event, "activity"), &["tournamentId", "complete", "sourceHash", "from", "through", "events"])
            && get(get(event, "activity"), "events").as_array().is_some_and(|es| {
                es.iter().all(|e| keys(e, &["kind", "fillId", "orderId", "exchangeId", "marketId", "at"]))
            }),
        "ACTIVITY_SCHEMA",
    )?;
    let before = get(get(get(s, "portfolio"), "active"), "before");
    let order = get(get(event, "orderStatus"), "state");
    let mut economics = V::Null;
    if get(get(get(order, "evidence"), "ack"), "quantityTraded") == &json!(1) {
        let mut binding = get(order, "binding").as_object().cloned().unwrap_or_default();
        binding.insert("orderId".into(), get(order, "orderId").clone());
        binding.insert("sideRelativeLimit".into(), get(get(order, "binding"), "limit").clone());
        binding.insert("preparedAt".into(), get(get(order, "binding"), "createdAt").clone());
        let mut ack = get(get(order, "evidence"), "ack").as_object().cloned().unwrap_or_default();
        ack.insert("tournamentId".into(), get(get(get(s, "plan"), "scope"), "tournamentId").clone());
        let rows: Vec<V> = get(get(order, "evidence"), "rows").as_array().cloned().unwrap_or_default()
            .iter()
            .map(|f| {
                let mut fo = f.as_object().cloned().unwrap_or_default();
                fo.insert("orderId".into(), json!(crate::js_key(get(f, "orderId"))));
                V::Object(fo)
            })
            .collect();
        let mut activity = get(event, "activity").as_object().cloned().unwrap_or_default();
        activity.insert("rows".into(), get(get(event, "activity"), "events").clone());
        economics = verify_no_buy_economics(&json!({
            "binding": V::Object(binding),
            "ack": V::Object(ack),
            "fills": {
                "complete": true,
                "orderId": get(order, "orderId"),
                "exchangeId": get(get(order, "binding"), "exchangeId"),
                "tournamentId": get(get(get(s, "plan"), "scope"), "tournamentId"),
                "rows": rows,
            },
            "before": economics_snapshot(before),
            "after": economics_snapshot(get(event, "after")),
            "activity": V::Object(activity),
            "readAt": get(event, "readAt"),
        }))?;
    }
    let portfolio = apply_reconciled_paired_order(
        get(s, "portfolio"),
        get(event, "orderStatus"),
        get(event, "after"),
        get(event, "activity"),
        get(event, "readAt"),
    )?;
    need(
        amount_v(get(&portfolio, "cumulativeBuy"))? <= amount_v(get(get(s, "plan"), "pairCostCeiling"))?
            && amount_v(get(&portfolio, "cumulativeBuy"))?
                <= amount_v(get(&paired_dry_run_policy(), "maxCumulativeBuy"))?,
        "CUMULATIVE_BUDGET",
    )?;
    let mut next = s.clone();
    let obj = next.as_object_mut().unwrap();
    obj.insert("portfolio".into(), portfolio);
    obj.insert("accountCompletedAt".into(), get(get(event, "after"), "completedAt").clone());
    let rec = json!({
        "orderId": get(order, "orderId"),
        "economics": economics,
        "activityHash": payload_hash(get(event, "activity"))?,
    });
    obj.entry("reconciliations".to_string())
        .or_insert_with(|| V::Array(Vec::new()))
        .as_array_mut()
        .unwrap()
        .push(rec);
    Ok(next)
}

fn reduce(s: &V, e: &V) -> Result<V, String> {
    if get(e, "kind").as_str() == Some("initialize") {
        need(
            keys(e, &["kind", "plan", "account", "globalRisk", "readAt"]),
            "EVENT_SCHEMA",
        )?;
        need(s.is_null(), "ALREADY_INITIALIZED")?;
        plan_check(get(e, "plan"))?;
        account_check(get(e, "account"), get(e, "plan"), get(e, "readAt"))?;
        global_check(get(e, "globalRisk"), get(e, "plan"), get(e, "readAt"), None)?;
        let mut last_books = Map::new();
        for l in get(get(e, "plan"), "legs").as_array().cloned().unwrap_or_default() {
            last_books.insert(
                get(&l, "exchangeId").as_str().unwrap_or("").to_string(),
                get(&l, "book").clone(),
            );
        }
        return Ok(json!({
            "plan": get(e, "plan"),
                "portfolio": create_paired_portfolio(get(get(e, "plan"), "scope"), get(e, "account"), get(e, "readAt"))?,
            "lastBooks": V::Object(last_books),
            "accountCompletedAt": get(get(e, "account"), "completedAt"),
            "reconciliations": [],
            "executionAuthority": false,
        }));
    }
    need(js_truthy(Some(s)), "NOT_INITIALIZED")?;
    if get(e, "kind").as_str() == Some("prepare") {
        need(
            keys(e, &["kind", "tournamentId", "account", "globalRisk", "book", "readAt"]),
            "EVENT_SCHEMA",
        )?;
        let t = fence(s, e, false)?;
        let intent_id = format!("{}:leg:{}", get(get(s, "plan"), "name").as_str().unwrap_or(""), t.2);
        let plan_hash = get(get(s, "plan"), "planHash").as_str().unwrap_or("");
        let key = format!("dry:{}:{}", &plan_hash[..plan_hash.len().min(40)], t.2);
        need(
            price_v(&json!(t.1))? <= amount_v(get(&paired_dry_run_policy(), "maxOrderCost"))?,
            "ORDER_CAP",
        )?;
        let expiration = baal_service_runtime::contract::ms_to_iso(instant_v(get(e, "readAt"))? as f64 + 30_000.0);
        let portfolio = reserve_paired_order(
            get(s, "portfolio"),
            &json!({
                "intentId": intent_id,
                "idempotencyKey": key,
                "marketId": get(&t.0, "marketId"),
                "exchangeId": get(&t.0, "exchangeId"),
                "action": "buy",
                "quantity": 1,
                "limit": t.1,
                "createdAt": get(e, "readAt"),
                "expirationDate": expiration,
            }),
            get(e, "account"),
            get(e, "readAt"),
        )?;
        let mut next = s.clone();
        let obj = next.as_object_mut().unwrap();
        obj.insert("portfolio".into(), portfolio);
        obj.entry("lastBooks".to_string())
            .or_insert_with(|| V::Object(Map::new()))
            .as_object_mut()
            .unwrap()
            .insert(get(&t.0, "exchangeId").as_str().unwrap_or("").to_string(), get(e, "book").clone());
        obj.insert("accountCompletedAt".into(), get(get(e, "account"), "completedAt").clone());
        return Ok(next);
    }
    need(
        get(e, "kind").as_str() == Some("reconcile")
            && js_truthy(get(s, "portfolio").get("active"))
            && keys(e, &["kind", "orderStatus", "after", "activity", "readAt"]),
        "EVENT_OR_ACTIVE",
    )?;
    reconcile(s, e)
}

// openPairedDryRun({dir,plan,account,globalRisk,now,canCommit})
//
// Local journal ownership is deliberately separate from the live account
// lease. All operations below simulate/record a dry run. No shared dispatcher
// is called.
pub struct PairedDryRun {
    folder: PathBuf,
    workspace_dir: PathBuf,
    plan: V,
    lease: Rc<std::cell::RefCell<baal_service_runtime::ledger::OwnerLease>>,
    closed: Rc<Cell<bool>>,
    failed: Rc<Cell<bool>>,
    can_commit: Rc<dyn Fn() -> Result<bool, String>>,
    order: Option<CanonicalOrderJournal>,
    state: V,
    size: u64,
    raw_hash: String,
    file: PathBuf,
    file_ino: u64,
    file_birth: (i64, i64),
    code: V,
    journal: Journal,
}

impl PairedDryRun {
    fn operate(&self) -> Result<(), String> {
        need(
            !self.closed.get() && !self.failed.get() && (self.can_commit)()? == true,
            "STOPPED_OR_POISONED",
        )?;
        self.lease.borrow().assert_ownership().map_err(ledger_err)
    }

    fn birthtime(meta: &fs::Metadata) -> (i64, i64) {
        let t = meta
            .created()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .unwrap_or_default();
        (t.as_secs() as i64, t.subsec_nanos() as i64)
    }

    pub fn open(
        workspace_dir: &Path,
        folder: &Path,
        plan: &V,
        account: &V,
        global_risk: &V,
        now: Rc<dyn Fn() -> f64>,
        can_commit: Rc<dyn Fn() -> Result<bool, String>>,
    ) -> Result<PairedDryRun, String> {
        need(folder.is_absolute(), "DEPENDENCIES")?;
        plan_check(plan)?;
        let plan = plan.clone();
        let lease = Rc::new(std::cell::RefCell::new(
            acquire_owner_lease(folder, "Codex", get(get(&plan, "scope"), "runId").as_str().unwrap_or(""), "paired-dry-run", now())
                .map_err(ledger_err)?,
        ));
        let closed = Rc::new(Cell::new(false));
        let failed = Rc::new(Cell::new(false));
        let file = folder.join("paired-dry-run.jsonl");
        let code = code_identity(workspace_dir)?;
        let mut order: Option<CanonicalOrderJournal> = None;
        let result = (|| {
            if !file.exists() {
                fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(&file)
                    .map_err(|e| e.to_string())?;
            }
            let bytes = fs::read(&file).map_err(|e| e.to_string())?;
            need(bytes.is_empty() || bytes.last() == Some(&10), "PARTIAL_JOURNAL")?;
            let journal = Journal::open(&file)?;
            let mut state = V::Null;
            for line in String::from_utf8_lossy(&bytes).split('\n').filter(|s| !s.is_empty()) {
                let row: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
                need(
                    payload_hash(get(get(&row, "event"), "codeIdentity"))? == payload_hash(&code)?,
                    "CODE_CHANGED",
                )?;
                if get(get(get(&row, "event"), "input"), "kind").as_str() == Some("reconcile") {
                    let reference = get(get(get(&row, "event"), "input"), "orderStatus");
                    let completed_len = get(get(&state, "portfolio"), "completed")
                        .as_array().map(|c| c.len()).unwrap_or(0);
                    let leg_dir = folder.join(format!("leg-{completed_len}"));
                    let mut prior = CanonicalOrderJournal::open(
                        &leg_dir,
                        &workspace_dir.join("execution"),
                        true,
                        Box::new(|| Ok(true)),
                    )?;
                    let check = (|| {
                        let st = prior.status();
                        need(
                            get(&st, "hash") == get(reference, "hash")
                                && payload_hash(get(&st, "state"))? == payload_hash(get(reference, "state"))?,
                            "ORDER_JOURNAL_CHANGED",
                        )
                    })();
                    let _ = prior.close();
                    check?;
                }
                state = reduce(&state, get(get(&row, "event"), "input"))?;
                need(
                    payload_hash(&state)? == payload_hash(get(&row, "state"))?,
                    "REPLAY_STATE",
                )?;
            }
            let size = bytes.len() as u64;
            let raw_hash = sha256(&bytes);
            let meta = fs::metadata(&file).map_err(|e| e.to_string())?;
            if !state.is_null() {
                need(
                    get(get(&state, "plan"), "planHash") == get(&plan, "planHash"),
                    "PLAN_CHANGED",
                )?;
            }
            Ok((journal, state, size, raw_hash, meta.ino(), Self::birthtime(&meta)))
        })();
        if let Err(e) = result {
            let close_err = order.as_mut().map(|o| o.close());
            let release_err = lease.borrow_mut().release();
            if let Err(re) = release_err {
                return Err(ledger_err(re));
            }
            if let Some(Err(ce)) = close_err {
                return Err(ce);
            }
            return Err(e);
        }
        let (journal, state, size, raw_hash, file_ino, file_birth) = result.unwrap();
        let mut run = PairedDryRun {
            folder: folder.to_path_buf(),
            workspace_dir: workspace_dir.to_path_buf(),
            plan,
            lease,
            closed,
            failed,
            can_commit,
            order,
            state,
            size,
            raw_hash,
            file,
            file_ino,
            file_birth,
            code,
            journal,
        };
        if run.state.is_null() {
            let read_at = baal_service_runtime::contract::ms_to_iso(now());
            let plan_v = run.plan.clone();
            run.append(&json!({
                "kind": "initialize",
                "plan": plan_v,
                "account": account,
                "globalRisk": global_risk,
                "readAt": read_at,
            }))?;
        }
        run.ensure_order()?;
        Ok(run)
    }

    fn ensure_order(&mut self) -> Result<Option<&mut CanonicalOrderJournal>, String> {
        if !js_truthy(get(&self.state, "portfolio").get("active")) {
            return Ok(None);
        }
        if self.order.is_none() {
            let index = get(get(&self.state, "portfolio"), "completed")
                .as_array().map(|c| c.len()).unwrap_or(0);
            let leg_dir = self.folder.join(format!("leg-{index}"));
            let source_dir = self.workspace_dir.join("execution");
            let closed = self.closed.clone();
            let failed = self.failed.clone();
            let can_commit = self.can_commit.clone();
            let lease = self.lease.clone();
            let order_can_commit = Box::new(move || -> Result<bool, String> {
                if closed.get() || failed.get() {
                    return Err("PAIRED_DRY_STOPPED_OR_POISONED".into());
                }
                if !(can_commit)()? {
                    return Err("PAIRED_DRY_STOPPED_OR_POISONED".into());
                }
                lease.borrow().assert_ownership().map_err(ledger_err)?;
                Ok(true)
            });
            let mut journal = CanonicalOrderJournal::open(&leg_dir, &source_dir, false, order_can_commit)?;
            journal.prepare(get(get(get(&self.state, "portfolio"), "active"), "preparation"))?;
            self.order = Some(journal);
        }
        Ok(self.order.as_mut())
    }

    fn append(&mut self, input: &V) -> Result<(), String> {
        self.operate()?;
        let prior = fs::read(&self.file).map_err(|e| e.to_string())?;
        let stat = fs::metadata(&self.file).map_err(|e| e.to_string())?;
        need(
            stat.ino() == self.file_ino
                && Self::birthtime(&stat) == self.file_birth
                && prior.len() as u64 == self.size
                && sha256(&prior) == self.raw_hash
                && payload_hash(&code_identity(&self.workspace_dir)?)? == payload_hash(&self.code)?,
            "JOURNAL_CHANGED",
        )?;
        let next = reduce(&self.state, input)?;
        let est = canonical_payload(&json!({"input": input, "state": next.clone()}))?.len() as u64;
        need(self.size + est < 64 * 1024 * 1024, "JOURNAL_SIZE")?;
        self.operate()?;
        let write_result = (|| -> Result<(), String> {
            self.journal.commit(&next, &json!({"codeIdentity": self.code.clone(), "input": input}))?;
            self.operate()?;
            let after = fs::read(&self.file).map_err(|e| e.to_string())?;
            need(
                after.len() as u64 > self.size && after[..self.size as usize] == prior[..],
                "JOURNAL_CHANGED",
            )?;
            self.state = next;
            self.size = after.len() as u64;
            self.raw_hash = sha256(&after);
            Ok(())
        })();
        if let Err(e) = write_result {
            self.failed.set(true);
            return Err(e);
        }
        Ok(())
    }

    fn view(&self) -> Result<V, String> {
        let p = get(&self.state, "portfolio");
        let markets = get(get(&self.state, "plan"), "scope");
        let markets = get(markets, "markets").as_array().cloned().unwrap_or_default();
        let mut filled = Vec::new();
        for m in &markets {
            let ex = get(m, "exchangeId").as_str().unwrap_or("");
            let lots = get(get(get(&self.state, "portfolio"), "owned"), ex);
            let q: i64 = get(lots, "lots").as_array().cloned().unwrap_or_default().iter()
                .map(|l| js_int(get(l, "quantity")).unwrap_or(0))
                .sum();
            filled.push(q);
        }
        let cost = amount_v(get(p, "cumulativeBuy"))?;
        let active = self.order.as_ref().map(|o| o.status()).unwrap_or(V::Null);
        let completed_len = get(p, "completed").as_array().map(|c| c.len()).unwrap_or(0);
        let pair_reserved = if filled.iter().all(|q| *q == 1)
            || completed_len == 2
            || (completed_len == 1 && filled.iter().all(|q| *q == 0))
        {
            0
        } else {
            amount_v(get(get(&self.state, "plan"), "pairCostCeiling"))? - cost
        };
        let active_phase = get(get(&active, "state"), "phase").as_str().unwrap_or("");
        let phase = if active_phase == "uncertain" || active_phase == "dispatching" {
            "unknown_blocks_all"
        } else if js_truthy(p.get("active")) {
            "order_pending"
        } else if filled.iter().all(|q| *q == 1) {
            "paired_reconciled"
        } else if completed_len == 2 {
            "orphaned"
        } else if completed_len == 1 && filled.iter().all(|q| *q == 0) {
            "first_leg_unfilled"
        } else {
            "ready"
        };
        Ok(json!({
            "mode": "paired_no_dry_run_v1",
            "planHash": get(&self.plan, "planHash"),
            "sequence": self.journal.sequence,
            "hash": self.journal.previous.clone(),
            "closed": self.closed.get(),
            "failed": self.failed.get(),
            "state": self.state.clone(),
            "order": active,
            "allocatedCapital": "2.000000",
            "allocatedCash": money(2_000_000 - cost),
            "accountingBaselineCapital": "25.000000",
            "pairFundingReserved": money(pair_reserved),
            "phase": phase,
            "executionAuthority": false,
            "ordersSubmitted": 0,
            "additionalApiClients": 0,
            "liveContractGaps": paired_live_contract_gaps(),
        }))
    }

    pub fn authorizes(&self) -> bool {
        false
    }

    pub fn status(&self) -> Result<V, String> {
        self.view()
    }

    pub fn prepare_next(&mut self, input: &V) -> Result<V, String> {
        self.operate()?;
        let mut e = Map::new();
        e.insert("kind".into(), json!("prepare"));
        if let Some(obj) = input.as_object() {
            for (k, v) in obj {
                e.insert(k.clone(), v.clone());
            }
        }
        self.append(&V::Object(e))?;
        self.ensure_order()?;
        let st = self.order.as_ref().unwrap().status();
        let request_body = get(get(&st, "state"), "requestBody").clone();
        Ok(json!({
            "kind": "wouldDispatch",
            "method": "POST",
            "endpoint": "/orders",
            "body": request_body,
            "bodyJson": canonical_payload(&request_body)?,
            "payloadHash": get(get(&st, "state"), "requestHash"),
            "executionAuthority": false,
        }))
    }

    pub fn check_fence(&mut self, input: &V) -> Result<bool, String> {
        self.operate()?;
        let active = get(get(&self.state, "portfolio"), "active");
        let order_state = self.order.as_ref().map(|o| get(&o.status(), "state").clone()).unwrap_or(V::Null);
        need(
            js_truthy(Some(active)) && get(&order_state, "phase").as_str() == Some("prepared"),
            "NOT_PREPARED",
        )?;
        fence(&self.state, input, true)?;
        Ok(true)
    }

    pub fn simulate_dispatch(&mut self, input: &V) -> Result<V, String> {
        self.check_fence(input)?;
        self.order.as_mut().unwrap().mark_dispatch()?;
        self.view()
    }

    pub fn record_unknown(&mut self, reason: &V) -> Result<V, String> {
        self.operate()?;
        let order = self
            .ensure_order()?
            .ok_or("Cannot read properties of null (reading 'uncertain')")?;
        order.uncertain(reason)?;
        self.view()
    }

    pub fn bind(&mut self, order_id: &V) -> Result<V, String> {
        self.operate()?;
        let order = self
            .ensure_order()?
            .ok_or("Cannot read properties of null (reading 'bind')")?;
        order.bind(order_id)?;
        self.view()
    }

    pub fn observe(&mut self, input: &V) -> Result<V, String> {
        self.operate()?;
        let order = self
            .ensure_order()?
            .ok_or("Cannot read properties of null (reading 'observe')")?;
        order.observe(input)?;
        self.view()
    }

    pub fn reconcile(&mut self, input: &V) -> Result<V, String> {
        self.operate()?;
        let status = self
            .ensure_order()?
            .ok_or("Cannot read properties of null (reading 'status')")?
            .status();
        self.append(&json!({
            "kind": "reconcile",
            "orderStatus": status,
            "after": get(input, "after"),
            "activity": get(input, "activity"),
            "readAt": get(input, "readAt"),
        }))?;
        if let Some(o) = self.order.as_mut() {
            o.close()?;
        }
        self.order = None;
        self.view()
    }

    pub fn exact_recovery(&mut self) -> Result<V, String> {
        self.operate()?;
        let s = self
            .ensure_order()?
            .map(|o| get(&o.status(), "state").clone())
            .unwrap_or(V::Null);
        need(
            !s.is_null()
                && matches!(get(&s, "phase").as_str(), Some("uncertain" | "dispatching")),
            "NO_UNKNOWN",
        )?;
        Ok(json!({
            "kind": "reconciliation_only",
            "knownOrderId": get(&s, "orderId"),
            "originalBody": get(&s, "requestBody"),
            "originalPayloadHash": get(&s, "requestHash"),
            "idempotencyKey": get(&s, "idempotencyKey"),
            "replacementAllowed": false,
            "automaticRetryAllowed": false,
            "executionAuthority": false,
        }))
    }

    pub fn cancel_request(&mut self, read_at: &V) -> Result<V, String> {
        self.operate()?;
        let s = self
            .ensure_order()?
            .map(|o| get(&o.status(), "state").clone())
            .unwrap_or(V::Null);
        need(
            js_truthy(s.get("orderId"))
                && matches!(get(&s, "phase").as_str(), Some("bound" | "observed"))
                && instant_v(read_at)? >= instant_v(get(get(&s, "binding"), "expirationDate"))?,
            "CANCEL_NOT_KNOWN_EXPIRED",
        )?;
        Ok(json!({
            "kind": "wouldCancel",
            "method": "DELETE",
            "endpoint": format!("/orders/{}", get(&s, "orderId").as_str().unwrap_or("")),
            "intentId": get(&s, "intentId"),
            "requiresFreshScopedReconciliation": true,
            "closureInferred": false,
            "executionAuthority": false,
        }))
    }

    pub fn close(&mut self) -> Result<(), String> {
        if self.closed.get() {
            return Ok(());
        }
        self.closed.set(true);
        let close_err = self.order.as_mut().map(|o| o.close());
        let release_err = self.lease.borrow_mut().release();
        if let Err(e) = release_err {
            return Err(ledger_err(e));
        }
        if let Some(Err(e)) = close_err {
            return Err(e);
        }
        Ok(())
    }
}
