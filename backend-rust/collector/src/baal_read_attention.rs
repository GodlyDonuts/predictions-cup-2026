// live-paper/baal-read-attention.mjs — auxiliary (tape+history) read budget
// reservations for the BAAL collector.

use baal_service_runtime::atomic::atomic_json_canonical;
use baal_service_runtime::json_text::js_int;
use baal_service_runtime::ledger::payload_hash;
use crate::trade_tape::select_auxiliary_reads;
use serde_json::{json, Value as V};
use std::fs;
use std::path::{Path, PathBuf};

fn need(ok: bool) -> Result<(), String> {
    if !ok { return Err("BAAL_AUXILIARY_STATE_INVALID".into()); }
    Ok(())
}

pub struct BaalReadAttention {
    file: PathBuf,
    state: V,
    can_operate: Box<dyn Fn() -> bool>,
}

impl BaalReadAttention {
    pub fn reserve(&mut self, budget: &V, maker_active: bool) -> Result<V, String> {
        need(js_int(budget).is_some_and(|b| (0..=2).contains(&b)))?;
        if !js_int(budget).is_some_and(|b| b != 0) || !(self.can_operate)() {
            return Ok(json!({"historyReads": 0, "tapeReads": 0, "totalReads": 0,
                "reservedOrdinal": V::Null}));
        }
        let ordinal = js_int(&self.state["cyclesReserved"]).unwrap_or(0);
        let tape_reads: i64 = if maker_active {
            if ordinal % 4 == 3 { 0 } else { 1 }
        } else {
            select_auxiliary_reads(budget, &json!(ordinal))?["tapeReads"].as_i64().unwrap_or(0)
        };
        let mut next = self.state.clone();
        next["cyclesReserved"] = json!(ordinal + 1);
        need(js_int(&next["cyclesReserved"]).is_some())?;
        atomic_json_canonical(&self.file, &next).map_err(|e| e.to_string())?;
        self.state = next;
        Ok(json!({
            "historyReads": js_int(budget).unwrap_or(0) - tape_reads,
            "tapeReads": tape_reads,
            "totalReads": budget.clone(),
            "reservedOrdinal": ordinal,
            "policy": if maker_active { "maker_tape_three_cycles_history_one_v1" }
                else { "alternating_history_tape_v1" },
            "executionAuthority": false,
        }))
    }
}

pub fn create_baal_read_attention(runtime_dir: &Path, tournament_id: &str,
    can_operate: Option<Box<dyn Fn() -> bool>>) -> Result<BaalReadAttention, String> {
    let file = runtime_dir.join("baal-auxiliary-reservations.json");
    let scope_hash = payload_hash(&json!({"tournamentId": tournament_id,
        "policy": "maker_tape_three_cycles_history_one_v1"})).map_err(|e| e.message)?;
    let default_state = || json!({"schemaVersion": 1, "scopeHash": scope_hash, "cyclesReserved": 0});
    let state = if file.exists() {
        serde_json::from_slice::<V>(&fs::read(&file).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?
    } else { default_state() };
    need(state["schemaVersion"].as_i64() == Some(1)
        && state["scopeHash"].as_str() == Some(scope_hash.as_str())
        && js_int(&state["cyclesReserved"]).is_some_and(|n| n >= 0))?;
    Ok(BaalReadAttention {
        file, state,
        can_operate: can_operate.unwrap_or_else(|| Box::new(|| true)),
    })
}
