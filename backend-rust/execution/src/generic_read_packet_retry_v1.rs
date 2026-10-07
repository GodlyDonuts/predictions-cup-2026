// execution/generic-read-packet-retry-v1.mjs — a failed fresh read packet grants
// no execution authority. Financial failures and any prepared/uncertain intent
// remain blocking and retain their provenance.
use serde_json::Value as V;

fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}

/// genericReadPacketMayRetry(error,status)
pub fn generic_read_packet_may_retry(error: &V, status: &V) -> bool {
    let code = error
        .get("code")
        .filter(|v| !v.is_null())
        .unwrap_or_else(|| get(error, "message"));
    get(error, "durableRecordingFailed") != &V::Bool(true)
        && get(error, "mayHaveExecuted") != &V::Bool(true)
        && code.as_str() == Some("REST_NO_RECEIPT_TIME")
        && get(status, "phase").as_str() == Some("ready")
        && get(status, "intent").is_null()
        && get(status, "unknown") != &V::Bool(true)
}
