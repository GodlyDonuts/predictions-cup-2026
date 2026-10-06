// paper/split.mjs — chronological availability-time split manifest.
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use baal_service_runtime::json_text::{canonical_json, sha256};
use serde_json::{json, Value as V};

const MIN_TIME: i64 = -62_135_596_800_000; // Date.parse('0000-01-01T00:00:00.000Z')
const MAX_TIME: i64 = 253_402_300_799_999; // Date.parse('9999-12-31T23:59:59.999Z')
const SAFE: i64 = 9_007_199_254_740_991;
const OPTION_KEYS: &[&str] = &["embargoMs", "labelHorizonMs", "trainEnd", "validationEnd"];

fn timestamp(value: &V, name: &str) -> Result<i64, String> {
    let s = value.as_str().ok_or_else(|| format!("{name} must be a canonical UTC timestamp with milliseconds"))?;
    let ms = parse_iso_ms(s).ok_or_else(|| format!("{name} must be a valid canonical UTC timestamp"))?;
    // Must be canonical millis: roundtrip check.
    let rt = ms_to_iso(ms);
    if rt != s {
        return Err(format!("{name} must be a canonical UTC timestamp with milliseconds"));
    }
    Ok(ms as i64)
}

fn duration(value: &V, name: &str) -> Result<i64, String> {
    match value.as_i64() {
        Some(n) if n >= 0 && n <= SAFE => Ok(n),
        _ => Err(format!("{name} must be a nonnegative safe integer in milliseconds")),
    }
}

fn add_time(time: i64, delta: i64, name: &str) -> Result<i64, String> {
    let result = time + delta;
    if result.abs() > SAFE || !(MIN_TIME..=MAX_TIME).contains(&result) {
        return Err(format!("{name} exceeds the supported timestamp range"));
    }
    Ok(result)
}

