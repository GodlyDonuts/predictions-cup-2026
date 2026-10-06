// research/election-baseline-v1/model.mjs — MIT election CSV parsing and the
// frozen ridge/baseline election model. Exact f64 math parity.
use indexmap::IndexMap;
use serde_json::{json, Value as V};
use std::collections::BTreeSet;

pub fn hash(bytes: &[u8]) -> String { baal_service_runtime::json_text::sha256(bytes) }

fn need(ok: bool, code: &str) -> Result<(), String> { if ok { Ok(()) } else { Err(code.into()) } }

fn js_number(v: &str) -> f64 {
    // Number(value): trims, empty -> 0, else strict float parse (NaN on failure).
    let t = v.trim();
    if t.is_empty() { return 0.0; }
    t.parse::<f64>().unwrap_or(f64::NAN)
}

/// parseCsv(text) — strict RFC-ish parser; returns rows keyed by header order.
pub fn parse_csv(text: &str) -> Result<Vec<V>, String> {
    let chars: Vec<char> = text.chars().collect();
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        if quoted {
            if c == '"' && chars.get(i + 1) == Some(&'"') { field.push('"'); i += 1; }
            else if c == '"' { quoted = false; }
            else { field.push(c); }
        } else if c == '"' {
            if !field.is_empty() { return Err("Quote inside unquoted field".into()); }
            quoted = true;
        } else if c == ',' {
            row.push(std::mem::take(&mut field));
        } else if c == '\n' {
            let f = field.strip_suffix('\r').unwrap_or(&field).to_string();
            row.push(f);
            if row.iter().any(|x| !x.is_empty()) { rows.push(std::mem::take(&mut row)); } else { row.clear(); }
            field.clear();
        } else {
            field.push(c);
        }
        i += 1;
    }
    if quoted { return Err("Unterminated CSV quote".into()); }
    if !field.is_empty() || !row.is_empty() {
        let f = field.strip_suffix('\r').unwrap_or(&field).to_string();
        row.push(f);
        rows.push(row);
    }
    if rows.is_empty() { return Err("Invalid CSV header".into()); }
    let header = rows.remove(0);
    if header.is_empty() || header.iter().collect::<BTreeSet<_>>().len() != header.len() {
        return Err("Invalid CSV header".into());
    }
    let mut out = Vec::with_capacity(rows.len());
    for (i, values) in rows.iter().enumerate() {
        if values.len() != header.len() { return Err(format!("CSV width {}", i)); }
        let mut obj = serde_json::Map::new();
        for (j, h) in header.iter().enumerate() { obj.insert(h.clone(), json!(values[j])); }
        out.push(V::Object(obj));
    }
    Ok(out)
}

fn name_tokens(name: &str) -> Vec<String> {
    let upper = name.to_uppercase();
    let cleaned: String = upper.chars().map(|c| if c.is_ascii_uppercase() || c.is_ascii_digit() || c == ' ' { c } else { ' ' }).collect();
    cleaned.split_whitespace()
        .filter(|x| !["JR", "SR", "II", "III", "IV"].contains(x))
        .map(|x| x.to_string()).collect()
}

/// identity(name) — uppercase, strip punctuation, drop suffixes, sort, join.
pub fn identity(name: &str) -> String {
    let mut tokens = name_tokens(name);
    tokens.sort();
    tokens.join(" ")
}

fn boolean(v: &V) -> bool { v.as_str().unwrap_or(&String::new()).eq_ignore_ascii_case("true") }
fn str_field<'a>(r: &'a V, k: &str) -> &'a str { r[k].as_str().unwrap_or("") }

fn vote_count(value: &V) -> Result<i64, String> {
    let n = js_number(value.as_str().unwrap_or(""));
    if !(n.is_finite() && n.fract() == 0.0 && n.abs() <= 9_007_199_254_740_991.0) || n < 0.0 {
        return Err("Invalid vote count".into());
    }
    Ok(n as i64)
}

fn major(value: &str) -> Option<&'static str> {
    match value { "REPUBLICAN" => Some("REPUBLICAN"), "DEMOCRAT" => Some("DEMOCRAT"), _ => None }
}

struct Candidate { id: String, name: String, parties: Vec<String>, lines: IndexMap<String, IndexMap<String, i64>> }

