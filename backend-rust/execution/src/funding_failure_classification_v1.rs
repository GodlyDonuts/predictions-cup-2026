// execution/funding-failure-classification-v1.mjs — a stale funding failure
// may be retried only when the journal is provably unchanged, no prior
// failure was recorded, and the thrown error neither reports durable-
// recording failure nor possible execution.
//
// Port notes:
// - `error?.code ?? error?.message` reads a JSON error object ({code,message,
//   durableRecordingFailed,mayHaveExecuted}); a null/absent value is the JS
//   `?.`/`??` fallthrough. Rust callers carrying plain String errors pass
//   {"message": code} (plus any flags the JS error object would have set).
use serde_json::Value as V;

fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}

/// staleFundingFailureCanRetry(error,{journalUnchanged,alreadyFailed})
pub fn stale_funding_failure_can_retry_v1(
    error: &V,
    journal_unchanged: bool,
    already_failed: bool,
) -> bool {
    if already_failed || !journal_unchanged {
        return false;
    }
    if get(error, "durableRecordingFailed") == &V::Bool(true)
        || get(error, "mayHaveExecuted") == &V::Bool(true)
    {
        return false;
    }
    // error?.code ?? error?.message — only a present non-null code wins.
    let code = match error.get("code") {
        Some(c) if !c.is_null() => c.as_str(),
        _ => get(error, "message").as_str(),
    };
    matches!(
        code,
        Some("REST_NO_RECEIPT_TIME") | Some("BAAL_SCALED_EVIDENCE_SOURCE_CLOCK")
    )
}
