// execution/median-controller.mjs — median-receipt production controller.
// MedianCompetitionController extends CompetitionController in JS; here it
// wraps the base and installs its gate/bootstrap overrides on the shared
// controller state the transport closures read.
use crate::controller::{
    entry_window_gate, CompetitionController, ControllerOptions,
};
use crate::core::{payload_hash, prepare_intent};
use crate::median::{
    MedianReferenceReader, MEDIAN_REFERENCE_POLICY, MEDIAN_TID,
};
use baal_service_runtime::atomic::atomic_json;
use baal_service_runtime::fixed::{amount, TICK};
use baal_service_runtime::journal::Journal;
use baal_service_runtime::json_text::{digest, js_int};
use serde_json::{json, Value as V};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

const NEED: &str = "MEDIAN_EXECUTION_";
fn need(ok: bool, code: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(format!("{NEED}{code}"))
    }
}
fn micros(v: &V) -> i128 {
    amount(&crate::js_key(v)).unwrap_or(i128::MIN)
}
fn instant(v: &V) -> i64 {
    crate::controller::parse(v)
}
fn iso(n: i64) -> String {
    crate::controller::iso(n)
}
fn stamp(v: &V) -> bool {
    instant(v) != i64::MIN && iso(instant(v)) == v.as_str().unwrap_or("")
}
fn hex64(v: &V) -> bool {
    v.as_str()
        .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()))
}
fn plain(v: &V) -> bool {
    v.is_object()
}

// timestamp(v) + instantNs from feed/contract.mjs: ISO with up to 9 fractional
// digits, parsed to milliseconds plus the sub-millisecond digits.
fn timestamp_iso(v: &V) -> Option<String> {
    let s = v.as_str()?;
    let re = regex_lite_instant(s)?;
    Some(re)
}
fn regex_lite_instant(s: &str) -> Option<String> {
    // ^(\d{4})-(\d\d)-(\d\d)T(\d\d):(\d\d):(\d\d)(?:\.(\d{1,9}))?(Z|([+-])(\d\d):(\d\d))$
    let b = s.as_bytes();
    if b.len() < 20 {
        return None;
    }
    let digs = |a: &[u8]| a.iter().all(|c| c.is_ascii_digit());
    if !(digs(&b[0..4]) && b[4] == b'-' && digs(&b[5..7]) && b[7] == b'-' && digs(&b[8..10])
        && b[10] == b'T' && digs(&b[11..13]) && b[13] == b':' && digs(&b[14..16])
        && b[16] == b':' && digs(&b[17..19]))
    {
        return None;
    }
    let year: i32 = s[0..4].parse().ok()?;
    let month: u32 = s[5..7].parse().ok()?;
    let day: u32 = s[8..10].parse().ok()?;
    let h: u32 = s[11..13].parse().ok()?;
    let mi: u32 = s[14..16].parse().ok()?;
    let se: u32 = s[17..19].parse().ok()?;
    let rest = &s[19..];
    let (fraction, zone) = if let Some(r) = rest.strip_prefix('.') {
        let fend = r.bytes().position(|c| !c.is_ascii_digit()).unwrap_or(r.len());
        (Some(&r[..fend]), &r[fend..])
    } else {
        (None, rest)
    };
    let fraction = fraction.unwrap_or("");
    if fraction.len() > 9 {
        return None;
    }
    let (mut oh, mut om) = (0i32, 0i32);
    if zone != "Z" {
        let zb = zone.as_bytes();
        if zb.len() != 6 || (zb[0] != b'+' && zb[0] != b'-') || zb[3] != b':' || !digs(&zb[1..3]) || !digs(&zb[4..6]) {
            return None;
        }
        oh = zone[1..3].parse().ok()?;
        om = zone[4..6].parse().ok()?;
        if zb[0] == b'-' {
            oh = -oh;
            om = -om;
        }
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    if year < 1970 || !(1..=12).contains(&month) || day < 1 || day > days[(month - 1) as usize]
        || h >= 24 || mi >= 60 || se >= 60 || oh.abs() >= 24 || om.abs() >= 60
    {
        return None;
    }
    let ms_frac = format!("{:0<3}", &fraction[..fraction.len().min(3)].to_string())[..3].to_string();
    let naive = format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{}Z",
        year, month, day, h, mi, se, ms_frac
    );
    let ms = chrono::DateTime::parse_from_rfc3339(&naive).ok()?.timestamp_millis() - (oh as i64) * 3600000 - (om as i64) * 60000;
    // Rebuild like JS: new Date(ms).toISOString() then splice fraction digits.
    let base = iso(ms);
    Some(format!("{}.{}Z", &base[..19], format!("{:0<9}", fraction)))
}
pub fn instant_ns(v: &V) -> Option<i128> {
    let iso_s = timestamp_iso(v)?;
    let ms = chrono::DateTime::parse_from_rfc3339(&format!("{}Z", &iso_s[..20])).ok()?.timestamp_millis();
    let ns_tail: i128 = iso_s[23..29].parse().ok()?;
    Some(ms as i128 * 1_000_000 + ns_tail)
}
// Official freshness compares sequence first, then nanosecond asOf.
fn compare_book(a: &V, b: &V) -> i64 {
    let sa: i128 = a.get("sequence").and_then(V::as_str).and_then(|s| s.parse().ok()).unwrap_or(0);
    let sb: i128 = b.get("sequence").and_then(V::as_str).and_then(|s| s.parse().ok()).unwrap_or(0);
    if sa > sb {
        1
    } else if sa < sb {
        -1
    } else {
        let (ta, tb) = (instant_ns(a.get("sourceAt").unwrap_or(&V::Null)), instant_ns(b.get("sourceAt").unwrap_or(&V::Null)));
        match (ta, tb) {
            (Some(ta), Some(tb)) => (ta > tb) as i64 - (ta < tb) as i64,
            _ => 0,
        }
    }
}

pub const MEDIAN_PAPER_POLICY_JSON: &str = r#"{"prospectiveRevision":1,"riskPriority":"stop_or_horizon_before_virtual_profit_fill","riskIntentRefresh":"new_intent_new_clock_and_source_on_bid_below_old_limit","stopIsNotGuaranteedLossBound":true,"entryDipTicks":[1,2,4],"historyMs":3600000,"minimumHistorySpanMs":1800000,"minimumHistoryReceipts":20,"latencyMs":3000,"intentLifetimeMs":120000,"maximumHoldMs":7200000,"lossLimitTicks":4,"profitTargetTicks":1,"cooldownMs":900000,"cashReserve":"90.000000","maxExposure":"3.000000","maxMarkets":3,"cumulativeBuyBudget":"25.000000","initialCash":"100.000000","fees":0,"depthFractionBps":100,"fillAdverseTicks":0,"limitCushionTicks":1}"#;
pub const MEDIAN_EXECUTION_POLICY_JSON: &str = r#"{"orderShares":1,"dipTicks":1,"takeProfitTicks":1,"stopLossTicks":4,"maximumHoldMs":7200000,"cooldownMs":900000,"proposalLifetimeMs":120000,"orderLifetimeMs":30000,"minimumFollowupMs":3000,"depthFractionBps":100,"maximumBookAgeMs":20000,"maximumAccountAgeMs":20000,"limitCushionTicks":1,"fees":0,"executionDifference":"official_order_expires_after_30s_then_reconcile_before_replacement"}"#;
pub fn median_paper_policy() -> V {
    serde_json::from_str(MEDIAN_PAPER_POLICY_JSON).unwrap()
}
pub fn median_execution_policy() -> V {
    serde_json::from_str(MEDIAN_EXECUTION_POLICY_JSON).unwrap()
}

