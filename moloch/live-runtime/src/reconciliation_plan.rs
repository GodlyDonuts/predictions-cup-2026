//! Pure scheduling of owned-order reads from a complete official open list.
//!
//! EVERY owned order still requires its complete OwnedFills tape each cycle.
//! A matched open list member replaces only its redundant OwnedOrder GET. It
//! cannot establish terminal status, a fill, cash, inventory or trade authority.
use baal_transport_service::{prices::official_tick_units, Receipt, Scope};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub type Result<T> = std::result::Result<T, &'static str>;
const MAX_ROWS: usize = 4096;

/// Actual ACK admission and current officially credited fills from the native
/// ledger. Never substitute the executable open remainder for admission size.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnedOrder {
    pub logical_id: u64,
    pub actual_order_id: String,
    pub exchange_id: String,
    pub market_id: String,
    pub side: String,
    pub action: String,
    pub limit_price: String,
    pub admitted_quantity: u64,
    pub credited_filled_quantity: u64,
    pub created_at_ms: u64,
    pub expires_at_ms: u64,
    pub cancel_pending: bool,
    pub acceptance_uncertain: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Limits { pub maximum_snapshot_age_ms: u64, pub maximum_rows: usize }
impl Default for Limits {
    fn default() -> Self { Self { maximum_snapshot_age_ms: 20_000, maximum_rows: MAX_ROWS } }
}

/// A previously validated complete fill receipt, recorded AFTER its fills have
/// been durably applied. Not Deserialize: restarting starts without this cache.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FillWatermark {
    scope_hash: String,
    pub actual_order_id: String,
    pub receipt_hash: String,
    pub received_at_ms: u64,
    pub projected_through_sequence: Option<u64>,
    pub credited_filled_quantity: u64,
}

