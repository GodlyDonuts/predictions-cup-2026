// live-paper/basket-complete-watch.mjs — durable complete-pair cohort:
// consumes the shared verified source journal through the incremental reader
// and projects each paired depth measurement with basket-pair-capture.
use crate::basket_observer::basket_report;
use crate::basket_pair_capture::advance_complete_basket_record;
use crate::basket_watch_v2::IncrementalSourceReader;
use crate::model::digest;
use baal_service_runtime::atomic::atomic_json;
use baal_service_runtime::json_text::js_int;
use serde_json::{json, Map, Value as V};
use std::io::Write;
use std::path::{Path, PathBuf};

fn js_str(v: &V) -> String {
    match v {
        V::String(s) => s.clone(),
        V::Number(n) => {
            let mut b = ryu_js::Buffer::new();
            b.format_finite(n.as_f64().unwrap_or(f64::NAN)).to_string()
        }
        V::Bool(b) => if *b { "true".into() } else { "false".into() },
        V::Null => "null".into(),
        _ => "[object Object]".into(),
    }
}
/// JS `===` for JSON values: scalars by value, objects/arrays never equal.
fn strict_eq(a: &V, b: &V) -> bool {
    match (a, b) {
        (V::Null, V::Null) => true,
        (V::Bool(x), V::Bool(y)) => x == y,
        (V::Number(x), V::Number(y)) => x.as_f64() == y.as_f64(),
        (V::String(x), V::String(y)) => x == y,
        _ => false,
    }
}
/// a === b + 1 under JS number semantics.
fn plus1_eq(a: &V, b: &V) -> bool {
    a.is_number() && b.is_number() && a.as_f64() == b.as_f64().map(|x| x + 1.0)
}
/// `records[i]?.hash !== expected` — a missing row/hash (undefined) is only
/// equal to an absent expected key.
fn row_hash_eq(rows: &[V], index: Option<usize>, expected: &V, expected_key_present: bool) -> bool {
    match index.and_then(|i| rows.get(i)) {
        Some(r) => strict_eq(&r["hash"], expected),
        None => !expected_key_present,
    }
}
fn json_index_usize(v: &V) -> Option<usize> {
    // arr[i] hits only for canonical integer indices.
    v.as_f64().filter(|x| x.fract() == 0.0 && *x >= 0.0 && *x <= usize::MAX as f64).map(|x| x as usize)
}
/// records.prefixHashes?.[bank.sourceRecords-1] === bank.sourceHash under !==.
fn prefix_matches(rows_hashes: &[String], bank: &V) -> bool {
    let idx = bank["sourceRecords"].as_f64().map(|x| x - 1.0)
        .filter(|x| x.fract() == 0.0 && *x >= 0.0 && *x <= usize::MAX as f64)
        .map(|x| x as usize);
    match (idx.and_then(|i| rows_hashes.get(i)), bank["sourceHash"].as_str()) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}
/// /^[a-z0-9-]{1,80}$/
fn valid_cohort_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 80
        && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// completeBasketCodeIdentity() — digests of the frozen JS sources relative to
/// the caller's live-paper directory (JS resolves its own dir).
pub fn complete_basket_code_identity(live_paper_dir: &Path) -> Result<V, String> {
    let mut map = Map::new();
    for p in [
        "basket-complete-watch.mjs", "basket-pair-observer.mjs", "basket-pair-capture.mjs",
        "basket-watch.mjs", "basket-observer.mjs", "basket-inputs.mjs", "model.mjs",
        "incremental-journal.mjs", "journal-lines.mjs", "runtime.mjs",
        "../paper/fixed.mjs", "../feed/contract.mjs",
    ] {
        let text = std::fs::read_to_string(live_paper_dir.join(p)).map_err(|e| e.to_string())?;
        map.insert(p.to_string(), json!(digest(&V::from(text))));
    }
    Ok(V::Object(map))
}

