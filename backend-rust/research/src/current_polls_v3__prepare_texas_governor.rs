// research/current-polls-v3/prepare-texas-governor.mjs — bind the recorded
// Texas governor root, re-assess current polls against a widened Senate error
// proxy and write the assessment + paper-bundle receipts.
use crate::current_polls_v2__assessment::assess_primary_polls;
use crate::files::{log_json, read_bytes, read_json, to_fixed, write_json_pretty_wx};
use crate::js::{date_parse, iso, js_number, now_iso, num, require_that};
use baal_collector::probability_paper::validate_research;
use baal_service_runtime::ledger::payload_hash;
use serde_json::{json, Value as V};
use std::path::Path;

fn eq_err(a: &V, b: &V) -> String {
    format!(
        "{} == {}",
        baal_service_runtime::json_text::js_stringify(a),
        baal_service_runtime::json_text::js_stringify(b)
    )
}
fn sha(bytes: &[u8]) -> String {
    baal_service_runtime::json_text::sha256(bytes)
}
fn poll(pollster: &str, population: &str, d: f64, r: f64, remaining: f64, field_end_date: &str, file: &str) -> V {
    json!({
        "pollster": pollster,
        "population": population,
        "democratic": num(d),
        "republican": num(r),
        "unallocated": num(remaining),
        "fieldEndDate": field_end_date,
        "fieldEndAt": format!("{field_end_date}T23:59:59.999Z"),
        "file": file,
    })
}