/// Aggregate observation only. Never advance a fill watermark from this value.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectionWatermark {
    scope_hash: String,
    pub receipt_hash: String,
    pub request_started_at_ms: u64,
    pub received_at_ms: u64,
    pub projected_through_sequence: u64,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    AggregateMissing, AggregateInvalid, AggregateStale, ProjectionRegressed,
    ForeignOpenOrder, FillWatermarkChanged, OrderMissing, OrderTermsChanged,
    RemainderInvalid, CancelPending, OrderExpiring, AcceptanceUncertain,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenMember {
    pub logical_id: u64,
    pub actual_order_id: String,
    pub aggregate_endpoint: String,
    pub aggregate_receipt_hash: String,
    pub aggregate_row_index: usize,
    pub aggregate_received_at_ms: u64,
    pub fill_request_must_start_by_ms: u64,
    pub projected_through_sequence: u64,
    pub observed_executable_remainder: u64,
    /// MUST start the full owned-fill request after aggregate.received_at.
    /// A changed remainder is not a fill and does not change admission size.
    pub complete_fill_tape_required: bool,
    pub retained_fill_watermark: Option<FillWatermark>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExactReadPair {
    pub logical_id: u64,
    pub actual_order_id: String,
    pub reason: Reason,
    /// Existing OwnedOrder -> later COMPLETE OwnedFills -> feedback::terminal.
    pub order_before_complete_fills: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", content = "evidence", rename_all = "snake_case")]
pub enum OrderDecision {
    OpenMemberNeedsFillTape(OpenMember),
    ExactPairFallback(ExactReadPair),
}
impl OrderDecision {
    pub fn actual_order_id(&self) -> &str {
        match self { Self::OpenMemberNeedsFillTape(v) => &v.actual_order_id, Self::ExactPairFallback(v) => &v.actual_order_id }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadPlan {
    pub decisions: Vec<OrderDecision>,
    pub observed_projection: Option<ProjectionWatermark>,
    pub foreign_open_order_ids: Vec<String>,
    pub blocks_new_submissions: bool,
    pub full_fill_tapes_required: usize,
    /// Logical endpoint counts, not HTTP admissions: paginated fills/open lists
    /// may need multiple reads. No achieved throughput is inferred from counts.
    pub owned_endpoint_operations: usize,
    pub endpoint_operations_including_aggregate: usize,
    pub baseline_endpoint_operations: usize,
    pub execution_authority: bool,
}

fn hash<T: Serialize>(v: &T) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(v).map_err(|_| "PLAN_SERIALIZATION")?)))
}
fn numeric(s: &str) -> bool {
    !s.is_empty() && !s.starts_with('0') && s.bytes().all(|b| b.is_ascii_digit())
        && s.parse::<u64>().is_ok_and(|n| n <= 9_007_199_254_740_991)
}
fn id(v: &Value) -> Result<String> {
    let s = v.as_str().map(str::to_string).or_else(|| v.as_u64().map(|n| n.to_string())).ok_or("PLAN_ID")?;
    if !numeric(&s) { return Err("PLAN_ID"); } Ok(s)
}
fn clock(s: &str) -> Result<u64> {
    u64::try_from(chrono::DateTime::parse_from_rfc3339(s).map_err(|_| "PLAN_CLOCK")?.timestamp_millis()).map_err(|_| "PLAN_CLOCK")
}
fn value_clock(v: &Value) -> Result<u64> { clock(v.as_str().ok_or("PLAN_CLOCK")?) }
fn receipt_clock(r: &Receipt, now: u64) -> Result<(u64, u64)> {
    let a = clock(&r.request_started_at)?; let b = clock(&r.received_at)?;
    if r.status != 200 || a > b || b > now { return Err("PLAN_RECEIPT_CLOCK_OR_STATUS"); } Ok((a, b))
}
fn complete(v: &Value) -> Result<Option<u64>> {
    if v["coverage"]["complete"] != true || v["pagination"]["hasMore"] != false
        || v["pagination"].get("nextCursor") != Some(&Value::Null)
        || v["pagination"]["limit"].as_u64().is_none_or(|n| n == 0 || n > 200) { return Err("PLAN_INCOMPLETE_PROJECTION"); }
    let sequence = v["coverage"].get("projectedThroughSequence").ok_or("PLAN_PROJECTION_SEQUENCE")?;
    if sequence.is_null() { Ok(None) } else { sequence.as_u64().map(Some).ok_or("PLAN_PROJECTION_SEQUENCE") }
}
fn validate_owned(scope: &Scope, q: &OwnedOrder) -> Result<()> {
    if q.logical_id == 0 || !numeric(&q.actual_order_id) || !numeric(&q.exchange_id) || !numeric(&q.market_id)
        || scope.exchanges.get(&q.exchange_id) != Some(&q.market_id)
        || !["yes", "no"].contains(&q.side.as_str()) || !["buy", "sell"].contains(&q.action.as_str())
        || q.admitted_quantity == 0 || q.admitted_quantity > i32::MAX as u64
        || q.credited_filled_quantity > q.admitted_quantity || q.created_at_ms >= q.expires_at_ms { return Err("PLAN_OWNED_BINDING"); }
    official_tick_units(&Value::String(q.limit_price.clone())).map_err(|_| "PLAN_OWNED_PRICE")?; Ok(())
}
fn row_terms(scope: &Scope, q: &OwnedOrder, row: &Value, received: u64) -> Result<()> {
    if id(&row["id"])? != q.actual_order_id || row["exchangeId"] != q.exchange_id
        || row["tournamentId"] != scope.tournament_id || row["side"] != q.side || row["action"] != q.action
        || row["open"] != true || row.get("marketId").is_some_and(|v| v != &q.market_id)
        || row.get("quantityFilled") != Some(&Value::Null) || row.get("terminalReasonCode") != Some(&Value::Null)
        || value_clock(&row["createdAt"])? < q.created_at_ms || value_clock(&row["createdAt"])? > received
        || value_clock(&row["expirationDate"])? != q.expires_at_ms
        || official_tick_units(&row["priceLimit"]).map_err(|_| "PLAN_ROW_PRICE")?
            != official_tick_units(&Value::String(q.limit_price.clone())).map_err(|_| "PLAN_OWNED_PRICE")? { return Err("PLAN_ORDER_TERMS"); }
    Ok(())
}

/// Call after normal complete feedback::fills succeeds and all events have been
/// durably applied. This verifies a count/watermark, not individual fill credit.
pub fn fill_watermark(scope: &Scope, q: &OwnedOrder, fills: &Receipt, now_ms: u64) -> Result<FillWatermark> {
    validate_owned(scope, q)?;
    let (_, received) = receipt_clock(fills, now_ms)?;
    if fills.endpoint != format!("/orders/{}/fills?limit=200", q.actual_order_id)
        || id(&fills.data["orderId"])? != q.actual_order_id || fills.data["exchangeId"] != q.exchange_id
        || fills.data["tournamentId"] != scope.tournament_id { return Err("PLAN_FILL_RECEIPT_BINDING"); }
    let sequence = complete(&fills.data)?;
    let total = fills.data["totalQuantityFilled"].as_i64().ok_or("PLAN_FILL_TOTAL")?;
    if total != 0 && ((total < 0) != (q.side == "no")) { return Err("PLAN_FILL_SIDE"); }
    let rows = fills.data["data"].as_array().filter(|a| a.len() <= 65536).ok_or("PLAN_FILL_ROWS")?;
    let mut seen = BTreeSet::new(); let mut sum = 0u64;
    for row in rows {
        let n = row["quantity"].as_i64().filter(|n| *n != 0 && *n != i64::MIN).ok_or("PLAN_FILL_SIZE")?;
        if !seen.insert(id(&row["id"])?) || row["side"] != q.side || ((n < 0) != (q.side == "no"))
            || value_clock(&row["filledAt"])? < q.created_at_ms || value_clock(&row["filledAt"])? > received {
            return Err("PLAN_FILL_BOUNDS");
        }
        sum = sum.checked_add(n.unsigned_abs()).ok_or("PLAN_FILL_SIZE")?;
    }
    if sum != total.unsigned_abs() || sum != q.credited_filled_quantity { return Err("PLAN_LEDGER_FILL_COUNT"); }
    Ok(FillWatermark { scope_hash: hash(scope)?, actual_order_id: q.actual_order_id.clone(), receipt_hash: hash(fills)?,
        received_at_ms: received, projected_through_sequence: sequence, credited_filled_quantity: sum })
}

