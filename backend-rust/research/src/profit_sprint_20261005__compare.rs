// research/profit-sprint-20261005/compare.mjs — broader primary-poll selection
// against the governor-error model family; writes texas-governor-comparison.json.
// A new public research proposal; it never changes a bank or wallet.
use crate::current_polls_v2__assessment::assess_primary_polls;
use crate::files::{log_json, read_json, write_json_pretty};
use crate::js::num;
use serde_json::{json, Value as V};
use std::path::Path;

fn poll(pollster: &str, democratic: f64, republican: f64, end: &str, file: &str, extra: V) -> V {
    let mut o = serde_json::Map::new();
    o.insert("pollster".into(), json!(pollster));
    o.insert("population".into(), json!("likely_voters"));
    o.insert("democratic".into(), num(democratic));
    o.insert("republican".into(), num(republican));
    o.insert("unallocated".into(), num(100.0 - democratic - republican));
    o.insert("fieldEndDate".into(), json!(end));
    o.insert("fieldEndAt".into(), json!(format!("{end}T23:59:59.999Z")));
    o.insert("file".into(), json!(file));
    if let V::Object(e) = extra {
        for (k, v) in e {
            o.insert(k, v);
        }
    }
    V::Object(o)
}

fn assess(polls: &[V], models: &V, definition: &V, at: &V) -> Result<V, String> {
    let result = assess_primary_polls(&json!({
        "polls": polls,
        "models": models,
        "definition": definition,
        "at": at,
    }))?;
    let mut summary = result.as_object().cloned().unwrap_or_default();
    let scenario_count = summary
        .get("scenarios")
        .and_then(V::as_array)
        .map(|s| s.len() as f64)
        .unwrap_or(0.0);
    summary.shift_remove("scenarios");
    summary.insert("scenarioCount".into(), num(scenario_count));
    Ok(V::Object(summary))
}

