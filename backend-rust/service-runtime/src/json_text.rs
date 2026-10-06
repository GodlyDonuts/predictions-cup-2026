use serde_json::Value as V;
use sha2::{Digest, Sha256};

// Exact bytes of JS JSON.stringify for parsed values: integer-index object keys
// are enumerated numerically first, other keys keep insertion order, and every
// number round-trips through the shortest f64 spelling the JS engine emits.
fn index_key(k: &str) -> Option<u32> {
    let v = k.parse::<u32>().ok()?;
    if v < u32::MAX && v.to_string() == k { Some(v) } else { None }
}
fn ordered_entries(v: &V) -> Vec<(String, V)> {
    let mut a: Vec<_> = v
        .as_object()
        .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();
    a.sort_by(|(a, _), (b, _)| match (index_key(a), index_key(b)) {
        (Some(a), Some(b)) => a.cmp(&b),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        _ => std::cmp::Ordering::Equal,
    });
    a
}
fn write_js(v: &V, out: &mut String) {
    match v {
        V::Object(_) => {
            out.push('{');
            for (i, (k, v)) in ordered_entries(v).iter().enumerate() {
                if i > 0 {
                    out.push(',')
                }
                out.push_str(&serde_json::to_string(k).unwrap());
                out.push(':');
                write_js(v, out)
            }
            out.push('}')
        }
        V::Array(a) => {
            out.push('[');
            for (i, v) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',')
                }
                write_js(v, out)
            }
            out.push(']')
        }
        V::Number(n) => {
            let mut b = ryu_js::Buffer::new();
            out.push_str(b.format_finite(n.as_f64().unwrap()));
        }
        _ => out.push_str(&serde_json::to_string(v).unwrap()),
    }
}
pub fn js_stringify(v: &V) -> String {
    let mut out = String::new();
    write_js(v, &mut out);
    out
}
pub fn digest(v: &V) -> String {
    format!("{:x}", Sha256::digest(js_stringify(v).as_bytes()))
}

// execution/ledger.mjs canonicalJson: every object key is compared as a UTF-16
// code-unit string (Object.keys().sort()), with no integer-index promotion.
fn sorted_entries(v: &V) -> Vec<(String, V)> {
    let mut a: Vec<_> = v
        .as_object()
        .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();
    a.sort_by(|(a, _), (b, _)| a.encode_utf16().cmp(b.encode_utf16()));
    a
}
fn finite(v: &V) -> Result<(), String> {
    match v {
        V::Number(n) if !n.is_f64() || n.as_f64().is_some_and(|x| x.is_finite()) => Ok(()),
        V::Number(_) => Err("INVALID_INPUT: Canonical JSON requires finite JSON values".into()),
        V::Array(a) => a.iter().try_for_each(finite),
        V::Object(m) => m.values().try_for_each(finite),
        _ => Ok(()),
    }
}
fn write_canonical(v: &V, out: &mut String) {
    match v {
        V::Object(_) => {
            out.push('{');
            for (i, (k, v)) in sorted_entries(v).iter().enumerate() {
                if i > 0 {
                    out.push(',')
                }
                out.push_str(&serde_json::to_string(k).unwrap());
                out.push(':');
                write_canonical(v, out)
            }
            out.push('}')
        }
        V::Array(a) => {
            out.push('[');
            for (i, v) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',')
                }
                write_canonical(v, out)
            }
            out.push(']')
        }
        V::Number(n) => {
            let mut b = ryu_js::Buffer::new();
            out.push_str(b.format_finite(n.as_f64().unwrap()));
        }
        _ => out.push_str(&serde_json::to_string(v).unwrap()),
    }
}
pub fn canonical_json(v: &V) -> Result<String, String> {
    finite(v)?;
    let mut out = String::new();
    write_canonical(v, &mut out);
    Ok(out)
}
pub fn canonical_hash(v: &V) -> Result<String, String> {
    Ok(format!("{:x}", Sha256::digest(canonical_json(v)?.as_bytes())))
}
pub fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

// JS Number.isInteger / Number.isSafeInteger also accept integral doubles.
pub fn js_int(v: &V) -> Option<i64> {
    match v {
        V::Number(n) => {
            if let Some(x) = n.as_i64() {
                // JS Number.isSafeInteger: integral value within ±(2^53-1).
                if x.unsigned_abs() <= 9_007_199_254_740_991 { Some(x) } else { None }
            } else if let Some(x) = n.as_u64() {
                if x <= 9_007_199_254_740_991 { i64::try_from(x).ok() } else { None }
            } else {
                let x = n.as_f64()?;
                if x.is_finite() && x.fract() == 0.0 && x.abs() <= 9_007_199_254_740_991.0 {
                    Some(x as i64)
                } else {
                    None
                }
            }
        }
        _ => None,
    }
}
pub fn js_truthy(v: Option<&V>) -> bool {
    match v {
        None | Some(V::Null) => false,
        Some(V::Bool(b)) => *b,
        Some(V::Number(n)) => n.as_f64().is_some_and(|x| x != 0.0 && !x.is_nan()),
        Some(V::String(s)) => !s.is_empty(),
        _ => true,
    }
}
