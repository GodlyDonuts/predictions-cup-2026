// data/capture-kansas-lead-20261004.mjs — capture primary Kansas sources and
// record an exact candidate-lead observation (no bank, no orders).
use baal_service_runtime::json_text::sha256;
use serde_json::{json, Value as V};
use sha1::Digest as _;
use std::path::{Path, PathBuf};

fn sha(b: &[u8]) -> String { sha256(b) }
fn localappdata() -> Result<PathBuf, String> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).ok_or_else(|| "LOCALAPPDATA".to_string())
}
fn now_iso() -> String {
    baal_service_runtime::contract::ms_to_iso(
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as f64)
}
fn write_wx(path: &Path, body: &[u8]) -> Result<(), String> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; o.mode(0o600); }
    let mut f = o.open(path).map_err(|e| e.to_string())?;
    std::io::Write::write_all(&mut f, body).map_err(|e| e.to_string())
}
fn amount(v: &V) -> Result<num_bigint::BigInt, String> {
    baal_paper::fixed::big_amount(v.as_str().unwrap_or(""))
}
fn money(v: &num_bigint::BigInt) -> String { baal_paper::fixed::money(v) }
fn need(ok: bool, code: &str) -> Result<(), String> { if ok { Ok(()) } else { Err(code.into()) } }

fn fetch(url: &str) -> Result<Vec<u8>, String> {
    // fetch(url, {redirect:'error', signal: AbortSignal.timeout(20000)})
    let client = reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_millis(20000))
        .build().map_err(|e| e.to_string())?;
    let response = client.get(url).send().map_err(|e| e.to_string())?;
    need(response.status().is_success(), "Primary public source unavailable")?;
    let bytes = response.bytes().map_err(|e| e.to_string())?.to_vec();
    need(bytes.len() < 10 * 1024 * 1024, "assertion failed")?;
    Ok(bytes)
}

