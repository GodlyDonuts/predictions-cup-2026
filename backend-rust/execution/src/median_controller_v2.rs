// execution/median-controller-v2.mjs — same-run median entry-cap revision
// controller. Extends CompetitionController in JS; here it wraps the base and
// installs the v2 reference gate on the shared inner state.
use crate::controller::{entry_window_gate, CompetitionController, ControllerOptions};
use crate::core::{format_micros, payload_hash, prepare_intent};
use crate::median::{MedianReferenceReader, MEDIAN_REFERENCE_POLICY, MEDIAN_TID};
use crate::median_controller::{
    compare_book, hex64, instant, iso, micros, need, plain, stamp, validate_median_bank,
    MEDIAN_EXECUTION_POLICY_JSON,
};
use crate::median_protected_candidate_plan::plan_median_protected_candidates;
use baal_service_runtime::atomic::atomic_json;
use baal_service_runtime::fixed::TICK;
use baal_service_runtime::journal::Journal;
use baal_service_runtime::json_text::digest;
use serde_json::{json, Value as V};
use sha2::{Digest, Sha256};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

fn sha(bytes: &[u8]) -> String {
    hex_str(&Sha256::digest(bytes))
}
fn hex_str(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// {...MEDIAN_EXECUTION_POLICY, remainingEntryMarginTicks: 1}
pub fn median_execution_policy_v2() -> V {
    let mut p = serde_json::from_str::<V>(MEDIAN_EXECUTION_POLICY_JSON).unwrap();
    p.as_object_mut().unwrap().insert("remainingEntryMarginTicks".into(), json!(1));
    p
}

pub fn median_controller_v2_identity(workspace: &Path) -> Result<V, String> {
    crate::frozen::collect_frozen_paper_files(workspace, &["execution/median-controller-v2.mjs"])
}

fn exact_keys(x: &V, keys: &[&str]) -> bool {
    let Some(m) = x.as_object() else { return false };
    let mut got: Vec<&str> = m.keys().map(String::as_str).collect();
    got.sort_unstable();
    let mut want: Vec<&str> = keys.to_vec();
    want.sort_unstable();
    got == want
}

const CHECKPOINT_KEYS: [&str; 3] = ["sequence", "hash", "stateHash"];
fn checkpoint_valid(p: &V) -> bool {
    exact_keys(p, &CHECKPOINT_KEYS)
        && p.get("sequence").and_then(V::as_i64).is_some_and(|n| n >= 0)
        && (if p.get("sequence").and_then(V::as_i64) == Some(0) {
            p.get("hash").and_then(V::as_str) == Some("")
        } else {
            hex64(p.get("hash").unwrap_or(&V::Null))
        })
        && hex64(p.get("stateHash").unwrap_or(&V::Null))
}

/// min(asks[0]+tick, reference−tick), with the reference≥ask+tick gate.
pub fn median_entry_cap_limit(book: &V, reference: &V) -> Result<String, String> {
    need(
        reference.get("outcome").and_then(V::as_str) == Some("reference")
            && {
                let r = micros(reference.get("reference").unwrap_or(&V::Null));
                let a = micros(book.pointer("/asks/0/price").unwrap_or(&V::Null));
                r != i128::MIN && a != i128::MIN && r - a >= TICK
            },
        "ENTRY_REFERENCE_GAP",
    )?;
    let original = micros(book.pointer("/asks/0/price").unwrap()) + TICK;
    let retained = micros(reference.get("reference").unwrap()) - TICK;
    Ok(format_micros(original.min(retained)))
}

/// A revision changes neither activation nor handoff. The original activation
/// hash pins owner, run, budget, authority and the original entry expiry.
pub fn validate_median_revision(
    revision: &V,
    bank: &V,
    prior_bank: &V,
    activation: &V,
    universe: &V,
    workspace: &Path,
    archive_dir: &Path,
    now: i64,
) -> Result<V, String> {
    need(
        exact_keys(
            revision,
            &[
                "schemaVersion", "kind", "revisionId", "ownerId", "runId", "tournamentId",
                "activationHash", "priorBankHash", "nextBankHash", "recordedAt",
                "coreCheckpoint", "proposalCheckpoint",
            ],
        ),
        "REVISION_SHAPE",
    )?;
    let revision_id_ok = revision
        .get("revisionId")
        .and_then(V::as_str)
        .is_some_and(|s| {
            s.len() >= 18 && s.len() <= 88
                && s.starts_with("median-entry-cap-")
                && s[17..].bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        });
    need(
        revision.get("schemaVersion").and_then(V::as_i64) == Some(1)
            && revision.get("kind").and_then(V::as_str) == Some("same_run_median_entry_cap_revision")
            && revision_id_ok
            && revision.get("ownerId").and_then(V::as_str) == Some("Codex")
            && revision.get("runId") == activation.get("runId")
            && revision.get("tournamentId") == activation.get("tournamentId")
            && revision.get("activationHash").and_then(V::as_str) == payload_hash(activation).ok().as_deref()
            && revision.get("priorBankHash") == activation.get("strategyBankHash")
            && hex64(revision.get("nextBankHash").unwrap_or(&V::Null))
            && stamp(revision.get("recordedAt").unwrap_or(&V::Null))
            && instant(revision.get("recordedAt").unwrap()) >= instant(activation.get("startedAt").unwrap())
            && instant(revision.get("recordedAt").unwrap()) <= now,
        "REVISION_BINDING",
    )?;
    let pc = revision.get("proposalCheckpoint").unwrap_or(&V::Null);
    need(
        checkpoint_valid(revision.get("coreCheckpoint").unwrap_or(&V::Null))
            && exact_keys(pc, &["sequence", "hash", "stateHash", "bytes", "sha256"])
            && checkpoint_valid(&json!({
                "sequence": pc.get("sequence").unwrap().clone(),
                "hash": pc.get("hash").unwrap().clone(),
                "stateHash": pc.get("stateHash").unwrap().clone(),
            }))
            && pc.get("bytes").and_then(V::as_i64).is_some_and(|n| n >= 0)
            && hex64(pc.get("sha256").unwrap_or(&V::Null)),
        "REVISION_CHECKPOINT",
    )?;
    validate_median_bank(prior_bank, activation, universe, workspace)?;
    // Traverse the archived entrypoint, not merely a caller-supplied file list.
    validate_median_bank(prior_bank, activation, universe, archive_dir)?;
    need(
        bank.get("schemaVersion").and_then(V::as_i64) == Some(2)
            && bank.get("mode").and_then(V::as_str) == Some("baal_historical_median_production_v2")
            && digest(bank) == revision.get("nextBankHash").and_then(V::as_str).unwrap_or(""),
        "BANK",
    )?;
    need(
        exact_keys(
            bank,
            &[
                "schemaVersion", "mode", "tournamentId", "universeHash", "cutoff", "paperBinding",
                "sourcePrefix", "policy", "codeIdentity", "legacyCompatibility",
            ],
        ),
        "BANK_SHAPE",
    )?;
    for k in ["tournamentId", "universeHash", "cutoff", "paperBinding", "sourcePrefix"] {
        need(
            payload_hash(bank.get(k).unwrap()) == payload_hash(prior_bank.get(k).unwrap()),
            "BANK_SCOPE_OR_SOURCE",
        )?;
    }
    need(
        payload_hash(bank.get("policy").unwrap()).ok() == payload_hash(&median_execution_policy_v2()).ok(),
        "POLICY",
    )?;
    need(
        exact_keys(
            bank.get("legacyCompatibility").unwrap_or(&V::Null),
            &["priorBankHash", "priorPolicyHash", "priorCodeIdentityHash"],
        )
            && bank.pointer("/legacyCompatibility/priorBankHash") == revision.get("priorBankHash")
            && bank.pointer("/legacyCompatibility/priorPolicyHash").and_then(V::as_str)
                == payload_hash(prior_bank.get("policy").unwrap()).ok().as_deref()
            && bank.pointer("/legacyCompatibility/priorCodeIdentityHash").and_then(V::as_str)
                == payload_hash(prior_bank.get("codeIdentity").unwrap()).ok().as_deref(),
        "LEGACY_COMPATIBILITY",
    )?;
    need(
        plain(bank.get("codeIdentity").unwrap_or(&V::Null))
            && payload_hash(bank.get("codeIdentity").unwrap()).ok()
                == payload_hash(&median_controller_v2_identity(workspace)?).ok(),
        "CODE_IDENTITY",
    )?;
    Ok(bank.clone())
}

/// Walks a journal to checkpoint `p`, verifying the hash chain; returns
/// {state, sequence, hash} at the checkpoint row.
fn journal_checkpoint(file: &Path, p: &V, fixed: bool, empty_state: &V) -> Result<V, String> {
    let bytes = if file.exists() { std::fs::read(file).map_err(|e| e.to_string())? } else { Vec::new() };
    if fixed {
        need(
            bytes.len() as i64 == p.get("bytes").and_then(V::as_i64).unwrap_or(-1)
                && sha(&bytes) == p.get("sha256").and_then(V::as_str).unwrap_or(""),
            "LEGACY_PROPOSAL_BYTES",
        )?;
    }
    let raw = String::from_utf8(bytes).map_err(|_| "MEDIAN_EXECUTION_REVISION_JOURNAL_PARTIAL".to_string())?;
    need(raw.is_empty() || raw.ends_with('\n'), "REVISION_JOURNAL_PARTIAL")?;
    let mut sequence = 0i64;
    let mut previous = String::new();
    let mut state = empty_state.clone();
    let mut found = p.get("sequence").and_then(V::as_i64) == Some(0)
        && p.get("stateHash").and_then(V::as_str) == payload_hash(empty_state).ok().as_deref();
    for line in raw.split('\n').filter(|l| !l.is_empty()) {
        let row: V = serde_json::from_str(line).map_err(|_| "MEDIAN_EXECUTION_REVISION_JOURNAL_CHAIN".to_string())?;
        let hash = row.get("hash").and_then(V::as_str).unwrap_or("").to_string();
        let mut body = row.clone();
        body.as_object_mut().unwrap().remove("hash");
        need(
            body.get("sequence").and_then(V::as_i64) == Some(sequence + 1)
                && body.get("previous").and_then(V::as_str) == Some(previous.as_str())
                && hash == digest(&body),
            "REVISION_JOURNAL_CHAIN",
        )?;
        sequence = body.get("sequence").and_then(V::as_i64).unwrap();
        previous = hash.clone();
        if sequence == p.get("sequence").and_then(V::as_i64).unwrap_or(-1) {
            need(
                hash == p.get("hash").and_then(V::as_str).unwrap_or("")
                    && payload_hash(row.get("state").unwrap_or(&V::Null)).ok().as_deref()
                        == p.get("stateHash").and_then(V::as_str),
                "REVISION_JOURNAL_CHECKPOINT",
            )?;
            state = row.get("state").cloned().unwrap_or(V::Null);
            found = true;
        }
    }
    need(
        found && (!fixed || (sequence == p.get("sequence").and_then(V::as_i64).unwrap_or(-1)
            && previous == p.get("hash").and_then(V::as_str).unwrap_or(""))),
        "REVISION_CHECKPOINT_MISSING",
    )?;
    Ok(json!({"state": state, "sequence": sequence, "hash": previous}))
}

fn proposal_terms(p: &V) -> V {
    let mut t = p.clone();
    t.as_object_mut().unwrap().remove("status");
    t
}

/// v2 validateProposals: v1's plus revision binding, legacy-term immutability,
/// and the entry cap on new buy proposals.
fn validate_proposals_v2(
    s: &V,
    bank_hash: &str,
    run_id: &str,
    universe: &V,
    revision_hash: Option<&str>,
    recorded_at: &V,
    new_bank_hash: &V,
    legacy: Option<&V>,
) -> Result<V, String> {
    need(
        plain(s)
            && s.get("schemaVersion").and_then(V::as_i64) == Some(1)
            && s.get("bankHash").and_then(V::as_str) == Some(bank_hash)
            && s.get("runId").and_then(V::as_str) == Some(run_id)
            && plain(s.get("proposals").unwrap_or(&V::Null))
            && plain(s.get("books").unwrap_or(&V::Null)),
        "PROPOSAL_STATE",
    )?;
    if let Some(rh) = revision_hash {
        need(
            s.get("revisionHash").and_then(V::as_str) == Some(rh)
                && s.get("activeBankHash") == Some(new_bank_hash),
            "PROPOSAL_REVISION",
        )?;
    }
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
        let prior = legacy.and_then(|l| l.pointer(&format!("/proposals/{id}")).cloned());
        let expected_bank = if revision_hash.is_some() && prior.is_none() {
            new_bank_hash.clone()
        } else {
            json!(bank_hash)
        };
        need(
            stamp(p.get("createdAt").unwrap_or(&V::Null))
                && stamp(p.get("expiresAt").unwrap_or(&V::Null))
                && instant(p.get("expiresAt").unwrap()) - instant(p.get("createdAt").unwrap()) == 120000
                && p.get("bankHash") == Some(&expected_bank)
                && p.pointer("/originalBook/marketId") == p.get("marketId")
                && p.pointer("/originalBook/exchangeId") == p.get("exchangeId")
                && hex64(p.get("proofHash").unwrap_or(&V::Null))
                && p.get("proofHash").and_then(V::as_str)
                    == payload_hash(p.get("proof").unwrap()).ok().as_deref()
                && p.pointer("/proof/bankHash") == Some(&expected_bank)
                && p.pointer("/proof/decisionAvailableAt") == p.get("createdAt")
                && payload_hash(p.pointer("/proof/originalBook").unwrap_or(&V::Null))
                    == payload_hash(p.get("originalBook").unwrap()),
            "PROPOSAL_PROOF",
        )?;
        if let Some(prior) = &prior {
            let old_status = prior.get("status").and_then(V::as_str).unwrap_or("");
            let status = p.get("status").and_then(V::as_str).unwrap_or("");
            let transition_ok = match old_status {
                "proposed" => ["proposed", "expired"].contains(&status),
                "dispatched" => ["dispatched", "closed"].contains(&status),
                other => status == other,
            };
            need(
                payload_hash(&proposal_terms(prior)) == payload_hash(&proposal_terms(&p)) && transition_ok,
                "LEGACY_PROPOSAL_CHANGED",
            )?;
        } else if let Some(rh) = revision_hash {
            need(
                p.pointer("/proof/revisionHash").and_then(V::as_str) == Some(rh)
                    && instant(p.get("createdAt").unwrap()) >= instant(recorded_at),
                "NEW_PROPOSAL_REVISION",
            )?;
            if p.get("action").and_then(V::as_str) == Some("buy") {
                need(
                    p.get("limit").and_then(V::as_str)
                        == median_entry_cap_limit(
                            p.get("originalBook").unwrap(),
                            p.pointer("/proof/reference").unwrap_or(&V::Null),
                        )
                        .ok()
                        .as_deref(),
                    "ENTRY_CAP",
                )?;
            }
        }
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
    if let Some(l) = legacy {
        let proposals = s.get("proposals").and_then(V::as_object).unwrap();
        need(
            l.get("proposals")
                .and_then(V::as_object)
                .map(|m| m.keys().all(|id| proposals.contains_key(id)))
                .unwrap_or(true),
            "LEGACY_PROPOSAL_REMOVED",
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

pub struct MedianV2State {
    pub revision: V,
    pub revision_hash: String,
    pub bank: V,
    pub prior_bank: V,
    pub proposals: V,
    pub legacy_proposals: V,
    pub legacy_core: V,
    pub reference: Option<MedianReferenceReader>,
    pub reference_ready: bool,
    pub reference_failure: Option<String>,
    pub candidate_ids: Vec<String>,
    pub last_reference: V,
    pub protected_metadata_cycle: i64,
    pub protected_metadata_cycles: V,
    pub last_protected_metadata_plan: V,
    pub proposal_journal_sequence: u64,
    pub proposal_journal_previous: String,
    pub revision_file: PathBuf,
    pub bank_file: PathBuf,
    pub prior_bank_file: PathBuf,
    pub archive_dir: PathBuf,
}

pub struct MedianCompetitionControllerV2 {
    pub base: CompetitionController,
    pub m: Rc<RefCell<MedianV2State>>,
    pub proposal_journal: Rc<RefCell<Journal>>,
    pub workspace: PathBuf,
    pub revision_file: PathBuf,
    pub bank_file: PathBuf,
    pub prior_bank_file: PathBuf,
    pub archive_dir: PathBuf,
}

impl MedianCompetitionControllerV2 {
    pub fn policy_bank_hash(&self) -> String {
        self.m.borrow().revision.get("nextBankHash").and_then(V::as_str).unwrap_or("").to_string()
    }
    pub fn scheduling_bank_hash(&self) -> String {
        self.policy_bank_hash()
    }

    pub fn new(options: ControllerOptions, workspace: PathBuf) -> Result<Self, String> {
        let mut base = CompetitionController::new(options)?;
        let universe = base.universe.clone();
        let activation = base.activation.clone();
        let runtime_dir = base.runtime_dir.clone();
        let inner = base.inner.clone();
        let result = (|| -> Result<(Rc<RefCell<MedianV2State>>, Rc<RefCell<Journal>>, PathBuf, PathBuf, PathBuf, PathBuf), String> {
            let revision_file = base.dir.join("median-entry-cap-revision.json");
            let revision: V = serde_json::from_str(
                &std::fs::read_to_string(&revision_file).map_err(|e| format!("MEDIAN_EXECUTION_REVISION:{e}"))?,
            )
            .map_err(|e| format!("MEDIAN_EXECUTION_REVISION:{e}"))?;
            let revision_hash = payload_hash(&revision)?;
            let bank_file = runtime_dir
                .join("production-banks")
                .join(format!("{}.json", revision.get("nextBankHash").and_then(V::as_str).unwrap_or("")));
            let prior_bank_file = runtime_dir
                .join("production-banks")
                .join(format!("{}.json", activation.get("strategyBankHash").and_then(V::as_str).unwrap_or("")));
            let archive_dir = runtime_dir
                .join("production-code")
                .join(activation.get("strategyBankHash").and_then(V::as_str).unwrap_or(""));
            let bank: V = serde_json::from_str(
                &std::fs::read_to_string(&bank_file).map_err(|e| format!("MEDIAN_EXECUTION_BANK:{e}"))?,
            )
            .map_err(|e| format!("MEDIAN_EXECUTION_BANK:{e}"))?;
            let prior_bank: V = serde_json::from_str(
                &std::fs::read_to_string(&prior_bank_file).map_err(|e| format!("MEDIAN_EXECUTION_BANK:{e}"))?,
            )
            .map_err(|e| format!("MEDIAN_EXECUTION_BANK:{e}"))?;
            validate_median_revision(
                &revision,
                &bank,
                &prior_bank,
                &activation,
                &universe,
                &workspace,
                &archive_dir,
                base.now_ms(),
            )?;
            let empty_proposals = json!({
                "schemaVersion": 1,
                "bankHash": activation.get("strategyBankHash").unwrap().clone(),
                "runId": activation.get("runId").unwrap().clone(),
                "proposals": {},
                "books": {},
            });
            let legacy_proposals = validate_proposals_v2(
                journal_checkpoint(
                    &base.dir.join("median-proposals.jsonl"),
                    revision.get("proposalCheckpoint").unwrap(),
                    true,
                    &empty_proposals,
                )?
                .get("state")
                .unwrap(),
                activation.get("strategyBankHash").and_then(V::as_str).unwrap_or(""),
                activation.get("runId").and_then(V::as_str).unwrap_or(""),
                &universe,
                None,
                &V::Null,
                &V::Null,
                None,
            )?;
            let empty_core = json!({
                "schemaVersion": 1,
                "tournamentId": inner.borrow().state.get("tournamentId").unwrap().clone(),
                "policy": inner.borrow().state.get("policy").unwrap().clone(),
                "intents": {},
                "owned": {},
                "control": {"schemaVersion": 1, "exits": {}, "cancellations": {}},
            });
            let legacy_core = journal_checkpoint(
                &base.journal.borrow().file.clone(),
                revision.get("coreCheckpoint").unwrap(),
                false,
                &empty_core,
            )?
            .get("state")
            .cloned()
            .unwrap_or(V::Null);
            {
                let inner = inner.borrow();
                need(
                    payload_hash(legacy_core.get("policy").unwrap_or(&V::Null))
                        == payload_hash(inner.state.get("policy").unwrap_or(&V::Null))
                        && legacy_core.get("tournamentId") == activation.get("tournamentId"),
                    "REVISION_CORE_POLICY",
                )?;
                need(
                    legacy_core
                        .get("intents")
                        .and_then(V::as_object)
                        .map(|m| {
                            m.values().all(|i| {
                                i.get("status").and_then(V::as_str) == Some("closed")
                                    && i.get("acceptanceUncertain") != Some(&V::Bool(true))
                            })
                        })
                        .unwrap_or(true),
                    "REVISION_UNRESOLVED_INTENT",
                )?;
                need(
                    legacy_core
                        .pointer("/control/cancellations")
                        .and_then(V::as_object)
                        .map(|m| m.values().all(|c| c.get("phase").and_then(V::as_str) == Some("reconciled")))
                        .unwrap_or(true),
                    "REVISION_UNRESOLVED_CANCEL",
                )?;
            }
            need(
                legacy_proposals
                    .get("proposals")
                    .and_then(V::as_object)
                    .map(|m| {
                        m.values().all(|p| {
                            p.get("status").and_then(V::as_str) != Some("proposed")
                                || instant(p.get("expiresAt").unwrap_or(&V::Null))
                                    <= instant(revision.get("recordedAt").unwrap())
                        })
                    })
                    .unwrap_or(true),
                "REVISION_LIVE_LEGACY_PROPOSAL",
            )?;
            let proposal_journal =
                Rc::new(RefCell::new(Journal::open(&base.dir.join("median-proposals-v2.jsonl"))?));
            if proposal_journal.borrow().last.is_none() {
                {
                    let j = base.journal.borrow();
                    need(
                        j.sequence == revision.pointer("/coreCheckpoint/sequence").and_then(V::as_i64).unwrap_or(-1) as u64
                            && j.previous == revision.pointer("/coreCheckpoint/hash").and_then(V::as_str).unwrap_or("")
                            && base.active().is_none(),
                        "REVISION_CORE_MOVED",
                    )?;
                }
                let _ = base.ledger.borrow().snapshot();
                need(base.can_operate(), "STOPPED")?;
                let mut state = legacy_proposals.clone();
                state.as_object_mut().unwrap().insert(
                    "activeBankHash".into(),
                    revision.get("nextBankHash").unwrap().clone(),
                );
                state
                    .as_object_mut()
                    .unwrap()
                    .insert("revisionHash".into(), json!(revision_hash));
                let validated = validate_proposals_v2(
                    &state,
                    activation.get("strategyBankHash").and_then(V::as_str).unwrap_or(""),
                    activation.get("runId").and_then(V::as_str).unwrap_or(""),
                    &universe,
                    Some(&revision_hash),
                    revision.get("recordedAt").unwrap(),
                    revision.get("nextBankHash").unwrap(),
                    Some(&legacy_proposals),
                )?;
                proposal_journal.borrow_mut().commit(
                    &validated,
                    &json!({
                        "type": "same_run_median_entry_cap_revision",
                        "revisionHash": revision_hash,
                        "revision": revision,
                    }),
                )?;
            }
            let proposals = validate_proposals_v2(
                proposal_journal
                    .borrow()
                    .last
                    .as_ref()
                    .and_then(|l| l.get("state"))
                    .unwrap_or(&V::Null),
                activation.get("strategyBankHash").and_then(V::as_str).unwrap_or(""),
                activation.get("runId").and_then(V::as_str).unwrap_or(""),
                &universe,
                Some(&revision_hash),
                revision.get("recordedAt").unwrap(),
                revision.get("nextBankHash").unwrap(),
                Some(&legacy_proposals),
            )?;
            let (seq, prev) = {
                let j = proposal_journal.borrow();
                (j.sequence, j.previous.clone())
            };
            let m = Rc::new(RefCell::new(MedianV2State {
                revision_file: revision_file.clone(),
                bank_file: bank_file.clone(),
                prior_bank_file: prior_bank_file.clone(),
                archive_dir: archive_dir.clone(),
                revision,
                revision_hash,
                bank,
                prior_bank,
                proposals,
                legacy_proposals,
                legacy_core,
                reference: None,
                reference_ready: false,
                reference_failure: None,
                candidate_ids: Vec::new(),
                last_reference: V::Null,
                protected_metadata_cycle: 0,
                protected_metadata_cycles: json!({}),
                last_protected_metadata_plan: V::Null,
                proposal_journal_sequence: seq,
                proposal_journal_previous: prev,
            }));
            assert_revision_intents(&inner, &m, &base)?;
            {
                let (m2, ws, act, uni) = (m.clone(), workspace.clone(), activation.clone(), universe.clone());
                inner.borrow_mut().entry_reference_gate = Some(Rc::new(
                    move |inner, _activation, runtime_dir, i: &V, permit: &V, now_ms: i64| {
                        median_v2_reference_gate(&m2, inner, &act, &uni, &ws, runtime_dir, i, permit, now_ms)
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
            Ok((m, proposal_journal, revision_file, bank_file, prior_bank_file, archive_dir))
        })();
        match result {
            Ok((m, proposal_journal, revision_file, bank_file, prior_bank_file, archive_dir)) => {
                let controller = Self {
                    base,
                    m,
                    proposal_journal,
                    workspace,
                    revision_file,
                    bank_file,
                    prior_bank_file,
                    archive_dir,
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

    /// bankCurrent: revision immutable on disk; both banks still valid.
    pub fn bank_current(&self) -> Result<bool, String> {
        let revision: V = serde_json::from_str(
            &std::fs::read_to_string(&self.revision_file).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        need(
            payload_hash(&revision).ok().as_deref() == Some(self.m.borrow().revision_hash.as_str()),
            "REVISION_CHANGED",
        )?;
        let bank: V = serde_json::from_str(&std::fs::read_to_string(&self.bank_file).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        let prior_bank: V =
            serde_json::from_str(&std::fs::read_to_string(&self.prior_bank_file).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        validate_median_revision(
            &revision,
            &bank,
            &prior_bank,
            &self.base.activation,
            &self.base.universe,
            &self.workspace,
            &self.archive_dir,
            self.base.now_ms(),
        )?;
        if !self.m.borrow().legacy_proposals.is_null() {
            let empty = json!({
                "schemaVersion": 1,
                "bankHash": self.base.activation.get("strategyBankHash").unwrap().clone(),
                "runId": self.base.activation.get("runId").unwrap().clone(),
                "proposals": {},
                "books": {},
            });
            journal_checkpoint(
                &self.base.dir.join("median-proposals.jsonl"),
                self.m.borrow().revision.get("proposalCheckpoint").unwrap(),
                true,
                &empty,
            )?;
        }
        Ok(true)
    }

    fn validate_proposal_state(&self, s: &V) -> Result<V, String> {
        let (rh, recorded, nbh, legacy) = {
            let m = self.m.borrow();
            (
                m.revision_hash.clone(),
                m.revision.get("recordedAt").cloned().unwrap_or(V::Null),
                m.revision.get("nextBankHash").cloned().unwrap_or(V::Null),
                m.legacy_proposals.clone(),
            )
        };
        validate_proposals_v2(
            s,
            self.base.activation.get("strategyBankHash").and_then(V::as_str).unwrap_or(""),
            self.base.activation.get("runId").and_then(V::as_str).unwrap_or(""),
            &self.base.universe,
            Some(&rh),
            &recorded,
            &nbh,
            Some(&legacy),
        )
    }

    pub fn initialize_median_reference(&mut self) -> Result<V, String> {
        if self.m.borrow().reference_ready {
            return Ok(self.m.borrow().reference.as_ref().unwrap().status());
        }
        need(self.m.borrow().reference_failure.is_none(), "REFERENCE_FAILED_RESTART_REQUIRED")?;
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

    /// ownedEntry: FIFO walk of fills pairing buy/sell legs per exchange.
    pub fn owned_entry(&self, exchange_id: &str) -> Result<V, String> {
        let own = self
            .base
            .inner
            .borrow()
            .state
            .pointer(&format!("/owned/{exchange_id}"))
            .cloned();
        let Some(own) = own.filter(|o| !o.is_null()) else { return Ok(V::Null) };
        let mut intents: Vec<V> = self
            .base
            .inner
            .borrow()
            .state
            .get("intents")
            .and_then(V::as_object)
            .map(|m| m.values().cloned().collect())
            .unwrap_or_default();
        intents.sort_by_key(|i| i.get("ordinal").and_then(V::as_i64).unwrap_or(0));
        let mut entry = V::Null;
        for i in intents.iter().filter(|i| i.get("exchangeId").and_then(V::as_str) == Some(exchange_id)) {
            for f in i.get("reconciledFills").and_then(V::as_array).cloned().unwrap_or_default() {
                need(f.get("quantity").and_then(V::as_i64) == Some(1), "OWNED_FILL")?;
                if i.get("action").and_then(V::as_str) == Some("buy") {
                    need(entry.is_null(), "OWNED_QUANTITY")?;
                    entry = json!({
                        "intentId": i.get("intentId").unwrap().clone(),
                        "modelHash": i.pointer("/attribution/modelHash").unwrap().clone(),
                        "fillId": f.get("id").unwrap().clone(),
                        "cost": f.get("price").unwrap().clone(),
                        "filledAt": f.get("filledAt").unwrap().clone(),
                        "marketId": i.get("marketId").unwrap().clone(),
                    });
                } else {
                    need(!entry.is_null(), "OWNED_PROVENANCE")?;
                    entry = V::Null;
                }
            }
        }
        let policy_bank_hash = self.policy_bank_hash();
        need(
            !entry.is_null()
                && own.get("quantity").and_then(V::as_i64) == Some(1)
                && own.get("marketId") == entry.get("marketId")
                && micros(own.get("cost").unwrap_or(&V::Null)) == micros(entry.get("cost").unwrap_or(&V::Null))
                && [
                    self.base.activation.get("strategyBankHash").and_then(V::as_str).unwrap_or("").to_string(),
                    policy_bank_hash,
                ]
                .contains(&entry.pointer("/modelHash").and_then(V::as_str).unwrap_or("").to_string()),
            "OWNED_BANK_OR_QUANTITY",
        )?;
        if entry.pointer("/modelHash").and_then(V::as_str)
            == self.base.activation.get("strategyBankHash").and_then(V::as_str)
        {
            need(
                self.m
                    .borrow()
                    .legacy_core
                    .pointer(&format!("/intents/{}", entry.get("intentId").and_then(V::as_str).unwrap_or("")))
                    .is_some(),
                "LEGACY_LOT_NOT_AT_REVISION",
            )?;
        }
        Ok(entry)
    }

    pub fn write_proposals(&self, next: &V, event: &V) -> Result<(), String> {
        let _ = self.base.ledger.borrow().snapshot();
        need(self.base.can_operate(), "STOPPED")?;
        self.bank_current()?;
        let next = self.validate_proposal_state(next)?;
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
                let want = if i.get("status").and_then(V::as_str) == Some("closed") { "closed" } else { "dispatched" };
                if p.get("status").and_then(V::as_str) != Some(want) {
                    p.as_object_mut().unwrap().insert("status".into(), json!(want));
                    changed = true;
                }
            } else if p.get("status").and_then(V::as_str) == Some("proposed") && now >= instant(p.get("expiresAt").unwrap()) {
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
                    let code = e.trim_start_matches("MEDIAN_EXECUTION_");
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
        let now = self.base.now_ms();
        let hint_at = iso(now);
        let mut hints: Vec<V> = Vec::new();
        {
            let mut m = self.m.borrow_mut();
            if m.reference_ready && m.reference_failure.is_none() {
                for q in args.get("protectedBulkHints").and_then(V::as_array).cloned().unwrap_or_default() {
                    // This clock is a metadata-ranking probe only. onBook
                    // independently uses the actual original request and never
                    // dispatches against a bulk quote.
                    if let Ok(r) = m.reference.as_mut().unwrap().reference(&json!({
                        "marketId": q.get("marketId").unwrap().clone(),
                        "exchangeId": q.get("exchangeId").unwrap().clone(),
                        "bookRequestStartedAt": hint_at,
                        "availableAt": hint_at,
                        "bestAsk": q.get("bestAsk").unwrap().clone(),
                    })) {
                        if r.get("outcome").and_then(V::as_str) == Some("reference")
                            && r.pointer("/sourceProof/frontier/sequence").and_then(V::as_i64).unwrap_or(0)
                                >= q.get("sourceSequence").and_then(V::as_i64).unwrap_or(i64::MAX)
                        {
                            let mut h = q.clone();
                            h["outcome"] = r.get("outcome").unwrap().clone();
                            h["reference"] = r.get("reference").unwrap().clone();
                            h["ask"] = r.get("ask").unwrap().clone();
                            hints.push(h);
                        }
                    }
                }
            }
        }
        let cycle = args
            .get("metadataCycle")
            .and_then(V::as_i64)
            .unwrap_or(self.m.borrow().protected_metadata_cycle + 1);
        let plan = plan_median_protected_candidates(
            &self.base.universe,
            self.base.activation.get("runId").and_then(V::as_str).unwrap_or(""),
            &self.scheduling_bank_hash(),
            &args
                .get("candidateMarketIds")
                .and_then(V::as_array)
                .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect::<Vec<String>>())
                .unwrap_or_default(),
            &self
                .base
                .inner
                .borrow()
                .state
                .get("owned")
                .and_then(V::as_object)
                .map(|o| {
                    o.values()
                        .filter_map(|x| x.get("marketId").and_then(V::as_str).map(str::to_string))
                        .collect::<Vec<String>>()
                })
                .unwrap_or_default(),
            &self
                .m
                .borrow()
                .proposals
                .get("proposals")
                .and_then(V::as_object)
                .map(|o| {
                    o.values()
                        .filter(|p| {
                            p.get("status").and_then(V::as_str) == Some("proposed")
                                && instant(p.get("expiresAt").unwrap_or(&V::Null)) > now
                        })
                        .cloned()
                        .collect::<Vec<V>>()
                })
                .unwrap_or_default(),
            &hints,
            now,
            cycle,
            &self.m.borrow().protected_metadata_cycles.clone(),
        )?;
        {
            let mut m = self.m.borrow_mut();
            m.candidate_ids = plan
                .get("nextMetadataMarketId")
                .and_then(V::as_str)
                .map(|s| vec![s.to_string()])
                .unwrap_or_default();
        }
        let mut args2 = args.clone();
        args2
            .as_object_mut()
            .unwrap()
            .insert("candidateMarketIds".into(), plan.get("candidateMarketIds").unwrap().clone());
        self.base.before_books(&args2)?;
        {
            let mut m = self.m.borrow_mut();
            m.protected_metadata_cycle = cycle;
            if let Some(next_id) = plan.get("nextMetadataMarketId").and_then(V::as_str) {
                if self.base.inner.borrow().selected.contains(&next_id.to_string()) {
                    m.protected_metadata_cycles = plan.get("lastMetadataCycles").unwrap().clone();
                }
            }
            m.last_protected_metadata_plan = plan;
        }
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
                .is_some_and(|a| a.iter().any(|x| x.get("marketId").and_then(V::as_str) == Some(market_id))),
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
        // that verified frontier before the original book request, preserving
        // the native newest-point exclusion rather than the earlier protected-read view.
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
                    let code = e.trim_start_matches("MEDIAN_EXECUTION_");
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
        let response = self.base.transport.market(market_id).map_err(|e| e.code)?;
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
                && (*k == "cash" || (p.get("complete") == Some(&V::Bool(true)) && p.get("rows").is_some_and(V::is_array)))
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

    /// v2 entryReferenceAdmitted — installed on inner.entry_reference_gate.
    pub fn entry_reference_admitted(&self, i: &V, permit: &V, now: i64) -> bool {
        median_v2_reference_gate(
            &self.m,
            &self.base.inner,
            &self.base.activation,
            &self.base.universe,
            &self.workspace,
            &self.base.runtime_dir,
            i,
            permit,
            now,
        )
    }

    pub fn check_progress(&mut self, book: &V) -> Result<(), String> {
        let market_id = book.get("marketId").and_then(V::as_str).unwrap_or("");
        let prior = self.m.borrow().proposals.pointer(&format!("/books/{market_id}")).cloned();
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

    /// ownedPolicy: policy selected by the entry's modelHash (prior vs next bank).
    pub fn owned_policy(&mut self, book: &V) -> Result<Option<V>, String> {
        let exchange_id = book.get("exchangeId").and_then(V::as_str).unwrap_or("");
        let own = self
            .base
            .inner
            .borrow()
            .state
            .pointer(&format!("/owned/{exchange_id}"))
            .cloned();
        let Some(own) = own.filter(|o| !o.is_null()) else { return Ok(None) };
        let _ = own;
        let entry = self.owned_entry(exchange_id)?;
        let policy = {
            let m = self.m.borrow();
            if entry.pointer("/modelHash").and_then(V::as_str)
                == self.base.activation.get("strategyBankHash").and_then(V::as_str)
            {
                m.prior_bank.get("policy").cloned().unwrap_or(V::Null)
            } else {
                m.bank.get("policy").cloned().unwrap_or(V::Null)
            }
        };
        let cost = micros(own.get("cost").unwrap_or(&V::Null));
        let bid = micros(book.pointer("/bids/0/price").unwrap_or(&V::Null));
        let at = instant(entry.get("filledAt").unwrap_or(&V::Null));
        let stop_loss_ticks = policy.get("stopLossTicks").and_then(V::as_i64).unwrap_or(0) as i128;
        let maximum_hold_ms = policy.get("maximumHoldMs").and_then(V::as_i64).unwrap_or(0);
        let take_profit_ticks = policy.get("takeProfitTicks").and_then(V::as_i64).unwrap_or(0) as i128;
        let risk = if bid <= cost - stop_loss_ticks * TICK {
            Some("stop_loss")
        } else if self.base.now_ms() - at >= maximum_hold_ms {
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
                "limit": format_micros(if bid > TICK { bid - TICK } else { TICK }),
            })));
        }
        let target = cost + take_profit_ticks * TICK;
        if bid >= target {
            return Ok(Some(json!({
                "reason": "profit_target",
                "limit": format_micros(if bid - TICK > target { bid - TICK } else { target }),
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
        let mut proof = json!({
            "bankHash": self.policy_bank_hash(),
            "revisionHash": self.m.borrow().revision_hash,
            "paperBinding": self.m.borrow().bank.get("paperBinding").unwrap().clone(),
            "reference": reference.cloned().unwrap_or(V::Null),
            "originalBook": book.clone(),
            "decisionAvailableAt": created_at,
            "referencePolicy": serde_json::from_str::<V>(MEDIAN_REFERENCE_POLICY).unwrap(),
        });
        if action == "sell" {
            proof["ownedEntry"] = self.owned_entry(book.get("exchangeId").and_then(V::as_str).unwrap_or(""))?;
        }
        let p = json!({
            "intentId": intent_id,
            "idempotencyKey": uuid::Uuid::new_v4().to_string(),
            "bankHash": self.policy_bank_hash(),
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
                    "modelHash": p.get("bankHash").unwrap().clone(),
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
                "bankHash": p.get("bankHash").unwrap().clone(),
                "revisionHash": self.m.borrow().revision_hash,
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
                        "modelHash": p.get("bankHash").unwrap().clone(),
                        "marketId": market_id,
                        "exchangeId": b.get("exchangeId").unwrap().clone(),
                    },
                }),
            )
            .map_err(|e| format!("MEDIAN_EXECUTION_LEDGER:{:?}", e))?;
            self.base.commit(&r, &json!({"type": "median_intent_prepared", "intentId": p.get("intentId").unwrap().clone(), "referenceProof": reference_proof}))?;
            self.sync_proposals()?;
            self.base.submit(r.get("intent").unwrap(), Some(&snapshot), false, Some(&reference_proof))?;
            if self.base.can_operate() {
                self.sync_proposals()?;
            }
            return Ok(());
        }
        if owned.is_some() {
            if let Some(exit) = exit {
                self.create_proposal(&b, "sell", exit.get("limit").and_then(V::as_str).unwrap_or(""), exit.get("reason").and_then(V::as_str).unwrap_or(""), None)?;
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
                    .is_some_and(|rows| rows.iter().any(|x| x.get("exchangeId").and_then(V::as_str) == Some(exchange_id)))
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
            let at = instant(
                i.pointer("/reconciledFills")
                    .and_then(V::as_array)
                    .and_then(|a| a.last())
                    .and_then(|f| f.get("filledAt"))
                    .unwrap_or(&V::Null),
            );
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
        if self.base.inner.borrow().state.get("owned").and_then(V::as_object).map(|o| o.len()).unwrap_or(0)
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
            self.base.inner.borrow_mut().reason = reference.get("reason").and_then(V::as_str).map(str::to_string);
            self.publish();
            return Ok(());
        }
        let original_limit = micros(b.pointer("/asks/0/price").unwrap_or(&V::Null)) + TICK;
        let limit = micros(&json!(median_entry_cap_limit(&b, &reference)?));
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
        let reserved: i128 = pending.iter().map(|x| micros(x.get("limit").unwrap_or(&V::Null))).sum();
        // Retain the baseline's conservative proposal budget admission. Only
        // the actual fixed new proposal limit and its reservation become smaller.
        let budget = micros(self.base.activation.get("maxCumulativeBuyCost").unwrap_or(&V::Null));
        if original_limit > 995000 || spent + reserved + original_limit > budget {
            return Ok(());
        }
        if limit > 995000 || spent + reserved + limit > budget {
            return Ok(());
        }
        self.create_proposal(
            &b,
            "buy",
            &format_micros(limit),
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
                            "bankHash": p.get("bankHash").unwrap().clone(),
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
        if let Some(o) = s.as_object_mut() {
            o.insert("strategyId".into(), self.base.activation.get("strategyId").unwrap().clone());
            o.insert(
                "strategyBankHash".into(),
                self.base.activation.get("strategyBankHash").unwrap().clone(),
            );
            o.insert("policyBankHash".into(), json!(self.policy_bank_hash()));
            o.insert("schedulingBankHash".into(), json!(self.scheduling_bank_hash()));
            o.insert(
                "medianRevision".into(),
                json!({
                    "revisionId": m.revision.get("revisionId").unwrap().clone(),
                    "revisionHash": m.revision_hash,
                    "priorBankHash": self.base.activation.get("strategyBankHash").unwrap().clone(),
                    "nextBankHash": m.revision.get("nextBankHash").unwrap().clone(),
                    "activationHash": self.base.activation_hash.as_str(),
                    "entryExpiresAt": self.base.activation.get("entryExpiresAt").unwrap().clone(),
                }),
            );
            o.insert(
                "median".into(),
                json!({
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
                }),
            );
        }
        let _ = atomic_json(&file, &s);
    }
    pub fn close(&mut self) -> Result<(), String> {
        self.base.close()
    }
}

fn assert_revision_intents(
    inner: &Rc<RefCell<crate::controller::ControllerInner>>,
    m: &Rc<RefCell<MedianV2State>>,
    base: &CompetitionController,
) -> Result<(), String> {
    let durable = base.ledger.borrow().snapshot().unwrap_or_else(|_| json!({}));
    let durable_intents = durable.get("intents").cloned().unwrap_or(json!({}));
    let inner = inner.borrow();
    let mm = m.borrow();
    for (id, i) in mm.legacy_core.get("intents").and_then(V::as_object).cloned().unwrap_or_default() {
        need(
            payload_hash(inner.state.get("intents").and_then(V::as_object).and_then(|x| x.get(&id)).unwrap_or(&V::Null))
                == payload_hash(&i),
            "LEGACY_INTENT_CHANGED",
        )?;
    }
    for (iid, i) in inner.state.get("intents").and_then(V::as_object).cloned().unwrap_or_default() {
        let p = mm
            .proposals
            .pointer(&format!("/proposals/{}", i.get("intentId").and_then(V::as_str).unwrap_or("")))
            .cloned()
            .unwrap_or(V::Null);
        let a = durable_intents
            .pointer(&format!("/{}/attribution", i.get("intentId").and_then(V::as_str).unwrap_or("")))
            .cloned();
        need(
            !p.is_null()
                && p.get("idempotencyKey") == i.get("idempotencyKey")
                && i.pointer("/attribution/modelHash") == p.get("bankHash")
                && i.pointer("/attribution/runId") == base.activation.get("runId")
                && i.pointer("/attribution/strategyId") == base.activation.get("strategyId")
                && i.get("marketId") == p.get("marketId")
                && i.get("exchangeId") == p.get("exchangeId")
                && i.get("action") == p.get("action")
                && i.get("quantity") == p.get("quantity")
                && micros(i.get("price").unwrap_or(&V::Null)) == micros(p.get("limit").unwrap_or(&V::Null)),
            "INTENT_PROPOSAL_MISMATCH",
        )?;
        let _ = iid;
        need(
            a.as_ref().and_then(|x| x.get("ownerId")).and_then(V::as_str) == Some("Codex")
                && a.as_ref().and_then(|x| x.get("runId")) == base.activation.get("runId")
                && a.as_ref().and_then(|x| x.get("modelHash")) == i.pointer("/attribution/modelHash")
                && a.as_ref().and_then(|x| x.get("marketId")) == i.get("marketId")
                && a.as_ref().and_then(|x| x.get("exchangeId")) == i.get("exchangeId")
                && a.as_ref().and_then(|x| x.get("strategyId")) == i.pointer("/attribution/strategyId"),
            "LEDGER_ATTRIBUTION_MISMATCH",
        )?;
    }
    // For each owned exchange, ownedEntry must validate — called via the same
    // FIFO walk (duplicated here since the method lives on the controller).
    let policy_bank_hash = mm.revision.get("nextBankHash").and_then(V::as_str).unwrap_or("").to_string();
    for id in inner.state.get("owned").and_then(V::as_object).map(|o| o.keys().cloned().collect::<Vec<_>>()).unwrap_or_default() {
        let own = inner.state.pointer(&format!("/owned/{id}")).cloned().unwrap();
        let mut intents: Vec<V> = inner
            .state
            .get("intents")
            .and_then(V::as_object)
            .map(|x| x.values().cloned().collect())
            .unwrap_or_default();
        intents.sort_by_key(|i| i.get("ordinal").and_then(V::as_i64).unwrap_or(0));
        let mut entry = V::Null;
        for i in intents.iter().filter(|i| i.get("exchangeId").and_then(V::as_str) == Some(id.as_str())) {
            for f in i.get("reconciledFills").and_then(V::as_array).cloned().unwrap_or_default() {
                need(f.get("quantity").and_then(V::as_i64) == Some(1), "OWNED_FILL")?;
                if i.get("action").and_then(V::as_str) == Some("buy") {
                    need(entry.is_null(), "OWNED_QUANTITY")?;
                    entry = json!({
                        "intentId": i.get("intentId").unwrap().clone(),
                        "modelHash": i.pointer("/attribution/modelHash").unwrap().clone(),
                        "fillId": f.get("id").unwrap().clone(),
                        "cost": f.get("price").unwrap().clone(),
                        "filledAt": f.get("filledAt").unwrap().clone(),
                        "marketId": i.get("marketId").unwrap().clone(),
                    });
                } else {
                    need(!entry.is_null(), "OWNED_PROVENANCE")?;
                    entry = V::Null;
                }
            }
        }
        need(
            !entry.is_null()
                && own.get("quantity").and_then(V::as_i64) == Some(1)
                && own.get("marketId") == entry.get("marketId")
                && micros(own.get("cost").unwrap_or(&V::Null)) == micros(entry.get("cost").unwrap_or(&V::Null))
                && [
                    base.activation.get("strategyBankHash").and_then(V::as_str).unwrap_or("").to_string(),
                    policy_bank_hash.clone(),
                ]
                .contains(&entry.pointer("/modelHash").and_then(V::as_str).unwrap_or("").to_string()),
            "OWNED_BANK_OR_QUANTITY",
        )?;
        if entry.pointer("/modelHash").and_then(V::as_str) == base.activation.get("strategyBankHash").and_then(V::as_str) {
            need(
                mm.legacy_core
                    .pointer(&format!("/intents/{}", entry.get("intentId").and_then(V::as_str).unwrap_or("")))
                    .is_some(),
                "LEGACY_LOT_NOT_AT_REVISION",
            )?;
        }
    }
    Ok(())
}

/// v2 entryReferenceAdmitted — invoked through the transport fences.
fn median_v2_reference_gate(
    m: &Rc<RefCell<MedianV2State>>,
    inner: &Rc<RefCell<crate::controller::ControllerInner>>,
    activation: &V,
    universe: &V,
    workspace: &Path,
    runtime_dir: &Path,
    i: &V,
    permit: &V,
    now_ms: i64,
) -> bool {
    let current = inner.borrow().current_permit.clone().unwrap_or(V::Null);
    // Expired identical recovery is not a new order, even after the entry window.
    if permit.get("retry") == Some(&V::Bool(true))
        && current.get("retry") == Some(&V::Bool(true))
        && i.get("attempts").and_then(V::as_i64).unwrap_or(0) > 0
        && now_ms >= instant(i.get("expirationDate").unwrap_or(&V::Null))
    {
        return true;
    }
    // bankCurrent(): revision immutable, both banks still valid.
    {
        let mm = m.borrow();
        let revision: V = std::fs::read_to_string(&mm.revision_file)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(V::Null);
        if payload_hash(&revision).ok().as_deref() != Some(mm.revision_hash.as_str()) {
            return false;
        }
        let bank: V = std::fs::read_to_string(&mm.bank_file)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(V::Null);
        let prior_bank: V = std::fs::read_to_string(&mm.prior_bank_file)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(V::Null);
        if validate_median_revision(&mm.revision, &bank, &prior_bank, activation, universe, workspace, &mm.archive_dir, now_ms).is_err() {
            return false;
        }
    }
    let _ = runtime_dir;
    let policy_bank_hash = m.borrow().revision.get("nextBankHash").and_then(V::as_str).unwrap_or("").to_string();
    let revision_hash = m.borrow().revision_hash.clone();
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
        && p.get("bankHash").and_then(V::as_str) == Some(policy_bank_hash.as_str())
        && i.pointer("/attribution/modelHash") == p.get("bankHash")
        && ["proposed", "dispatched"].contains(&p.get("status").and_then(V::as_str).unwrap_or(""))
        && !proof.is_null()
        && proof.get("bankHash") == p.get("bankHash")
        && proof.get("revisionHash").and_then(V::as_str) == Some(revision_hash.as_str())
        && proof.get("proposalHash").and_then(V::as_str) == payload_hash(p.get("proof").unwrap_or(&V::Null)).ok().as_deref()
        && p.get("proofHash").and_then(V::as_str) == payload_hash(p.get("proof").unwrap_or(&V::Null)).ok().as_deref()
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
    if p.get("action").and_then(V::as_str) == Some("buy")
        && p.get("limit").and_then(V::as_str)
            != median_entry_cap_limit(
                p.get("originalBook").unwrap(),
                p.pointer("/proof/reference").unwrap_or(&V::Null),
            )
            .ok()
            .as_deref()
    {
        return false;
    }
    let snapshot_book = snapshot.get("book").cloned().unwrap_or(V::Null);
    if snapshot_book.get("marketId") != b.get("marketId")
        || snapshot_book.get("exchangeId") != b.get("exchangeId")
        || snapshot_book.get("at") != b.get("receivedAt")
        || snapshot_book.get("requestStartedAt") != b.get("requestStartedAt")
        || payload_hash(&json!({"bids": snapshot_book.get("bids").unwrap_or(&V::Null).clone(), "asks": snapshot_book.get("asks").unwrap_or(&V::Null).clone()})).ok()
            != payload_hash(&json!({"bids": b.get("bids").unwrap_or(&V::Null).clone(), "asks": b.get("asks").unwrap_or(&V::Null).clone()})).ok()
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
    if i.get("action").and_then(V::as_str) == Some("sell") {
        let owned_qty = inner
            .borrow()
            .state
            .pointer(&format!("/owned/{}/quantity", i.get("exchangeId").and_then(V::as_str).unwrap_or("")))
            .and_then(V::as_i64);
        if owned_qty != Some(1) {
            return false;
        }
        // payloadHash(p.proof.ownedEntry) === payloadHash(ownedEntry(exchangeId))
        let exchange_id = i.get("exchangeId").and_then(V::as_str).unwrap_or("");
        let own = inner.borrow().state.pointer(&format!("/owned/{exchange_id}")).cloned();
        let entry = (|| -> Option<V> {
            let own = own.filter(|o| !o.is_null())?;
            let mut intents: Vec<V> = inner
                .borrow()
                .state
                .get("intents")
                .and_then(V::as_object)
                .map(|x| x.values().cloned().collect())
                .unwrap_or_default();
            intents.sort_by_key(|i| i.get("ordinal").and_then(V::as_i64).unwrap_or(0));
            let mut entry = V::Null;
            for it in intents.iter().filter(|it| it.get("exchangeId").and_then(V::as_str) == Some(exchange_id)) {
                for f in it.get("reconciledFills").and_then(V::as_array).cloned().unwrap_or_default() {
                    if f.get("quantity").and_then(V::as_i64) != Some(1) {
                        return None;
                    }
                    if it.get("action").and_then(V::as_str) == Some("buy") {
                        if !entry.is_null() {
                            return None;
                        }
                        entry = json!({
                            "intentId": it.get("intentId").unwrap().clone(),
                            "modelHash": it.pointer("/attribution/modelHash").unwrap().clone(),
                            "fillId": f.get("id").unwrap().clone(),
                            "cost": f.get("price").unwrap().clone(),
                            "filledAt": f.get("filledAt").unwrap().clone(),
                            "marketId": it.get("marketId").unwrap().clone(),
                        });
                    } else {
                        if entry.is_null() {
                            return None;
                        }
                        entry = V::Null;
                    }
                }
            }
            let _ = own;
            Some(entry)
        })();
        let Some(entry) = entry else { return false };
        if payload_hash(p.pointer("/proof/ownedEntry").unwrap_or(&V::Null)).ok() != payload_hash(&entry).ok() {
            return false;
        }
    }
    true
}
