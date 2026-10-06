// execution/entry-path-quality-audit.mjs — replays verified publication +
// core journal, recomputes entry path quality for each attributed entry.
use crate::candidate_markouts_audit::book_publication::{apply_book_publication, create_book_publication_state};
use crate::entry_path_quality::entry_path_quality;
use baal_service_runtime::json_text::{digest, js_int};
use serde_json::{json, Value as V};
use std::path::Path;

// lines(file): bytes through last '\n' only, then trim + split + JSON.parse.
fn lines(file: &Path) -> Result<Vec<V>, String> {
    let bytes = std::fs::read(file).map_err(|e| e.to_string())?;
    let end = bytes.iter().rposition(|b| *b == b'\n').map(|i| i + 1).unwrap_or(0);
    if end == 0 {
        return Err("No complete evidence".into());
    }
    let text = String::from_utf8_lossy(&bytes[..end]);
    text.trim()
        .split('\n')
        .map(|l| serde_json::from_str(l).map_err(|e| e.to_string()))
        .collect()
}
/// auditEntryPaths(runtimeDir, universe, referenceReport)
pub fn audit_entry_paths(runtime_dir: &Path, universe: &V, reference_report: &V) -> Result<V, String> {
    let mut publication = create_book_publication_state(universe)?;
    let mut books: Vec<V> = Vec::new();
    let mut invalidations: Vec<V> = Vec::new();
    for row in &lines(&runtime_dir.join("public-books.jsonl"))? {
        publication = apply_book_publication(&publication, row)?;
        if row["kind"] == json!("book") {
            books.push(row["payload"]["book"].clone());
        }
        if row["kind"] == json!("invalidation") {
            invalidations.push(json!({"marketId": row["payload"]["marketId"], "at": row["at"]}));
        }
        if row["kind"] == json!("session_start") || row["kind"] == json!("session_stop") {
            invalidations.push(json!({"marketId": V::Null, "at": row["at"]}));
        }
    }
    let core = lines(&runtime_dir.join("competition/core-journal.jsonl"))?;
    let mut seq: i64 = 0;
    let mut previous = String::new();
    for row in &core {
        let mut body = row.clone();
        let hash = body.as_object_mut().and_then(|b| b.shift_remove("hash")).unwrap_or(V::Null);
        if js_int(&row["sequence"]) != Some(seq + 1)
            || row["previous"].as_str().unwrap_or("\u{0}") != previous
            || hash.as_str().unwrap_or("") != digest(&body)
        {
            return Err("Core journal chain failed".into());
        }
        seq += 1;
        previous = hash.as_str().unwrap().to_string();
    }
    // JS referenceReport.entries.map: non-array -> TypeError.
    let report_entries = match reference_report.get("entries").and_then(V::as_array) {
        Some(a) => a.clone(),
        None => {
            return Err(match reference_report.get("entries") {
                None => "Cannot read properties of undefined (reading 'map')".into(),
                Some(V::Null) => "Cannot read properties of null (reading 'map')".into(),
                _ => "referenceReport.entries.map is not a function".into(),
            })
        }
    };
    fn key_of(v: &V) -> String {
        match v {
            V::String(s) => s.clone(),
            V::Number(_) => baal_service_runtime::json_text::js_stringify(v),
            V::Bool(b) => b.to_string(),
            V::Null => "null".into(),
            V::Array(a) => a.iter().map(|x| if x.is_null() { String::new() } else { key_of(x) }).collect::<Vec<_>>().join(","),
            V::Object(_) => "[object Object]".into(),
        }
    }
    let mut entries = Vec::new();
    for entry in report_entries {
        let proof = core.iter().find(|r| r.get("hash") == entry.get("coreHash"));
        let intent = proof.and_then(|p| p["state"]["intents"].get(&key_of(&p["event"]["intentId"])));
        // JS || short-circuit: attribution is only dereferenced when the
        // earlier conditions all pass.
        let mut ok = intent.is_some()
            && intent.unwrap()["action"] == json!("buy")
            && intent.unwrap().get("marketId") == entry.get("marketId")
            && intent.unwrap().get("exchangeId") == entry.get("exchangeId")
            && intent.unwrap().get("preparedAt") == entry.get("preparedAt");
        if ok {
            // JS intent.attribution.signalId: null/absent attribution -> TypeError.
            let attribution = intent.unwrap()["attribution"].clone();
            let signal_id = match &attribution {
                V::Object(_) => attribution["signalId"].clone(),
                V::Null => {
                    return Err("Cannot read properties of undefined (reading 'signalId')".into())
                }
                _ => V::Null,
            };
            ok = signal_id == entry["bookVersion"]
                && proof.map(|p| digest(&p["event"]["referenceProof"])) == Some(digest(&entry["referenceProof"]));
        }
        if !ok {
            return Err("Exact entry proof mismatch".into());
        }
        // JS entry.quality.lastObservationAgeMs: null/absent -> TypeError;
        // other primitives -> undefined.
        let quality_age = match entry.get("quality") {
            Some(V::Object(_)) => entry["quality"]["lastObservationAgeMs"].clone(),
            None => return Err("Cannot read properties of undefined (reading 'lastObservationAgeMs')".into()),
            Some(V::Null) => return Err("Cannot read properties of null (reading 'lastObservationAgeMs')".into()),
            _ => V::Null,
        };
        // JS entry.signal.samples/center: null/absent signal -> TypeError;
        // other primitives -> undefined values.
        let signal = entry.get("signal");
        let (samples, center) = match signal {
            Some(V::Object(_)) => (entry["signal"]["samples"].clone(), entry["signal"]["center"].clone()),
            None => return Err("Cannot read properties of undefined (reading 'samples')".into()),
            Some(V::Null) => return Err("Cannot read properties of null (reading 'samples')".into()),
            _ => (V::Null, V::Null),
        };
        let mut out = serde_json::Map::new();
        out.insert("marketId".into(), entry["marketId"].clone());
        out.insert("exchangeId".into(), entry["exchangeId"].clone());
        out.insert("preparedAt".into(), entry["preparedAt"].clone());
        out.insert("referenceLastAgeMs".into(), quality_age);
        out.insert("referenceSamples".into(), samples);
        out.insert("referenceCenter".into(), center);
        out.insert(
            "path".into(),
            entry_path_quality(&json!({
                "books": books,
                "invalidations": invalidations,
                "entry": entry,
                "at": entry["preparedAt"],
            }))?,
        );
        out.insert("economicOutcomeSource".into(), json!("actual_attributed_ledger_only"));
        entries.push(V::Object(out));
    }
    Ok(json!({
        "schemaVersion": 1,
        "at": baal_service_runtime::contract::ms_to_iso(
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0)
        ),
        "coreSequence": seq,
        "coreHash": previous,
        "publicationSequence": publication["sequence"],
        "publicationHash": publication["hash"],
        "entries": entries,
        "networkRequests": 0,
        "competitionOrdersChanged": 0,
        "profitabilityEstablished": false,
        "limitations": [
            "Only pre-entry verified receipts; older entries can precede public publication coverage.",
            "Session/invalidation gaps reset observed context.",
            "Four actual entries do not establish a predictive admission rule.",
        ],
    }))
}
