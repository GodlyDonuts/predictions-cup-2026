use crate::core::{conservative_cash_floor, decimal_micros, format_micros, payload_hash};
use baal_service_runtime::fixed::{amount, instant};
use baal_service_runtime::json_text::digest;
use baal_service_runtime::market::account_fills;
use serde_json::{json, Map, Value as V};
use std::collections::HashSet;
use std::path::Path;

// execution/production-activation.mjs
const OWNER_ASSIGNMENT: i64 = 5_984_685_670;
const OWNER_ACK: i64 = 5_984_845_412;
const HUMAN_THREAD: &str = "01a0f8cc-b9bb-77d2-9f4c-2844396f1b79";
const STOP_THREAD: &str = "01a0f8cb-e876-739f-b149-0d446a5be0c3";
const NEED_PREFIX: &str = "PRODUCTION_";
fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(format!("{NEED_PREFIX}{code}")) }
}
fn hash(s: &V) -> bool {
    s.as_str().is_some_and(|x| x.len() == 64 && x.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()))
}
fn stamp(s: &V) -> bool {
    s.as_str().is_some_and(|x| {
        chrono::DateTime::parse_from_rfc3339(x)
            .map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Millis, true) == x)
            .unwrap_or(false)
    })
}
fn ms(s: &V) -> i64 {
    chrono::DateTime::parse_from_rfc3339(s.as_str().unwrap_or(""))
        .map(|d| d.timestamp_millis())
        .unwrap_or(i64::MIN)
}
fn comment(v: &V) -> bool {
    baal_service_runtime::json_text::js_int(v).is_some_and(|n| n > 0)
}
fn median_human_activation(a: &V) -> Result<(), String> {
    let u = a.get("activationAuthority");
    need(
        a.get("authorityKind").and_then(V::as_str) == Some("direct_human_assignment")
            && a.get("assignmentCommentId").and_then(V::as_i64) == Some(OWNER_ASSIGNMENT)
            && a.get("ackCommentId").and_then(V::as_i64) == Some(OWNER_ACK)
            && u.and_then(|u| u.get("kind")).and_then(V::as_str) == Some("direct_human_activation")
            && u.and_then(|u| u.get("threadId")).and_then(V::as_str) == Some(HUMAN_THREAD)
            && u.and_then(|u| u.get("runId")).and_then(V::as_str) == a.get("runId").and_then(V::as_str)
            && u.and_then(|u| u.get("exploratory")) == Some(&V::Bool(true))
            && u.and_then(|u| u.get("profitabilityEstablished")) == Some(&V::Bool(false))
            && u.and_then(|u| u.get("quote")).and_then(V::as_str).is_some_and(|q| !q.is_empty() && q.len() <= 2000)
            && u.map(|u| stamp(u.get("recordedAt").unwrap_or(&V::Null))).unwrap_or(false)
            && ms(&u.unwrap().get("recordedAt").unwrap().clone()) <= ms(a.get("startedAt").unwrap()),
        "MEDIAN_HUMAN_ACTIVATION",
    )
}
pub fn production_authority_fields(a: &V) -> Result<Vec<&'static str>, String> {
    if a.get("authorityKind").and_then(V::as_str) == Some("direct_human_assignment") {
        let ok = a.get("handoffId").and_then(V::as_str).is_some_and(|s| {
            s.strip_prefix("human-to-baal-").is_some_and(|r| {
                !r.is_empty() && r.len() <= 70 && r.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            })
        }) && comment(a.get("assignmentCommentId").unwrap_or(&V::Null))
            && a.get("releaseCommentId").is_none();
        need(ok, "HUMAN_ASSIGNMENT")?;
        return Ok(vec!["handoffId", "authorityKind", "assignmentCommentId", "ackCommentId"]);
    }
    let ok = a.get("authorityKind").is_none()
        && a.get("handoffId").and_then(V::as_str).is_some_and(|s| {
            s.strip_prefix("daenerys-to-baal-").is_some_and(|r| {
                !r.is_empty() && r.len() <= 70 && r.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            })
        })
        && comment(a.get("releaseCommentId").unwrap_or(&V::Null))
        && a.get("assignmentCommentId").is_none();
    need(ok, "AGENT_RELEASE")?;
    Ok(vec!["handoffId", "releaseCommentId", "ackCommentId"])
}
pub fn validate_production_activation(a: &V) -> Result<V, String> {
    let run_ok = a.get("runId").and_then(V::as_str).is_some_and(|s| {
        s.strip_prefix("sig-pilot-").is_some_and(|r| {
            !r.is_empty() && r.len() <= 64 && r.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
    });
    need(
        a.get("schemaVersion").and_then(V::as_i64) == Some(2)
            && a.get("enabled") == Some(&V::Bool(true))
            && a.get("ownerId").and_then(V::as_str) == Some("Codex")
            && a.get("tournamentId").and_then(V::as_str) == Some(crate::TOURNAMENT_ID)
            && run_ok
            && ["settlement-value-native-v1", "probability-receipt-native-v1", "historical-median-receipt-v1"]
                .contains(&a.get("strategyId").and_then(V::as_str).unwrap_or("")),
        "ACTIVATION",
    )?;
    need(
        hash(a.get("handoffHash").unwrap_or(&V::Null))
            && hash(a.get("strategyBankHash").unwrap_or(&V::Null))
            && a.get("handoffId").is_some_and(V::is_string)
            && comment(a.get("ackCommentId").unwrap_or(&V::Null)),
        "HANDOFF",
    )?;
    production_authority_fields(a)?;
    need(
        a.get("feeBps").and_then(V::as_i64) == Some(0)
            && a.get("depthFractionBps").and_then(V::as_i64) == Some(100)
            && a.get("maxManagedMarkets").and_then(V::as_i64) == Some(3),
        "POLICY",
    )?;
    let dm = |k: &str| decimal_micros(a.get(k).unwrap_or(&V::Null), "exact").unwrap_or(-1);
    let (order, total, buys, reserve, capital) =
        (dm("maxOrderCost"), dm("maxAggregateCost"), dm("maxCumulativeBuyCost"), dm("cashReserve"), dm("sizingCapital"));
    need(
        order > 0 && order <= 1_000_000
            && total >= order && total <= 10_000_000
            && buys >= order && buys <= 25_000_000
            && reserve >= 0
            && capital > reserve
            && capital <= 100_000_000_000,
        "LIMITS",
    )?;
    need(
        stamp(a.get("startedAt").unwrap_or(&V::Null))
            && stamp(a.get("entryExpiresAt").unwrap_or(&V::Null))
            && ms(a.get("entryExpiresAt").unwrap()) > ms(a.get("startedAt").unwrap())
            && ms(a.get("entryExpiresAt").unwrap()) - ms(a.get("startedAt").unwrap()) <= 21_600_000,
        "WINDOW",
    )?;
    if a.get("strategyId").and_then(V::as_str) == Some("historical-median-receipt-v1") {
        median_human_activation(a)?;
    }
    Ok(a.clone())
}
pub fn validate_production_handoff(h: &V, a: &V) -> Result<V, String> {
    validate_production_activation(a)?;
    if h.get("schemaVersion").and_then(V::as_i64) == Some(3) {
        let fields = production_authority_fields(a)?;
        need(
            a.get("strategyId").and_then(V::as_str) == Some("historical-median-receipt-v1")
                && h.get("kind").and_then(V::as_str) == Some("same_owner_explicit_human_activation")
                && h.get("executionOwner").and_then(V::as_str) == Some("Codex")
                && h.get("ownershipTransferred") == Some(&V::Bool(false))
                && payload_hash(h).ok().as_deref() == a.get("handoffHash").and_then(V::as_str)
                && fields.iter().all(|k| h.get(*k) == a.get(*k)),
            "SAME_OWNER_ACTIVATION",
        )?;
        let old = h.get("inheritedOwnershipRecord").cloned().unwrap_or(V::Null);
        let prior = h.get("inheritedActivation").cloned().unwrap_or(V::Null);
        need(
            old.get("schemaVersion").and_then(V::as_i64) == Some(2)
                && prior.get("authorityKind").and_then(V::as_str) == Some("direct_human_assignment")
                && prior.get("strategyId").and_then(V::as_str) != Some("historical-median-receipt-v1")
                && prior.get("runId").and_then(V::as_str) != a.get("runId").and_then(V::as_str)
                && prior.get("assignmentCommentId").and_then(V::as_i64) == Some(OWNER_ASSIGNMENT)
                && prior.get("ackCommentId").and_then(V::as_i64) == Some(OWNER_ACK)
                && fields.iter().all(|k| prior.get(*k) == a.get(*k)),
            "INHERITED_OWNERSHIP",
        )?;
        validate_production_handoff(&old, &prior)?;
        need(
            ms(prior.get("startedAt").unwrap()) <= ms(a.pointer("/activationAuthority/recordedAt").unwrap()),
            "ACTIVATION_AUTHORITY_CLOCK",
        )?;
        need(
            payload_hash(h.get("humanActivation").unwrap()).ok()
                == payload_hash(a.get("activationAuthority").unwrap()).ok()
                && payload_hash(h.get("humanActivation").unwrap()).is_ok(),
            "ACTIVATION_AUDIT",
        )?;
        let r = h.get("reconciliation").cloned().unwrap_or(V::Null);
        need(
            r.get("complete") == Some(&V::Bool(true))
                && r.get("tournamentId").and_then(V::as_str) == Some(crate::TOURNAMENT_ID)
                && hash(r.get("receiptHash").unwrap_or(&V::Null))
                && stamp(r.get("oldestReceivedAt").unwrap_or(&V::Null))
                && stamp(r.get("newestReceivedAt").unwrap_or(&V::Null))
                && stamp(h.get("reconciledAt").unwrap_or(&V::Null))
                && ms(r.get("oldestReceivedAt").unwrap()) <= ms(r.get("newestReceivedAt").unwrap())
                && ms(r.get("newestReceivedAt").unwrap()) <= ms(h.get("reconciledAt").unwrap())
                && ms(r.get("oldestReceivedAt").unwrap()) >= ms(a.pointer("/activationAuthority/recordedAt").unwrap())
                && ms(a.get("startedAt").unwrap()) - ms(r.get("oldestReceivedAt").unwrap()) <= 120_000,
            "RECONCILIATION",
        )?;
        need(
            stamp(h.get("acknowledgedAt").unwrap_or(&V::Null))
                && ms(h.get("reconciledAt").unwrap()) <= ms(h.get("acknowledgedAt").unwrap())
                && ms(h.get("acknowledgedAt").unwrap()) <= ms(a.get("startedAt").unwrap()),
            "HANDOFF_CLOCK",
        )?;
        need(
            h.get("writerStopEvidence").is_none()
                && h.get("releasedAt").is_none()
                && h.get("releaseCommentId").is_none(),
            "NO_NEW_REMOTE_STOP_OR_RELEASE",
        )?;
        return Ok(h.clone());
    }
    need(
        a.get("strategyId").and_then(V::as_str) != Some("historical-median-receipt-v1"),
        "MEDIAN_SAME_OWNER_RECORD_REQUIRED",
    )?;
    let fields = production_authority_fields(a)?;
    need(
        payload_hash(h).ok().as_deref() == a.get("handoffHash").and_then(V::as_str)
            && fields.iter().all(|k| h.get(*k) == a.get(*k))
            && h.get("formerWriter").and_then(V::as_str) == Some("Daenerys")
            && h.get("executionOwner").and_then(V::as_str) == Some("Codex")
            && h.get("writerStopped") == Some(&V::Bool(true))
            && h.get("inFlightResolved") == Some(&V::Bool(true))
            && h.get("formerReadsPerMinute").and_then(V::as_i64) == Some(0)
            && h.get("formerWritesPerMinute").and_then(V::as_i64) == Some(0),
        "HANDOFF_RECORD",
    )?;
    if a.get("authorityKind").and_then(V::as_str) == Some("direct_human_assignment") {
        let u = h.get("humanAuthority").cloned().unwrap_or(V::Null);
        let s = h.get("writerStopEvidence").cloned().unwrap_or(V::Null);
        let r = h.get("reconciliation").cloned().unwrap_or(V::Null);
        need(
            h.get("schemaVersion").and_then(V::as_i64) == Some(2)
                && h.get("releaseCommentId").is_none()
                && h.get("releasedAt").is_none()
                && u.get("kind").and_then(V::as_str) == Some("direct_human_message")
                && u.get("commentId") == a.get("assignmentCommentId")
                && u.get("threadId").and_then(V::as_str) == Some(HUMAN_THREAD)
                && u.get("quote").and_then(V::as_str).is_some_and(|q| !q.is_empty() && q.len() <= 2000)
                && stamp(u.get("recordedAt").unwrap_or(&V::Null)),
            "HUMAN_AUTHORITY",
        )?;
        // Chat activity can be unrelated human work. Record the Cup writer's
        // stop evidence explicitly; an idle chat alone is not a process-stop
        // proof.
        need(
            comment(s.get("commentId").unwrap_or(&V::Null))
                && s.get("cupWriterReportedStopped") == Some(&V::Bool(true))
                && s.get("cupInFlightReportedClear") == Some(&V::Bool(true))
                && stamp(s.get("reportedAt").unwrap_or(&V::Null))
                && stamp(s.get("checkedAt").unwrap_or(&V::Null))
                && s.get("threadId").and_then(V::as_str) == Some(STOP_THREAD)
                && ["idle", "active"].contains(&s.get("threadStatus").and_then(V::as_str).unwrap_or(""))
                && ms(s.get("reportedAt").unwrap()) <= ms(s.get("checkedAt").unwrap())
                && ms(s.get("checkedAt").unwrap()) >= ms(u.get("recordedAt").unwrap()),
            "WRITER_STOP_EVIDENCE",
        )?;
        need(
            r.get("complete") == Some(&V::Bool(true))
                && r.get("tournamentId").and_then(V::as_str) == Some(crate::TOURNAMENT_ID)
                && hash(r.get("receiptHash").unwrap_or(&V::Null))
                && stamp(r.get("oldestReceivedAt").unwrap_or(&V::Null))
                && stamp(r.get("newestReceivedAt").unwrap_or(&V::Null))
                && ms(r.get("oldestReceivedAt").unwrap()) <= ms(r.get("newestReceivedAt").unwrap())
                && ms(r.get("newestReceivedAt").unwrap()) <= ms(h.get("reconciledAt").unwrap())
                && ms(a.get("startedAt").unwrap()) - ms(r.get("oldestReceivedAt").unwrap()) <= 120_000,
            "RECONCILIATION",
        )?;
        need(
            stamp(h.get("assignedAt").unwrap_or(&V::Null))
                && h.get("assignedAt") == u.get("recordedAt")
                && ms(h.get("assignedAt").unwrap()) <= ms(h.get("reconciledAt").unwrap())
                && ms(s.get("checkedAt").unwrap()) <= ms(h.get("reconciledAt").unwrap()),
            "ASSIGNMENT_CLOCK",
        )?;
    } else {
        need(
            h.get("schemaVersion").and_then(V::as_i64) == Some(1)
                && stamp(h.get("releasedAt").unwrap_or(&V::Null))
                && ms(h.get("releasedAt").unwrap()) <= ms(h.get("reconciledAt").unwrap()),
            "RELEASE_CLOCK",
        )?;
    }
    need(
        stamp(h.get("reconciledAt").unwrap_or(&V::Null))
            && stamp(h.get("acknowledgedAt").unwrap_or(&V::Null))
            && ms(h.get("reconciledAt").unwrap()) <= ms(h.get("acknowledgedAt").unwrap())
            && ms(h.get("acknowledgedAt").unwrap()) <= ms(a.get("startedAt").unwrap()),
        "HANDOFF_CLOCK",
    )?;
    Ok(h.clone())
}
pub fn production_handoff_current(runtime_dir: &Path, a: &V) -> bool {
    std::fs::read_to_string(runtime_dir.join("production-handoff.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<V>(&s).ok())
        .and_then(|h| validate_production_handoff(&h, a).ok())
        .is_some()
}

pub fn probability_receipt_policy() -> V {
    serde_json::from_str(PROBABILITY_RECEIPT_POLICY_JSON).unwrap()
}
pub const PROBABILITY_RECEIPT_POLICY_JSON: &str = r#"{"cumulativeBuyBudget":"100.000000","latenciesMs":[1000,3000],"maxHoldMs":172800000,"maxMarketCost":"1.000000","maxOpenMarkets":3,"maximumAccountAgeMs":20000,"maximumResearchAgeMs":21600000,"maximumSpread":"0.020000","minimumLowerEdge":"0.020000","takeProfit":"0.020000"}"#;

// live-paper/probability-receipt.mjs (bank validator only — the paper engine's
// own validators that production activation binds to).
pub fn validate_probability_receipt_bank(bank: &V) -> Result<(), String> {
    let n = |ok: bool, code: &str| -> Result<(), String> {
        if ok { Ok(()) } else { Err(format!("PROBABILITY_RECEIPT_{code}")) }
    };
    n(
        bank.get("schemaVersion").and_then(V::as_i64) == Some(2)
            && bank.get("mode").and_then(V::as_str) == Some("source_probability_actual_receipt_paper")
            && digest(bank.get("policy").unwrap_or(&V::Null)) == digest(&probability_receipt_policy()),
        "BANK",
    )?;
    validate_config(bank.get("config").unwrap_or(&V::Null))
        .map_err(|_| "PROBABILITY_RECEIPT_FILL_ASSUMPTIONS".to_string())?;
    let c = bank.get("config").unwrap();
    n(
        c.get("feeBps").and_then(V::as_i64) == Some(0)
            && c.get("depthFractionBps").and_then(V::as_i64) == Some(100)
            && c.get("slippageTicks").and_then(V::as_i64) == Some(1)
            && c.get("orderShares").and_then(V::as_i64) == Some(1)
            && c.get("maxPositionShares").and_then(V::as_i64) == Some(1)
            && c.get("initialCash").and_then(V::as_str) == Some("100.000000")
            && c.get("cashReserve").and_then(V::as_str) == Some("90.000000")
            && c.get("maxExposure").and_then(V::as_str) == Some("10.000000")
            && c.get("stopLoss").and_then(V::as_str) == Some("0.050000")
            && c.get("strategyUniverse").and_then(V::as_str) == Some("diversified-dip-v1")
            && c.get("maxDataAgeMs").and_then(V::as_i64) == Some(20000)
            && c.get("marketIds").and_then(V::as_array).map(|a| a.len())
                == bank.get("research").and_then(V::as_array).map(|a| a.len()),
        "FILL_ASSUMPTIONS",
    )?;
    let research = bank.get("research").and_then(V::as_array).cloned().unwrap_or_default();
    n(
        !research.is_empty()
            && research.len() <= 3
            && bank.get("identities").and_then(V::as_object).map(|m| m.len()) == Some(research.len()),
        "RESEARCH_SET",
    )?;
    let mut seen = HashSet::new();
    for r in &research {
        validate_research(r)?;
        let market_id = r.get("marketId").and_then(V::as_str).unwrap_or("");
        let spec = bank.pointer(&format!("/identities/{market_id}"));
        n(
            seen.insert(market_id.to_string())
                && instant(r.get("recordedAt").and_then(V::as_str).unwrap_or(""))
                    .unwrap_or(i64::MAX)
                    <= instant(bank.get("cutoff").and_then(V::as_str).unwrap_or("")).unwrap_or(i64::MIN)
                && spec.and_then(|s| s.get("exchangeId")).and_then(V::as_str)
                    == r.get("exchangeId").and_then(V::as_str)
                && spec.and_then(|s| s.get("predicateHash")).and_then(V::as_str)
                    == r.get("predicateHash").and_then(V::as_str)
                && bank.pointer("/universe/exchanges").and_then(V::as_array).is_some_and(|xs| {
                    xs.iter().any(|x| {
                        x.get("marketId").and_then(V::as_str) == Some(market_id)
                            && x.get("exchangeId").and_then(V::as_str)
                                == r.get("exchangeId").and_then(V::as_str)
                            && x.get("option").and_then(V::as_str) == Some("YES")
                    })
                })
                && c.get("marketIds").and_then(V::as_array).is_some_and(|ids| {
                    ids.iter().any(|i| i.as_str() == Some(market_id))
                }),
            "RESEARCH_BINDING",
        )?;
    }
    Ok(())
}
fn validate_config(c: &V) -> Result<(), String> {
    let int = |k: &str| c.get(k).and_then(V::as_i64).filter(|n| *n >= 0);
    if c.get("schemaVersion").and_then(V::as_i64) != Some(1)
        || amount(c.get("initialCash").and_then(V::as_str).unwrap_or("")).unwrap_or(0) <= 0
    {
        return Err("Invalid paper configuration".into());
    }
    for k in ["readIntervalMs", "cycleIntervalMs", "maxDataAgeMs", "latencyMs", "feeBps", "slippageTicks", "depthFractionBps", "orderShares", "maxPositionShares", "warmupObservations", "lookback", "exitAfterMs", "port"] {
        if int(k).is_none() {
            return Err(format!("Invalid {k}"));
        }
    }
    let g = |k: &str| int(k).unwrap();
    if g("readIntervalMs") < 5000
        || g("cycleIntervalMs") < 10000
        || g("maxDataAgeMs") < 1000
        || g("orderShares") < 1
        || g("maxPositionShares") < g("orderShares")
        || g("warmupObservations") < 2
        || g("lookback") < g("warmupObservations")
        || g("depthFractionBps") < 1
        || g("depthFractionBps") > 1000
        || g("feeBps") > 10000
        || g("slippageTicks") > 5
        || g("latencyMs") < 1
        || !(1024..=65535).contains(&g("port"))
        || g("exitAfterMs") < 1000
    {
        return Err("Unsafe paper limits".into());
    }
    let diversified = ["diversified-dip-v1", "diversified-dip-scale-v1"]
        .contains(&c.get("strategyUniverse").and_then(V::as_str).unwrap_or(""));
    let scaled = c.get("strategyUniverse").and_then(V::as_str) == Some("diversified-dip-scale-v1");
    if c.get("strategyUniverse").is_some() && !diversified {
        return Err("Unknown strategy universe".into());
    }
    let initial = amount(c.get("initialCash").and_then(V::as_str).unwrap_or("")).unwrap_or(0);
    let max_pos = g("maxPositionShares");
    if diversified
        && (g("feeBps") != 0
            || max_pos > if scaled { 2000 } else { 200 }
            || amount(c.get("maxExposure").and_then(V::as_str).unwrap_or("")).unwrap_or(i128::MAX)
                > initial / if scaled { 2 } else { 10 }
            || amount(c.get("cashReserve").and_then(V::as_str).unwrap_or("")).unwrap_or(i128::MAX)
                < initial * if scaled { 5 } else { 9 } / 10)
    {
        return Err("Unsafe diversified paper scope".into());
    }
    let ids = c.get("marketIds").and_then(V::as_array).cloned().unwrap_or_default();
    if ids.is_empty()
        || ids.len() > if diversified { 300 } else { 8 }
        || ids.iter().collect::<HashSet<_>>().len() != ids.len()
        || ids.iter().any(|x| !x.as_str().is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())))
    {
        return Err("Invalid market scope".into());
    }
    if c.get("tournamentId").and_then(V::as_str) != Some(crate::TOURNAMENT_ID)
        || c.get("tournamentSlug").and_then(V::as_str) != Some("midterm-elections")
    {
        return Err("Invalid tournament scope".into());
    }
    let exposure = amount(c.get("maxExposure").and_then(V::as_str).unwrap_or("")).unwrap_or(0);
    let reserve = amount(c.get("cashReserve").and_then(V::as_str).unwrap_or("")).unwrap_or(-1);
    if !c.get("entryZ").and_then(V::as_f64).is_some_and(|z| z.is_finite() && z >= 0.0)
        || amount(c.get("minEdge").and_then(V::as_str).unwrap_or("")).unwrap_or(0) <= 0
        || amount(c.get("stopLoss").and_then(V::as_str).unwrap_or("")).unwrap_or(0) <= 0
        || exposure <= 0
        || exposure > initial
        || reserve < 0
        || reserve >= initial
    {
        return Err("Invalid strategy risk parameters".into());
    }
    Ok(())
}
fn validate_research(r: &V) -> Result<(), String> {
    let keys = [
        "schemaVersion", "id", "marketId", "exchangeId", "side", "predicateHash",
        "recordedAt", "sourceAt", "expiresAt", "lower", "upper", "method",
        "settlementReviewed", "flags", "sources",
    ];
    let mut actual: Vec<&str> = r.as_object().map(|m| m.keys().map(|s| s.as_str()).collect()).unwrap_or_default();
    actual.sort();
    let mut expected: Vec<&str> = keys.to_vec();
    expected.sort();
    if actual != expected {
        return Err("Unexpected research fields".into());
    }
    let time = |v: &V| -> Result<f64, String> {
        v.as_str()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|d| d.timestamp_millis() as f64)
            .ok_or("Invalid research time".to_string())
    };
    let price_ok = |v: &V| {
        v.as_str().is_some_and(|s| {
            s == "1.000000"
                || (s.starts_with("0.") && s.len() == 8 && s[2..].bytes().all(|b| b.is_ascii_digit()))
        })
    };
    if !(r.get("schemaVersion").and_then(V::as_i64) == Some(1)
        && r.get("id").and_then(V::as_str).is_some_and(|s| {
            !s.is_empty() && s.len() <= 80 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        })
        && r.get("marketId").and_then(V::as_str).is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
        && r.get("exchangeId").and_then(V::as_str).is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
        && r.get("side").and_then(V::as_str) == Some("YES")
        && r.get("predicateHash").and_then(V::as_str).is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()))
        && ["subjective_range", "calibrated_model"].contains(&r.get("method").and_then(V::as_str).unwrap_or(""))
        && r.get("settlementReviewed").and_then(V::as_bool).is_some())
    {
        return Err("Invalid research identity".into());
    }
    if !(price_ok(r.get("lower").unwrap_or(&V::Null))
        && price_ok(r.get("upper").unwrap_or(&V::Null))
        && amount(r.get("lower").and_then(V::as_str).unwrap_or("")).unwrap_or(i128::MAX)
            <= amount(r.get("upper").and_then(V::as_str).unwrap_or("")).unwrap_or(i128::MIN))
    {
        return Err("Invalid probability interval".into());
    }
    if !(time(r.get("sourceAt").unwrap_or(&V::Null))? <= time(r.get("recordedAt").unwrap_or(&V::Null))?
        && time(r.get("expiresAt").unwrap_or(&V::Null))? > time(r.get("recordedAt").unwrap_or(&V::Null))?)
    {
        return Err("Invalid research chronology".into());
    }
    let flags = r.get("flags").and_then(V::as_array).cloned().unwrap_or_default();
    let sources = r.get("sources").and_then(V::as_array).cloned().unwrap_or_default();
    if !(r.get("flags").is_some_and(V::is_array)
        && flags.iter().all(|f| {
            ["predicate_uncertain", "settlement_uncertain", "source_conflict", "superseded"]
                .contains(&f.as_str().unwrap_or(""))
        })
        && flags.iter().collect::<HashSet<_>>().len() == flags.len()
        && r.get("sources").is_some_and(V::is_array)
        && !sources.is_empty())
    {
        return Err("Invalid research evidence".into());
    }
    for s in &sources {
        let mut k: Vec<&str> = s.as_object().map(|m| m.keys().map(|x| x.as_str()).collect()).unwrap_or_default();
        k.sort();
        if k != ["contentHash", "url"] {
            return Err("Invalid research reference".into());
        }
        let url = s.get("url").and_then(V::as_str).unwrap_or("");
        let url_ok = url.starts_with("https://")
            && !url.contains('?')
            && !url.contains('#')
            && {
                let rest = &url[8..];
                !rest.contains('@')
                    && rest.split('/').next().is_some_and(|h| !h.is_empty() && !h.contains(':'))
            }
            && !url.to_lowercase().contains("ace_")
            && !{
                // /ace_[a-f0-9]{20,}/i
                let lower = url.to_lowercase();
                lower.match_indices("ace_").any(|(i, _)| {
                    lower[i + 4..].chars().take_while(|c| c.is_ascii_hexdigit()).count() >= 20
                })
            };
        if !(url_ok
            && s.get("contentHash").and_then(V::as_str).is_some_and(|h| {
                h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            }))
        {
            return Err("Invalid research reference".into());
        }
    }
    Ok(())
}