/// openCompleteBasketWatch(runtime, name, {initialize=false, definitions, cutoff=now})
/// `cutoff: None` defaults to the current instant, as in JS.
pub struct CompleteBasketWatch {
    dir: PathBuf,
    journal_file: PathBuf,
    reader: IncrementalSourceReader,
    name: String,
    bank: V,
    state: V,
    sequence: i64,
    previous: String,
}
impl CompleteBasketWatch {
    pub fn dir(&self) -> &Path { &self.dir }
    pub fn bank(&self) -> &V { &self.bank }
    pub fn state(&self) -> &V { &self.state }
    /// consume() — None while the source journal has an incomplete tail.
    pub fn consume(&mut self) -> Result<Option<i64>, String> {
        if !self.reader.read()? { return Ok(None); }
        let rows = &self.reader.rows;
        let src_seq = self.state["sourceSequence"].as_f64().unwrap_or(f64::NAN);
        let prior_ok = row_hash_eq(rows,
            json_index_usize(&self.state["sourceSequence"]).map(|i| i.wrapping_sub(1)).filter(|_| src_seq.is_finite()),
            &self.state["lastRecordHash"],
            self.state.as_object().map(|m| m.contains_key("lastRecordHash")).unwrap_or(false));
        if !prefix_matches(&self.reader.prefix_hashes, &self.bank)
            || (rows.len() as f64) < src_seq
            || !prior_ok
        {
            return Err("COMPLETE_BASKET_SOURCE_CHANGED".into());
        }
        let start = if src_seq.is_finite() && src_seq >= 0.0 { (src_seq as usize).min(rows.len()) } else { 0 };
        let mut consumed = 0i64;
        for record in rows[start..].to_vec() {
            let next = advance_complete_basket_record(&self.state, &record, &self.bank)?;
            let mut body = json!({
                "sequence": self.sequence + 1, "previous": self.previous,
                "sourceSequence": record["sequence"], "sourceHash": record["hash"],
                "stateHash": digest(&next["state"]), "observationsHash": digest(&next["observations"]),
                "observations": next["observations"],
            });
            let hash = digest(&body);
            body.as_object_mut().unwrap().insert("hash".into(), json!(hash));
            let bytes = format!("{}\n", baal_service_runtime::json_text::js_stringify(&body));
            let mut fd = std::fs::OpenOptions::new().create(true).append(true).open(&self.journal_file).map_err(|e| e.to_string())?;
            fd.write_all(bytes.as_bytes()).map_err(|e| {
                // 'COMPLETE_BASKET_SHORT_WRITE' only for a zero-byte write.
                if e.kind() == std::io::ErrorKind::WriteZero { "COMPLETE_BASKET_SHORT_WRITE".to_string() } else { e.to_string() }
            })?;
            fd.sync_all().map_err(|e| e.to_string())?;
            drop(fd);
            self.state = next["state"].clone();
            self.sequence += 1;
            self.previous = hash;
            consumed += 1;
        }
        atomic_json(&self.dir.join("state.json"), &self.state)?;
        Ok(Some(consumed))
    }
    pub fn status(&self) -> Result<V, String> {
        let mut pairs = Vec::new();
        for p in self.bank["pairs"].as_array().cloned().unwrap_or_default() {
            let pid = js_str(&p["id"]);
            let report = basket_report(&self.state["pairs"][&pid], &p["bank"])?;
            let mut o = Map::new();
            o.insert("pairId".into(), p["id"].clone());
            if let Some(obj) = report.as_object() {
                for (k, v) in obj { o.insert(k.clone(), v.clone()); }
            }
            pairs.push(V::Object(o));
        }
        Ok(json!({
            "schemaVersion": 1, "cohort": self.name, "mode": "dual_no_complete_pair_measurement",
            "cutoff": self.bank["cutoff"], "bankHash": digest(&self.bank),
            "sourceSequence": self.state["sourceSequence"], "sourceHash": self.state["lastRecordHash"],
            "journalSequence": self.sequence, "journalHash": self.previous,
            "sourceReadMetrics": self.reader.metrics(), "pairs": pairs,
            "networkRequests": 0, "ordersSubmitted": 0, "fills": null, "pnl": null,
            "executionAuthority": false,
        }))
    }
}
pub fn open_complete_basket_watch(runtime: &Path, live_paper_dir: &Path, name: &str,
    initialize: bool, definitions: &V, cutoff: Option<&str>) -> Result<CompleteBasketWatch, String> {
    // JS default `cutoff = new Date().toISOString()` resolves at call time.
    let now_iso;
    let cutoff = match cutoff {
        Some(c) => c,
        None => {
            let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0);
            now_iso = baal_service_runtime::contract::ms_to_iso(ms);
            &now_iso
        }
    };
    if !valid_cohort_name(name) { return Err("INVALID_COMPLETE_BASKET_COHORT".into()); }
    let dir = runtime.join("basket-complete-pair-cohorts").join(name);
    let source = runtime.join("journal.jsonl");
    let bank_file = dir.join("bank.json");
    let journal_file = dir.join("observations.jsonl");
    let identity = complete_basket_code_identity(live_paper_dir)?;
    let mut reader = IncrementalSourceReader::new(&source, 1_048_576, 16_777_216);
    if !reader.read()? { return Err("INCOMPLETE_BASKET_SOURCE".into()); }
    let bank: V;
    if initialize {
        if bank_file.exists() || journal_file.exists() { return Err("REFUSE_COMPLETE_BASKET_REPLACEMENT".into()); }
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let records = crate::ml_shadow::VerifiedSource {
            records: reader.rows.clone(), prefix_hashes: reader.prefix_hashes.clone() };
        bank = crate::basket_watch::create_basket_capture(&records, definitions, cutoff, &identity)?["bank"].clone();
        let pretty = format!("{}\n", serde_json::to_string_pretty(&bank).map_err(|e| e.to_string())?);
        std::fs::OpenOptions::new().write(true).create_new(true).open(&bank_file)
            .and_then(|mut f| f.write_all(pretty.as_bytes())).map_err(|e| e.to_string())?;
        std::fs::OpenOptions::new().write(true).create_new(true).open(&journal_file).map_err(|e| e.to_string())?;
    } else {
        bank = serde_json::from_str(&std::fs::read_to_string(&bank_file).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    }
    if digest(&bank["codeIdentity"]) != digest(&identity) || !prefix_matches(&reader.prefix_hashes, &bank) {
        return Err("COMPLETE_BASKET_FROZEN_INPUT_CHANGED".into());
    }
    let src_idx = bank["sourceRecords"].as_f64().map(|x| x - 1.0)
        .filter(|x| x.fract() == 0.0 && *x >= 0.0 && *x <= usize::MAX as f64)
        .map(|x| x as usize);
    let last_hash = src_idx.and_then(|i| reader.rows.get(i)).map(|r| r["hash"].clone())
        .ok_or_else(|| "Cannot read properties of undefined (reading 'hash')".to_string())?;
    let mut pair_states = Map::new();
    for p in bank["pairs"].as_array().cloned().unwrap_or_default() {
        pair_states.insert(js_str(&p["id"]), p["initialState"].clone());
    }
    let mut state = json!({
        "bankHash": digest(&bank),
        "sourceSequence": bank["sourceRecords"],
        "lastRecordHash": last_hash,
        "pairs": V::Object(pair_states),
    });
    let mut sequence = 0i64;
    let mut previous = digest(&bank);
    let raw = std::fs::read_to_string(&journal_file).map_err(|e| e.to_string())?;
    if !raw.is_empty() && !raw.ends_with('\n') { return Err("INCOMPLETE_COMPLETE_BASKET_JOURNAL".into()); }
    for line in raw.split('\n').filter(|l| !l.is_empty()) {
        let row: V = serde_json::from_str(line).map_err(|e| e.to_string())?;
        let hash = row["hash"].as_str().unwrap_or("").to_string();
        let mut body = row.clone();
        if let Some(m) = body.as_object_mut() { m.shift_remove("hash"); }
        sequence += 1;
        let src_idx = json_index_usize(&body["sourceSequence"]).map(|i| i.wrapping_sub(1));
        let expected_src = plus1_eq(&body["sourceSequence"], &state["sourceSequence"]);
        if hash != digest(&body)
            || js_int(&body["sequence"]) != Some(sequence)
            || body["previous"].as_str() != Some(previous.as_str())
            || !expected_src
            || !row_hash_eq(&reader.rows, src_idx, &body["sourceHash"], body.as_object().map(|m| m.contains_key("sourceHash")).unwrap_or(false))
        {
            return Err("COMPLETE_BASKET_JOURNAL_INTEGRITY".into());
        }
        let rec = src_idx.and_then(|i| reader.rows.get(i))
            .ok_or_else(|| "Cannot read properties of undefined (reading 'sequence')".to_string())?;
        let next = advance_complete_basket_record(&state, rec, &bank)?;
        if digest(&next["state"]) != body["stateHash"].as_str().unwrap_or("")
            || digest(&next["observations"]) != body["observationsHash"].as_str().unwrap_or("")
            || digest(&body["observations"]) != body["observationsHash"].as_str().unwrap_or("")
        {
            return Err("COMPLETE_BASKET_REPLAY_MISMATCH".into());
        }
        state = next["state"].clone();
        previous = hash;
    }
    Ok(CompleteBasketWatch {
        dir, journal_file, reader, name: name.to_string(), bank, state, sequence, previous,
    })
}
