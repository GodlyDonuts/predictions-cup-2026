// research/current-polls-v1/build.mjs — freeze-check the poll/ballot captures,
// derive structural priors from the verified baseline, assess both parties and
// write assessments-20261005-v1.json.
use crate::current_polls_v1__assessment::assess_party_poll;
use crate::election_baseline_v1__model::{parse_csv, presidential_tables};
use crate::election_baseline_v2__model::{predict, race_features_v2, senate_races_v2};
use crate::files::{
    log_json, read_bytes, read_json, read_text, runtime_dir, sha256, write_json_pretty_wx,
};
use crate::js::{js_number, now_iso, num, require_that};
use baal_service_runtime::json_text::js_stringify;
use serde_json::{json, Value as V};
use std::path::Path;

pub fn run() -> Result<V, String> {
    let base = Path::new("research/current-polls-v1");
    let runtime = runtime_dir()?;
    let mut manifests: Vec<V> = Vec::new();
    for f in ["sources.json", "ballot-sources.json"] {
        manifests.push(read_json(&base.join(f))?);
    }
    let captures: Vec<V> = manifests
        .iter()
        .flat_map(|m| m["captures"].as_array().cloned().unwrap_or_default())
        .collect();
    for s in &captures {
        let dir = if s["file"].as_str() == Some("kansas-official.html") {
            "research-sources-current-polls-20261005-ballots"
        } else {
            "research-sources-current-polls-20261005"
        };
        let actual = sha256(&read_bytes(&runtime.join(dir).join(s["file"].as_str().unwrap_or("")))?);
        require_that(
            actual == s["contentHash"].as_str().unwrap_or(""),
            "PRIMARY_SOURCE_CHANGED",
        )?;
    }
    let source = |file: &str| -> V {
        captures
            .iter()
            .find(|s| s["file"].as_str() == Some(file))
            .cloned()
            .unwrap_or(V::Null)
    };
    let model = read_json(Path::new(
        "research/election-baseline-v2/model-summary-affiliation-invariant.json",
    ))?;
    let senate = senate_races_v2(
        &parse_csv(&read_text(Path::new(
            "research/election-baseline-v1/sources/1976-2024-senate-state.csv",
        ))?)?,
        &read_json(Path::new(
            "research/election-baseline-v2/final-stage-overrides.json",
        ))?
        .as_array()
        .cloned()
        .unwrap_or_default(),
        true,
    )?;
    let president = presidential_tables(&parse_csv(&read_text(Path::new(
        "research/election-baseline-v1/sources/1976-2024-president.csv",
    ))?)?)?;
    let mut definition = read_json(Path::new("research/current-election/definition.json"))?
        .as_object()
        .cloned()
        .unwrap_or_default();
    definition.insert(
        "id".into(),
        json!("causal-two-party-poll-sensitivity-20261005-v1"),
    );
    let definition = V::Object(definition);
    let at = now_iso();
    let election_at = "2026-11-04T06:00:00.000Z";
    let races: Vec<V> = vec![
        json!({
            "state": "IA",
            "entrants": ["Ashley Hinson", "Josh Turek", "Thomas Laehn"],
            "parties": {"republican": "Ashley Hinson", "democratic": "Josh Turek", "libertarian": "Thomas Laehn"},
            "pollFile": "iowa-emerson.html",
            "ballotFile": "iowa-official.html",
            "marketIds": {"republican": "261", "democratic": "260"},
            "poll": {
                "republican": num(0.50), "democratic": num(0.45), "other": 0, "unallocated": num(0.05),
                "unallocatedMeaning": "Unspecified remaining support in rounded publisher headline, conservatively allocated; not an observed zero for Laehn or a measured5% undecided result.",
                "fieldStartDate": "2026-08-31", "fieldEndDate": "2026-09-01",
                "fieldEndLatestAt": "2026-09-02T12:00:00.000Z", "publicationDate": "2026-09-03"
            }
        }),
        json!({
            "state": "KS",
            "entrants": ["Roger Marshall", "Adam Hamilton", "David C Graham"],
            "parties": {"republican": "Roger Marshall", "democratic": "Adam Hamilton", "libertarian": "David C Graham"},
            "pollFile": "kansas-emerson.html",
            "ballotFile": "kansas-official.html",
            "marketIds": {"democratic": "356"},
            "poll": {
                "republican": num(0.43), "democratic": num(0.45), "other": num(0.04), "unallocated": num(0.08),
                "unallocatedMeaning": "Publisher reports4% Libertarian and8% undecided.",
                "fieldStartDate": "2026-09-15", "fieldEndDate": "2026-09-16",
                "fieldEndLatestAt": "2026-09-17T12:00:00.000Z", "publicationDate": "2026-09-18"
            }
        }),
    ];
    let mut assessments: Vec<V> = Vec::new();
    for race in &races {
        let features = race_features_v2(
            &json!({
                "year": 2026,
                "state": race["state"],
                "entrants": race["entrants"],
            }),
            &senate,
            &president,
        )?
        .ok_or("race_features".to_string())?;
        let prior = json!({
            "structuralMargin": num(predict(&model["model"], &x_vec(&features))),
            "residualRms": model["residualRms"],
            "trainedThroughYear": model["model"]["trainedThroughYear"],
            "modelHash": model["modelHash"],
            "features": features,
        });
        let mut poll = race["poll"].as_object().cloned().unwrap_or_default();
        poll.insert("population".into(), json!("likely_voters"));
        poll.insert("sampleSize".into(), json!(750));
        poll.insert("publisherCredibilityInterval".into(), num(0.036));
        poll.insert("publisherIntervalUsedAsMarginSd".into(), json!(false));
        poll.insert("observedAt".into(), source(race["pollFile"].as_str().unwrap_or(""))["observedAt"].clone());
        poll.insert("ballotVerified".into(), json!(true));
        poll.insert("publicationTimeUtc".into(), V::Null);
        poll.insert(
            "methodology".into(),
            json!("Weighted mixed text-to-web/voter-file and panel sample; respondent weights/effective sample size unavailable."),
        );
        let poll = V::Object(poll);
        for party in ["republican", "democratic"] {
            let assessment = assess_party_poll(&json!({
                "poll": poll,
                "prior": prior,
                "definition": definition,
                "party": party,
                "assessmentAt": at,
                "electionAt": election_at,
            }))?;
            assessments.push(json!({
                "id": format!("{}-{}-poll-assessment-20261005-v1",
                    race["state"].as_str().unwrap_or("").to_lowercase(), party),
                "state": race["state"],
                "party": party,
                "marketId": race["marketIds"].get(party).cloned().unwrap_or(V::Null),
                "candidates": race["parties"],
                "poll": poll,
                "prior": prior,
                "sources": [
                    source(race["pollFile"].as_str().unwrap_or("")),
                    source(race["ballotFile"].as_str().unwrap_or("")),
                ],
                "assessment": assessment,
            }));
        }
    }
    let record = json!({
        "schemaVersion": 1,
        "id": "current-poll-breadth-20261005-v1",
        "at": at,
        "definition": definition,
        "definitionHash": sha256(js_stringify(&definition).as_bytes()),
        "assessmentCodeHash": sha256(&read_bytes(&base.join("assessment.mjs"))?),
        "assessments": assessments,
        "excludedEvidence": [json!({
            "source": source("iowa-marist.html"),
            "state": "IA",
            "population": "registered_voters",
            "sampleSize": 1050,
            "republican": num(0.42),
            "democratic": num(0.50),
            "remaining": num(0.08),
            "fieldStartDate": "2026-09-17",
            "fieldEndDate": "2026-09-20",
            "publicationDate": "2026-09-22",
            "reason": "Newer contradictory registered-voter survey is retained but not promoted to a likely-voter model without tested population transport."
        })],
        "nationalContext": {
            "source": source("national-emerson.html"),
            "population": "likely_voters",
            "sampleSize": 1000,
            "republican": num(0.42),
            "democratic": num(0.53),
            "undecided": num(0.05),
            "fieldStartDate": "2026-09-21",
            "fieldEndDate": "2026-09-22",
            "publicationDate": "2026-09-24",
            "usedAsNewNumericalInput": false,
            "meaning": "The existing fixed\u{2212}15/\u{2212}10/\u{2212}5 national-margin sensitivity is unchanged. House/Senate transport remains unvalidated."
        },
        "limitations": [
            "Subjective valuations are uncalibrated; no observed profit or guaranteed payout.",
            "These polls are not independent replications of each other or of the fundamentals.",
            "The Iowa likely-voter poll is older than a conflicting registered-voter survey. This does not establish current polling consensus.",
            "All source field/publication clocks are retained; retrieval time is not fresh fieldwork.",
            "Kansas Republicans versus the strongest opponent cannot be automatically complemented into the separate Democratic Party contract.",
            "No active bank, activation, window, journal or wallet is changed by this offline artifact."
        ],
        "executionAuthority": false,
        "competitionOrders": 0,
        "fees": "0.000000",
        "profitabilityEstablished": false
    });
    write_json_pretty_wx(&base.join("assessments-20261005-v1.json"), &record)?;
    let out = json!({
        "at": at,
        "assessments": record["assessments"].as_array().cloned().unwrap_or_default().iter().map(|a| json!({
            "id": a["id"],
            "marketId": a["marketId"],
            "prior": a["prior"]["structuralMargin"],
            "incumbent": a["prior"]["features"]["x"].as_array().and_then(|x| x.last()).cloned().unwrap_or(V::Null),
            "ordinary": [a["assessment"]["ordinary"]["lower"], a["assessment"]["ordinary"]["upper"]],
            "stress": [a["assessment"]["stress"]["lower"], a["assessment"]["stress"]["upper"]],
        })).collect::<Vec<V>>(),
        "excludedRegisteredVoterEvidence": 1,
        "networkRequests": 0,
        "productionBankChanged": false,
    });
    log_json(&out);
    Ok(out)
}

fn x_vec(features: &V) -> Vec<f64> {
    features["x"]
        .as_array()
        .map(|a| a.iter().map(|v| js_number(Some(v))).collect())
        .unwrap_or_default()
}
