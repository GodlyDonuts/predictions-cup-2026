// Shared JavaScript-semantics helpers for the research ports: Number/String
// coercions, truthiness, ISO clock, and assertion plumbing that mirror the .mjs
// sources. Private to the crate (not a research module).
use serde_json::Value as V;

pub fn require_that(cond: bool, message: impl Into<String>) -> Result<(), String> {
    if cond {
        Ok(())
    } else {
        Err(message.into())
    }
}
pub fn fail<T>(message: impl Into<String>) -> Result<T, String> {
    Err(message.into())
}

// JS Number(v) for every JSON-representable input plus absent (undefined).
pub fn js_number(v: Option<&V>) -> f64 {
    match v {
        None => f64::NAN,
        Some(V::Null) => 0.0,
        Some(V::Bool(b)) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        Some(V::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        Some(V::String(s)) => js_number_str(s),
        Some(V::Array(a)) => {
            if a.is_empty() {
                0.0
            } else if a.len() == 1 {
                js_number(a.first())
            } else {
                f64::NAN
            }
        }
        Some(V::Object(_)) => f64::NAN,
    }
}

// JS Number(string): trims whitespace, "" -> 0, 0x/0o/0b radix forms, Infinity.
pub fn js_number_str(s: &str) -> f64 {
    let t = s.trim();
    if t.is_empty() {
        return 0.0;
    }
    let (sign, body) = match t.strip_prefix('-') {
        Some(b) => (-1.0, b),
        None => match t.strip_prefix('+') {
            Some(b) => (1.0, b),
            None => (1.0, t),
        },
    };
    if body == "Infinity" {
        return sign * f64::INFINITY;
    }
    for (prefix, radix) in [("0x", 16), ("0X", 16), ("0o", 8), ("0O", 8), ("0b", 2), ("0B", 2)] {
        if let Some(h) = body.strip_prefix(prefix) {
            if h.is_empty() {
                return f64::NAN;
            }
            return match u128::from_str_radix(h, radix) {
                Ok(x) => sign * x as f64,
                Err(_) => f64::NAN,
            };
        }
    }
    // Rust's f64 parser is more permissive than JS ("inf", "infinity", "NaN",
    // underscores are not accepted). Restrict to a JS decimal grammar.
    let ok = {
        let b = body.as_bytes();
        let mut i = 0;
        let mut digits = false;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
            digits = true;
        }
        if i < b.len() && b[i] == b'.' {
            i += 1;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
                digits = true;
            }
        }
        if !digits {
            return f64::NAN;
        }
        if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
            i += 1;
            if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
                i += 1;
            }
            let start = i;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
            if i == start {
                return f64::NAN;
            }
        }
        i == b.len()
    };
    if !ok {
        return f64::NAN;
    }
    t.parse::<f64>().unwrap_or(f64::NAN)
}

// JS String(v) coercion.
pub fn js_string(v: &V) -> String {
    match v {
        V::Null => "null".into(),
        V::Bool(b) => b.to_string(),
        V::Number(n) => {
            if let Some(x) = n.as_f64() {
                let mut b = ryu_js::Buffer::new();
                b.format_finite(x).to_string()
            } else {
                n.to_string()
            }
        }
        V::String(s) => s.clone(),
        V::Array(a) => a.iter().map(js_string).collect::<Vec<_>>().join(","),
        V::Object(_) => "[object Object]".into(),
    }
}

// JS `${v}` template interpolation: absent (undefined) -> "undefined".
pub fn js_template(v: Option<&V>) -> String {
    match v {
        None => "undefined".into(),
        Some(v) => js_string(v),
    }
}

// JS truthiness of an optional JSON value.
pub fn js_truthy(v: Option<&V>) -> bool {
    baal_service_runtime::json_text::js_truthy(v)
}