pub fn median_controller_identity(workspace: &Path) -> Result<V, String> {
    crate::frozen::collect_frozen_paper_files(workspace, &["execution/median-controller.mjs"])
}

pub fn validate_median_bank(bank: &V, activation: &V, universe: &V, workspace: &Path) -> Result<V, String> {
    need(
        activation.get("strategyId").and_then(V::as_str) == Some("historical-median-receipt-v1")
            && bank.get("schemaVersion").and_then(V::as_i64) == Some(1)
            && bank.get("mode").and_then(V::as_str) == Some("baal_historical_median_production_v1")
            && digest(bank) == activation.get("strategyBankHash").and_then(V::as_str).unwrap_or(""),
        "BANK",
    )?;
    need(
        bank.get("tournamentId") == activation.get("tournamentId")
            && bank.get("universeHash") == universe.get("hash")
            && universe.get("tournamentId") == activation.get("tournamentId")
            && stamp(bank.get("cutoff").unwrap_or(&V::Null))
            && instant(bank.get("cutoff").unwrap()) <= instant(activation.get("startedAt").unwrap()),
        "BANK_SCOPE",
    )?;
    need(
        hex64(bank.pointer("/paperBinding/bankHash").unwrap_or(&V::Null))
            && bank.pointer("/paperBinding/policyHash").and_then(V::as_str)
                == Some(digest(&median_paper_policy()).as_str())
            && payload_hash(bank.get("policy").unwrap()).ok().as_deref()
                == payload_hash(&median_execution_policy()).ok().as_deref(),
        "POLICY",
    )?;
    let p = bank.get("sourcePrefix").cloned().unwrap_or(V::Null);
    need(
        js_int(p.get("bytes").unwrap_or(&V::Null)).is_some_and(|n| n > 0)
            && js_int(p.get("sequence").unwrap_or(&V::Null)).is_some_and(|n| n > 0)
            && hex64(p.get("sha256").unwrap_or(&V::Null))
            && hex64(p.get("hash").unwrap_or(&V::Null)),
        "SOURCE_PREFIX",
    )?;
    need(
        plain(bank.get("codeIdentity").unwrap_or(&V::Null))
            && payload_hash(bank.get("codeIdentity").unwrap()).ok().as_deref()
                == payload_hash(&median_controller_identity(workspace)?).ok().as_deref(),
        "CODE_IDENTITY",
    )?;
    Ok(bank.clone())
}