/// Ballot lines of the same person belong to that person. A fusion line is not a separate voter.
fn candidate_totals(rows: &[V]) -> Result<Vec<V>, String> {
    let mut candidates: IndexMap<String, Candidate> = IndexMap::new();
    for row in rows {
        let id = identity(str_field(row, "candidate"));
        if id.is_empty() { continue; }
        let c = candidates.entry(id.clone()).or_insert_with(|| Candidate {
            id: id.clone(), name: str_field(row, "candidate").to_string(), parties: Vec::new(), lines: IndexMap::new(),
        });
        if let Some(p) = major(str_field(row, "party_simplified")) {
            if !c.parties.iter().any(|x| x == p) { c.parties.push(p.to_string()); }
        }
        let line = format!("{}\u{0}writein={}", str_field(row, "party_detailed"), boolean(&row["writein"]));
        let mode = { let m = str_field(row, "mode"); if m.is_empty() { "total".to_string() } else { m.to_string() } };
        let modes = c.lines.entry(line).or_default();
        if modes.contains_key(&mode) { return Err("Duplicate candidate ballot-line/mode".into()); }
        modes.insert(mode, vote_count(&row["candidatevotes"])?);
    }
    let mut out = Vec::new();
    for (_, c) in candidates {
        let mut votes = 0i64;
        for (_, modes) in &c.lines {
            votes += if let Some(t) = modes.get("total") { *t } else { modes.values().sum() };
        }
        out.push(json!({"id": c.id, "name": c.name, "parties": c.parties, "votes": votes}));
    }
    Ok(out)
}

fn votes_of(c: &V) -> f64 { c["votes"].as_f64().unwrap_or(0.0) }
fn has_party(c: &V, p: &str) -> bool { c["parties"].as_array().is_some_and(|a| a.iter().any(|x| x.as_str() == Some(p))) }

pub struct PresidentialTables { pub states: IndexMap<String, V>, pub national: IndexMap<i64, V>, pub years: Vec<i64> }

/// presidentialTables(rows) — keyed "year:ST" state margins plus per-year national.
pub fn presidential_tables(rows: &[V]) -> Result<PresidentialTables, String> {
    let mut grouped: IndexMap<String, Vec<V>> = IndexMap::new();
    for r in rows {
        let state = str_field(r, "state_po");
        if str_field(r, "office") != "US PRESIDENT" || !(state.len() == 2 && state.bytes().all(|b| b.is_ascii_uppercase())) { continue; }
        grouped.entry(format!("{}:{}", str_field(r, "year"), state)).or_default().push(r.clone());
    }
    let mut states: IndexMap<String, V> = IndexMap::new();
    let mut national_votes: IndexMap<i64, (i64, i64, i64)> = IndexMap::new();
    for (key, group) in &grouped {
        let candidates = candidate_totals(group)?;
        if candidates.iter().any(|c| c["parties"].as_array().map(|a| a.len()).unwrap_or(0) > 1) {
            return Err("Ambiguous presidential candidate party".into());
        }
        let republican: i64 = candidates.iter().filter(|c| has_party(c, "REPUBLICAN")).map(|c| c["votes"].as_i64().unwrap_or(0)).sum();
        let democratic: i64 = candidates.iter().filter(|c| has_party(c, "DEMOCRAT")).map(|c| c["votes"].as_i64().unwrap_or(0)).sum();
        if republican == 0 || democratic == 0 { return Err("Missing presidential major party".into()); }
        let mut split = key.splitn(2, ':');
        let year = js_number(split.next().unwrap_or("")) as i64;
        let state = split.next().unwrap_or("");
        let margin = (republican - democratic) as f64 / (republican + democratic) as f64;
        states.insert(key.clone(), json!({"year": year, "state": state, "republican": republican,
            "democratic": democratic, "margin": margin}));
        let n = national_votes.entry(year).or_insert((0, 0, 0));
        n.0 += republican; n.1 += democratic; n.2 += 1;
    }
    let mut national: IndexMap<i64, V> = IndexMap::new();
    let mut years = Vec::new();
    for (year, n) in &national_votes {
        if n.2 != 51 { return Err(format!("Incomplete presidential states {}:{}", year, n.2)); }
        let margin = (n.0 - n.1) as f64 / (n.0 + n.1) as f64;
        national.insert(*year, json!({"republican": n.0, "democratic": n.1, "states": n.2, "year": year, "margin": margin}));
        years.push(*year);
    }
    years.sort();
    Ok(PresidentialTables { states, national, years })
}

