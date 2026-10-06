// data/prepare-probability-research-20261003.mjs — capture public research
// sources and build the subjective-range probability research bundle.
// fetch_bytes: (&url) -> Result<bytes> stands in for fetch() + arrayBuffer().
use crate::publication_pack::{now_ms, runtime_dir, sha};
use baal_collector::model::digest;
use baal_collector::probability_paper::validate_research;
use baal_service_runtime::atomic::atomic_json;
use baal_service_runtime::contract::{ms_to_iso, parse_iso_ms};
use serde_json::{json, Value as V};
use std::path::Path;

pub fn run(fetch_bytes: &dyn Fn(&str) -> Result<Vec<u8>, String>) -> Result<V, String> {
    let rt = runtime_dir();
    let dir = format!("{rt}/research-sources-20261003");
    if Path::new("data/probability-research-bundle-20261003-v1.json").exists() {
        return Err("RESEARCH_BUNDLE_ALREADY_EXISTS".into());
    }
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let sources = [
        ("TN-poll-methodology.html", "https://tennsight.com/polls/august-2026/"),
        ("TN-poll-marginals.pdf", "https://tennsight.com/wp-content/uploads/2026/09/Beacon-Poll-of-TN-August-2026-Marginals.pdf"),
        ("TN-ballot.pdf", "https://www.wilsontnvotes.gov/uploads/files/11ecd6ab8b7a94e16cfcde1a441679a5be17f3df.pdf"),
        ("WV-ballot.pdf", "https://apps.sos.wv.gov/elections/2026GeneralSampleBallots/SAMPLE%20-%20WV%20Pocahontas%20261103%20General%20-%2014inch.pdf"),
        ("WV-2024-result.pdf", "https://www.archives.gov/files/electoral-college/2024/ascertainment-west-virginia.pdf"),
    ];
    let mut captured: Vec<V> = Vec::new();
    for (file, url) in sources {
        let bytes = fetch_bytes(url)?;
        if file.ends_with(".pdf") && !(bytes.len() >= 5 && &bytes[..5] == b"%PDF-") {
            return Err("PUBLIC_RESEARCH_NOT_PDF".into());
        }
        std::fs::write(format!("{dir}/{file}"), &bytes).map_err(|e| e.to_string())?;
        captured.push(json!({
            "file": file, "url": url, "contentHash": sha(&bytes),
            "observedAt": ms_to_iso(now_ms() as f64), "bytes": bytes.len(),
        }));
    }
    for (state, file) in [("tennessee", "TN-forecast.html"), ("west-virginia", "WV-forecast.html")] {
        let bytes = std::fs::read(format!("{dir}/{file}")).map_err(|e| e.to_string())?;
        captured.push(json!({
            "file": file,
            "url": format!("https://votes.decisiondeskhq.com/races/2026-11-03/{state}-us-senate-general-election/forecast"),
            "contentHash": sha(&bytes),
            "observedAt": if state == "tennessee" { "2026-10-03T21:10:57.741Z" } else { "2026-10-03T21:10:58.518Z" },
            "bytes": bytes.len(),
        }));
    }
    let recorded_at = ms_to_iso(now_ms() as f64);
    let assessed_at = recorded_at.clone();
    let mut identities = serde_json::Map::<String, V>::new();
    let mut research = Vec::new();
    let mut research_evidence = Vec::new();
    for (market_id, exchange_id, state, lower, upper, files, band) in [
        ("363", "1052", "Tennessee", "0.930000", "0.990000",
            vec!["TN-poll-methodology.html", "TN-poll-marginals.pdf", "TN-ballot.pdf", "TN-forecast.html"], "0.95–0.99"),
        ("297", "986", "West Virginia", "0.940000", "0.995000",
            vec!["WV-ballot.pdf", "WV-2024-result.pdf", "WV-forecast.html"], "0.97–0.995"),
    ] {
        let binding = json!({
            "marketId": market_id, "exchangeId": exchange_id, "option": "YES",
            "event": format!("2026 {state} U.S. Senate general election"),
            "winnerAffiliation": "Republican Party",
            "resolution": "final certified winner",
            "fusion": "every party listed for winner counts",
            "certification": "recounts and litigation can delay settlement",
            "administrative": "sponsor may change rules/outcome/resolution",
            "reviewAt": if market_id == "363" { "2026-10-03T21:14:21Z" } else { "2026-10-03T21:15:06Z" },
            "reviewEvidence": "https://github.com/GodlyDonuts/predictions-cup-2026/issues/1#issuecomment-5973557936",
            "provenance": "Daenerys manual browser transcript summary; supplied attestation, not authenticated API node",
        });
        let predicate_hash = digest(&binding);
        identities.insert(market_id.to_string(), json!({
            "exchangeId": exchange_id, "predicateHash": predicate_hash,
        }));
        let id = format!("republican-{}-20261003-v1", state.to_lowercase().replace(' ', "-"));
        let r = json!({
            "schemaVersion": 1, "id": id, "marketId": market_id,
            "exchangeId": exchange_id, "side": "YES", "predicateHash": predicate_hash,
            "recordedAt": recorded_at, "sourceAt": assessed_at,
            "expiresAt": ms_to_iso(parse_iso_ms(&assessed_at).unwrap_or(0.0) + 21600000.0),
            "lower": lower, "upper": upper, "method": "subjective_range",
            "settlementReviewed": true, "flags": [],
            "sources": files.iter().map(|file| {
                let x = captured.iter().find(|x| x["file"] == json!(file)).unwrap();
                json!({"url": x["url"], "contentHash": x["contentHash"]})
            }).collect::<Vec<V>>(),
        });
        validate_research(&r)?;
        research.push(r);
        research_evidence.push(json!({
            "researchId": id, "binding": binding, "assessedAt": assessed_at,
            "researchComment": "https://github.com/GodlyDonuts/predictions-cup-2026/issues/1#issuecomment-5973574570",
            "researcherOrdinaryElectionBand": band,
            "selectedPaperSensitivityRange": [lower, upper],
            "rangeInterpretation": "Subjective stress scenarios; lower endpoints are not statistical confidence bounds or proven probability floors. Additional 2pp TN / 3pp WV haircut is a judgment stress, not an estimated sponsor default probability.",
            "sourceAtMeaning": "UTC time this paper assessment became available; does not replace or refresh underlying poll field dates or DDHQ publisher update.",
            "primaryEvidence": if market_id == "363" {
                "August15–26 Beacon/Targoz likely-voter Senate58/33, n607; one weighted online poll, not independent replications. Verified original marginals by Daenerys. Official ballot confirms Hagerty REP."
            } else {
                "Official ballot lists Capito REP, Anderson DEM, Wilson CST. 2024 statewide presidential margin is old supporting context. No current independent race-specific poll verified."
            },
            "forecastCaveat": "DDHQ97% TN / >99% WV, displayedOct2 2:02PM without exposed timezone; model75%fundamentals25%markets overlaps other inputs.",
            "invalidation": ["new independent poll", "candidate/ballot/party change",
                "material campaign shock", "changed exact contract rules", "six-hour expiry"],
            "sharedRisk": "National swings, methodology and sponsor treatment correlate both contracts; two latencies are duplicate exposures, not independent strategies.",
            "exitCaveat": "Settlement priors do not predict a profitable near-term sale. Two-day holding cap / five-cent loss trigger / two-cent take-profit use later sampled depth, and can lose.",
            "noProfitabilityClaim": true,
        }));
    }
    let bundle = json!({
        "identities": V::Object(identities), "research": research,
        "researchEvidence": research_evidence, "captures": captured,
        "recordedAt": recorded_at, "mode": "public_source_subjective_research_paper_only",
        "competitionOrders": 0, "newApiClients": 0,
    });
    atomic_json(Path::new("data/probability-research-bundle-20261003-v1.json"), &bundle)?;
    Ok(json!({
        "recordedAt": bundle["recordedAt"],
        "markets": bundle["research"].as_array().map(|a| a.iter().map(|r| json!({
            "marketId": r["marketId"], "lower": r["lower"], "upper": r["upper"],
            "expiresAt": r["expiresAt"],
        })).collect::<Vec<V>>()).unwrap_or_default(),
        "captures": bundle["captures"].as_array().map(|a| a.iter().map(|x| json!({
            "file": x["file"], "bytes": x["bytes"], "contentHash": x["contentHash"],
        })).collect::<Vec<V>>()).unwrap_or_default(),
        "profitabilityEstablished": false,
    }))
}