pub fn run() -> Result<V, String> {
    let runtime = crate::files::runtime_dir()?;
    let base = Path::new("research/current-polls-v3");
    let prior = read_json(Path::new(
        "research/current-polls-v2/assessments-20261005-v2.json",
    ))?;
    let definition = read_json(Path::new("research/current-polls-v2/definition.json"))?;
    let mut sources: Vec<V> = Vec::new();
    for f in [
        "research/current-polls-v3/sources.json",
        "research/current-polls-v2/sources.json",
        "research/current-polls-v2/comparator-sources.json",
    ] {
        sources.extend(read_json(Path::new(f))?["captures"].as_array().cloned().unwrap_or_default());
    }
    let wanted = [
        "reconmr-texas.pdf",
        "texas-marist.pdf",
        "texas-emerson.html",
        "texas-official.pdf",
    ];
    let used: Vec<V> = sources
        .into_iter()
        .filter(|s| wanted.contains(&s["file"].as_str().unwrap_or("")))
        .collect();
    for s in &used {
        let dir = if s["file"].as_str() == Some("reconmr-texas.pdf") {
            "research-sources-siena-breadth-20261005-v3"
        } else {
            "research-sources-current-marist-20261005-v2"
        };
        let actual = sha(&read_bytes(&runtime.join(dir).join(s["file"].as_str().unwrap_or("")))?);
        require_that(
            actual == s["contentHash"].as_str().unwrap_or(""),
            eq_err(&json!(actual), &s["contentHash"]),
        )?;
    }
    let root = read_json(
        &runtime
            .join("production-metadata")
            .join("current-siena-breadth-20261005-v3")
            .join("195.json"),
    )?;
    let c = &root["root"]["contract_details"];
    require_that(
        payload_hash(&root["root"]).map_err(|e| e.message)?
            == root["rootHash"].as_str().unwrap_or(""),
        eq_err(
            &json!(payload_hash(&root["root"]).map_err(|e| e.message)?),
            &root["rootHash"],
        ),
    )?;
    require_that(
        c["raceName"].as_str() == Some("Governor of Texas")
            && c["usState"].as_str() == Some("TX")
            && c["raceStage"].as_str() == Some("General")
            && c["resolutionType"].as_str() == Some("Party Winner")
            && c["winnerName"].as_str() == Some("Democratic Party")
            && c["electionDate"].as_str() == Some("2026-11-03")
            && root["root"].get("settled_with").is_some_and(V::is_null),
        "assert(c.raceName==='Governor of Texas'&&c.usState==='TX'&&c.raceStage==='General'&&c.resolutionType==='Party Winner'&&c.winnerName==='Democratic Party'&&c.electionDate==='2026-11-03'&&root.root.settled_with===null)",
    )?;
    // A Senate error distribution is a declared proxy, not fitted Governor
    // history. Keep its cycle sensitivity and widen every scale to at least
    // 12 margin points.
    let models = V::Array(
        prior["models"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|m| {
                let mut m = m.as_object().cloned().unwrap_or_default();
                let sd = js_number(m.get("standardDeviationPoints"));
                m.insert(
                    "standardDeviationPoints".into(),
                    num(crate::js::js_max(12.0, sd)),
                );
                m.insert(
                    "transport".into(),
                    json!("senate_error_proxy_to_governor_not_empirically_calibrated"),
                );
                V::Object(m)
            })
            .collect(),
    );
    let polls = V::Array(vec![
        poll("ReconMR", "likely_voters", 49.0, 45.0, 6.0, "2026-09-11", "reconmr-texas.pdf"),
        poll("Marist", "registered_voters", 49.0, 46.0, 5.0, "2026-09-20", "texas-marist.pdf"),
        poll("Emerson", "likely_voters", 46.0, 49.0, 5.0, "2026-09-14", "texas-emerson.html"),
    ]);
    let at = now_iso();
    let valuation = assess_primary_polls(&json!({
        "polls": polls,
        "models": models,
        "definition": definition,
        "at": at,
    }))?;
    let research = json!({
        "schemaVersion": 1,
        "id": "texas-governor-primary-proxy-20261005-v3",
        "marketId": "195",
        "exchangeId": "884",
        "side": "YES",
        "predicateHash": root["rootHash"],
        "recordedAt": at,
        "sourceAt": at,
        "expiresAt": iso(date_parse(&at) + 21_600_000.0),
        "lower": to_fixed(js_number(valuation["ordinaryRange"].as_array().and_then(|x| x.first())), 6),
        "upper": to_fixed(js_number(valuation["ordinaryRange"].as_array().and_then(|x| x.get(1))), 6),
        "method": "subjective_range",
        "settlementReviewed": true,
        "flags": [],
        "sources": used.iter().map(|s| json!({"url": s["url"], "contentHash": s["contentHash"]})).collect::<Vec<V>>(),
    });
    validate_research(&research)?;
    let severe: Vec<V> = [-3.0, 0.0, 3.0]
        .iter()
        .map(|shift| {
            let shifted: Vec<V> = polls
                .as_array()
                .cloned()
                .unwrap_or_default()
                .iter()
                .map(|p| {
                    let mut p = p.as_object().cloned().unwrap_or_default();
                    let d = js_number(p.get("democratic"));
                    let r = js_number(p.get("republican"));
                    p.insert("democratic".into(), num(d + shift / 2.0));
                    p.insert("republican".into(), num(r - shift / 2.0));
                    V::Object(p)
                })
                .collect();
            let mut v = assess_primary_polls(&json!({
                "polls": shifted,
                "models": models,
                "definition": definition,
                "at": at,
            }))?
            .as_object()
            .cloned()
            .unwrap_or_default();
            v.shift_remove("shiftPoints");
            let mut o = serde_json::Map::new();
            o.insert("shiftPoints".into(), num(*shift));
            for (k, val) in v {
                o.insert(k, val);
            }
            Ok(V::Object(o))
        })
        .collect::<Result<Vec<V>, String>>()?;
    let assessment = json!({
        "schemaVersion": 1,
        "id": research["id"],
        "at": at,
        "polls": polls,
        "definition": definition,
        "models": models,
        "valuation": valuation,
        "research": research,
        "rootHash": root["rootHash"],
        "primaryBallotReviewed": true,
        "primaryPacketPages": {"reconmr": 3, "marist": 9, "texasOfficial": 2},
        "method": "subjective_senate_error_proxy_widened_for_governor",
        "corroboratingUnmodeledEvidence": {
            "source": "siena-governors.html",
            "publicationDate": "2026-10-04",
            "democratic": 49,
            "republican": 46,
            "whyNotModeled": "The accessible primary summary does not supply the exact field clock; no field date is invented."
        },
        "severeTransportSensitivity": severe,
        "limitations": [
            "Governor forecast error is not calibrated here; the widened Senate proxy and scenario endpoints are judgment estimates, not confidence bounds.",
            "Current sources disagree, and the forecast can lose its entire one-share principal. Severe transport and undecided-allocation stresses can contradict the apparent ordinary edge.",
            "The primary ballot certifies Hinojosa/Abbott/Dixon; party-winner resolution, administrative override and refundable cancellation remain distinct from a guaranteed election payout.",
            "This experimental valuation does not predict a profitable two-day exit; no bank is re-based from existing Kansas or other holdings."
        ],
        "executionAuthority": false,
        "competitionOrders": 0,
        "profitabilityEstablished": false,
    });
    write_json_pretty_wx(
        &base.join("texas-governor-assessment-20261005-v3.json"),
        &assessment,
    )?;
    write_json_pretty_wx(
        &base.join("texas-governor-paper-bundle-20261005-v3.json"),
        &json!({
            "identities": {"195": {"exchangeId": "884", "predicateHash": root["rootHash"]}},
            "research": [research],
            "researchEvidence": [assessment],
        }),
    )?;
    let out = json!({
        "ordinary": valuation["ordinaryRange"],
        "central": valuation["centralEstimate"],
        "severeOpponentAllocation": valuation["severeOpponentAllocationStress"],
        "marketId": "195",
        "executionAuthority": false,
    });
    log_json(&out);
    Ok(out)
}