/// See split.mjs: train t < trainEnd, validation trainEnd <= t < validationEnd,
/// test t >= validationEnd on receivedAt; symmetric embargo intervals
/// [cut - embargoMs, cut + embargoMs); purge train/validation rows whose closed
/// label interval [t, t+horizon] reaches the next nominal cut.
pub fn chronological_split(events: &[V], options: &V) -> Result<V, String> {
    if !options.is_object() {
        return Err("options must be a plain JSON object".into());
    }
    canonical_json(options).map_err(|_| "options must contain only finite JSON values".to_string())?;
    let mut keys: Vec<&str> = options.as_object().unwrap().keys().map(String::as_str).collect();
    keys.sort();
    if keys != OPTION_KEYS {
        return Err("options must contain exactly trainEnd, validationEnd, labelHorizonMs, embargoMs".into());
    }
    let train_end_v = &options["trainEnd"];
    let validation_end_v = &options["validationEnd"];
    let train_cut = timestamp(train_end_v, "trainEnd")?;
    let validation_cut = timestamp(validation_end_v, "validationEnd")?;
    let label_horizon = duration(&options["labelHorizonMs"], "labelHorizonMs")?;
    let embargo = duration(&options["embargoMs"], "embargoMs")?;
    if train_cut >= validation_cut {
        return Err("trainEnd must precede validationEnd".into());
    }

    let cuts: Vec<(String, i64, i64, i64, i64)> = [("trainEnd", train_cut), ("validationEnd", validation_cut)]
        .into_iter()
        .map(|(name, time)| {
            Ok((
                name.to_string(),
                time,
                add_time(time, -embargo, &format!("{name} minus embargoMs"))?,
                add_time(time, embargo, &format!("{name} plus embargoMs"))?,
                add_time(time, -label_horizon, &format!("{name} minus labelHorizonMs"))?,
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let (_, _, _t_start, t_end, _) = &cuts[0];
    let (_, _, v_start, _, v_purge) = &cuts[1];
    if t_end >= v_start {
        return Err("embargo windows overlap or leave no validation interval".into());
    }
    if t_end >= v_purge {
        return Err("labelHorizonMs and embargoMs leave no leakage-free validation interval".into());
    }

    let source_json = canonical_json(&V::Array(events.to_vec()))?;
    let mut seen_ids = std::collections::HashSet::new();
    let mut previous = i64::MIN;
    let mut parsed = Vec::new();
    for (index, event) in events.iter().enumerate() {
        if !event.is_object() {
            return Err(format!("events[{index}] must be a plain JSON object"));
        }
        for key in ["schemaVersion", "eventId", "receivedAt", "occurredAt"] {
            if event.get(key).is_none() {
                return Err(format!("events[{index}].{key} is required"));
            }
        }
        if event["schemaVersion"] != 1 {
            return Err(format!("events[{index}].schemaVersion must be 1"));
        }
        let event_id = event["eventId"].as_str().unwrap_or("");
        if event_id.is_empty() {
            return Err(format!("events[{index}].eventId must be a nonempty string"));
        }
        if !seen_ids.insert(event_id.to_string()) {
            return Err(format!("duplicate eventId: {event_id}"));
        }
        let received = timestamp(&event["receivedAt"], &format!("events[{index}].receivedAt"))?;
        let occurred = timestamp(&event["occurredAt"], &format!("events[{index}].occurredAt"))?;
        if occurred > received {
            return Err(format!("events[{index}].occurredAt cannot follow receivedAt"));
        }
        if received < previous {
            return Err("events must be sorted by receivedAt ascending".into());
        }
        previous = received;
        let label_end = add_time(received, label_horizon, &format!("events[{index}] label horizon"))?;
        parsed.push((event, received, label_end));
    }

    let mut train_ids: Vec<String> = Vec::new();
    let mut validation_ids: Vec<String> = Vec::new();
    let mut test_ids: Vec<String> = Vec::new();
    let mut excluded = Vec::new();
    for (event, received, label_end) in &parsed {
        let nominal = if *received < train_cut {
            "train"
        } else if *received < validation_cut {
            "validation"
        } else {
            "test"
        };
        let mut reasons: Vec<String> = cuts
            .iter()
            .filter(|(_, _, start, end, _)| received >= start && received < end)
            .map(|(name, _, _, _, _)| format!("embargo:{name}"))
            .collect();
        if nominal == "train" && label_end >= &train_cut {
            reasons.push("label-horizon:trainEnd".into());
        }
        if nominal == "validation" && label_end >= &validation_cut {
            reasons.push("label-horizon:validationEnd".into());
        }
        if reasons.is_empty() {
            match nominal {
                "train" => train_ids.push(event["eventId"].as_str().unwrap().to_string()),
                "validation" => validation_ids.push(event["eventId"].as_str().unwrap().to_string()),
                _ => test_ids.push(event["eventId"].as_str().unwrap().to_string()),
            }
        } else {
            excluded.push(json!({
                "eventId": event["eventId"],
                "receivedAt": event["receivedAt"],
                "nominalPartition": nominal,
                "reasons": reasons,
            }));
        }
    }

    let first = events.first();
    let last = events.last();
    let (n_train, n_validation, n_test, n_excluded) =
        (train_ids.len(), validation_ids.len(), test_ids.len(), excluded.len());
    Ok(json!({
        "schemaVersion": 1,
        "kind": "chronological-split",
        "basis": "receivedAt",
        "parameters": {
            "trainEnd": train_end_v,
            "validationEnd": validation_end_v,
            "labelHorizonMs": label_horizon,
            "embargoMs": embargo,
        },
        "source": {
            "checksumAlgorithm": "sha256",
            "checksumEncoding": "canonical-json-v1",
            "checksum": sha256(source_json.as_bytes()),
            "eventCount": events.len(),
            "firstReceivedAt": first.map(|e| e["receivedAt"].clone()).unwrap_or(V::Null),
            "lastReceivedAt": last.map(|e| e["receivedAt"].clone()).unwrap_or(V::Null),
        },
        "semantics": {
            "partitionIntervals": "[startInclusive, endExclusive); null means unbounded",
            "embargoIntervals": "[startInclusive, endExclusive); zero width excludes nothing",
            "labelInterval": "[receivedAt, receivedAt + labelHorizonMs]",
            "purge": "Exclude train/validation rows when label end >= the next nominal cut",
            "timestampGroups": "All rows with the same receivedAt share one assignment or exclusion",
            "claims": "Split assignment only; no model training, label-maturity or performance claim",
        },
        "embargoWindows": cuts.iter().map(|(name, _, start, end, _)| json!({
            "cut": name,
            "startInclusive": ms_to_iso(*start as f64),
            "endExclusive": ms_to_iso(*end as f64),
        })).collect::<Vec<_>>(),
        "partitions": {
            "train": {"startInclusive": V::Null, "endExclusive": train_end_v, "eventIds": train_ids},
            "validation": {"startInclusive": train_end_v, "endExclusive": validation_end_v, "eventIds": validation_ids},
            "test": {"startInclusive": validation_end_v, "endExclusive": V::Null, "eventIds": test_ids},
        },
        "excluded": excluded,
        "counts": {
            "source": events.len(),
            "train": n_train,
            "validation": n_validation,
            "test": n_test,
            "excluded": n_excluded,
        },
    }))
}