/// Script body; `now_ms` allows deterministic stamping.
pub fn run() -> Result<V, String> {
    let runtime = localappdata()?.join("SigTradingCup").join("paper-runtime");
    let dir = runtime.join("research-sources").join(format!("kansas-20261004-{}",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    }
    let mut sources: Vec<V> = Vec::new();
    for (name, url) in [
        ("emerson", "https://emersoncollegepolling.com/kansas-2026/"),
        ("ddhq-forecast", "https://votes.decisiondeskhq.com/races/2026-11-03/kansas-us-senate-general-election/forecast"),
        ("ddhq-polling", "https://votes.decisiondeskhq.com/races/2026-11-03/kansas-us-senate-general-election/polling")] {
        let bytes = fetch(url)?;
        write_wx(&dir.join(format!("{}.html", name)), &bytes)?;
        sources.push(json!({"url": url, "contentHash": sha(&bytes), "bytes": bytes.len(),
            "retrievedAt": now_iso(), "role": name}));
    }
    let cached = Path::new("data/cup-identity-readonly-20261004.json");
    let identity_map: V = if cached.exists() {
        serde_json::from_slice(&std::fs::read(cached).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?
    } else {
        let compressed = std::fs::read(
            "research/crossvenue_screen_2026-10-02/cup_ui_identity_map_2026-10-02.json.gz")
            .map_err(|e| e.to_string())?;
        // git blob sha1: 'blob <len>\0' + content
        let mut blob = format!("blob {}\0", compressed.len()).into_bytes();
        blob.extend_from_slice(&compressed);
        let mut h = sha1::Sha1::new();
        h.update(&blob);
        need(format!("{:x}", h.finalize()) == "9581ac06796235a0a6ab425b4e944727b9c93440",
            "assertion failed")?;
        let mut d = flate2::read::GzDecoder::new(&compressed[..]);
        let mut text = String::new();
        std::io::Read::read_to_string(&mut d, &mut text).map_err(|e| e.to_string())?;
        serde_json::from_str(&text).map_err(|e| e.to_string())?
    };
    let identities: Vec<V> = identity_map["markets"].as_array().cloned().unwrap_or_default()
        .into_iter()
        .filter(|x| ["356", "357"].contains(&x["market_id"].to_string().trim_matches('"').to_string().as_str())
            || x["market_id"].as_i64().map(|n| [356, 357].contains(&n)).unwrap_or(false))
        .collect();
    let quotes: V = serde_json::from_slice(&std::fs::read("data/kansas-verified-quotes-20261004.json")
        .map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    need(identities.len() == 2, "assertion failed")?;
    need(quotes["quotes"].as_array().map(|a| a.len()).unwrap_or(0) == 2, "assertion failed")?;
    let quotes_arr = quotes["quotes"].as_array().cloned().unwrap_or_default();
    let find = |id: &str| -> V {
        quotes_arr.iter().find(|x| x["marketId"].to_string().trim_matches('"') == id
            || x["marketId"].as_i64().map(|n| n.to_string() == id).unwrap_or(false))
            .cloned().unwrap_or(V::Null)
    };
    let republican_quote = find("357");
    let democratic_quote = find("356");
    let tick = num_bigint::BigInt::from(baal_paper::fixed::TICK);
    let scale = num_bigint::BigInt::from(baal_paper::fixed::SCALE);
    let best_ask = |q: &V| -> Result<num_bigint::BigInt, String> {
        // JS: amount(republicanQuote.bestAsk.toFixed(6))
        let n = q["bestAsk"].as_f64().unwrap_or(f64::NAN);
        amount(&json!(format!("{:.6}", n)))
    };
    let best_bid = |q: &V| -> Result<num_bigint::BigInt, String> {
        let n = q["bestBid"].as_f64().unwrap_or(f64::NAN);
        amount(&json!(format!("{:.6}", n)))
    };
    let point_surplus = amount(&json!("0.770000"))? - best_ask(&republican_quote)? - tick.clone();
    let paired_cost = (scale.clone() - best_bid(&republican_quote)?)
        + (scale.clone() - best_bid(&democratic_quote)?) + num_bigint::BigInt::from(2) * tick;
    let record = json!({"schemaVersion": 1, "recordedAt": now_iso(),
        "status": "exact_candidate_lead_requires_forecast_and_rule_review", "sources": sources,
        "identities": identities.iter().map(|x| json!({
            "marketId": x["market_id"].to_string().trim_matches('"'),
            "title": x["title"], "party": x["party"], "url": x["url"],
            "identityObservedAt": x["observed_at"], "resolutionVerified": false})).collect::<Vec<_>>(),
        "indicativeQuotes": quotes["quotes"], "quotePrefix": quotes["prefix"],
        "quoteHostOutputHash": quotes["hostOutputHash"],
        "sourceObservations": [
            {"role": "emerson", "fieldDates": ["2026-09-15", "2026-09-16"],
                "sample": "750 likely voters",
                "voteSharesPercent": {"Hamilton": 45, "Marshall": 43, "Graham": 4, "undecided": 8},
                "credibilityIntervalPercent": 3.6,
                "observationBasis": "Primary page reviewed through web tool; content archive retained locally",
                "winProbability": null},
            {"role": "ddhq-forecast", "publisherUpdatedText": "Oct 2, 2:02 PM",
                "publisherTimezoneKnown": false,
                "winProbability": {"Marshall": 0.77, "Hamilton": 0.23},
                "modelComponentsPercent": {"fundamentals": 73, "polls": 2, "markets": 25},
                "observedPollingAverage": {"dateText": "Oct 4, 11:33 AM EDT",
                    "Hamilton": 45.60, "Marshall": 45.40},
                "observationBasis": "Primary forecast page reviewed; polling-average update does not refresh the older race probability"},
            {"role": "ddhq-polling", "displayedMarshallWinProbability": 0.83,
                "publisherUpdateExposed": false,
                "observationBasis": "Primary polling-page footer reviewed; distinct cached display is not a second independent model"}],
        "scenarioOnly": {"RepublicanYesAsk": republican_quote["bestAsk"], "adverseTick": 0.005,
            "ddhqPointMinusCushionedAsk": money(&point_surplus),
            "interpretation": "Arithmetic on the indicative quote and one published model point, not a confidence-bound edge, depth-confirmed fill or booked gain"},
        "pairedQuoteLead": {"canonicalNoCostWithTwoCushions": money(&paired_cost),
            "conditionalOrdinaryMutualExclusionSurplus": money(&(scale - paired_cost.clone())),
            "mutualExclusionReviewed": false, "refundCompatibilityReviewed": false,
            "depthConfirmed": false,
            "interpretation": "Separate indicative dual-NO lead; winner-party/fusion/admin rules and all leg depth must be verified. No guaranteed floor, fill, batch atomicity or profit inferred."},
        "captureObservationBoundary": "Source observations came from web-tool renderings. Raw HTML captures can differ from cached rendering; hashes prove archive integrity, not automated verification of the observation fields.",
        "unresolved": ["Exact Cup individual resolution conditions and party binding need current review",
            "Candidate-list link returned403 through web tool; general ballot not independently read",
            "Polling-page83% versus forecast-page77% display timestamps are unresolved; do not blend them as independent forecasts",
            "Poll vote share and election win probability are different quantities",
            "Fundamentals, polls and the25% market component are correlated inputs",
            "No conservative calibrated probability interval or prospective fill/depth evidence established"],
        "method": null, "probabilityInterval": null, "newPaperBank": false,
        "competitionOrders": 0, "sigApiRequests": 0, "publicResearchHttpRequests": 3,
        "profitabilityEstablished": false});
    write_wx(Path::new("data/kansas-source-lead-20261004.json"),
        format!("{}\n", serde_json::to_string(&record).map_err(|e| e.to_string())?).as_bytes())?;
    Ok(json!({"status": record["status"],
        "sourceCaptures": sources.iter().map(|x| json!({"role": x["role"],
            "bytes": x["bytes"], "contentHash": x["contentHash"]})).collect::<Vec<_>>(),
        "cupMarkets": record["identities"].as_array().cloned().unwrap_or_default().iter()
            .map(|x| x["marketId"].clone()).collect::<Vec<_>>(),
        "pointScenarioSurplus": record["scenarioOnly"]["ddhqPointMinusCushionedAsk"],
        "depthConfirmed": false, "bankCreated": false, "profitabilityEstablished": false}))
}
