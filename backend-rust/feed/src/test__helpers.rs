// feed/test/helpers.mjs — synthetic market-data fixtures shared by the feed
// test modules. Byte/semantic port: every builder returns a fresh JSON value
// with the same key insertion order (serde_json preserve_order) and number
// spellings as the JS object literals.
use crate::ms_to_iso;
use baal_service_runtime::json_text::js_stringify;
use serde_json::{json, Map, Value as V};

/// scope — {tournamentId:'11111111-1111-4111-8111-111111111111',marketId:'277',
/// exchangeIds:['701','702']}. Fresh object per call; JS exports one shared
/// const but every call site only reads it or spreads it ({...scope}).
pub fn scope() -> V {
    json!({
        "tournamentId": "11111111-1111-4111-8111-111111111111",
        "marketId": "277",
        "exchangeIds": ["701", "702"],
    })
}

/// start — Date.parse('2026-10-01T21:00:00.000Z').
pub fn start() -> f64 { 1_790_888_400_000.0 }

/// at(ms) — new Date(start + ms).toISOString(). ms_to_iso truncates toward
/// zero like JS TimeClip, so fractional and negative offsets land identically.
pub fn at(ms: impl Into<f64>) -> String { ms_to_iso(start() + ms.into()) }

/// `{...src}` — own enumerable string-keyed properties of a JSON value spread
/// into `m`: objects merge in insertion order (an existing key keeps its
/// position and takes the new value, new keys append — the same semantics
/// preserve_order's Map::insert gives), arrays and strings spread numeric
/// index keys, and everything else (null included) spreads nothing.
fn spread(m: &mut Map<String, V>, src: Option<&V>) {
    match src {
        Some(V::Object(o)) => o.iter().for_each(|(k, v)| {
            m.insert(k.clone(), v.clone());
        }),
        Some(V::Array(a)) => a.iter().enumerate().for_each(|(i, v)| {
            m.insert(i.to_string(), v.clone());
        }),
        Some(V::String(s)) => s.chars().enumerate().for_each(|(i, c)| {
            m.insert(i.to_string(), json!(c.to_string()));
        }),
        _ => {}
    }
}

/// book(id = 701, sequence = 100, ms = 0, extra = {}).
/// Each Option parameter is JS undefined-vs-explicit: None applies the default,
/// Some(v) is used verbatim (Some(V::Null) is an explicit JS null, which JS
/// defaults do not replace).
pub fn book(id: Option<V>, sequence: Option<V>, ms: Option<f64>, extra: Option<&V>) -> V {
    let id = id.unwrap_or_else(|| json!(701));
    let sequence = sequence.unwrap_or_else(|| json!(100));
    let ms = ms.unwrap_or(0.0);
    let mut m = Map::new();
    m.insert("exchangeId".into(), id);
    m.insert("asOf".into(), json!({ "sequence": sequence, "at": at(ms) }));
    m.insert("nextExpiryAt".into(), V::Null);
    m.insert("bids".into(), json!([{ "price": 0.4, "quantity": 100 }]));
    m.insert("asks".into(), json!([{ "price": 0.6, "quantity": 200 }]));
    spread(&mut m, extra);
    V::Object(m)
}

/// trade(id = '101', sequence = 101, ms = 1, extra = {}). id is a string,
/// exchangeId/marketId are numbers, and tournamentId is scope.tournamentId —
/// all exactly as the JS literal writes them.
pub fn trade(id: Option<V>, sequence: Option<V>, ms: Option<f64>, extra: Option<&V>) -> V {
    let id = id.unwrap_or_else(|| json!("101"));
    let sequence = sequence.unwrap_or_else(|| json!(101));
    let ms = ms.unwrap_or(1.0);
    let tournament_id = scope()["tournamentId"].clone();
    let mut m = Map::new();
    m.insert("id".into(), id);
    m.insert("sequence".into(), sequence);
    m.insert("exchangeId".into(), json!(701));
    m.insert("marketId".into(), json!(277));
    m.insert("tournamentId".into(), tournament_id);
    m.insert("price".into(), json!(0.5));
    m.insert("quantity".into(), json!(10));
    m.insert("executedAt".into(), json!(at(ms)));
    spread(&mut m, extra);
    V::Object(m)
}

