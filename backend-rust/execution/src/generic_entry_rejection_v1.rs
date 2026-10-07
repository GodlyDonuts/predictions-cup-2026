// execution/generic-entry-rejection-v1.mjs — candidate-local read failures can
// defer that pair without ending discovery. Account uncertainty,
// transport/backoff, durability and any durable intent stop it.
use baal_service_runtime::json_text::js_truthy;
use serde_json::Value as V;

fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}

// /^BAAL_SCALED_EVIDENCE_(ENTRY_[A-Z_]+|CHEAP_FIRST_CHANGED_REQUIRES_NEW_PACKET)$/
fn deferrable_code(code: &str) -> bool {
    if code == "BAAL_SCALED_EVIDENCE_CHEAP_FIRST_CHANGED_REQUIRES_NEW_PACKET" {
        return true;
    }
    match code.strip_prefix("BAAL_SCALED_EVIDENCE_ENTRY_") {
        Some(rest) => {
            !rest.is_empty()
                && rest
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b == b'_')
        }
        None => false,
    }
}

/// genericEntryRejectionMayDefer(error,status)
pub fn generic_entry_rejection_may_defer(error: &V, status: &V) -> bool {
    js_truthy(Some(error))
        && get(error, "fatal") != &V::Bool(true)
        && get(error, "durableRecordingFailed") != &V::Bool(true)
        && error.get("retryAfterMs").is_none()
        && !js_truthy(status.get("intent"))
        && get(status, "unknownAcceptance") == &V::Bool(false)
        && get(status, "phase").as_str() == Some("ready")
        && deferrable_code(get(error, "code").as_str().unwrap_or(""))
}
