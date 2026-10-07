// execution/generic-entry-rejection-v1.mjs — candidate-local read failures
// can defer that pair without ending discovery. Account uncertainty,
// transport/backoff, durability and any durable intent stop it.
//
// Port notes:
// - `error.retryAfterMs===undefined`: absent key only; an explicit null is
//   not undefined in JS, so Some(Null) fails the check exactly like JS.
// - The code regex /^BAAL_SCALED_EVIDENCE_(ENTRY_[A-Z_]+|CHEAP_FIRST_CHANGED_
//   REQUIRES_NEW_PACKET)$/ is hand-matched: a non-string code is stringified
//   by RegExp.test but can never match this pattern, so it simply fails.
// - `status?.x` optional chains on a missing status yield undefined; only
//   `!status?.intent` is truthy-tolerant, the other two are strict ===.
use baal_service_runtime::json_text::js_truthy;
use serde_json::Value as V;

fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}

// ^BAAL_SCALED_EVIDENCE_(ENTRY_[A-Z_]+|CHEAP_FIRST_CHANGED_REQUIRES_NEW_PACKET)$
fn evidence_code_matches(code: &str) -> bool {
    let rest = match code.strip_prefix("BAAL_SCALED_EVIDENCE_") {
        Some(r) => r,
        None => return false,
    };
    if rest == "CHEAP_FIRST_CHANGED_REQUIRES_NEW_PACKET" {
        return true;
    }
    match rest.strip_prefix("ENTRY_") {
        Some(tail) => {
            !tail.is_empty() && tail.bytes().all(|b| b == b'_' || b.is_ascii_uppercase())
        }
        None => false,
    }
}

/// genericEntryRejectionMayDefer(error,status)
pub fn generic_entry_rejection_may_defer(error: &V, status: &V) -> bool {
    js_truthy(Some(error))
        && !js_truthy(error.get("fatal"))
        && !js_truthy(error.get("durableRecordingFailed"))
        && error.get("retryAfterMs").is_none()
        && !js_truthy(status.get("intent"))
        && status.get("unknownAcceptance") == Some(&V::Bool(false))
        && status.get("phase").and_then(V::as_str) == Some("ready")
        && evidence_code_matches(get(error, "code").as_str().unwrap_or(""))
}