pub fn run() -> Result<V, String> {
    let historical = read_json(Path::new(
        "research/governor-error-v1/results-20261005-v1.json",
    ))?;
    let definition = read_json(Path::new("research/current-polls-v2/definition.json"))?;
    let previous = read_json(Path::new(
        "research/current-polls-v3/texas-governor-assessment-20261005-v3.json",
    ))?;
    let sources = read_json(Path::new("research/profit-sprint-20261005/sources.json"))?;
    let at = sources["at"].clone();
    let additions = V::Array(vec![
        poll("Beacon-Shaw-Fox", 47.0, 52.0, "2026-09-28", "fox-texas-topline.pdf", json!({
            "sample": 881, "fieldStartDate": "2026-09-24", "includesLeaners": true,
            "primaryClockVerified": true,
        })),
        poll("TSU", 45.0, 49.0, "2026-09-19", "tsu-september.pdf", json!({
            "sample": 1800, "fieldStartDate": "2026-09-15", "primaryClockVerified": true,
        })),
        poll("BigData", 44.4, 48.9, "2026-09-26", "bigdata-texas.html", json!({
            "sample": 698, "fieldStartDate": "2026-09-24", "includesLeaners": true,
            "primaryClockVerified": true,
        })),
    ]);
    let mut primary_polls = previous["polls"].as_array().cloned().unwrap_or_default();
    primary_polls.extend(additions.as_array().cloned().unwrap_or_default());
    let primary_polls = V::Array(primary_polls);
    let candidate_siena = poll("Siena-NYT", 49.0, 46.0, "2026-09-30", "siena-governors.html", json!({
        "sample": 615, "fieldStartDate": "2026-09-21", "primaryClockVerified": false,
        "fieldClockSource": "UT Texas Politics Project tracker; NYT original unavailable403",
        "publisherAffiliation": "Siena Research Institute is part of ReconMR; correlated with earlier ReconMR source",
    }));
    let candidate_tpor = poll("TPOR", 46.0, 50.0, "2026-09-22", "tpor-september.html", json!({
        "sample": 1007, "fieldStartDate": "2026-09-19", "primaryClockVerified": true,
        "includesLeaners": true,
        "partisanStatus": "publisher claims nonpartisan; external partisan labels are disputed or unverified",
    }));
    let models = &historical["models"];
    let assess_all = |polls: &[V]| -> Result<V, String> {
        assess(polls, models, &definition, &at)
    };
    let mut with_siena = primary_polls.as_array().cloned().unwrap_or_default();
    with_siena.push(candidate_siena.clone());
    let mut with_tpor = primary_polls.as_array().cloned().unwrap_or_default();
    with_tpor.push(candidate_tpor.clone());
    let mut with_both = primary_polls.as_array().cloned().unwrap_or_default();
    with_both.push(candidate_siena.clone());
    with_both.push(candidate_tpor.clone());
    let leave_one: Vec<V> = primary_polls
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|p| {
            let rest: Vec<V> = primary_polls
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(|q| q["pollster"] != p["pollster"])
                .collect();
            Ok(json!({
                "omittedPollster": p["pollster"],
                "assessment": assess_all(&rest)?,
            }))
        })
        .collect::<Result<Vec<V>, String>>()?;
    let record = json!({
        "schemaVersion": 1,
        "id": "texas-governor-broader-primary-comparison-20261005",
        "at": at,
        "marketId": "195",
        "exchangeId": "884",
        "predicateHash": "f111fc44afa587bf162e80e5d6f16aee7d1aef875a5d7cffbb67eb59551bc504",
        "modelSource": "research/governor-error-v1/results-20261005-v1.json",
        "definitionSource": "research/current-polls-v2/definition.json",
        "primaryPolls": primary_polls,
        "unverifiedClockSensitivityPoll": candidate_siena,
        "partisanClassificationSensitivityPoll": candidate_tpor,
        "previousSelectionAtCurrentClock": assess_all(previous["polls"].as_array().cloned().unwrap_or_default().as_slice())?,
        "allVerifiedPrimaryPolls": assess_all(primary_polls.as_array().cloned().unwrap_or_default().as_slice())?,
        "withUnverifiedSienaClockSensitivity": assess_all(&with_siena)?,
        "withTPORClassificationSensitivity": assess_all(&with_tpor)?,
        "withBothSensitivities": assess_all(&with_both)?,
        "leaveOnePollsterOut": leave_one,
        "executionAuthority": false,
        "activeBanksUnchanged": true,
        "profitabilityEstablished": false,
        "limitations": [
            "New sources contradict the prior three-poll selection. Existing banks and their counterfactual fills retain original research identities.",
            "Siena vote shares are publisher-verified but its field clock/sample is only a primary-hosted third-party tracker report, not original NYT methodology. Do not silently model as primary-clock-confirmed.",
            "ReconMR and NYT/Siena are affiliated: their observations are not independent experiments.",
            "This is a broader verified selection, not an exhaustive poll universe. Original Mason-Dixon toplines and additional smaller-pollster publications remain unresolved; pollster inclusion and partisan definitions must not be selected for preferred valuations.",
            "Poll-derived scenario ranges remain judgment values, not calibrated local confidence bounds or short-horizon returns.",
            "All cached catalogue markets settle November4; an October5 same-day realized profit needs executable resale or documented same-exchange pairs, not election settlement."
        ],
    });
    // NOTE: the JS uses a plain overwrite write (no 'wx' flag) here.
    write_json_pretty(
        Path::new("research/profit-sprint-20261005/texas-governor-comparison.json"),
        &record,
    )?;
    let out = json!({
        "at": at,
        "previous": record["previousSelectionAtCurrentClock"]["ordinaryRange"],
        "allPrimary": record["allVerifiedPrimaryPolls"]["ordinaryRange"],
        "central": record["allVerifiedPrimaryPolls"]["centralEstimate"],
        "severe": record["allVerifiedPrimaryPolls"]["severeOpponentAllocationStress"],
        "sienaSensitivity": record["withUnverifiedSienaClockSensitivity"]["ordinaryRange"],
        "sienaCentral": record["withUnverifiedSienaClockSensitivity"]["centralEstimate"],
    });
    log_json(&out);
    Ok(out)
}