/// Check immediately before requesting fills; a large cohort can age the
/// aggregate while earlier orders are processed. Expired hints use exact pairs.
pub fn member_is_fresh(member: &OpenMember, now_ms: u64) -> bool {
    member.aggregate_received_at_ms <= now_ms && now_ms <= member.fill_request_must_start_by_ms
}

/// Check BEFORE applying feedback::fills. Preserve the original full aggregate
/// and full fill receipts; never create an imitation /orders/{id} receipt.
pub fn validate_member_fills(member: &OpenMember, fills: &Receipt, now_ms: u64) -> Result<()> {
    let (started, _) = receipt_clock(fills, now_ms)?;
    if fills.endpoint != format!("/orders/{}/fills?limit=200", member.actual_order_id)
        || id(&fills.data["orderId"])? != member.actual_order_id
        || !member_is_fresh(member, started)
        || complete(&fills.data)?.is_none_or(|s| s < member.projected_through_sequence) {
        return Err("PLAN_MEMBER_FILL_RECEIPT");
    }
    Ok(())
}

fn aggregate<'a>(scope: &Scope, owned: &BTreeSet<String>, r: &'a Receipt, prior: Option<&ProjectionWatermark>, now: u64, limits: Limits)
    -> std::result::Result<(BTreeMap<String, (usize, &'a Value)>, ProjectionWatermark), (Reason, Vec<String>)> {
    let bad = || (Reason::AggregateInvalid, Vec::new());
    let (started, received) = receipt_clock(r, now).map_err(|_| bad())?;
    if r.endpoint != format!("/orders?status=open&limit=200&tournamentId={}", scope.tournament_id) { return Err(bad()); }
    if now - started > limits.maximum_snapshot_age_ms { return Err((Reason::AggregateStale, vec![])); }
    let seq = complete(&r.data).map_err(|_| bad())?.ok_or_else(bad)?;
    let scope_hash = hash(scope).map_err(|_| bad())?;
    if prior.is_some_and(|p| p.scope_hash != scope_hash || p.projected_through_sequence > seq
        || p.request_started_at_ms > started || p.received_at_ms > received) { return Err((Reason::ProjectionRegressed, vec![])); }
    let rows = r.data["data"].as_array().filter(|a| a.len() <= limits.maximum_rows).ok_or_else(bad)?;
    let mut map = BTreeMap::new(); let mut foreign = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        let id = id(&row["id"]).map_err(|_| bad())?;
        if map.insert(id.clone(), (index, row)).is_some() { return Err(bad()); }
        if !owned.contains(&id) { foreign.push(id); }
        if row["open"] != true || row["tournamentId"] != scope.tournament_id { return Err(bad()); }
    }
    if !foreign.is_empty() { return Err((Reason::ForeignOpenOrder, foreign)); }
    Ok((map, ProjectionWatermark { scope_hash, receipt_hash: hash(r).map_err(|_| bad())?, request_started_at_ms: started,
        received_at_ms: received, projected_through_sequence: seq }))
}

