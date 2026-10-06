// research/election-baseline-v2/screen.mjs — screen every cached Senate market
// against the frozen affiliation-invariant model plus a byte-frozen prefix of
// the public book tape. No API traffic.
use crate::election_baseline_v1__model::{
    parse_csv, predict, presidential_tables, probability_from_margin,
};
use crate::election_baseline_v2__model::senate_races_v2;
use crate::files::{log_json, read_json, read_text, write_json_pretty_wx};
use crate::js::{date_parse, js_max, js_min, js_number, js_string, js_truthy, now_ms, num};
use serde_json::{json, Value as V};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

const STATE_NAMES: &[&str] = &[
    "Alabama:AL","Alaska:AK","Arizona:AZ","Arkansas:AR","California:CA","Colorado:CO",
    "Connecticut:CT","Delaware:DE","Florida:FL","Georgia:GA","Hawaii:HI","Idaho:ID",
    "Illinois:IL","Indiana:IN","Iowa:IA","Kansas:KS","Kentucky:KY","Louisiana:LA",
    "Maine:ME","Maryland:MD","Massachusetts:MA","Michigan:MI","Minnesota:MN",
    "Mississippi:MS","Missouri:MO","Montana:MT","Nebraska:NE","Nevada:NV",
    "New Hampshire:NH","New Jersey:NJ","New Mexico:NM","New York:NY",
    "North Carolina:NC","North Dakota:ND","Ohio:OH","Oklahoma:OK","Oregon:OR",
    "Pennsylvania:PA","Rhode Island:RI","South Carolina:SC","South Dakota:SD",
    "Tennessee:TN","Texas:TX","Utah:UT","Vermont:VT","Virginia:VA","Washington:WA",
    "West Virginia:WV","Wisconsin:WI","Wyoming:WY",
];

