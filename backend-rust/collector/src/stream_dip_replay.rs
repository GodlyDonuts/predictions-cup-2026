// live-paper/stream-dip-replay.mjs — bounded read-only development replay of a
// stream-book cohort journal. It is not a prospective wallet and never
// substitutes recorded gateway availability for an actual live decision.
// (process.argv CLI block intentionally not ported — no sibling pattern.)
use crate::stream_book_adapter::{
    advance_stream_book_adapter, create_stream_book_adapter, stream_digest,
};
use crate::stream_dip::{
    advance_stream_dip, create_stream_dip, stream_dip_hash, stream_dip_policy,
    stream_dip_report, warm_stream_dip,
};
use baal_service_runtime::json_text::{js_int, sha256};
use serde_json::{json, Value as V};

fn need(ok: bool, code: &str) -> Result<(), String> {
    if !ok { return Err(code.into()); }
    Ok(())
}
/// replayStreamDip({inputBank,source,checkpoints,markets?})
/// source is the raw capture journal bytes; checkpoints parsed journal rows.
pub fn replay_stream_dip(input_bank: &V, source: &[u8], checkpoints: &[V], markets: Option<&V>)
    -> Result<V, String> {
    let prefix_bytes = js_int(&input_bank["sourcePrefixBytes"]).unwrap_or(0).max(0) as usize;
    need(
        sha256(&source[..prefix_bytes.min(source.len())])
            == input_bank["sourceHash"].as_str().unwrap_or("")
            && prefix_bytes <= source.len(),
        "STREAM_DIP_REPLAY_PREFIX",
    )?;
    let mut adapter = create_stream_book_adapter(input_bank)?;
    let mut offset = 0usize;
    let mut index: i64 = 0;
    let mut previous = stream_digest(input_bank);
    let mut checkpoint_count: i64 = 0;
    let mut warm: Vec<V> = Vec::new();
    let markets = markets.cloned().unwrap_or_else(|| input_bank["scope"]["markets"].clone());
    let bank = json!({"schemaVersion":1,"mode":"stream_native_dip_paper",
        "tournamentId":input_bank["scope"]["tournamentId"],
        "inputBankHash":stream_digest(input_bank),"cutoff":input_bank["cutoff"],
        "startEventId":0,"markets":markets,"policy":stream_dip_policy()});
    // next(): read one newline-terminated row, verify sequential captureIndex.
    let next = |offset: &mut usize, index: &mut i64| -> Result<(V, Vec<u8>), String> {
        let end = source[*offset..].iter().position(|&b| b == 10).map(|p| *offset + p);
        let end = match end {
            Some(e) => e,
            None => return Err("STREAM_DIP_REPLAY_SOURCE_TRUNCATED".into()),
        };
        let raw = source[*offset..=end].to_vec();
        *offset = end + 1;
        let row: V = serde_json::from_slice(&raw).map_err(|_| "Unexpected token in JSON".to_string())?;
        let expected = *index;
        *index += 1;
        need(
            js_int(&row["captureIndex"]) == Some(expected),
            "STREAM_DIP_REPLAY_CAPTURE_INDEX",
        )?;
        Ok((row, raw))
    };
    while index < js_int(&input_bank["sourcePrefixRecords"]).unwrap_or(0) {
        let (row, _raw) = next(&mut offset, &mut index)?;
        let before = adapter["latest"].clone();
        adapter = advance_stream_book_adapter(
            &adapter, &row, input_bank,
            input_bank["cutoff"].as_str().unwrap_or(""),
        )?["state"].clone();
        for (id, e) in adapter["latest"].as_object().cloned().unwrap_or_default() {
            // adapter.latest is deep-cloned each advance, so e !== before[id]
            // always; the effective test is digest-vs-old or missing.
            if before[id.as_str()].is_null()
                || stream_digest(&e) != stream_digest(&before[id.as_str()])
            {
                warm.push(e);
            }
        }
    }
    let mut state = warm_stream_dip(&create_stream_dip(&bank)?, &json!(warm), &bank)?;
    let mut last_at = bank["cutoff"].as_str().unwrap_or("").to_string();
    let first_hash = stream_dip_hash(&state);
    let mut events_consumed: i64 = 0;
    for saved in checkpoints {
        let mut body = saved.as_object().cloned().unwrap_or_default();
        let actual = body.remove("hash").unwrap_or(V::Null);
        checkpoint_count += 1;
        need(
            actual.as_str() == Some(stream_digest(&V::Object(body.clone())).as_str())
                && js_int(&body["sequence"]) == Some(checkpoint_count)
                && body["previous"].as_str() == Some(previous.as_str())
                && body["bankHash"].as_str()
                    == Some(stream_digest(input_bank).as_str()),
            "STREAM_DIP_REPLAY_CHAIN",
        )?;
        previous = actual.as_str().unwrap_or("").to_string();
        if body["kind"].as_str() == Some("prefix") {
            need(
                checkpoint_count == 1
                    && body["stateHash"].as_str()
                        == Some(stream_digest(&adapter).as_str()),
                "STREAM_DIP_REPLAY_PREFIX_STATE",
            )?;
        } else {
            need(body["kind"].as_str() == Some("capture"), "STREAM_DIP_REPLAY_KIND")?;
            let (row, raw) = next(&mut offset, &mut index)?;
            need(
                sha256(&raw) == body["sourceLineHash"].as_str().unwrap_or("")
                    && js_int(&row["captureIndex"]) == js_int(&body["sourceCaptureIndex"]),
                "STREAM_DIP_REPLAY_SOURCE_HASH",
            )?;
            let available_at = body["availableAt"].as_str().unwrap_or("").to_string();
            let out = advance_stream_book_adapter(
                &adapter, &row, input_bank, &available_at,
            )?;
            need(
                stream_digest(&out["events"])
                    == stream_digest(&body["events"]),
                "STREAM_DIP_REPLAY_EVENTS",
            )?;
            adapter = out["state"].clone();
            state = advance_stream_dip(&state, &out["events"], &bank,
                &json!({"availableAt":available_at}))?;
            events_consumed += out["events"].as_array().map(|a| a.len() as i64).unwrap_or(0);
            last_at = available_at;
        }
        need(
            body["stateHash"].as_str() == Some(stream_digest(&adapter).as_str()),
            "STREAM_DIP_REPLAY_INPUT_STATE",
        )?;
    }
    let mut report = stream_dip_report(&state, &bank, &last_at)?
        .as_object().cloned().unwrap_or_default();
    report.insert("mode".into(), json!("retrospective_stream_dip_development_replay"));
    report.insert("sourcePrefixHash".into(), input_bank["sourceHash"].clone());
    report.insert("inputCheckpointCount".into(), json!(checkpoint_count));
    report.insert("inputJournalHash".into(), json!(previous));
    report.insert("inputCaptureIndex".into(), adapter["capture"]["captureIndex"].clone());
    report.insert("sourceBytes".into(), json!(offset));
    report.insert("eventsConsumed".into(), json!(events_consumed));
    report.insert("initialStateHash".into(), json!(first_hash));
    report.insert("finalStateHash".into(), json!(stream_dip_hash(&state)));
    report.insert("evidence".into(),
        json!("actual_capture_replayed_with_recorded_gateway_availability_not_a_live_strategy_run"));
    report.insert("limitations".into(), json!([
        "Replay used for correctness and diagnosis only; not held-out or prospective performance evidence.",
        "Gateway availability stands in for a consumer during this replay. Live wallets must persist their own actual consumption timestamps."
    ]));
    Ok(json!({"bank":bank,"state":state,"warm":warm,"verifiedInputState":adapter,
        "report":V::Object(report)}))
}