pub struct SenateRaces { pub races: IndexMap<String, V>, pub exclusions: Vec<V> }

/// senateRaces(rows) — certified general two-party margins with exclusion reasons.
pub fn senate_races(rows: &[V]) -> SenateRaces {
    let mut grouped: IndexMap<String, Vec<V>> = IndexMap::new();
    let mut exclusions = Vec::new();
    for r in rows {
        if str_field(r, "office") != "US SENATE"
            || !str_field(r, "stage").eq_ignore_ascii_case("gen")
            || boolean(&r["special"]) || boolean(&r["unofficial"]) { continue; }
        grouped.entry(format!("{}:{}", str_field(r, "year"), str_field(r, "state_po"))).or_default().push(r.clone());
    }
    let mut races: IndexMap<String, V> = IndexMap::new();
    for (key, group) in &grouped {
        match senate_race_one(key, group) {
            Ok(v) => { races.insert(key.clone(), v); }
            Err(reason) => exclusions.push(json!({"key": key, "reason": reason})),
        }
    }
    SenateRaces { races, exclusions }
}

fn senate_race_one(key: &str, group: &[V]) -> Result<V, String> {
    let candidates = candidate_totals(group)?;
    if candidates.iter().any(|c| c["parties"].as_array().map(|a| a.len()).unwrap_or(0) > 1) {
        return Err("cross_major_party_fusion".into());
    }
    let mut total = 0i64;
    for r in group { total = total.max(vote_count(&r["totalvotes"])?); }
    if total < 100 { return Err("uncontested_or_invalid_total".into()); }
    let ranked = |party: &str| -> Vec<V> {
        let mut v: Vec<V> = candidates.iter().filter(|c| has_party(c, party)).cloned().collect();
        v.sort_by(|a, b| b["votes"].as_i64().cmp(&a["votes"].as_i64()));
        v
    };
    let r = ranked("REPUBLICAN");
    let d = ranked("DEMOCRAT");
    let rv0 = r.first().map(|c| c["votes"].as_i64().unwrap_or(0)).unwrap_or(0);
    let dv0 = d.first().map(|c| c["votes"].as_i64().unwrap_or(0)).unwrap_or(0);
    if rv0 == 0 || dv0 == 0 { return Err("missing_major_party".into()); }
    let (r0, d0) = (r[0].clone(), d[0].clone());
    let r1 = r.get(1).map(|c| c["votes"].as_i64().unwrap_or(0)).unwrap_or(0) as f64;
    let d1 = d.get(1).map(|c| c["votes"].as_i64().unwrap_or(0)).unwrap_or(0) as f64;
    if r1 / total as f64 >= 0.01 || d1 / total as f64 >= 0.01 { return Err("multiple_material_major_candidates".into()); }
    let outside: Vec<&V> = candidates.iter().filter(|c| c["parties"].as_array().map(|a| a.is_empty()).unwrap_or(true)).collect();
    if outside.iter().any(|c| c["votes"].as_i64().unwrap_or(0) >= r0["votes"].as_i64().unwrap_or(0).max(d0["votes"].as_i64().unwrap_or(0)))
        || outside.iter().map(|c| c["votes"].as_i64().unwrap_or(0)).sum::<i64>() as f64 / total as f64 >= 0.20 {
        return Err("material_independent_competition".into());
    }
    if candidates.iter().map(|c| c["votes"].as_i64().unwrap_or(0)).sum::<i64>() as f64 > total as f64 * 1.001 {
        return Err("candidate_total_exceeds_reported_total".into());
    }
    let mut split = key.splitn(2, ':');
    let year = js_number(split.next().unwrap_or("")) as i64;
    let state = split.next().unwrap_or("");
    let rv = r0["votes"].as_i64().unwrap_or(0);
    let dv = d0["votes"].as_i64().unwrap_or(0);
    let winner = if rv > dv { r0.clone() } else { d0.clone() };
    Ok(json!({"year": year, "state": state, "republican": r0, "democratic": d0,
        "margin": (rv - dv) as f64 / (rv + dv) as f64, "total": total, "winner": winner}))
}

fn prior_president(tables: &PresidentialTables, year: i64) -> Result<i64, String> {
    tables.years.iter().copied().filter(|y| *y < year).last()
        .ok_or_else(|| "Missing strictly prior president".to_string())
}

