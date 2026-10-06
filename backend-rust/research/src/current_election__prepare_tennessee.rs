// research/current-election/prepare-tennessee.mjs — build the Tennessee causal
// assessment record from the preserved October-3 evidence bundle and the
// frozen baseline, then write tennessee-20261004-v1.json.
use crate::current_election__assessment::assess_election;
use crate::election_baseline_v1__model::parse_csv;
use crate::election_baseline_v2__model::{predict, presidential_tables, race_features_v2 as race_features, senate_races_v2 as senate_races};
use crate::files::{log_json, read_bytes, read_json, read_text, runtime_dir, sha256, write_json_pretty_wx};
use crate::js::{js_number, now_iso, num};
use serde_json::{json, Value as V};
use std::path::Path;

pub fn run() -> Result<V, String> {
    let base = Path::new("research/current-election");
    let runtime = runtime_dir()?;
    let evidence = read_json(Path::new("data/probability-research-bundle-20261003-v1.json"))?;
    let model = read_json(Path::new(
        "research/election-baseline-v2/model-summary-affiliation-invariant.json",
    ))?;
    let definition = read_json(&base.join("definition.json"))?;
    let entrants = json!([
        "Bill Hagerty", "Marquita Bradshaw", "Tharon Chandler", "Andrew Gerena",
        "Jeremy Dean Hearn", "Robert Jones", "James William Macon III",
        "Yoshi D. Matthews", "David Sutman Jr", "Catherine Barcel Whitson"
    ]);
    let source_names = ["TN-poll-marginals.pdf", "TN-poll-methodology.html", "TN-ballot.pdf"];
    let mut sources: Vec<V> = Vec::new();
    for name in source_names {
        let s = evidence["captures"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .find(|c| c["file"].as_str() == Some(name));
        let bytes = read_bytes(&runtime.join("research-sources-20261003").join(name))?;
        let hash = sha256(&bytes);
        if s.is_none()
            || hash != s.as_ref().and_then(|v| v["contentHash"].as_str()).unwrap_or("")
        {
            return Err("PRIMARY_SOURCE_HASH_CHANGED".to_string());
        }
        let s = s.unwrap_or(V::Null);
        sources.push(json!({
            "url": s["url"],
            "contentHash": hash,
            "observedAt": s["observedAt"],
        }));
    }
    let facts = json!({
        "schemaVersion": 1,
        "state": "TN",
        "marketId": "363",
        "exchangeId": "1052",
        "entrants": entrants,
        "poll": {
            "population": "likely_voters",
            "sampleSize": 607,
            "overallSurveySampleSize": 1200,
            "senateRegisteredVoterResponses": 1157,
            "republican": num(0.58),
            "democratic": num(0.33),
            "independent": num(0.05),
            "undecided": num(0.04),
            "observedAt": sources[0]["observedAt"],
            "fieldStartDate": "2026-08-15",
            "fieldEndDate": "2026-08-26",
            "fieldEndLatestAt": "2026-08-27T12:00:00.000Z",
            "fieldEndClockMeaning": "Conservative latest UTC bound for the unspecified local time on the final Tennessee field day; not a new poll date.",
            "publicationDate": "2026-09-03",
            "publicationTimeUtc": V::Null,
            "pdfGeneratedDate": "2026-08-28",
            "pdfPage": 46,
            "publisherOverallEstimatedMoe": num(0.0277),
            "publisherMoeUsedAsLvMarginSd": false,
            "ballotVerified": true,
            "largestNamedOpponent": "democratic",
            "methodology": "Stratified, weighted online registered-voter panel;607 Senate likely-voter responses. Effective sample size and respondent-level weights not supplied."
        },
        "sources": sources,
        "ballotCrossCheckUrl": "https://montgomerytn.gov/storage/departments/election/QualifiedCandidatesNOV26_State.pdf",
        "ballotCrossCheckMeaning": "Official November3 state general-election candidates, U.S. Senate page1, independently read October4. Primary Wilson County PDF hash is preserved.",
        "excludesMarketDerivedForecasts": true,
        "ddhqUsedAsInput": false,
    });
    let senate = senate_races(
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
    let features = race_features(
        &json!({"year": 2026, "state": "TN", "entrants": facts["entrants"]}),
        &senate,
        &president,
    )?
    .ok_or("race_features".to_string())?;
    let prior = json!({
        "structuralMargin": num(predict(&model["model"], &x(&features))),
        "residualRms": model["residualRms"],
        "trainedThroughYear": model["model"]["trainedThroughYear"],
        "modelHash": model["modelHash"],
        "features": features,
    });
    let assessment = assess_election(&json!({
        "poll": facts["poll"],
        "prior": prior,
        "definition": definition,
        "assessmentAt": now_iso(),
        "electionAt": "2026-11-04T06:00:00.000Z",
    }))?;
    let record = json!({
        "schemaVersion": 1,
        "id": "tennessee-causal-assessment-20261004-v1",
        "facts": facts,
        "prior": prior,
        "definitionHash": sha256(&read_bytes(&base.join("definition.json"))?),
        "assessment": assessment,
        "originalResearchNotOverwritten": true,
        "sourceAtMeaning": "Time this new V2/poll/ballot assessment became available. Original component clocks, source hashes and age remain explicit.",
        "invalidation": [
            "New independent poll",
            "Candidate/affiliation/ballot change",
            "Material campaign shock",
            "Contract change",
            "Six-hour assessment expiry"
        ],
        "liveOrdersSubmitted": 0,
        "profitabilityEstablished": false,
    });
    write_json_pretty_wx(&base.join("tennessee-20261004-v1.json"), &record)?;
    let out = json!({
        "assessedAt": assessment["assessedAt"],
        "ordinary": [assessment["ordinary"]["lower"], assessment["ordinary"]["upper"]],
        "stress": [assessment["stress"]["lower"], assessment["stress"]["upper"]],
        "pollSample": assessment["sampleSize"],
        "fieldAgeDays": assessment["componentClocks"]["pollFieldAgeDays"],
        "modelHash": prior["modelHash"],
        "apiRequests": 0,
    });
    log_json(&out);
    Ok(out)
}

fn x(features: &V) -> Vec<f64> {
    features["x"]
        .as_array()
        .map(|a| a.iter().map(|v| js_number(Some(v))).collect())
        .unwrap_or_default()
}
