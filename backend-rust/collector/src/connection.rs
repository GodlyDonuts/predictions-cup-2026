// live-paper/connection.mjs — synchronous port. The JS version races a stop
// flag with an async request; the Rust port checks stopped() before and after
// the blocking request.
use serde_json::{json, Value as V};
pub struct ConnectHooks<'a> {
    pub stopped: &'a dyn Fn() -> bool,
    pub on_retry: &'a dyn Fn(&V),
    pub pause: &'a dyn Fn(u64),
    pub now: &'a dyn Fn() -> f64,
    pub cooldown_remaining_ms: &'a dyn Fn() -> f64,
    pub manual_review: bool,
}
/// connectTournament(api_request, c, hooks) — api_request(path) returns the
/// response body {data} or Err(recoverable).
pub fn connect_tournament(
    request: &dyn Fn(&str) -> Result<V, /*recoverable*/ bool>,
    c: &V,
    h: &ConnectHooks,
) -> Result<Option<V>, String> {
    let mut attempts = 0u32;
    while !(h.stopped)() {
        match request(&format!("/tournaments/{}", c["tournamentSlug"].as_str().unwrap_or(""))) {
            Ok(body) => {
                let data = &body["data"];
                if data.is_null() || data["id"] != c["tournamentId"]
                    || data["currencyName"] != "SUSQies" || data["status"] != "active"
                    || data["isPendingEnrolment"] == V::Bool(true) {
                    return Err("Tournament identity/status mismatch".into());
                }
                return Ok(Some(data.clone()));
            }
            Err(recoverable) => {
                if (h.stopped)() { return Ok(None); }
                if !recoverable || h.manual_review { return Err("tournament_connect_failed".into()); }
                let backoff = (60000f64).min(10000.0 * 2f64.powi(attempts.min(3) as i32));
                let delay = backoff.max((h.cooldown_remaining_ms)().max(0.0)) as u64;
                attempts += 1;
                (h.on_retry)(&json!({"attempts": attempts, "delay": delay, "reason": "connection_unavailable"}));
                (h.pause)(delay);
            }
        }
    }
    Ok(None)
}