/// raceFeatures({year,state,republican,democratic}, senate, president).
/// republican/democratic may be a candidate-name string or an object with .name.
pub fn race_features(race: &V, senate: &SenateRaces, president: &PresidentialTables) -> Result<Option<V>, String> {
    let year = race["year"].as_i64().unwrap_or(0);
    let state = race["state"].as_str().unwrap_or("");
    let pres_year = prior_president(president, year)?;
    let nat = president.national.get(&pres_year).cloned().unwrap_or(V::Null);
    let state_pres = president.states.get(&format!("{}:{}", pres_year, state)).cloned().unwrap_or(V::Null);
    let prior = senate.races.get(&format!("{}:{}", year - 6, state)).cloned().unwrap_or(V::Null);
    if state_pres.is_null() || prior.is_null() { return Ok(None); }
    let old_pres_year = prior_president(president, prior["year"].as_i64().unwrap_or(0))?;
    let old_national = president.national.get(&old_pres_year).cloned().unwrap_or(V::Null);
    let name_of = |v: &V| -> String {
        if let Some(s) = v.as_str() { identity(s) } else { identity(v["name"].as_str().unwrap_or("")) }
    };
    let r_id = name_of(&race["republican"]);
    let d_id = name_of(&race["democratic"]);
    let w_id = prior["winner"]["id"].as_str().unwrap_or("");
    let incumbent = if w_id == r_id { 1 } else if w_id == d_id { -1 } else { 0 };
    Ok(Some(json!({
        "x": [1.0, state_pres["margin"].as_f64().unwrap_or(0.0) - nat["margin"].as_f64().unwrap_or(0.0),
            prior["margin"].as_f64().unwrap_or(0.0) - old_national["margin"].as_f64().unwrap_or(0.0), incumbent],
        "clocks": {"presYear": pres_year, "priorSeatYear": prior["year"], "priorSeatPresYear": old_pres_year},
        "nationalBaseline": nat["margin"], "stateMargin": state_pres["margin"],
        "priorSeatMargin": prior["margin"], "incumbent": incumbent,
        "incumbentNameMatch": incumbent != 0 })))
}

/// buildRows(senate, president, minimumYear=1990).
pub fn build_rows(senate: &SenateRaces, president: &PresidentialTables, minimum_year: Option<i64>) -> Result<Vec<V>, String> {
    let minimum_year = minimum_year.unwrap_or(1990);
    let mut out = Vec::new();
    for (_, r) in &senate.races {
        if r["year"].as_i64().unwrap_or(0) < minimum_year { continue; }
        if let Some(f) = race_features(r, senate, president)? {
            out.push(json!({"key": format!("{}:{}", r["year"].as_i64().unwrap_or(0), r["state"].as_str().unwrap_or("")),
                "year": r["year"], "state": r["state"], "x": f["x"],
                "y": r["margin"].as_f64().unwrap_or(0.0) - f["nationalBaseline"].as_f64().unwrap_or(0.0),
                "actualMargin": r["margin"], "nationalBaseline": f["nationalBaseline"], "clocks": f["clocks"]}));
        }
    }
    out.sort_by(|a, b| a["year"].as_i64().cmp(&b["year"].as_i64())
        .then_with(|| a["state"].as_str().unwrap_or("").cmp(b["state"].as_str().unwrap_or(""))));
    Ok(out)
}

fn solve(matrix: &[Vec<f64>], vector: &[f64]) -> Result<Vec<f64>, String> {
    let n = vector.len();
    let mut a: Vec<Vec<f64>> = matrix.iter().enumerate().map(|(i, row)| {
        let mut r = row.clone(); r.push(vector[i]); r
    }).collect();
    for col in 0..n {
        let mut pivot = col;
        for i in col + 1..n { if a[i][col].abs() > a[pivot][col].abs() { pivot = i; } }
        a.swap(pivot, col);
        if a[col][col].abs() < 1e-12 { return Err("Singular regression".into()); }
        let scale = a[col][col];
        for j in col..=n { a[col][j] /= scale; }
        for i in 0..n {
            if i != col {
                let factor = a[i][col];
                for j in col..=n { a[i][j] -= factor * a[col][j]; }
            }
        }
    }
    Ok(a.iter().map(|row| row[n]).collect())
}

