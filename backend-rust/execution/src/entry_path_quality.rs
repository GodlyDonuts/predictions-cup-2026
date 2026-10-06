// execution/entry-path-quality.mjs — causal description of observed bids around
// a verified entry book; never a probability of recovery.
use baal_service_runtime::contract::instant_ns;
use baal_service_runtime::fixed::{amount, instant, TICK};
use serde_json::{json, Map, Value as V};

pub const ENTRY_PATH_WINDOW_MS: f64 = 600_000.0;
fn ms(v: &V) -> Result<i64, String> {
    instant(v.as_str().unwrap_or("\u{0}"))
}
fn ns(v: &V) -> Result<i128, String> {
    instant_ns(v.as_str().unwrap_or("\u{0}")).map_err(|e| e.0)
}
// JS a.bids[0].price member access with TypeError-shaped errors.
fn top_price(book: &V, side: &str) -> Result<V, String> {
    match book.get(side) {
        Some(V::Array(a)) => match a.first() {
            Some(level) => Ok(level.get("price").cloned().unwrap_or(V::Null)),
            None => Err("Cannot read properties of undefined (reading 'price')".into()),
        },
        Some(V::String(s)) if !s.is_empty() => Ok(V::Null),
        _ => Err("Cannot read properties of undefined (reading '0')".into()),
    }
}
fn amount_at(book: &V, side: &str) -> Result<i128, String> {
    let p = top_price(book, side)?;
    amount(p.as_str().unwrap_or("\u{0}"))
}
// JS `a <= b` on two JSON values: string compare iff both strings, else numeric.
fn lte(a: &V, b: &V) -> bool {
    match (a, b) {
        (V::String(x), V::String(y)) => x <= y,
        _ => {
            let (x, y) = (a.as_f64().unwrap_or(f64::NAN), b.as_f64().unwrap_or(f64::NAN));
            x <= y
        }
    }
}
/// entryPathQuality({books, invalidations, entry, at, windowMs})
pub fn entry_path_quality(input: &V) -> Result<V, String> {
    let books = input.get("books").cloned().unwrap_or(V::Null);
    let invalidations = input.get("invalidations").cloned().unwrap_or(V::Null);
    let entry = input.get("entry").cloned().unwrap_or(V::Null);
    if !entry.is_object() {
        return Err(format!("Cannot read properties of {} (reading 'marketId')", if entry.is_null() { "null" } else { "undefined" }));
    }
    let window_ms = input.get("windowMs").and_then(V::as_f64).unwrap_or(ENTRY_PATH_WINDOW_MS);
    let now = ms(input.get("at").unwrap_or(&V::Null))? as f64;
    let mut stop = now - window_ms;
    for x in invalidations.as_array().cloned().unwrap_or_default() {
        // JS: x.marketId===null (strict — absent key is undefined, not null)
        if x.get("marketId") == Some(&V::Null) || x.get("marketId") == entry.get("marketId") {
            let t = ms(x.get("at").unwrap_or(&V::Null))? as f64;
            if t <= now && t > stop {
                stop = t;
            }
        }
    }
    let mut candidates: Vec<V> = Vec::new();
    for b in books.as_array().cloned().unwrap_or_default() {
        if b.get("marketId") != entry.get("marketId") || b.get("exchangeId") != entry.get("exchangeId") {
            continue;
        }
        let recv = ms(b.get("receivedAt").unwrap_or(&V::Null))? as f64;
        if !(recv <= now && recv > stop) {
            continue;
        }
        if ms(b.get("requestStartedAt").unwrap_or(&V::Null))? as f64 > recv {
            continue;
        }
        candidates.push(b);
    }
    candidates.sort_by(|a, b| {
        let (ta, tb) = (
            ms(a.get("receivedAt").unwrap_or(&V::Null)).unwrap_or(0),
            ms(b.get("receivedAt").unwrap_or(&V::Null)).unwrap_or(0),
        );
        ta.cmp(&tb)
    });
    let mut path: Vec<V> = Vec::new();
    for b in candidates {
        if let Some(prior) = path.last() {
            let rollback = lte(b.get("sequence").unwrap_or(&V::Null), prior.get("sequence").unwrap_or(&V::Null))
                || ns(b.get("sourceAt").unwrap_or(&V::Null))? <= ns(prior.get("sourceAt").unwrap_or(&V::Null))?
                || ms(b.get("receivedAt").unwrap_or(&V::Null))? <= ms(prior.get("receivedAt").unwrap_or(&V::Null))?;
            if rollback {
                continue;
            }
        }
        amount_at(&b, "bids")?;
        amount_at(&b, "asks")?;
        path.push(b);
    }
    let current = path
        .iter()
        .find(|b| {
            b.get("versionKey") == entry.get("bookVersion") && b.get("receivedAt") == entry.get("bookReceivedAt")
        })
        .cloned();
    let Some(current) = current else {
        let mut out = Map::new();
        out.insert("available".into(), json!(false));
        out.insert("reason".into(), json!("exact_entry_book_not_in_verified_window"));
        out.insert("observations".into(), json!(path.len()));
        out.insert("profitabilityEstablished".into(), json!(false));
        return Ok(V::Object(out));
    };
    let current_ms = ms(current.get("receivedAt").unwrap_or(&V::Null))?;
    let a: Vec<V> = path
        .iter()
        .filter(|b| ms(b.get("receivedAt").unwrap_or(&V::Null)).map(|t| t <= current_ms).unwrap_or(false))
        .cloned()
        .collect();
    let last = a.last().unwrap();
    let previous = a.get(a.len().wrapping_sub(2)).filter(|_| a.len() >= 2);
    let bid = amount_at(last, "bids")?;
    let ask = amount_at(last, "asks")?;
    let max = a.iter().try_fold(bid, |p, b| Ok::<i128, String>(std::cmp::max(p, amount_at(b, "bids")?)))?;
    let gaps: Vec<i64> = a
        .iter()
        .skip(1)
        .enumerate()
        .map(|(i, b)| ms(b.get("receivedAt").unwrap_or(&V::Null)).unwrap_or(0) - ms(a[i].get("receivedAt").unwrap_or(&V::Null)).unwrap_or(0))
        .collect();
    let span = ms(last.get("receivedAt").unwrap_or(&V::Null))? - ms(a[0].get("receivedAt").unwrap_or(&V::Null))?;
    let first = &a[0];
    let mut out = Map::new();
    out.insert("available".into(), json!(true));
    out.insert("reason".into(), json!("discrete_verified_pre_entry_books"));
    out.insert("observations".into(), json!(a.len()));
    out.insert("spanMs".into(), json!(span));
    out.insert("maximumObservedGapMs".into(), if gaps.is_empty() { V::Null } else { json!(gaps.iter().max().unwrap()) });
    out.insert(
        "latestIntervalMs".into(),
        match previous {
            Some(p) => json!(ms(last.get("receivedAt").unwrap_or(&V::Null))? - ms(p.get("receivedAt").unwrap_or(&V::Null))?),
            None => V::Null,
        },
    );
    out.insert(
        "latestBidFalling".into(),
        match previous {
            Some(p) => json!(bid < amount_at(p, "bids")?),
            None => V::Null,
        },
    );
    out.insert(
        "latestBidChangeTicks".into(),
        match previous {
            Some(p) => json!(((bid - amount_at(p, "bids")?) / TICK) as i64),
            None => V::Null,
        },
    );
    out.insert("drawdownFromObservedPeakTicks".into(), json!(((max - bid) / TICK) as i64));
    out.insert("spreadTicks".into(), json!(((ask - bid) / TICK) as i64));
    out.insert("firstBid".into(), top_price(first, "bids")?);
    out.insert("latestBid".into(), top_price(last, "bids")?);
    out.insert("latestAsk".into(), top_price(last, "asks")?);
    out.insert("firstReceiptAt".into(), first.get("receivedAt").cloned().unwrap_or(V::Null));
    out.insert("lastReceiptAt".into(), last.get("receivedAt").cloned().unwrap_or(V::Null));
    out.insert("quoteAgeAtPreparationMs".into(), json!((now - ms(last.get("receivedAt").unwrap_or(&V::Null))? as f64) as i64));
    out.insert("contextInterrupted".into(), json!(stop > now - window_ms));
    out.insert("wellSampledContext".into(), json!(a.len() >= 4 && gaps.iter().all(|x| *x >= 1000 && *x <= 180000)));
    let path_out: Vec<V> = a
        .iter()
        .map(|b| {
            let mut o = Map::new();
            o.insert("receivedAt".into(), b.get("receivedAt").cloned().unwrap_or(V::Null));
            o.insert("requestStartedAt".into(), b.get("requestStartedAt").cloned().unwrap_or(V::Null));
            o.insert("versionKey".into(), b.get("versionKey").cloned().unwrap_or(V::Null));
            o.insert("bid".into(), top_price(b, "bids").unwrap_or(V::Null));
            o.insert("ask".into(), top_price(b, "asks").unwrap_or(V::Null));
            V::Object(o)
        })
        .collect();
    out.insert("path".into(), V::Array(path_out));
    out.insert("profitabilityEstablished".into(), json!(false));
    Ok(V::Object(out))
}