/// The caller obtains a fresh full open list through the SAME existing account
/// transport, then still requests COMPLETE OwnedFills for EVERY decision. For a
/// matched open member, record the original aggregate receipt and DO NOT invoke
/// terminal feedback. For fallback, keep the original order-then-fills sequence.
/// Unbound unknown writes must be supplied through `unresolved_acceptance`.
pub fn plan(scope: &Scope, owned: &[OwnedOrder], watermarks: &[FillWatermark], previous_projection: Option<&ProjectionWatermark>,
    snapshot: Option<&Receipt>, unresolved_acceptance: bool, now_ms: u64, limits: Limits) -> Result<ReadPlan> {
    if !baal_transport_service::relationships::uuid(&scope.tournament_id) || scope.exchanges.is_empty()
        || scope.exchanges.len() > MAX_ROWS || limits.maximum_rows == 0 || limits.maximum_rows > MAX_ROWS
        || owned.len() > limits.maximum_rows || watermarks.len() > limits.maximum_rows
        || limits.maximum_snapshot_age_ms == 0 || limits.maximum_snapshot_age_ms > 20_000 { return Err("PLAN_BOUNDS_OR_SCOPE"); }
    let mut ids = BTreeSet::new(); let mut logical = BTreeSet::new();
    for q in owned {
        validate_owned(scope, q)?;
        if !ids.insert(q.actual_order_id.clone()) || !logical.insert(q.logical_id) { return Err("PLAN_DUPLICATE_OWNERSHIP"); }
    }
    let mut confirmed = BTreeMap::new();
    for c in watermarks {
        if !ids.contains(&c.actual_order_id) || confirmed.insert(c.actual_order_id.as_str(), c).is_some() { return Err("PLAN_WATERMARK_OWNERSHIP"); }
    }
    let scope_hash = hash(scope)?;
    let observed = snapshot.map(|r| aggregate(scope, &ids, r, previous_projection, now_ms, limits));
    let mut out = ReadPlan { decisions: vec![], observed_projection: None, foreign_open_order_ids: vec![], blocks_new_submissions: unresolved_acceptance,
        full_fill_tapes_required: owned.len(), owned_endpoint_operations: owned.len(), endpoint_operations_including_aggregate: 0,
        baseline_endpoint_operations: owned.len() * 2, execution_authority: false };
    let (rows, global_reason) = match observed {
        None => (BTreeMap::new(), Some(Reason::AggregateMissing)),
        Some(Err((reason, foreign))) => {out.blocks_new_submissions = true; out.foreign_open_order_ids = foreign; (BTreeMap::new(), Some(reason))},
        Some(Ok((rows, point))) => {out.observed_projection = Some(point); (rows, None)},
    };
    for q in owned {
        let watermark = confirmed.get(q.actual_order_id.as_str()).copied();
        let remaining = q.admitted_quantity - q.credited_filled_quantity;
        let mut open_row = None;
        let reason = if unresolved_acceptance || q.acceptance_uncertain {out.blocks_new_submissions = true; Some(Reason::AcceptanceUncertain)}
            else if q.cancel_pending {Some(Reason::CancelPending)}
            else if now_ms >= q.expires_at_ms || remaining == 0 {Some(Reason::OrderExpiring)}
            else if let Some(reason) = global_reason {Some(reason)}
            else if watermark.is_some_and(|c| c.scope_hash != scope_hash || c.credited_filled_quantity != q.credited_filled_quantity) {Some(Reason::FillWatermarkChanged)}
            else if watermark.is_some_and(|c| c.received_at_ms > out.observed_projection.as_ref().unwrap().request_started_at_ms
                || c.projected_through_sequence.is_some_and(|s| s > out.observed_projection.as_ref().unwrap().projected_through_sequence)) {Some(Reason::ProjectionRegressed)}
            else if let Some((index, row)) = rows.get(&q.actual_order_id) {
                if row_terms(scope, q, row, out.observed_projection.as_ref().unwrap().received_at_ms).is_err() {
                    out.blocks_new_submissions = true; Some(Reason::OrderTermsChanged)
                } else if row["quantity"].as_u64().is_none_or(|n| n == 0 || n > remaining) {Some(Reason::RemainderInvalid)}
                else {open_row = Some(*index); None}
            } else {Some(Reason::OrderMissing)};
        if let Some(reason) = reason {
            out.owned_endpoint_operations += 1;
            out.decisions.push(OrderDecision::ExactPairFallback(ExactReadPair { actual_order_id: q.actual_order_id.clone(), logical_id: q.logical_id,
                reason, order_before_complete_fills: true }));
        } else {
            let r = snapshot.unwrap(); let index = open_row.unwrap(); let point = out.observed_projection.as_ref().unwrap();
            out.decisions.push(OrderDecision::OpenMemberNeedsFillTape(OpenMember { actual_order_id: q.actual_order_id.clone(), logical_id: q.logical_id,
                aggregate_endpoint: r.endpoint.clone(), aggregate_receipt_hash: point.receipt_hash.clone(), aggregate_row_index: index,
                aggregate_received_at_ms: point.received_at_ms,
                fill_request_must_start_by_ms: point.request_started_at_ms.checked_add(limits.maximum_snapshot_age_ms).ok_or("PLAN_CLOCK_RANGE")?,
                projected_through_sequence: point.projected_through_sequence,
                observed_executable_remainder: r.data["data"][index]["quantity"].as_u64().unwrap(), complete_fill_tape_required: true,
                retained_fill_watermark: watermark.cloned() }));
        }
    }
    out.endpoint_operations_including_aggregate = out.owned_endpoint_operations + usize::from(snapshot.is_some());
    Ok(out)
}