pub struct RidgeModel { pub lambda: f64, pub n: usize, pub trained_through_year: i64,
    pub means: Vec<f64>, pub scales: Vec<f64>, pub coefficients: Vec<f64> }

/// fitRidge(rows, lambda) — standardized ridge via normal equations.
pub fn fit_ridge(rows: &[V], lambda: f64) -> Result<RidgeModel, String> {
    need(rows.len() >= 20, "assertion failed")?;
    need(lambda.is_finite() && lambda > 0.0, "assertion failed")?;
    let width = rows[0]["x"].as_array().map(|a| a.len()).unwrap_or(0);
    let mut means = vec![0.0];
    let mut scales = vec![1.0];
    for j in 1..width {
        let mean = rows.iter().map(|r| r["x"][j].as_f64().unwrap_or(0.0)).sum::<f64>() / rows.len() as f64;
        let sd = (rows.iter().map(|r| {
            let d = r["x"][j].as_f64().unwrap_or(0.0) - mean; d * d
        }).sum::<f64>() / rows.len() as f64).sqrt();
        means.push(mean);
        scales.push(if sd > 1e-9 { sd } else { 1.0 });
    }
    let mut matrix = vec![vec![0.0f64; width]; width];
    let mut vector = vec![0.0f64; width];
    for r in rows {
        let x: Vec<f64> = (0..width).map(|j| {
            (r["x"][j].as_f64().unwrap_or(0.0) - means[j]) / scales[j]
        }).collect();
        let y = r["y"].as_f64().unwrap_or(0.0);
        for j in 0..width {
            vector[j] += x[j] * y;
            for k in 0..width { matrix[j][k] += x[j] * x[k]; }
        }
    }
    for j in 1..width { matrix[j][j] += lambda; }
    Ok(RidgeModel {
        lambda, n: rows.len(),
        trained_through_year: rows.iter().map(|r| r["year"].as_i64().unwrap_or(0)).max().unwrap_or(0),
        means, scales, coefficients: solve(&matrix, &vector)?,
    })
}

/// predict(model, x).
pub fn predict(model: &RidgeModel, x: &[f64]) -> f64 {
    x.iter().enumerate().map(|(j, value)| {
        model.coefficients[j] * (value - model.means[j]) / model.scales[j]
    }).sum()
}

/// expandingCheck(rows, lambda, lastYear, minimumTrainRows=80).
pub fn expanding_check(rows: &[V], lambda: f64, last_year: i64, minimum_train_rows: Option<usize>) -> Result<Vec<V>, String> {
    let minimum_train_rows = minimum_train_rows.unwrap_or(80);
    let mut years: Vec<i64> = Vec::new();
    for r in rows {
        let y = r["year"].as_i64().unwrap_or(0);
        if !years.contains(&y) { years.push(y); }
    }
    let mut predictions = Vec::new();
    for year in years.iter().copied().filter(|y| *y <= last_year) {
        let train: Vec<V> = rows.iter().filter(|r| r["year"].as_i64().unwrap_or(0) < year).cloned().collect();
        if train.len() < minimum_train_rows { continue; }
        let model = fit_ridge(&train, lambda)?;
        let x = |r: &V| r["x"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap_or(0.0)).collect::<Vec<f64>>();
        for r in rows.iter().filter(|r| r["year"].as_i64() == Some(year)) {
            let p = predict(&model, &x(r));
            predictions.push(json!({"key": r["key"], "year": year,
                "trainedThroughYear": model.trained_through_year,
                "predictedMargin": p + r["nationalBaseline"].as_f64().unwrap_or(0.0),
                "actualMargin": r["actualMargin"],
                "residual": r["y"].as_f64().unwrap_or(0.0) - p, "clocks": r["clocks"]}));
        }
    }
    Ok(predictions)
}

/// metrics(predictions).
pub fn metrics(predictions: &[V]) -> Result<V, String> {
    need(!predictions.is_empty(), "assertion failed")?;
    let n = predictions.len() as f64;
    let mse = predictions.iter().map(|r| {
        let x = r["residual"].as_f64().unwrap_or(0.0); x * x
    }).sum::<f64>() / n;
    let mut cycles: Vec<i64> = Vec::new();
    for r in predictions {
        let y = r["year"].as_i64().unwrap_or(0);
        if !cycles.contains(&y) { cycles.push(y); }
    }
    Ok(json!({"n": predictions.len(), "rmse": mse.sqrt(),
        "mae": predictions.iter().map(|r| r["residual"].as_f64().unwrap_or(0.0).abs()).sum::<f64>() / n,
        "directionAccuracy": predictions.iter().filter(|r| {
            (r["predictedMargin"].as_f64().unwrap_or(0.0) > 0.0) == (r["actualMargin"].as_f64().unwrap_or(0.0) > 0.0)
        }).count() as f64 / n,
        "cycles": cycles}))
}

