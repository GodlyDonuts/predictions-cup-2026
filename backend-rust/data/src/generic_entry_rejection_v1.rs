// execution/generic-entry-rejection-v1.mjs — candidate-local read failures
// can defer that pair without ending discovery. Account uncertainty,
// transport/backoff, durability and any durable intent stop it.
use baal_service_runtime::json_text::js_truthy;
use regex::Regex;
use serde_json::Value as V;
use std::sync::OnceLock;

fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}

/// /^BAAL_SCALED_EVIDENCE_(ENTRY_[A-Z_]+|CHEAP_FIRST_CHANGED_REQUIRES_NEW_PACKET)$/
fn pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"^BAAL_SCALED_EVIDENCE_(ENTRY_[A-Z_]+|CHEAP_FIRST_CHANGED_REQUIRES_NEW_PACKET)$",
        )
        .unwrap()
    })
}

/// genericEntryRejectionMayDefer(error,status)
///
/// JS `error.retryAfterMs === undefined` is true only when the key is absent
/// (a parsed JSON `null` is not `undefined`), so absence is tested directly.
pub fn generic_entry_rejection_may_defer(error: &V, status: &V) -> bool {
    js_truthy(Some(error))
        && !js_truthy(error.get("fatal"))
        && !js_truthy(error.get("durableRecordingFailed"))
        && error.get("retryAfterMs").is_none()
        && !js_truthy(status.get("intent"))
        && status.get("unknownAcceptance") == Some(&V::Bool(false))
        && status.get("phase").and_then(V::as_str) == Some("ready")
        && pattern().is_match(get(error, "code").as_str().unwrap_or(""))
}
