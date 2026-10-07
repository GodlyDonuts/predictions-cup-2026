// execution/funding-failure-classification-v1.mjs — a stale funding failure
// may retry only while the journal is provably unchanged, no failure was
// already recorded, and the error carries no execution uncertainty.
use serde_json::Value as V;

fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}

/// staleFundingFailureCanRetry(error,{journalUnchanged,alreadyFailed})
/// `error` is the same JSON error-object adapter used by
/// generic_read_packet_may_retry: {code,message,durableRecordingFailed,
/// mayHaveExecuted}; `code ?? message` picks the first present key.
pub fn stale_funding_failure_can_retry(
    error: &V,
    journal_unchanged: bool,
    already_failed: bool,
) -> bool {
    let code = error
        .get("code")
        .filter(|v| !v.is_null())
        .unwrap_or_else(|| get(error, "message"));
    !already_failed
        && journal_unchanged
        && get(error, "durableRecordingFailed") != &V::Bool(true)
        && get(error, "mayHaveExecuted") != &V::Bool(true)
        && ["REST_NO_RECEIPT_TIME", "BAAL_SCALED_EVIDENCE_SOURCE_CLOCK"]
            .contains(&code.as_str().unwrap_or(""))
}