// Date.parse(s): Option milliseconds. Accepts the same ISO strings the callers
// pass; also plain "YYYY-MM-DD" which JS parses as UTC midnight.
pub fn date_parse(s: &str) -> f64 {
    if let Some(ms) = baal_service_runtime::contract::parse_iso_ms(s) {
        return ms;
    }
    chrono::DateTime::parse_from_rfc3339(&format!("{s}T00:00:00.000Z"))
        .map(|d| d.timestamp_millis() as f64)
        .unwrap_or(f64::NAN)
}

// new Date(ms).toISOString()
pub fn iso(ms: f64) -> String {
    baal_service_runtime::contract::ms_to_iso(ms)
}

// Date.now()
pub fn now_ms() -> f64 {
    chrono::Utc::now().timestamp_millis() as f64
}

// new Date().toISOString()
pub fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

// Number.isFinite on a JSON value.
pub fn finite(v: Option<&V>) -> bool {
    matches!(v, Some(V::Number(n)) if n.as_f64().is_some_and(|x| x.is_finite()))
}

// Number.isSafeInteger on a JSON value.
pub fn safe_int(v: Option<&V>) -> Option<i64> {
    baal_service_runtime::json_text::js_int(v?)
}

// Store a computed f64 in a JSON value exactly as JS JSON.stringify would:
// integral doubles become integer numbers ("1976" not "1976.0"); non-finite
// values would have serialized to null in JS anyway.
pub fn num(x: f64) -> V {
    if x.is_finite() && x.fract() == 0.0 && x.abs() <= 9.0e15 {
        V::from(x as i64)
    } else if x.is_finite() {
        V::from(x)
    } else {
        V::Null
    }
}

// JS `v || ''` then String(...): falsy/absent -> "", otherwise String(v).
pub fn or_empty(v: Option<&V>) -> String {
    match v {
        Some(v) if js_truthy(Some(v)) => js_string(v),
        _ => String::new(),
    }
}

// JS String(number) / `${number}` — "NaN", "Infinity", "-Infinity", else the
// ECMAScript shortest round-trip form (ryu-js).
pub fn num_str(x: f64) -> String {
    if x.is_nan() {
        "NaN".into()
    } else if x == f64::INFINITY {
        "Infinity".into()
    } else if x == f64::NEG_INFINITY {
        "-Infinity".into()
    } else {
        let mut b = ryu_js::Buffer::new();
        b.format_finite(x).to_string()
    }
}

// A string key preserving JS Map-key (SameValueZero) equality for primitive
// values: number 1 and string "1" are different Map keys. Compound values fall
// back to their String coercion (identity-keyed in JS; closest faithful
// encoding available).
pub fn map_key(v: Option<&V>) -> String {
    match v {
        None => "u:".into(),
        Some(V::Null) => "l:".into(),
        Some(V::Bool(b)) => format!("b:{b}"),
        Some(V::Number(n)) => format!("n:{}", num_str(n.as_f64().unwrap_or(f64::NAN))),
        Some(V::String(s)) => format!("s:{s}"),
        Some(v) => format!("o:{}", js_string(v)),
    }
}

// JS Math.min/Math.max propagate NaN (Rust f64::min/max discard it).
pub fn js_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.min(b)
    }
}
pub fn js_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.max(b)
    }
}

// JS `v === x` where x is a number: requires v to actually be a number
// (no coercion — "2020" !== 2020), then value compare (NaN !== NaN).
pub fn js_strict_eq_num(v: Option<&V>, x: f64) -> bool {
    matches!(v, Some(V::Number(n)) if n.as_f64() == Some(x))
}

// Number.isSafeInteger(n)
pub fn safe_integer(n: f64) -> bool {
    n.is_finite() && n.fract() == 0.0 && n.abs() <= 9_007_199_254_740_991.0
}

// v.field as f64 (undefined -> NaN).
pub fn field_f64(v: &V, key: &str) -> f64 {
    js_number(v.get(key))
}

// v.field as &str (undefined -> "").
pub fn field_str<'a>(v: &'a V, key: &str) -> &'a str {
    v.get(key).and_then(V::as_str).unwrap_or("")
}