/// [book(701), book(702)] — the shared books default for batch and snapshot,
/// evaluated per call like the JS parameter initializer.
fn default_books() -> Vec<V> {
    vec![
        book(Some(json!(701)), None, None, None),
        book(Some(json!(702)), None, None, None),
    ]
}

/// batch(revision = 1, previousRevision = 0,
///       books = [book(701),book(702)], extra = {}).
/// books: None is the JS undefined default; Some(vec![]) is an explicit [].
pub fn batch(revision: Option<V>, previous_revision: Option<V>, books: Option<Vec<V>>, extra: Option<&V>) -> V {
    let revision = revision.unwrap_or_else(|| json!(1));
    let previous_revision = previous_revision.unwrap_or_else(|| json!(0));
    let books = books.unwrap_or_else(default_books);
    let mut m = Map::new();
    m.insert("trades".into(), json!([]));
    m.insert("bookDirty".into(), json!([]));
    m.insert("marketSettled".into(), json!([]));
    m.insert("books".into(), V::Array(books));
    m.insert("delivery".into(), json!({
        "model": "best-effort-authoritative-resync",
        "correlationId": "synthetic-batch",
        "revision": revision,
        "previousRevision": previous_revision,
        "sourceSequenceFrom": 100,
        "sourceSequenceThrough": 101,
    }));
    spread(&mut m, extra);
    V::Object(m)
}

/// String(v) — JS ToString over a JSON value: strings pass through, numbers
/// take JS's shortest spelling (via js_stringify), null -> "null", absent ->
/// "undefined", bool -> "true"/"false", arrays join like String([a,b]) with
/// null/undefined elements empty, objects -> "[object Object]".
fn js_string(v: Option<&V>) -> String {
    match v {
        None => "undefined".into(),
        Some(V::Null) => "null".into(),
        Some(V::Bool(b)) => b.to_string(),
        Some(n @ V::Number(_)) => js_stringify(n),
        Some(V::String(s)) => s.clone(),
        Some(V::Array(a)) => a
            .iter()
            .map(|x| if x.is_null() { String::new() } else { js_string(Some(x)) })
            .collect::<Vec<_>>()
            .join(","),
        Some(V::Object(_)) => "[object Object]".into(),
    }
}

/// snapshot(books = [book(701),book(702)]) — every book spreads unchanged,
/// then exchangeId is overwritten in place (keeping its key position) with
/// String(b.exchangeId), and option/impliedProbability append after it.
pub fn snapshot(books: Option<Vec<V>>) -> V {
    let books = books.unwrap_or_else(default_books);
    let exchanges: Vec<V> = books
        .iter()
        .map(|b| {
            let mut m = Map::new();
            spread(&mut m, Some(b));
            m.insert("exchangeId".into(), json!(js_string(b.get("exchangeId"))));
            m.insert("option".into(), json!("YES"));
            m.insert("impliedProbability".into(), json!(0.6));
            V::Object(m)
        })
        .collect();
    json!({
        "exchanges": [],
        "contexts": [{
            "type": "tournament",
            "tournament": { "id": scope()["tournamentId"].clone() },
            "orderbook": {
                "exchanges": exchanges,
                "overround": 1.2,
                "hasArbitrageOpportunity": false,
            },
        }],
    })
}

/// clone(v) — structuredClone(v). A serde JSON value is already plain data, so
/// a deep clone is identical; the JS version would additionally drop
/// functions/symbols/undefined that a JSON value cannot represent anyway.
pub fn clone(v: &V) -> V { v.clone() }