pub fn validate_proposals(s: &V, bank_hash: &str, run_id: &str, universe: &V) -> Result<V, String> {
    need(
        plain(s)
            && s.get("schemaVersion").and_then(V::as_i64) == Some(1)
            && s.get("bankHash").and_then(V::as_str) == Some(bank_hash)
            && s.get("runId").and_then(V::as_str) == Some(run_id)
            && plain(s.get("proposals").unwrap_or(&V::Null))
            && plain(s.get("books").unwrap_or(&V::Null)),
        "PROPOSAL_STATE",
    )?;
    let scope: std::collections::BTreeMap<String, String> = universe
        .get("exchanges")
        .and_then(V::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|x| {
                    Some((
                        x.get("marketId").and_then(V::as_str)?.to_string(),
                        x.get("exchangeId").and_then(V::as_str)?.to_string(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    let mut keys = std::collections::BTreeSet::new();
    for (id, p) in s.get("proposals").and_then(V::as_object).cloned().unwrap_or_default() {
        let valid_id = id.len() == 43
            && id.starts_with("median-")
            && id[7..].bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase() || b == b'-');
        need(
            valid_id
                && plain(&p)
                && p.get("intentId").and_then(V::as_str) == Some(id.as_str())
                && scope.get(p.get("marketId").and_then(V::as_str).unwrap_or(""))
                    == p.get("exchangeId").and_then(V::as_str).map(str::to_string).as_ref()
                && p.get("quantity").and_then(V::as_i64) == Some(1)
                && ["buy", "sell"].contains(&p.get("action").and_then(V::as_str).unwrap_or(""))
                && ["proposed", "dispatched", "closed", "revoked", "expired"]
                    .contains(&p.get("status").and_then(V::as_str).unwrap_or(""))
                && p.get("idempotencyKey").and_then(V::as_str).is_some()
                && !keys.contains(p.get("idempotencyKey").and_then(V::as_str).unwrap()),
            "PROPOSAL_IDENTITY",
        )?;
        keys.insert(p.get("idempotencyKey").and_then(V::as_str).unwrap().to_string());
        need(
            stamp(p.get("createdAt").unwrap_or(&V::Null))
                && stamp(p.get("expiresAt").unwrap_or(&V::Null))
                && instant(p.get("expiresAt").unwrap()) - instant(p.get("createdAt").unwrap()) == 120000
                && p.get("bankHash").and_then(V::as_str) == Some(bank_hash)
                && p.pointer("/originalBook/marketId") == p.get("marketId")
                && p.pointer("/originalBook/exchangeId") == p.get("exchangeId")
                && hex64(p.get("proofHash").unwrap_or(&V::Null))
                && p.get("proofHash").and_then(V::as_str)
                    == payload_hash(p.get("proof").unwrap()).ok().as_deref()
                && p.pointer("/proof/bankHash").and_then(V::as_str) == Some(bank_hash)
                && p.pointer("/proof/decisionAvailableAt") == p.get("createdAt")
                && payload_hash(p.pointer("/proof/originalBook").unwrap_or(&V::Null)).ok()
                    == payload_hash(p.get("originalBook").unwrap()).ok(),
            "PROPOSAL_PROOF",
        )?;
        need(
            stamp(p.pointer("/originalBook/requestStartedAt").unwrap_or(&V::Null))
                && stamp(p.pointer("/originalBook/receivedAt").unwrap_or(&V::Null))
                && instant(p.pointer("/originalBook/requestStartedAt").unwrap())
                    <= instant(p.pointer("/originalBook/receivedAt").unwrap())
                && instant(p.pointer("/originalBook/receivedAt").unwrap()) <= instant(p.get("createdAt").unwrap())
                && p.pointer("/originalBook/sequence")
                    .and_then(V::as_str)
                    .is_some_and(|s| s.bytes().all(|b| b.is_ascii_digit()))
                && p.pointer("/originalBook/versionKey").and_then(V::as_str).is_some()
                && p.pointer("/originalBook/sourceAt").and_then(V::as_str).is_some(),
            "PROPOSAL_CLOCK",
        )?;
        let limit = micros(p.get("limit").unwrap_or(&V::Null));
        need(
            limit >= TICK && limit <= 995000 && limit % TICK == 0
                && [
                    "ask_below_earlier_hour_bid_median",
                    "profit_target",
                    "stop_loss",
                    "holding_timeout",
                ]
                .contains(&p.get("reason").and_then(V::as_str).unwrap_or("")),
            "PROPOSAL_TERMS",
        )?;
        need(
            p.get("action").and_then(V::as_str) == Some("sell")
                || p.pointer("/proof/reference/outcome").and_then(V::as_str) == Some("reference"),
            "PROPOSAL_REFERENCE",
        )?;
    }
    let pending: Vec<&V> = s
        .get("proposals")
        .and_then(V::as_object)
        .map(|m| m.values().filter(|p| p.get("status").and_then(V::as_str) == Some("proposed")).collect())
        .unwrap_or_default();
    need(
        pending
            .iter()
            .map(|p| p.get("marketId").and_then(V::as_str).unwrap_or(""))
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            == pending.len(),
        "DUPLICATE_PROPOSAL",
    )?;
    for (market_id, b) in s.get("books").and_then(V::as_object).cloned().unwrap_or_default() {
        need(
            scope.get(&market_id) == b.get("exchangeId").and_then(V::as_str).map(str::to_string).as_ref()
                && b.get("sequence")
                    .and_then(V::as_str)
                    .is_some_and(|s| s.bytes().all(|b| b.is_ascii_digit()))
                && b.get("sourceAt").and_then(V::as_str).is_some()
                && stamp(b.get("receivedAt").unwrap_or(&V::Null))
                && hex64(b.get("depthHash").unwrap_or(&V::Null)),
            "BOOK_STATE",
        )?;
    }
    Ok(s.clone())
}

pub struct MedianState {
    pub bank: V,
    pub bank_file: PathBuf,
    pub proposals: V,
    pub reference: Option<MedianReferenceReader>,
    pub reference_ready: bool,
    pub reference_failure: Option<String>,
    pub candidate_ids: Vec<String>,
    pub last_reference: V,
    pub proposal_journal_sequence: u64,
    pub proposal_journal_previous: String,
}

// One in-process controller, the collector's queue, and the existing execution
// lease own both journals. Proposals are not orders or paper fills.
pub struct MedianCompetitionController {
    pub base: CompetitionController,
    pub m: Rc<RefCell<MedianState>>,
    pub proposal_journal: Rc<RefCell<Journal>>,
    pub workspace: PathBuf,
}

impl MedianCompetitionController {
    pub fn new(options: ControllerOptions, workspace: PathBuf) -> Result<Self, String> {
        let mut base = CompetitionController::new(options)?;
        let universe = base.universe.clone();
        let activation = base.activation.clone();
        let runtime_dir = base.runtime_dir.clone();
        let inner = base.inner.clone();
        let result = (|| -> Result<(Rc<RefCell<MedianState>>, Rc<RefCell<Journal>>), String> {
            let bank_file = runtime_dir
                .join("production-banks")
                .join(format!("{}.json", activation.get("strategyBankHash").and_then(V::as_str).unwrap_or("")));
            let bank = validate_median_bank(
                &serde_json::from_str::<V>(
                    &std::fs::read_to_string(&bank_file).map_err(|e| format!("{NEED}BANK:{e}"))?,
                )
                .map_err(|e| format!("{NEED}BANK:{e}"))?,
                &activation,
                &universe,
                &workspace,
            )?;
            let proposal_journal =
                Rc::new(RefCell::new(Journal::open(&base.dir.join("median-proposals.jsonl"))?));
            let proposals = validate_proposals(
                &proposal_journal
                    .borrow()
                    .last
                    .as_ref()
                    .and_then(|l| l.get("state").cloned())
                    .unwrap_or_else(|| {
                        json!({
                            "schemaVersion": 1,
                            "bankHash": activation.get("strategyBankHash").unwrap().clone(),
                            "runId": activation.get("runId").unwrap().clone(),
                            "proposals": {},
                            "books": {},
                        })
                    }),
                activation.get("strategyBankHash").and_then(V::as_str).unwrap_or(""),
                activation.get("runId").and_then(V::as_str).unwrap_or(""),
                &universe,
            )?;
            for i in inner
                .borrow()
                .state
                .get("intents")
                .and_then(V::as_object)
                .map(|m| m.values().cloned().collect::<Vec<_>>())
                .unwrap_or_default()
            {
                let p = proposals
                    .pointer(&format!("/proposals/{}", i.get("intentId").and_then(V::as_str).unwrap_or("")));
                need(
                    p.is_some_and(|p| p.get("idempotencyKey") == i.get("idempotencyKey"))
                        && i.pointer("/attribution/modelHash") == activation.get("strategyBankHash"),
                    "INTENT_PROPOSAL_MISMATCH",
                )?;
            }
            let (seq, prev) = {
                let j = proposal_journal.borrow();
                (j.sequence, j.previous.clone())
            };
            let m = Rc::new(RefCell::new(MedianState {
                bank,
                bank_file,
                proposals,
                reference: None,
                reference_ready: false,
                reference_failure: None,
                candidate_ids: Vec::new(),
                last_reference: V::Null,
                proposal_journal_sequence: seq,
                proposal_journal_previous: prev,
            }));
            // Install the median overrides on the shared inner state — this is
            // how the JS subclass changes entryReferenceAdmitted.
            {
                let (m2, ws, act, uni) = (m.clone(), workspace.clone(), activation.clone(), universe.clone());
                inner.borrow_mut().entry_reference_gate = Some(Rc::new(
                    move |inner, _activation, _runtime_dir, i: &V, permit: &V, now_ms: i64| {
                        median_reference_gate(&m2, inner, &act, &uni, &ws, i, permit, now_ms)
                    },
                ));
                let first = universe
                    .get("exchanges")
                    .and_then(V::as_array)
                    .and_then(|a| a.first())
                    .and_then(|x| x.get("marketId"))
                    .and_then(V::as_str)
                    .unwrap_or("")
                    .to_string();
                inner.borrow_mut().bootstrap_market_id = Some(Rc::new(move || first.clone()));
            }
            Ok((m, proposal_journal))
        })();
        match result {
            Ok((m, proposal_journal)) => {
                let controller = Self {
                    base,
                    m,
                    proposal_journal,
                    workspace,
                };
                controller.publish();
                Ok(controller)
            }
            Err(e) => {
                let _ = base.close();
                Err(e)
            }
        }
    }
    pub fn bank_current(&self) -> Result<bool, String> {
        validate_median_bank(
            &serde_json::from_str::<V>(
                &std::fs::read_to_string(&self.m.borrow().bank_file).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?,
            &self.base.activation,
            &self.base.universe,
            &self.workspace,
        )?;
        Ok(true)
    }
    pub fn initialize_median_reference(&mut self) -> Result<V, String> {
        if self.m.borrow().reference_ready {
            return Ok(self.m.borrow().reference.as_ref().unwrap().status());
        }
        need(
            self.m.borrow().reference_failure.is_none(),
            "REFERENCE_FAILED_RESTART_REQUIRED",
        )?;
        self.bank_current()?;
        let bank = self.m.borrow().bank.clone();
        let mut reference = MedianReferenceReader::open(
            &self.base.runtime_dir.join("market-prices.jsonl"),
            &self.base.universe,
            bank.get("sourcePrefix").unwrap(),
            bank.get("cutoff").and_then(V::as_str).unwrap_or(""),
        )?;
        let status = reference.refresh()?;
        let mut m = self.m.borrow_mut();
        m.reference = Some(reference);
        m.reference_ready = true;
        m.reference_failure = None;
        Ok(status)
    }
    pub fn eligible_market_ids(&self) -> Vec<String> {
        let mut ids = self.base.managed_market_ids();
        let m = self.m.borrow();
        for p in m
            .proposals
            .get("proposals")
            .and_then(V::as_object)
            .map(|o| o.values().collect::<Vec<_>>())
            .unwrap_or_default()
        {
            if p.get("status").and_then(V::as_str) == Some("proposed") {
                if let Some(id) = p.get("marketId").and_then(V::as_str) {
                    ids.push(id.to_string());
                }
            }
        }
        ids.extend(m.candidate_ids.iter().cloned());
        let mut seen = std::collections::BTreeSet::new();
        ids.into_iter().filter(|id| seen.insert(id.clone())).collect()
    }
    pub fn write_proposals(&self, next: &V, event: &V) -> Result<(), String> {
        let _ = self.base.ledger.borrow().snapshot();
        need(self.base.can_operate(), "STOPPED")?;
        self.bank_current()?;
        let next = validate_proposals(
            next,
            self.base.activation.get("strategyBankHash").and_then(V::as_str).unwrap_or(""),
            self.base.activation.get("runId").and_then(V::as_str).unwrap_or(""),
            &self.base.universe,
        )?;
        self.proposal_journal.borrow_mut().commit(&next, event)?;
        let mut m = self.m.borrow_mut();
        let j = self.proposal_journal.borrow();
        m.proposal_journal_sequence = j.sequence;
        m.proposal_journal_previous = j.previous.clone();
        drop(j);
        m.proposals = next;
        Ok(())
    }
    pub fn update_proposals(&self, mutator: impl FnOnce(&mut V), event: &V) -> Result<(), String> {
        let mut next = self.m.borrow().proposals.clone();
        mutator(&mut next);
        self.write_proposals(&next, event)
    }
    pub fn sync_proposals(&self) -> Result<(), String> {
        let mut changed = false;
        let mut next = self.m.borrow().proposals.clone();
        let now = self.base.now_ms();
        for (id, p) in next
            .get_mut("proposals")
            .and_then(V::as_object_mut)
            .map(|o| o.iter_mut().map(|(k, v)| (k.clone(), v)).collect::<Vec<_>>())
            .unwrap_or_default()
        {
            let i = self
                .base
                .inner
                .borrow()
                .state
                .get("intents")
                .and_then(V::as_object)
                .and_then(|m| m.get(&id).cloned())
                .unwrap_or(V::Null);
            if !i.is_null() {
                let want = if i.get("status").and_then(V::as_str) == Some("closed") {
                    "closed"
                } else {
                    "dispatched"
                };
                if p.get("status").and_then(V::as_str) != Some(want) {
                    p.as_object_mut()
                        .unwrap()
                        .insert("status".into(), json!(want));
                    changed = true;
                }
            } else if p.get("status").and_then(V::as_str) == Some("proposed")
                && now >= instant(p.get("expiresAt").unwrap())
            {
                p.as_object_mut().unwrap().insert("status".into(), json!("expired"));
                changed = true;
            }
        }
        if changed {
            self.write_proposals(&next, &json!({"type": "median_intent_projection", "at": iso(now)}))?;
        }
        Ok(())
    }
    pub fn before_books(&mut self, args: &V) -> Result<(), String> {
        if !self.base.can_operate() {
            return self.base.before_books(args);
        }
        self.bank_current()?;
        {
            let mut m = self.m.borrow_mut();
            if m.reference_failure.is_none() {
                let r = if !m.reference_ready {
                    drop(m);
                    self.initialize_median_reference().map(|_| ())
                } else {
                    m.reference.as_mut().unwrap().refresh().map(|_| ())
                };
                if let Err(e) = r {
                    let mut m = self.m.borrow_mut();
                    let code = e.trim_start_matches(NEED);
                    m.reference_failure = Some(
                        if code.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_') {
                            code.to_string()
                        } else {
                            "MEDIAN_REFERENCE_UNAVAILABLE".into()
                        },
                    );
                    m.reference_ready = false;
                }
            }
        }
        {
            let mut m = self.m.borrow_mut();
            let scope: std::collections::BTreeSet<String> = self
                .base
                .universe
                .get("exchanges")
                .and_then(V::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.get("marketId").and_then(V::as_str).map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let mut seen = std::collections::BTreeSet::new();
            m.candidate_ids = args
                .get("candidateMarketIds")
                .and_then(V::as_array)
                .map(|a| a.iter().filter_map(|x| x.as_str()).collect::<Vec<&str>>())
                .unwrap_or_default()
                .into_iter()
                .filter(|id: &&str| seen.insert(id.to_string()) && scope.contains(*id))
                .map(str::to_string)
                .collect();
        }
        let mut args2 = args.clone();
        args2
            .as_object_mut()
            .unwrap()
            .insert("candidateMarketIds".into(), json!(self.eligible_market_ids()));
        self.base.before_books(&args2)?;
        if self.base.can_operate() {
            self.sync_proposals()?;
        }
        Ok(())
    }
    pub fn before_book(&mut self, market_id: &str) -> Result<(), String> {
        if !self.base.can_write()
            || self.base.active().is_some()
            || !self.base.inner.borrow().selected.contains(&market_id.to_string())
        {
            return Ok(());
        }
        self.bank_current()?;
        let now = self.base.now_ms();
        let clocks: Vec<i64> = {
            let inner = self.base.inner.borrow();
            [
                inner.cash.as_ref().and_then(|c| c.get("at").cloned()),
                inner.account.as_ref().and_then(|a| a.pointer("/positions/at").cloned()),
                inner.account.as_ref().and_then(|a| a.pointer("/orders/at").cloned()),
            ]
            .iter()
            .map(|v| instant(v.as_ref().unwrap_or(&V::Null)))
            .collect()
        };
        if clocks.iter().any(|t| *t == i64::MIN || *t > now || now - *t > 10000) {
            self.base.cash_refresh()?;
            self.base.refresh_account()?;
        }
        Ok(())
    }
    pub fn before_candidate_read(&mut self, market_id: &str) -> Result<bool, String> {
        need(
            self.base
                .universe
                .get("exchanges")
                .and_then(V::as_array)
                .is_some_and(|a| {
                    a.iter()
                        .any(|x| x.get("marketId").and_then(V::as_str) == Some(market_id))
                }),
            "CANDIDATE_SCOPE",
        )?;
        if !self.base.can_operate() {
            return Ok(false);
        }
        if !self.base.inner.borrow().bootstrapped {
            let r = self.base.bootstrap();
            if r.get("complete") != Some(&V::Bool(true)) {
                return Ok(false);
            }
        }
        if self.base.active().is_some() {
            return Ok(false);
        }
        self.bank_current()?;
        // Auxiliary reads follow the cycle's bulk-price publication. Consume
        // that verified frontier before the original book request.
        {
            let mut m = self.m.borrow_mut();
            if m.reference_failure.is_none() {
                let r = if !m.reference_ready {
                    drop(m);
                    self.initialize_median_reference().map(|_| ())
                } else {
                    m.reference.as_mut().unwrap().refresh().map(|_| ())
                };
                if let Err(e) = r {
                    let mut m = self.m.borrow_mut();
                    let code = e.trim_start_matches(NEED);
                    m.reference_failure = Some(
                        if code.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_') {
                            code.to_string()
                        } else {
                            "MEDIAN_REFERENCE_UNAVAILABLE".into()
                        },
                    );
                    m.reference_ready = false;
                }
            }
        }
        let response = self
            .base
            .transport
            .market(market_id)
            .map_err(|e| e.code)?;
        let ctx = response.pointer("/data/contexts/0").cloned().unwrap_or(V::Null);
        if !(ctx.get("status").and_then(V::as_str) == Some("open")
            && ctx.get("settledWith").is_some_and(V::is_null)
            && response.pointer("/data/status").and_then(V::as_str) == Some("open")
            && !response.pointer("/data/isComposite").and_then(V::as_bool).unwrap_or(false)
            && !response.pointer("/data/isMultiOutcome").and_then(V::as_bool).unwrap_or(false))
        {
            return Ok(false);
        }
        {
            let mut inner = self.base.inner.borrow_mut();
            inner.markets.insert(
                market_id.to_string(),
                json!({"at": response.get("receivedAt").unwrap().clone(), "status": "open"}),
            );
            let managed = self.base.managed_market_ids();
            let mut seen2 = std::collections::BTreeSet::new();
            inner.selected = managed
                .into_iter()
                .chain(std::iter::once(market_id.to_string()))
                .filter(|id| seen2.insert(id.clone()))
                .collect();
            let mut m = self.m.borrow_mut();
            let mut ids = m.candidate_ids.clone();
            ids.push(market_id.to_string());
            let mut seen3 = std::collections::BTreeSet::new();
            let ids: Vec<String> = ids.into_iter().filter(|id| seen3.insert(id.clone())).collect();
            m.candidate_ids = if ids.len() > 3 { ids[ids.len() - 3..].to_vec() } else { ids };
        }
        self.before_book(market_id)?;
        Ok(self.base.can_write())
    }
    pub fn evidence_admitted(&self, book: &V, snapshot: &V, now: i64) -> bool {
        if book.is_null() || snapshot.is_null() {
            return false;
        }
        let request = instant(book.get("requestStartedAt").unwrap_or(&V::Null));
        let received = instant(book.get("receivedAt").unwrap_or(&V::Null));
        if request == i64::MIN
            || received == i64::MIN
            || request > received
            || received > now
            || now - received > 20000
            || received - request > 20000
        {
            return false;
        }
        ["cash", "positions", "orders"].iter().all(|k| {
            let p = snapshot.get(k).cloned().unwrap_or(V::Null);
            let at = instant(p.get("at").unwrap_or(&V::Null));
            p.get("tournamentId").and_then(V::as_str) == Some(MEDIAN_TID)
                && at != i64::MIN
                && at <= request
                && now - at <= 20000
                && (*k == "cash"
                    || (p.get("complete") == Some(&V::Bool(true)) && p.get("rows").is_some_and(V::is_array)))
        })
    }
    pub fn depth_admitted(&self, p: &V, book: &V) -> bool {
        book.get(if p.get("action").and_then(V::as_str) == Some("buy") { "asks" } else { "bids" })
            .and_then(V::as_array)
            .is_some_and(|a| {
                a.iter().any(|l| {
                    l.get("quantity").and_then(V::as_i64).unwrap_or(0) >= 100
                        && if p.get("action").and_then(V::as_str) == Some("buy") {
                            micros(l.get("price").unwrap_or(&V::Null)) <= micros(p.get("limit").unwrap_or(&V::Null))
                        } else {
                            micros(l.get("price").unwrap_or(&V::Null)) >= micros(p.get("limit").unwrap_or(&V::Null))
                        }
                })
            })
    }
    pub fn check_progress(&mut self, book: &V) -> Result<(), String> {
        let market_id = book.get("marketId").and_then(V::as_str).unwrap_or("");
        let prior = self
            .m
            .borrow()
            .proposals
            .pointer(&format!("/books/{market_id}"))
            .cloned();
        let depth_hash = payload_hash(&json!({
            "bids": book.get("bids").unwrap().clone(),
            "asks": book.get("asks").unwrap().clone(),
        }))?;
        if let Some(prior) = prior {
            let version = compare_book(book, &prior);
            need(
                instant(book.get("receivedAt").unwrap()) >= instant(prior.get("receivedAt").unwrap())
                    && version >= 0,
                "BOOK_ROLLBACK",
            )?;
            if version == 0 {
                need(
                    depth_hash == prior.get("depthHash").and_then(V::as_str).unwrap_or(""),
                    "BOOK_SEQUENCE_CONFLICT",
                )?;
            }
        }
        let exchange_id = book.get("exchangeId").unwrap().clone();
        let sequence = book.get("sequence").unwrap().clone();
        let source_at = book.get("sourceAt").unwrap().clone();
        let received_at = book.get("receivedAt").unwrap().clone();
        let mid = market_id.to_string();
        self.update_proposals(
            move |s| {
                s.pointer_mut("/books").unwrap().as_object_mut().unwrap().insert(
                    mid.clone(),
                    json!({
                        "exchangeId": exchange_id,
                        "sequence": sequence,
                        "sourceAt": source_at,
                        "receivedAt": received_at,
                        "depthHash": depth_hash,
                    }),
                );
            },
            &json!({"type": "median_book", "marketId": market_id, "at": iso(self.base.now_ms())}),
        )
    }
    pub fn owned_policy(&mut self, book: &V) -> Result<Option<V>, String> {
        let exchange_id = book.get("exchangeId").and_then(V::as_str).unwrap_or("");
        let own = self
            .base
            .inner
            .borrow()
            .state
            .pointer(&format!("/owned/{exchange_id}"))
            .cloned();
        let Some(own) = own else { return Ok(None) };
        let entries: Vec<V> = self
            .base
            .inner
            .borrow()
            .state
            .get("intents")
            .and_then(V::as_object)
            .map(|m| {
                m.values()
                    .filter(|i| {
                        i.get("exchangeId") == book.get("exchangeId")
                            && i.get("action").and_then(V::as_str) == Some("buy")
                            && i.get("filledQuantity").and_then(V::as_i64).unwrap_or(0) > 0
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        let bank_hash = self.base.activation.get("strategyBankHash").and_then(V::as_str).unwrap_or("");
        need(
            own.get("quantity").and_then(V::as_i64) == Some(1)
                && !entries.is_empty()
                && entries
                    .iter()
                    .all(|i| i.pointer("/attribution/modelHash").and_then(V::as_str) == Some(bank_hash)),
            "OWNED_BANK_OR_QUANTITY",
        )?;
        let entry = entries.last().unwrap();
        need(
            entry.get("reconciledFills").and_then(V::as_array).map(|a| a.len()) == Some(1)
                && entry.pointer("/reconciledFills/0/quantity").and_then(V::as_i64) == Some(1),
            "OWNED_FILL",
        )?;
        let cost = micros(own.get("cost").unwrap_or(&V::Null));
        let bid = micros(book.pointer("/bids/0/price").unwrap_or(&V::Null));
        let at = instant(entry.pointer("/reconciledFills/0/filledAt").unwrap_or(&V::Null));
        let risk = if bid <= cost - 4 * TICK {
            Some("stop_loss")
        } else if self.base.now_ms() - at >= 7200000 {
            Some("holding_timeout")
        } else {
            None
        };
        if risk.is_some()
            && self
                .base
                .inner
                .borrow()
                .state
                .pointer(&format!("/control/exits/{exchange_id}"))
                .is_none()
        {
            let obligation = json!({
                "marketId": book.get("marketId").unwrap().clone(),
                "reason": risk.unwrap(),
                "triggeredAt": iso(self.base.now_ms()),
                "triggerBid": book.pointer("/bids/0/price").unwrap().clone(),
                "sourceVersion": book.get("versionKey").unwrap().clone(),
            });
            self.base.update_control(
                move |c| {
                    c.pointer_mut("/exits").unwrap().as_object_mut().unwrap().insert(exchange_id.to_string(), obligation);
                },
                &json!({"type": "exit_obligation_latched", "exchangeId": exchange_id}),
            )?;
        }
        let obligation = self
            .base
            .inner
            .borrow()
            .state
            .pointer(&format!("/control/exits/{exchange_id}"))
            .cloned();
        if let Some(o) = obligation {
            return Ok(Some(json!({
                "reason": o.get("reason").unwrap().clone(),
                "limit": crate::core::format_micros(if bid > TICK { bid - TICK } else { TICK }),
            })));
        }
        if bid >= cost + TICK {
            return Ok(Some(json!({
                "reason": "profit_target",
                "limit": crate::core::format_micros(if bid - TICK > cost + TICK { bid - TICK } else { cost + TICK }),
            })));
        }
        Ok(None)
    }
    pub fn create_proposal(
        &mut self,
        book: &V,
        action: &str,
        limit: &str,
        reason: &str,
        reference: Option<&V>,
    ) -> Result<V, String> {
        let intent_id = format!("median-{}", uuid::Uuid::new_v4());
        let created_at = iso(self.base.now_ms());
        let proof = json!({
            "bankHash": self.base.activation.get("strategyBankHash").unwrap().clone(),
            "paperBinding": self.m.borrow().bank.get("paperBinding").unwrap().clone(),
            "reference": reference.cloned().unwrap_or(V::Null),
            "originalBook": book.clone(),
            "decisionAvailableAt": created_at,
            "referencePolicy": serde_json::from_str::<V>(MEDIAN_REFERENCE_POLICY).unwrap(),
        });
        let p = json!({
            "intentId": intent_id,
            "idempotencyKey": uuid::Uuid::new_v4().to_string(),
            "bankHash": self.base.activation.get("strategyBankHash").unwrap().clone(),
            "marketId": book.get("marketId").unwrap().clone(),
            "exchangeId": book.get("exchangeId").unwrap().clone(),
            "action": action,
            "quantity": 1,
            "limit": limit,
            "reason": reason,
            "createdAt": created_at,
            "expiresAt": iso(self.base.now_ms() + 120000),
            "originalBook": book.clone(),
            "proof": proof,
            "proofHash": payload_hash(&proof)?,
            "status": "proposed",
        });
        let id = intent_id.clone();
        let p2 = p.clone();
        let (action_s, reason_s) = (action.to_string(), reason.to_string());
        self.update_proposals(
            move |s| {
                s.pointer_mut("/proposals").unwrap().as_object_mut().unwrap().insert(id.clone(), p2);
            },
            &json!({"type": "median_proposal", "intentId": intent_id, "action": action_s, "reason": reason_s, "at": created_at}),
        )?;
        Ok(p)
    }
    pub fn on_book(&mut self, response: &V) -> Result<(), String> {
        let market_id = response
            .get("endpoint")
            .and_then(V::as_str)
            .and_then(|e| {
                let p = e.strip_prefix("/markets/")?;
                let e2 = p.find("/orderbook")?;
                let id = &p[..e2];
                if id.bytes().all(|b| b.is_ascii_digit()) {
                    Some(id.to_string())
                } else {
                    None
                }
            })
            .unwrap_or_default();
        if !self.base.can_write()
            || self.base.active().is_some()
            || !self.base.inner.borrow().selected.contains(&market_id)
            || !self.base.inner.borrow().markets.contains_key(&market_id)
            || self.base.now_ms()
                - instant(
                    self.base
                        .inner
                        .borrow()
                        .markets
                        .get(&market_id)
                        .and_then(|m| m.get("at"))
                        .unwrap_or(&V::Null),
                )
                > 180000
        {
            return Ok(());
        }
        self.bank_current()?;
        let b = self.base.checked_book(response, &market_id)?;
        let snapshot = self.base.snapshot(Some(&json!({
            "tournamentId": MEDIAN_TID,
            "marketId": market_id,
            "exchangeId": b.get("exchangeId").unwrap().clone(),
            "at": b.get("receivedAt").unwrap().clone(),
            "requestStartedAt": b.get("requestStartedAt").unwrap().clone(),
            "bids": b.get("bids").unwrap().clone(),
            "asks": b.get("asks").unwrap().clone(),
        })));
        if !self.evidence_admitted(&b, &snapshot, self.base.now_ms()) {
            self.base.inner.borrow_mut().reason = Some("MEDIAN_STALE_OR_NONCAUSAL_ACCOUNT".into());
            self.publish();
            return Ok(());
        }
        self.check_progress(&b)?;
        self.sync_proposals()?;
        let owned = self
            .base
            .inner
            .borrow()
            .state
            .pointer(&format!("/owned/{}", b.get("exchangeId").and_then(V::as_str).unwrap_or("")))
            .cloned();
        let exit = self.owned_policy(&b)?;
        let mut p = self
            .m
            .borrow()
            .proposals
            .get("proposals")
            .and_then(V::as_object)
            .and_then(|o| {
                o.values()
                    .find(|x| {
                        x.get("marketId").and_then(V::as_str) == Some(market_id.as_str())
                            && x.get("status").and_then(V::as_str) == Some("proposed")
                    })
                    .cloned()
            });
        if let Some(pp) = &p {
            let bid = micros(b.pointer("/bids/0/price").unwrap_or(&V::Null));
            let stale_limit = pp.get("action").and_then(V::as_str) == Some("sell")
                && pp.get("reason").and_then(V::as_str) != Some("profit_target")
                && bid < micros(pp.get("limit").unwrap_or(&V::Null));
            let revoke = self.base.now_ms() >= instant(pp.get("expiresAt").unwrap())
                || (pp.get("action").and_then(V::as_str) == Some("buy") && owned.is_some())
                || (pp.get("action").and_then(V::as_str) == Some("sell") && owned.is_none())
                || (pp.get("action").and_then(V::as_str) == Some("sell")
                    && exit.is_some()
                    && pp.get("reason").and_then(V::as_str) == Some("profit_target")
                    && exit.as_ref().and_then(|e| e.get("reason")).and_then(V::as_str) != Some("profit_target"))
                || stale_limit;
            if revoke {
                let status = if self.base.now_ms() >= instant(pp.get("expiresAt").unwrap()) {
                    "expired"
                } else {
                    "revoked"
                };
                let id = pp.get("intentId").and_then(V::as_str).unwrap_or("").to_string();
                self.update_proposals(
                    move |s| {
                        s.pointer_mut(&format!("/proposals/{id}"))
                            .unwrap()
                            .as_object_mut()
                            .unwrap()
                            .insert("status".into(), json!(status));
                    },
                    &json!({"type": "median_proposal_revoked", "intentId": pp.get("intentId").unwrap().clone(), "at": iso(self.base.now_ms())}),
                )?;
                p = None;
            }
        }
        if let Some(p) = p {
            if instant(b.get("requestStartedAt").unwrap()) < instant(p.get("createdAt").unwrap()) + 3000
                || compare_book(&b, p.get("originalBook").unwrap()) <= 0
                || !self.depth_admitted(&p, &b)
            {
                return Ok(());
            }
            if p.get("action").and_then(V::as_str) == Some("buy") {
                let window = entry_window_gate(&self.base.inner, &self.base.activation, self.base.now_ms());
                let m = self.m.borrow();
                if !(window && m.reference_ready && m.reference_failure.is_none()) {
                    return Ok(());
                }
            }
            let spec = json!({
                "intentId": p.get("intentId").unwrap().clone(),
                "idempotencyKey": p.get("idempotencyKey").unwrap().clone(),
                "marketId": market_id,
                "exchangeId": b.get("exchangeId").unwrap().clone(),
                "side": "yes",
                "action": p.get("action").unwrap().clone(),
                "quantity": 1,
                "price": p.get("limit").unwrap().clone(),
                "expirationDate": iso((self.base.now_ms() + 30000).min(instant(p.get("expiresAt").unwrap()))),
                "attribution": {
                    "runId": self.base.activation.get("runId").unwrap().clone(),
                    "strategyId": self.base.activation.get("strategyId").unwrap().clone(),
                    "modelHash": self.base.activation.get("strategyBankHash").unwrap().clone(),
                    "signalId": p.get("proofHash").unwrap().clone(),
                    "reason": p.get("reason").unwrap().clone(),
                },
            });
            let r = prepare_intent(&self.base.inner.borrow().state, &spec, &snapshot, &json!(self.base.now_ms()));
            if r.get("ok") != Some(&V::Bool(true)) {
                self.base.inner.borrow_mut().reason = r.get("code").and_then(V::as_str).map(str::to_string);
                self.publish();
                return Ok(());
            }
            let reference_proof = json!({
                "bankHash": self.base.activation.get("strategyBankHash").unwrap().clone(),
                "proposalId": p.get("intentId").unwrap().clone(),
                "proposalHash": p.get("proofHash").unwrap().clone(),
                "followupBook": b.clone(),
                "followupHash": payload_hash(&b)?,
            });
            self.base.ledger.borrow_mut().prepare_intent(
                &json!({
                    "method": "POST",
                    "path": "/orders",
                    "intentId": p.get("intentId").unwrap().clone(),
                    "idempotencyKey": p.get("idempotencyKey").unwrap().clone(),
                    "body": r.pointer("/intent/body").unwrap().clone(),
                    "bodyJson": r.pointer("/intent/bodyJson").unwrap().clone(),
                    "payloadHash": r.pointer("/intent/payloadHash").unwrap().clone(),
                    "attribution": {
                        "ownerId": "Codex",
                        "runId": self.base.activation.get("runId").unwrap().clone(),
                        "strategyId": spec.pointer("/attribution/strategyId").unwrap().clone(),
                        "modelHash": self.base.activation.get("strategyBankHash").unwrap().clone(),
                        "marketId": market_id,
                        "exchangeId": b.get("exchangeId").unwrap().clone(),
                    },
                }),
            )
            .map_err(|e| format!("{NEED}LEDGER:{:?}", e))?;
            self.base.commit(&r, &json!({"type": "median_intent_prepared", "intentId": p.get("intentId").unwrap().clone(), "referenceProof": reference_proof}))?;
            self.sync_proposals()?;
            self.base
                .submit(r.get("intent").unwrap(), Some(&snapshot), false, Some(&reference_proof))?;
            if self.base.can_operate() {
                self.sync_proposals()?;
            }
            return Ok(());
        }
        if owned.is_some() {
            if let Some(exit) = exit {
                self.create_proposal(
                    &b,
                    "sell",
                    exit.get("limit").and_then(V::as_str).unwrap_or(""),
                    exit.get("reason").and_then(V::as_str).unwrap_or(""),
                    None,
                )?;
            }
            self.publish();
            return Ok(());
        }
        {
            let window = entry_window_gate(&self.base.inner, &self.base.activation, self.base.now_ms());
            let m = self.m.borrow();
            if self.base.inner.borrow().reconciliation_risk_only
                || !window
                || !m.reference_ready
                || m.reference_failure.is_some()
            {
                return Ok(());
            }
        }
        {
            let inner = self.base.inner.borrow();
            let exchange_id = b.get("exchangeId").and_then(V::as_str).unwrap_or("");
            if inner
                .account
                .as_ref()
                .and_then(|a| a.pointer("/positions/rows"))
                .and_then(V::as_array)
                .is_some_and(|rows| {
                    rows.iter().any(|x| {
                        x.get("exchangeId").and_then(V::as_str) == Some(exchange_id)
                            && x.get("quantity").and_then(V::as_i64).unwrap_or(0) != 0
                    })
                })
                || inner
                    .account
                    .as_ref()
                    .and_then(|a| a.pointer("/orders/rows"))
                    .and_then(V::as_array)
                    .is_some_and(|rows| {
                        rows.iter()
                            .any(|x| x.get("exchangeId").and_then(V::as_str) == Some(exchange_id))
                    })
            {
                return Ok(());
            }
        }
        let last_exit = self
            .base
            .inner
            .borrow()
            .state
            .get("intents")
            .and_then(V::as_object)
            .map(|m| {
                m.values()
                    .filter(|i| {
                        i.get("exchangeId") == b.get("exchangeId")
                            && i.get("action").and_then(V::as_str) == Some("sell")
                            && i.get("filledQuantity").and_then(V::as_i64).unwrap_or(0) > 0
                            && i.get("status").and_then(V::as_str) == Some("closed")
                    })
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
            .last()
            .cloned();
        if let Some(i) = last_exit {
            let at = instant(i.pointer("/reconciledFills").and_then(V::as_array).and_then(|a| a.last()).and_then(|f| f.get("filledAt")).unwrap_or(&V::Null));
            if self.base.now_ms() - at < 900000 {
                return Ok(());
            }
        }
        let pending: Vec<V> = self
            .m
            .borrow()
            .proposals
            .get("proposals")
            .and_then(V::as_object)
            .map(|o| {
                o.values()
                    .filter(|x| {
                        x.get("status").and_then(V::as_str) == Some("proposed")
                            && x.get("action").and_then(V::as_str) == Some("buy")
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        if self
            .base
            .inner
            .borrow()
            .state
            .get("owned")
            .and_then(V::as_object)
            .map(|o| o.len())
            .unwrap_or(0)
            + pending.len()
            >= 3
        {
            return Ok(());
        }
        let reference = {
            let mut m = self.m.borrow_mut();
            m.reference.as_mut().unwrap().reference(&json!({
                "marketId": market_id,
                "exchangeId": b.get("exchangeId").unwrap().clone(),
                "bookRequestStartedAt": b.get("requestStartedAt").unwrap().clone(),
                "availableAt": iso(self.base.now_ms()),
                "bestAsk": b.pointer("/asks/0/price").unwrap().clone(),
            }))?
        };
        self.m.borrow_mut().last_reference = json!({
            "marketId": market_id,
            "outcome": reference.get("outcome").unwrap().clone(),
            "reason": reference.get("reason").cloned().unwrap_or(V::Null),
            "reference": reference.get("reference").unwrap().clone(),
            "sourceProof": reference.get("sourceProof").unwrap().clone(),
        });
        if reference.get("outcome").and_then(V::as_str) != Some("reference") {
            self.base.inner.borrow_mut().status = "waiting_for_signal".into();
            self.base.inner.borrow_mut().reason = reference
                .get("reason")
                .and_then(V::as_str)
                .map(str::to_string);
            self.publish();
            return Ok(());
        }
        let limit = micros(b.pointer("/asks/0/price").unwrap_or(&V::Null)) + TICK;
        let spent: i128 = self
            .base
            .inner
            .borrow()
            .state
            .get("intents")
            .and_then(V::as_object)
            .map(|m| {
                m.values()
                    .filter(|i| i.get("action").and_then(V::as_str) == Some("buy"))
                    .map(|i| {
                        i.get("reconciledFills")
                            .and_then(V::as_array)
                            .map(|a| {
                                a.iter()
                                    .map(|f| {
                                        micros(f.get("price").unwrap_or(&V::Null))
                                            * f.get("quantity").and_then(V::as_i64).unwrap_or(0) as i128
                                    })
                                    .sum::<i128>()
                            })
                            .unwrap_or(0)
                    })
                    .sum()
            })
            .unwrap_or(0);
        let reserved: i128 = pending
            .iter()
            .map(|x| micros(x.get("limit").unwrap_or(&V::Null)))
            .sum();
        if limit > 995000
            || spent + reserved + limit
                > micros(self.base.activation.get("maxCumulativeBuyCost").unwrap_or(&V::Null))
        {
            return Ok(());
        }
        self.create_proposal(
            &b,
            "buy",
            &crate::core::format_micros(limit),
            "ask_below_earlier_hour_bid_median",
            Some(&reference),
        )?;
        self.base.inner.borrow_mut().status = "median_proposal_pending".into();
        self.base.inner.borrow_mut().reason = Some("AWAITING_LATER_INDEPENDENT_BOOK".into());
        self.publish();
        Ok(())
    }
    pub fn publish(&self) {
        self.base.publish();
        let m = self.m.borrow();
        let file = self.base.runtime_dir.join("competition-stats.json");
        let Ok(s_raw) = std::fs::read_to_string(&file) else { return };
        let Ok(mut s) = serde_json::from_str::<V>(&s_raw) else { return };
        let pending: Vec<V> = m
            .proposals
            .get("proposals")
            .and_then(V::as_object)
            .map(|o| {
                o.values()
                    .filter(|p| p.get("status").and_then(V::as_str) == Some("proposed"))
                    .map(|p| {
                        json!({
                            "intentId": p.get("intentId").unwrap().clone(),
                            "marketId": p.get("marketId").unwrap().clone(),
                            "exchangeId": p.get("exchangeId").unwrap().clone(),
                            "action": p.get("action").unwrap().clone(),
                            "quantity": p.get("quantity").unwrap().clone(),
                            "limit": p.get("limit").unwrap().clone(),
                            "reason": p.get("reason").unwrap().clone(),
                            "createdAt": p.get("createdAt").unwrap().clone(),
                            "expiresAt": p.get("expiresAt").unwrap().clone(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let median = json!({
            "referenceReady": m.reference_ready,
            "referenceFailure": m.reference_failure.clone().map_or(V::Null, V::String),
            "reference": m.reference.as_ref().map(|r| r.status()).unwrap_or(V::Null),
            "lastReference": m.last_reference.clone(),
            "pendingProposals": pending,
            "journalSequence": m.proposal_journal_sequence,
            "journalHash": m.proposal_journal_previous,
            "policy": m.bank.get("policy").unwrap().clone(),
            "paperBinding": m.bank.get("paperBinding").unwrap().clone(),
            "executionAuthority": true,
        });
        if let Some(o) = s.as_object_mut() {
            o.insert("strategyId".into(), self.base.activation.get("strategyId").unwrap().clone());
            o.insert("strategyBankHash".into(), self.base.activation.get("strategyBankHash").unwrap().clone());
            o.insert("median".into(), median);
        }
        let _ = atomic_json(&file, &s);
    }
    pub fn close(&mut self) -> Result<(), String> {
        self.base.close()
    }
}

// Median entryReferenceAdmitted — invoked through the transport fences.
fn median_reference_gate(
    m: &Rc<RefCell<MedianState>>,
    inner: &Rc<RefCell<crate::controller::ControllerInner>>,
    activation: &V,
    universe: &V,
    workspace: &Path,
    i: &V,
    permit: &V,
    now_ms: i64,
) -> bool {
    let current = inner.borrow().current_permit.clone().unwrap_or(V::Null);
    if permit.get("retry") == Some(&V::Bool(true))
        && current.get("retry") == Some(&V::Bool(true))
        && i.get("attempts").and_then(V::as_i64).unwrap_or(0) > 0
        && now_ms >= instant(i.get("expirationDate").unwrap_or(&V::Null))
    {
        return true;
    }
    let bank = m.borrow().bank.clone();
    let bank_file = m.borrow().bank_file.clone();
    if validate_median_bank(
        &std::fs::read_to_string(&bank_file)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(V::Null),
        activation,
        universe,
        workspace,
    )
    .is_err()
    {
        return false;
    }
    let _ = bank;
    let intent_id = i.get("intentId").and_then(V::as_str).unwrap_or("");
    let p = m
        .borrow()
        .proposals
        .pointer(&format!("/proposals/{intent_id}"))
        .cloned()
        .unwrap_or(V::Null);
    let proof = current.get("referenceProof").cloned().unwrap_or(V::Null);
    let b = proof.get("followupBook").cloned().unwrap_or(V::Null);
    let ok = !p.is_null()
        && ["proposed", "dispatched"].contains(&p.get("status").and_then(V::as_str).unwrap_or(""))
        && !proof.is_null()
        && proof.get("bankHash") == activation.get("strategyBankHash")
        && proof.get("proposalHash").and_then(V::as_str)
            == payload_hash(p.get("proof").unwrap_or(&V::Null)).ok().as_deref()
        && proof.get("proposalId") == p.get("intentId")
        && proof.get("followupHash").and_then(V::as_str) == payload_hash(&b).ok().as_deref()
        && i.get("idempotencyKey") == p.get("idempotencyKey")
        && i.get("quantity").and_then(V::as_i64) == Some(1)
        && i.get("action") == p.get("action")
        && micros(i.get("price").unwrap_or(&V::Null)) == micros(p.get("limit").unwrap_or(&V::Null));
    if !ok {
        return false;
    }
    // evidenceAdmitted(b, currentPermit.snapshot, now)
    let snapshot = current.get("snapshot").cloned().unwrap_or(V::Null);
    let request = instant(b.get("requestStartedAt").unwrap_or(&V::Null));
    let received = instant(b.get("receivedAt").unwrap_or(&V::Null));
    if request == i64::MIN
        || received == i64::MIN
        || request > received
        || received > now_ms
        || now_ms - received > 20000
        || received - request > 20000
    {
        return false;
    }
    for k in ["cash", "positions", "orders"] {
        let pv = snapshot.get(k).cloned().unwrap_or(V::Null);
        let at = instant(pv.get("at").unwrap_or(&V::Null));
        let good = pv.get("tournamentId").and_then(V::as_str) == Some(MEDIAN_TID)
            && at != i64::MIN
            && at <= request
            && now_ms - at <= 20000
            && (k == "cash" || (pv.get("complete") == Some(&V::Bool(true)) && pv.get("rows").is_some_and(V::is_array)));
        if !good {
            return false;
        }
    }
    // depthAdmitted(p, b)
    let depth = b
        .get(if p.get("action").and_then(V::as_str) == Some("buy") { "asks" } else { "bids" })
        .and_then(V::as_array)
        .is_some_and(|a| {
            a.iter().any(|l| {
                l.get("quantity").and_then(V::as_i64).unwrap_or(0) >= 100
                    && if p.get("action").and_then(V::as_str) == Some("buy") {
                        micros(l.get("price").unwrap_or(&V::Null)) <= micros(p.get("limit").unwrap_or(&V::Null))
                    } else {
                        micros(l.get("price").unwrap_or(&V::Null)) >= micros(p.get("limit").unwrap_or(&V::Null))
                    }
            })
        });
    if !depth {
        return false;
    }
    let snapshot_book = snapshot.get("book").cloned().unwrap_or(V::Null);
    if snapshot_book.get("marketId") != b.get("marketId")
        || snapshot_book.get("exchangeId") != b.get("exchangeId")
        || snapshot_book.get("at") != b.get("receivedAt")
        || snapshot_book.get("requestStartedAt") != b.get("requestStartedAt")
        || payload_hash(&json!({"bids": snapshot_book.get("bids").unwrap().clone(), "asks": snapshot_book.get("asks").unwrap().clone()}))
            .ok()
            != payload_hash(&json!({"bids": b.get("bids").unwrap().clone(), "asks": b.get("asks").unwrap().clone()})).ok()
    {
        return false;
    }
    if now_ms >= instant(p.get("expiresAt").unwrap_or(&V::Null))
        || instant(b.get("requestStartedAt").unwrap_or(&V::Null)) < instant(p.get("createdAt").unwrap_or(&V::Null)) + 3000
        || compare_book(&b, p.get("originalBook").unwrap_or(&V::Null)) <= 0
    {
        return false;
    }
    if i.get("action").and_then(V::as_str) == Some("buy") {
        let mm = m.borrow();
        if !(entry_window_gate(inner, activation, now_ms) && mm.reference_ready && mm.reference_failure.is_none()) {
            return false;
        }
    }
    if i.get("action").and_then(V::as_str) == Some("sell")
        && inner
            .borrow()
            .state
            .pointer(&format!("/owned/{}/quantity", i.get("exchangeId").and_then(V::as_str).unwrap_or("")))
            .and_then(V::as_i64)
            != Some(1)
    {
        return false;
    }
    true
}
