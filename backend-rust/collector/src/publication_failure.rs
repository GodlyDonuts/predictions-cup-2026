// live-paper/publication-failure.mjs — retain stable diagnostic codes, never
// an arbitrary exception message that could contain response data,
// credentials, paths, or account information.
use serde_json::Value as V;

/// Thrown-object stand-in for the JS `Object.assign(Error(msg), {code,
/// publicationOperation, durableRecordingFailed, fatal})`.
#[derive(Debug, Clone)]
pub struct PublicationFailure {
    pub message: String,
    pub code: String,
    pub publication_operation: String,
    pub durable_recording_failed: bool,
    pub fatal: bool,
}
impl std::fmt::Display for PublicationFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for PublicationFailure {}
impl From<PublicationFailure> for String {
    fn from(e: PublicationFailure) -> String {
        e.message
    }
}

// /^[A-Z][A-Z0-9_]{1,120}$/ — first char A-Z, then 1–120 of [A-Z0-9_]; the JS
// character classes only match ASCII bytes.
fn bounded_code(s: &str) -> bool {
    let b = s.as_bytes();
    s.is_ascii()
        && (2..=121).contains(&b.len())
        && b[0].is_ascii_uppercase()
        && b[1..]
            .iter()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == b'_')
}

// RegExp.test(raw) stringifies its argument: JS String(v). Numbers print via
// JS toString, arrays join on ',' with null/undefined elements empty, and any
// object is '[object Object]'.
fn js_string(v: &V) -> String {
    match v {
        V::Null => "null".to_string(),
        V::Bool(b) => (if *b { "true" } else { "false" }).to_string(),
        V::Number(n) => {
            let mut b = ryu_js::Buffer::new();
            b.format_finite(n.as_f64().unwrap_or_default()).to_string()
        }
        V::String(s) => s.clone(),
        V::Array(a) => a
            .iter()
            .map(|x| match x {
                V::Null => String::new(),
                _ => js_string(x),
            })
            .collect::<Vec<_>>()
            .join(","),
        V::Object(_) => "[object Object]".to_string(),
    }
}

/// publicationFailure(cause, method). `cause` is the thrown value viewed as
/// JSON — for an Error-like cause pass {"code": .., "message": ..}; a plain
/// message string cause maps to {"message": ..} via
/// publication_failure_message.
pub fn publication_failure(cause: &V, method: &str) -> PublicationFailure {
    // cause?.code ?? cause?.message — `??` falls through only on
    // null/undefined; a non-nullish non-string code (0, false, {}) is taken
    // and then fails the bounded-code test like the JS.
    let raw = cause
        .get("code")
        .filter(|v| !v.is_null())
        .or_else(|| cause.get("message").filter(|v| !v.is_null()));
    let raw = raw.map(js_string).unwrap_or_default();
    let safe = if bounded_code(&raw) {
        raw
    } else {
        "UNCLASSIFIED_PUBLICATION_FAILURE".to_string()
    };
    PublicationFailure {
        message: format!("Public book recording failed: {}", safe),
        publication_operation: if ["publishBook", "invalidate", "close"].contains(&method) {
            method.to_string()
        } else {
            "unknown".to_string()
        },
        code: safe,
        durable_recording_failed: true,
        fatal: true,
    }
}

/// `publicationFailure(Error(message), method)` for callers holding a plain
/// Rust error string.
pub fn publication_failure_message(cause_message: &str, method: &str) -> PublicationFailure {
    publication_failure(&serde_json::json!({"message": cause_message}), method)
}
