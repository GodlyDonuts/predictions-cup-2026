// execution/read-only-receipt-callback-v1.mjs — forwards to the host's
// onFetchStart when present; otherwise GETs are ignored and any write aborts.
use serde_json::{json, Value as V};

fn sget<'a>(v: &'a V, k: &str) -> Option<&'a str> {
    v.get(k).and_then(V::as_str)
}

/// readOnlyReceiptCallbackV1(host,event) — `host` is represented by its
/// optional onFetchStart callback (JS `typeof host?.onFetchStart==='function'`).
/// Returns whatever the host callback returns, `false` for GET without a
/// callback, or the WRITE_RECEIPT_CALLBACK_REQUIRED error for any write.
pub fn read_only_receipt_callback_v1(
    on_fetch_start: Option<&dyn Fn(&V) -> Result<V, String>>,
    event: &V,
) -> Result<V, String> {
    if let Some(cb) = on_fetch_start {
        return cb(event);
    }
    if sget(event, "method") == Some("GET") {
        return Ok(json!(false));
    }
    // Object.assign(Error('WRITE_RECEIPT_CALLBACK_REQUIRED'),
    //   {fatal:true,durableRecordingFailed:true,mayHaveExecuted:true})
    Err("WRITE_RECEIPT_CALLBACK_REQUIRED".into())
}