pub fn run() -> Result<V, String> {
    let base = Path::new("research/election-baseline-v2");
    let v1 = Path::new("research/election-baseline-v1");
    let report = read_json(&base.join("model-summary-affiliation-invariant.json"))?;
    let senate = senate_races_v2(
        &parse_csv(&read_text(&v1.join("sources/1976-2024-senate-state.csv"))?)?,
        &serde_json::from_str::<V>(&read_text(&base.join("final-stage-overrides.json"))?)
            .map_err(|e| e.to_string())?
            .as_array()
            .cloned()
            .unwrap_or_default(),
        true,
    )?;
    let president = presidential_tables(&parse_csv(&read_text(
        &v1.join("sources/1976-2024-president.csv"),
    )?)?)?;
    let catalogue = read_json(Path::new("data/cup-identity-readonly-20261004.json"))?;
    let markets = catalogue["markets"].as_array().cloned().unwrap_or_default();
    let runtime: PathBuf = match std::env::var("BAAL_RUNTIME").ok().filter(|s| !s.is_empty()) {
        Some(v) => PathBuf::from(v),
        None => crate::files::runtime_dir()?,
    };
    let book_file = runtime.join("public-books.jsonl");
    let byte_cutoff = std::fs::metadata(&book_file).map_err(|e| e.to_string())?.len() as usize;
    // Freeze the byte boundary of the append-only public tape; no API traffic.
    let f = std::fs::File::open(&book_file).map_err(|e| e.to_string())?;
    let mut limited = f.take(byte_cutoff as u64);
    let mut prefix = Vec::new();
    limited.read_to_end(&mut prefix).map_err(|e| e.to_string())?;
    let mut latest: HashMap<String, V> = HashMap::new();
    let reader = BufReader::new(&prefix[..]);
    for line in reader.lines() {
        let line = line.map_err(|e| e.to_string())?;
        let event: V = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if event["kind"].as_str() != Some("book")
            || !js_truthy(event.get("payload").and_then(|p| p.get("book")))
        {
            continue;
        }
        let b = event["payload"]["book"].clone();
        if !b.get("instrument").and_then(V::as_str).is_some_and(|i| i.ends_with(":YES")) {
            continue;
        }
        let key = b
            .get("marketId")
            .map(|v| js_string(v))
            .unwrap_or_else(|| "undefined".to_string());
        let replace = match latest.get(&key) {
            None => true,
            Some(prior) => {
                date_parse(&js_string(b.get("receivedAt").unwrap_or(&V::Null)))
                    > date_parse(&js_string(prior.get("receivedAt").unwrap_or(&V::Null)))
            }
        };
        if replace {
            latest.insert(key, b);
        }
    }
    let screening_at = now_ms();
    let mut rows: Vec<V> = Vec::new();
    let mut missing: Vec<V> = Vec::new();
    for pair in STATE_NAMES {
        let mut it = pair.splitn(2, ':');
        let name = it.next().unwrap_or_default();
        let state = it.next().unwrap_or_default();
        let title = format!("Will the Republican Party win the {name} Senate?");
        let market = match markets
            .iter()
            .find(|m| m.get("title").and_then(V::as_str) == Some(title.as_str()))
        {
            Some(m) => m,
            None => continue,
        };
        let prior = senate.races.get(&format!("2020:{state}"));
        let sp = president.states.get(&format!("2024:{state}"));
        let nat = president.national.get(&2024);
        let market_id = market
            .get("market_id")
            .map(|v| js_string(v))
            .unwrap_or_else(|| "undefined".to_string());
        if prior.is_none() || sp.is_none() {
            missing.push(json!({
                "state": state,
                "marketId": market_id,
                "reason": "missing_verified_prior_seat_or_president",
            }));
            continue;
        }
        let prior = prior.unwrap();
        let sp = sp.unwrap();
        let nat = nat.cloned().unwrap_or(V::Null);
        let prior_nat = president.national.get(&2016).cloned().unwrap_or(V::Null);
        let x = vec![
            1.0,
            js_number(sp.get("margin")) - js_number(nat.get("margin")),
            js_number(prior.get("margin")) - js_number(prior_nat.get("margin")),
            0.0,
        ];
        let structural_margin = predict(&report["model"], &x);
        // These declared sensitivities are assumptions, not current polling or
        // confidence bounds.
        let mut variants: Vec<V> = Vec::new();
        for incumbent in [-1.0_f64, 0.0, 1.0] {
            for national_margin in [-0.15_f64, -0.10, -0.05, 0.0, 0.05] {
                let mut x2 = x.clone();
                x2[3] = incumbent;
                variants.push(json!({
                    "incumbent": num(incumbent),
                    "nationalMargin": num(national_margin),
                    "probability": num(probability_from_margin(
                        predict(&report["model"], &x2) + national_margin,
                        js_number(report.get("residualRms")).hypot(0.06),
                    )?),
                }));
            }
        }
        let probs: Vec<f64> = variants
            .iter()
            .map(|v| js_number(v.get("probability")))
            .collect();
        let lower = probs.iter().cloned().fold(f64::INFINITY, js_min);
        let upper = probs.iter().cloned().fold(f64::NEG_INFINITY, js_max);
        let book = latest.get(&market_id);
        let best_of = |side: &str, better_low: bool| -> Option<V> {
            let book = book?;
            book.get(side)?.as_array()?.iter().fold(None::<&V>, |a, b| {
                let bp = js_number(b.get("price"));
                match a {
                    None => Some(b),
                    Some(a) => {
                        let ap = js_number(a.get("price"));
                        if (better_low && bp < ap) || (!better_low && bp > ap) {
                            Some(b)
                        } else {
                            Some(a)
                        }
                    }
                }
            }).cloned()
        };
        let best_ask = best_of("asks", true);
        let best_bid = best_of("bids", false);
        let age_ms: Option<f64> = book
            .map(|b| screening_at - date_parse(&js_string(b.get("receivedAt").unwrap_or(&V::Null))));
        let indicative_gap = best_ask
            .as_ref()
            .map(|a| lower - js_number(a.get("price")));
        rows.push(json!({
            "state": state,
            "marketId": market_id,
            "title": market["title"],
            "structuralMargin": num(structural_margin),
            "scenarioMinimum": num(lower),
            "scenarioMaximum": num(upper),
            "ask": best_ask.as_ref().map(|a| a.get("price").cloned().unwrap_or(V::Null)).unwrap_or(V::Null),
            "askQuantity": best_ask.as_ref().map(|a| a.get("quantity").cloned().unwrap_or(V::Null)).unwrap_or(V::Null),
            "bid": best_bid.as_ref().map(|b| b.get("price").cloned().unwrap_or(V::Null)).unwrap_or(V::Null),
            "bookReceivedAt": book.map(|b| b.get("receivedAt").cloned().unwrap_or(V::Null)).unwrap_or(V::Null),
            "bookAgeMs": age_ms.map_or(V::Null, num),
            "indicativeScenarioGap": indicative_gap.map_or(V::Null, num),
            "quoteRecent": age_ms.is_some_and(|a| a >= 0.0 && a <= 120_000.0),
            "productionEligible": false,
            "reason": "Research screen: current entrant/source/contract review and calibrated executable edge remain unverified.",
        }));
    }
    rows.sort_by(|a, b| {
        let ga = a.get("indicativeScenarioGap").and_then(V::as_f64).unwrap_or(f64::NEG_INFINITY);
        let gb = b.get("indicativeScenarioGap").and_then(V::as_f64).unwrap_or(f64::NEG_INFINITY);
        gb.partial_cmp(&ga).unwrap_or(std::cmp::Ordering::Equal)
    });
    let result = json!({
        "at": crate::js::iso(screening_at),
        "modelHash": report["modelHash"],
        "tapeByteCutoff": num(byte_cutoff as f64),
        "networkRequests": 0,
        "nationalMarginAssumptions": [num(-0.15), num(-0.10), num(-0.05), num(0.0), num(0.05)],
        "incumbentSensitivity": [num(-1.0), num(0.0), num(1.0)],
        "rows": rows,
        "missing": missing,
        "qualifiedProductionForecasts": 0,
        "profitabilityEstablished": false,
        "fees": 0,
    });
    write_json_pretty_wx(&base.join("market-screen-affiliation-invariant.json"), &result)?;
    let result_rows = result["rows"].as_array().cloned().unwrap_or_default();
    let result_missing = result["missing"].as_array().cloned().unwrap_or_default();
    let out = json!({
        "screened": num(result_rows.len() as f64),
        "missing": num(result_missing.len() as f64),
        "recentQuotes": num(result_rows.iter().filter(|r| r["quoteRecent"] == V::Bool(true)).count() as f64),
        "top": V::Array(result_rows.iter().take(6).cloned().collect()),
        "qualifiedProductionForecasts": 0,
        "networkRequests": 0,
    });
    log_json(&out);
    Ok(out)
}