// execution/production-reconciliation.mjs
const FILLS_BASE: &str = "/tournaments/midterm-elections/portfolio/fills?limit=100";
fn recon_need(ok: bool, code: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(format!("PRODUCTION_RECONCILIATION_{code}")) }
}
pub fn fills_read_authorized(spec: &V) -> bool {
    spec.get("method").and_then(V::as_str) == Some("GET")
        && spec.get("body").is_none()
        && spec.get("bodyJson").is_none()
        && spec.get("endpoint").and_then(V::as_str).is_some_and(|e| {
            if e == FILLS_BASE {
                return true;
            }
            e.strip_prefix(&format!("{FILLS_BASE}&cursor=")).is_some_and(|c| {
                !c.is_empty()
                    && c.bytes().all(|b| {
                        b.is_ascii_alphanumeric() || matches!(b, b'%' | b'.' | b'_' | b'~' | b'-')
                    })
            })
        })
}
pub fn reconcile_production_account(
    transport: &crate::transport::OrderTransport,
    dispatch: &dyn Fn(&V) -> Result<V, String>,
    universe: &V,
    now: &dyn Fn() -> i64,
    max_pages: usize,
) -> Result<V, String> {
    let history = transport.orders("all").map_err(|e| format!("PRODUCTION_RECONCILIATION_{}", e.code))?;
    let mut fills: Vec<V> = Vec::new();
    let mut pages: Vec<V> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut cursors: HashSet<String> = HashSet::new();
    let mut cursor: Option<String> = None;
    let mut sequence = i64::MIN;
    let mut first_rows_hash = String::new();
    let mut first_has_more = false;
    let universe_exchanges = universe.get("exchanges").and_then(V::as_array).cloned().unwrap_or_default();
    for page in 0..max_pages {
        let endpoint = match &cursor {
            Some(c) => format!("{FILLS_BASE}&cursor={}", encode(c)),
            None => FILLS_BASE.to_string(),
        };
        let spec = json!({"method": "GET", "endpoint": endpoint});
        recon_need(fills_read_authorized(&spec), "FILL_ROUTE")?;
        let r = dispatch(&spec)?;
        let p = account_fills(&r)?;
        let coverage = r.pointer("/data/coverage").cloned().unwrap_or(V::Null);
        let pg = r.pointer("/data/pagination").cloned().unwrap_or(V::Null);
        recon_need(
            r.get("status").and_then(V::as_i64).is_some_and(|s| (200..300).contains(&s))
                && coverage.get("complete") == Some(&V::Bool(true)),
            "FILL_COVERAGE",
        )?;
        recon_need(
            pg.get("limit").and_then(V::as_i64) == Some(100)
                && baal_service_runtime::json_text::js_int(coverage.get("projectedThroughSequence").unwrap_or(&V::Null))
                    .is_some_and(|n| n >= 0),
            "FILL_PAGINATION",
        )?;
        if page == 0 {
            first_rows_hash = payload_hash(p.get("rows").unwrap_or(&V::Null)).map_err(|e| e.clone())?;
            first_has_more = p.get("hasMore") == Some(&V::Bool(true));
        } else {
            recon_need(
                baal_service_runtime::json_text::js_int(coverage.get("projectedThroughSequence").unwrap_or(&V::Null)).unwrap() >= sequence,
                "FILL_PROJECTION_REGRESSED",
            )?;
        }
        sequence = baal_service_runtime::json_text::js_int(coverage.get("projectedThroughSequence").unwrap_or(&V::Null)).unwrap();
        for f in p.get("rows").and_then(V::as_array).cloned().unwrap_or_default() {
            let id = f.get("id").and_then(|v| v.as_str().map(str::to_string).or(v.as_i64().map(|n| n.to_string()))).unwrap_or_default();
            recon_need(
                !seen.contains(&id)
                    && universe_exchanges.iter().any(|x| {
                        x.get("exchangeId") == f.get("exchangeId") && x.get("marketId") == f.get("marketId")
                    }),
                "FILL_IDENTITY",
            )?;
            seen.insert(id);
            fills.push(f);
        }
        pages.push(json!({
            "receivedAt": r.get("receivedAt").unwrap().clone(),
            "bodyHash": payload_hash(r.get("data").unwrap_or(&V::Null)).map_err(|e| e.clone())?,
            "coverage": coverage,
        }));
        if pg.get("hasMore") != Some(&V::Bool(true)) {
            recon_need(pg.get("nextCursor").map(|c| c.is_null()).unwrap_or(true), "FILL_PAGINATION")?;
            break;
        }
        let next = pg.get("nextCursor").and_then(V::as_str);
        recon_need(
            !p.get("rows").and_then(V::as_array).map(|a| a.is_empty()).unwrap_or(true)
                && next.is_some_and(|c| !c.is_empty() && !cursors.contains(c)),
            "FILL_PAGINATION",
        )?;
        cursors.insert(next.unwrap().to_string());
        cursor = Some(next.unwrap().to_string());
        recon_need(page + 1 < max_pages, "PAGE_LIMIT")?;
    }
    // The projection watermark can advance from other market/account activity.
    // Verify this account's immutable newest fills directly after the full
    // scan.
    let head = dispatch(&json!({"method": "GET", "endpoint": FILLS_BASE}))?;
    let head_view = account_fills(&head)?;
    recon_need(
        head.get("status").and_then(V::as_i64).is_some_and(|s| (200..300).contains(&s))
            && payload_hash(head_view.get("rows").unwrap_or(&V::Null)).ok().as_deref() == Some(first_rows_hash.as_str())
            && head_view.get("hasMore") == Some(&V::Bool(first_has_more))
            && baal_service_runtime::json_text::js_int(head.pointer("/data/coverage/projectedThroughSequence").unwrap_or(&V::Null))
                .is_some_and(|n| n >= sequence),
        "ACCOUNT_FILLS_CHANGED",
    )?;
    pages.push(json!({
        "receivedAt": head.get("receivedAt").unwrap().clone(),
        "bodyHash": payload_hash(head.get("data").unwrap_or(&V::Null)).map_err(|e| e.clone())?,
        "coverage": head.pointer("/data/coverage").unwrap().clone(),
        "kind": "newest_fills_recheck",
    }));
    // Refresh current exposure after the historical scans, then cash last. Old
    // history is not misrepresented as an atomic account snapshot.
    let positions = transport.positions().map_err(|e| format!("PRODUCTION_RECONCILIATION_{}", e.code))?;
    let orders = transport.orders("open").map_err(|e| format!("PRODUCTION_RECONCILIATION_{}", e.code))?;
    let cash = transport.tournament().map_err(|e| format!("PRODUCTION_RECONCILIATION_{}", e.code))?;
    let at = chrono::DateTime::from_timestamp_millis(now())
        .unwrap()
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    recon_need(
        cash.pointer("/data/status").and_then(V::as_str) == Some("active")
            && cash.pointer("/data/isPendingEnrolment") == Some(&V::Bool(false)),
        "TOURNAMENT",
    )?;
    let mut times: Vec<i64> = [
        positions.get("receivedAt").and_then(V::as_str),
        orders.get("receivedAt").and_then(V::as_str),
        cash.get("receivedAt").and_then(V::as_str),
        pages.first().and_then(|p| p.get("receivedAt")).and_then(V::as_str),
    ]
    .iter()
    .filter_map(|t| t.and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok()).map(|d| d.timestamp_millis()))
    .collect();
    times.sort();
    let at_ms = now();
    recon_need(
        times.len() == 4
            && times.iter().all(|t| *t <= at_ms)
            && at_ms - times[0] <= 90000,
        "FRESHNESS",
    )?;
    let receipt = json!({
        "schemaVersion": 1,
        "tournamentId": crate::TOURNAMENT_ID,
        "complete": true,
        "at": at,
        "oldestReceivedAt": chrono::DateTime::from_timestamp_millis(times[0]).unwrap().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "newestReceivedAt": chrono::DateTime::from_timestamp_millis(*times.last().unwrap()).unwrap().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "positions": positions,
        "orders": orders,
        "cash": cash,
        "history": history,
        "fills": {"complete": true, "rows": fills, "pages": pages},
        "cashFloor": format_micros(conservative_cash_floor(cash.pointer("/data/myBalance").unwrap_or(&V::Null))?),
        "limitations": [
            "Sequential complete reads, not an atomic exchange snapshot. Each dispatch reconciles current cash/positions/orders again.",
            "Existing orders and inventory retain their original attribution."
        ],
    });
    let receipt_hash = payload_hash(&receipt)?;
    Ok(json!({
        "receipt": receipt,
        "receiptHash": receipt_hash.clone(),
        "summary": {
            "complete": true,
            "tournamentId": crate::TOURNAMENT_ID,
            "oldestReceivedAt": receipt.get("oldestReceivedAt").unwrap().clone(),
            "newestReceivedAt": receipt.get("newestReceivedAt").unwrap().clone(),
            "receiptHash": receipt_hash,
            "positions": positions.pointer("/data/positions").and_then(V::as_array).map(|a| a.iter().filter(|x| x.get("quantity").and_then(V::as_i64).unwrap_or(0) != 0).count()).unwrap_or(0),
            "openOrders": orders.pointer("/data/data").and_then(V::as_array).map(|a| a.len()).unwrap_or(0),
            "historicalOrders": history.pointer("/data/data").and_then(V::as_array).map(|a| a.len()).unwrap_or(0),
            "fills": fills.len(),
        },
    }))
}
fn encode(s: &str) -> String {
    let mut out = String::new();
    for &b in s.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

// execution/receipt-production-bank.mjs
pub const RECEIPT_PRODUCTION_POLICY: &str =
    r#"{"cooldownMs":900000,"maximumHoldMs":172800000,"orderShares":1,"stopLoss":"0.050000","takeProfit":"0.020000"}"#;
pub fn prepare_receipt_production_bank(
    paper_bank: &V,
    native_backend: &V,
    contracts: &V,
) -> Result<V, String> {
    let n = |ok: bool, code: &str| -> Result<(), String> {
        if ok { Ok(()) } else { Err(format!("RECEIPT_PRODUCTION_{code}")) }
    };
    validate_probability_receipt_bank(paper_bank)
        .map_err(|e| if e.starts_with("PROBABILITY_RECEIPT_") { e } else { format!("RECEIPT_PRODUCTION_{e}") })?;
    n(
        native_backend.get("schemaVersion").and_then(V::as_i64) == Some(1)
            && native_backend.get("mode").and_then(V::as_str) == Some("native_public_book_prefix_v1")
            && ["binaryHash", "engineHash", "runtimeLibraryHash"].iter().all(|k| {
                native_backend.get(*k).and_then(V::as_str).is_some_and(|h| {
                    h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
                })
            }),
        "NATIVE_BINDING",
    )?;
    let research = paper_bank.get("research").and_then(V::as_array).cloned().unwrap_or_default();
    let mut candidates = Vec::new();
    for r in &research {
        let market_id = r.get("marketId").and_then(V::as_str).unwrap_or("");
        let contract = contracts.get(market_id).cloned().unwrap_or(V::Null);
        n(
            r.get("side").and_then(V::as_str) == Some("YES")
                && contract.get("exchangeId").and_then(V::as_str) == r.get("exchangeId").and_then(V::as_str)
                && contract.get("predicateHash").and_then(V::as_str) == r.get("predicateHash").and_then(V::as_str)
                && contract.get("rootHash").and_then(V::as_str).is_some_and(|h| {
                    h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
                })
                && baal_service_runtime::json_text::js_int(contract.get("settlesAtMs").unwrap_or(&V::Null)).is_some(),
            "CONTRACT_BINDING",
        )?;
        candidates.push(json!({
            "id": format!("{market_id}:YES"),
            "marketId": market_id,
            "exchangeId": r.get("exchangeId").unwrap().clone(),
            "side": r.get("side").unwrap().clone(),
            "predicateHash": r.get("predicateHash").unwrap().clone(),
            "riskGroups": ["all", format!("market_{market_id}")],
            "settlesAtMs": contract.get("settlesAtMs").unwrap().clone(),
            "research": {
                "id": r.get("id").unwrap().clone(),
                "predicateHash": r.get("predicateHash").unwrap().clone(),
                "side": r.get("side").unwrap().clone(),
                "lower": r.get("lower").unwrap().clone(),
                "upper": r.get("upper").unwrap().clone(),
                "recordedAtMs": instant(r.get("recordedAt").and_then(V::as_str).unwrap_or(""))?,
                "sourceAtMs": instant(r.get("sourceAt").and_then(V::as_str).unwrap_or(""))?,
                "expiresAtMs": instant(r.get("expiresAt").and_then(V::as_str).unwrap_or(""))?,
                "method": r.get("method").unwrap().clone(),
                "settlementReviewed": r.get("settlementReviewed").unwrap().clone(),
                "flags": r.get("flags").unwrap().clone(),
                "sources": r.get("sources").unwrap().clone(),
            },
        }));
    }
    let policy_obj: V = serde_json::from_str(RECEIPT_PRODUCTION_POLICY).unwrap();
    let research_hashes: Vec<V> = research
        .iter()
        .map(|r| json!({"id": r.get("id").unwrap().clone(), "hash": digest(r)}))
        .collect();
    let mut group_caps = Map::new();
    group_caps.insert("all".into(), json!("10.000000"));
    for c in &candidates {
        group_caps.insert(
            format!("market_{}", c.get("marketId").and_then(V::as_str).unwrap()),
            json!("1.000000"),
        );
    }
    let mut contract_map = Map::new();
    for c in &candidates {
        let market_id = c.get("marketId").and_then(V::as_str).unwrap();
        contract_map.insert(
            market_id.to_string(),
            json!({
                "rootHash": contracts.get(market_id).unwrap().get("rootHash").unwrap().clone(),
                "exchangeId": c.get("exchangeId").unwrap().clone(),
                "predicateHash": c.get("predicateHash").unwrap().clone(),
            }),
        );
    }
    Ok(json!({
        "schemaVersion": 2,
        "mode": "baal_probability_receipt_production_v1",
        "cutoff": paper_bank.get("cutoff").unwrap().clone(),
        "nativeBackend": native_backend.clone(),
        "paperBinding": {
            "bankHash": digest(paper_bank),
            "policyHash": digest(paper_bank.get("policy").unwrap_or(&V::Null)),
            "configHash": digest(paper_bank.get("config").unwrap_or(&V::Null)),
            "cutoff": paper_bank.get("cutoff").unwrap().clone(),
            "researchHashes": research_hashes,
        },
        "receiptPolicy": policy_obj,
        "policy": {
            "minimumEdge": paper_bank.pointer("/policy/minimumLowerEdge").unwrap().clone(),
            "maximumSpread": paper_bank.pointer("/policy/maximumSpread").unwrap().clone(),
            "maximumBookAgeMs": paper_bank.pointer("/config/maxDataAgeMs").unwrap().clone(),
            "maximumResearchAgeMs": paper_bank.pointer("/policy/maximumResearchAgeMs").unwrap().clone(),
            "fractionalKellyBps": 2500,
            "slippageTicks": paper_bank.pointer("/config/slippageTicks").unwrap().clone(),
            "depthFractionBps": paper_bank.pointer("/config/depthFractionBps").unwrap().clone(),
            "maxOrderCost": "1.000000",
            "maxExposure": "10.000000",
            "cashReserve": "90.000000",
            "cumulativeBuyBudget": "25.000000",
            "groupCaps": V::Object(group_caps),
        },
        "contracts": V::Object(contract_map),
        "candidates": candidates,
    }))
}