/// probabilityFromMargin(margin, standardDeviation) — fixed t4 CDF clamped [0.01, 0.99].
pub fn probability_from_margin(margin: f64, standard_deviation: f64) -> Result<f64, String> {
    need(margin.is_finite() && standard_deviation.is_finite() && standard_deviation > 0.0, "assertion failed")?;
    let t = margin / (standard_deviation / 2f64.sqrt());
    let z = t / (4.0 + t * t).sqrt();
    Ok((0.5 + 0.75 * z - 0.25 * z * z * z).clamp(0.01, 0.99))
}

/// scenarios({model, features, environment, assessmentAtMs, residualRms, definition}).
pub fn scenarios(model: &RidgeModel, features: &V, environment: &V, assessment_at_ms: f64,
    residual_rms: f64, definition: &V) -> Result<V, String> {
    need(assessment_at_ms.is_finite(), "assertion failed")?;
    let source_at_ms = baal_service_runtime::contract::parse_iso_ms(environment["sourceAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN);
    let observed_at_ms = baal_service_runtime::contract::parse_iso_ms(environment["observedAt"].as_str().unwrap_or("")).unwrap_or(f64::NAN);
    need(source_at_ms.is_finite() && observed_at_ms.is_finite(), "assertion failed")?;
    need(source_at_ms <= observed_at_ms && observed_at_ms <= assessment_at_ms, "Causal polling clocks")?;
    need(assessment_at_ms - source_at_ms <= environment["maximumAgeMs"].as_f64().unwrap_or(f64::NAN), "Expired current environment")?;
    let rep = environment["republicanPercent"].as_f64().unwrap_or(0.0);
    let dem = environment["democraticPercent"].as_f64().unwrap_or(0.0);
    need(rep > 0.0 && dem > 0.0, "assertion failed")?;
    let national_margin = (rep - dem) / (rep + dem);
    let x: Vec<f64> = features["x"].as_array().map(|a| a.iter().map(|v| v.as_f64().unwrap_or(0.0)).collect()).unwrap_or_default();
    let center = predict(model, &x) + national_margin;
    let residual_sd = residual_rms.max(0.10);
    let mut variants = Vec::new();
    for error in definition["pollingErrorSensitivityPoints"].as_array().cloned().unwrap_or_default() {
        let error = error.as_f64().unwrap_or(0.0);
        for bias in definition["additionalNationalBiasStressPoints"].as_array().cloned().unwrap_or_default() {
            let bias = bias.as_f64().unwrap_or(0.0);
            let sd = (residual_sd * residual_sd + (error / 100.0) * (error / 100.0)).sqrt();
            let margin = center + bias / 100.0;
            variants.push(json!({"nationalBiasPoints": bias, "pollingRmsPoints": error, "margin": margin,
                "standardDeviation": sd, "winProbability": probability_from_margin(margin, sd)?}));
        }
    }
    let lower = variants.iter().map(|v| v["winProbability"].as_f64().unwrap()).fold(f64::INFINITY, f64::min);
    let upper = variants.iter().map(|v| v["winProbability"].as_f64().unwrap()).fold(f64::NEG_INFINITY, f64::max);
    Ok(json!({"meanMargin": center, "nationalMargin": national_margin, "residualSd": residual_sd,
        "variants": variants, "lower": lower, "upper": upper,
        "recordedAt": baal_service_runtime::contract::ms_to_iso(assessment_at_ms),
        "sourceAt": environment["sourceAt"],
        "expiresAt": baal_service_runtime::contract::ms_to_iso(source_at_ms + environment["maximumAgeMs"].as_f64().unwrap_or(0.0)),
        "limitations": ["Scenario range is not a calibrated confidence interval.",
            "Uniform national swing transport and residual distribution are assumptions.",
            "Current polling has market dependence only if its stated sources do; this model does not import DDHQ win-probability outputs."]}))
}
