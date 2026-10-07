// execution/blocked-admission-read-collector-v1.mjs — called only by the
// existing serialized account client. Owns no credentials, fetch, timers,
// write permission, STOP mutation or replacement policy.
use std::cell::Cell;

use baal_service_runtime::contract::parse_iso_ms;
use baal_service_runtime::json_text::js_int;
use serde_json::{json, Map, Value as V};

fn need(ok: bool, c: &str) -> Result<(), String> {
    if !ok {
        Err(format!("BLOCKED_READ_{c}"))
    } else {
        Ok(())
    }
}

fn get<'a>(v: &'a V, k: &str) -> &'a V {
    v.get(k).unwrap_or(&V::Null)
}

/// collectBlockedAdmissionAccountV1({api,receiptOf,scope,intent,canRead,now})
///
/// `api_dispatch` is the serialized client's dispatch (JS api.dispatch); the
/// guard functions JS puts on the spec (beforeDispatch/beforeNetwork) are
/// carried as unique numeric tokens — the same token for both fields, matching
/// `beforeDispatch:guard,beforeNetwork:guard`.
pub struct BlockedAdmissionReadCollectorDeps<'a> {
    pub api_dispatch: &'a dyn Fn(&V) -> Result<V, String>,
    pub receipt_of: &'a dyn Fn(&V, &V) -> Result<V, String>,
    pub can_read: &'a dyn Fn() -> Result<bool, String>,
    pub now: &'a dyn Fn() -> f64,
}

pub fn collect_blocked_admission_account_v1(
    scope: &V,
    intent: &V,
    deps: &BlockedAdmissionReadCollectorDeps<'_>,
) -> Result<V, String> {
    need(
        intent.get("orderId") == Some(&V::Null)
            && js_int(intent.get("actualFetchCount").unwrap_or(&V::Null)) == Some(0)
            && get(intent, "body")
                .get("expirationDate")
                .and_then(V::as_str)
                .and_then(parse_iso_ms)
                .map(|x| x < (deps.now)())
                .unwrap_or(false),
        "EXPIRED_UNFETCHED",
    )?;
    let t = get(scope, "tournamentId").as_str().unwrap_or("").to_string();
    let slug = get(scope, "tournamentSlug").as_str().unwrap_or("").to_string();
    let ex = get(get(intent, "body"), "exchangeId")
        .as_str()
        .unwrap_or("")
        .to_string();
    let endpoints = [
        format!("/tournaments/{slug}"),
        format!("/tournaments/{slug}/portfolio/positions"),
        format!("/orders?status=open&limit=200&tournamentId={t}"),
        format!("/orders?status=all&exchangeId={ex}&limit=200&tournamentId={t}"),
    ];
    let mut receipts = Vec::new();
    let guard_serial = Cell::new(0u64);
    for endpoint in endpoints {
        need((deps.can_read)()? == true, "OWNER_OR_STOP_PROVENANCE")?;
        // JS creates one fresh guard closure per endpoint and puts the same
        // closure on both spec fields — one shared token per endpoint.
        let token = guard_serial.get() + 1;
        guard_serial.set(token);
        let spec = json!({
            "method": "GET",
            "endpoint": endpoint,
            "beforeDispatch": token,
            "beforeNetwork": token,
        });
        let response = (deps.api_dispatch)(&spec)?;
        receipts.push((deps.receipt_of)(
            &response,
            &json!({"method": "GET", "requestBodyHash": V::Null}),
        )?);
    }
    for r in &receipts[2..] {
        let body_text = get(r, "bodyText").as_str().unwrap_or("");
        let d: V = serde_json::from_str(body_text).map_err(|e| e.to_string())?;
        need(
            get(&d, "pagination").get("hasMore") == Some(&V::Bool(false))
                && get(&d, "pagination").get("nextCursor") == Some(&V::Null),
            "COMPLETE_SINGLE_PAGE",
        )?;
    }
    let mut out = Map::new();
    out.insert("cash".into(), receipts[0].clone());
    out.insert("positions".into(), receipts[1].clone());
    out.insert("openOrdersPages".into(), json!([receipts[2].clone()]));
    out.insert("scopedOrdersPages".into(), json!([receipts[3].clone()]));
    Ok(V::Object(out))
}
