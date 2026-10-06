// research/election-baseline-v1/model.mjs — MIT election CSV ingestion and the
// structural ridge-margin model. Internal Maps become IndexMap (insertion
// order); outputs keep the JS object key order for JSON.stringify parity.
// Computed f64 values are stored through num() so integral doubles serialize
// like JS numbers ("1976", not "1976.0").
use crate::js::{date_parse, field_str, iso, js_number, js_number_str, js_string, js_template, num, or_empty, require_that};
use baal_service_runtime::json_text::sha256;
use indexmap::IndexMap;
use serde_json::{json, Map, Value as V};
use std::collections::HashSet;

pub fn hash(bytes: &[u8]) -> String {
    sha256(bytes)
}

pub fn parse_csv(text: &str) -> Result<Vec<V>, String> {
    let chars: Vec<char> = text.chars().collect();
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if quoted {
            if c == '"' && chars.get(i + 1) == Some(&'"') {
                field.push('"');
                i += 1;
            } else if c == '"' {
                quoted = false;
            } else {
                field.push(c);
            }
        } else if c == '"' {
            if !field.is_empty() {
                return Err("Quote inside unquoted field".into());
            }
            quoted = true;
        } else if c == ',' {
            row.push(std::mem::take(&mut field));
        } else if c == '\n' {
            row.push(field.strip_suffix('\r').unwrap_or(&field).to_string());
            field.clear();
            if row.iter().any(|x| !x.is_empty()) {
                rows.push(std::mem::take(&mut row));
            } else {
                row.clear();
            }
        } else {
            field.push(c);
        }
        i += 1;
    }
    if quoted {
        return Err("Unterminated CSV quote".into());
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field.strip_suffix('\r').unwrap_or(&field).to_string());
        rows.push(row);
    }
    if rows.is_empty() {
        return Err("Invalid CSV header".into());
    }
    let header = rows.remove(0);
    if header.is_empty() || header.iter().collect::<HashSet<_>>().len() != header.len() {
        return Err("Invalid CSV header".into());
    }
    let mut out = Vec::with_capacity(rows.len());
    for (i, values) in rows.iter().enumerate() {
        if values.len() != header.len() {
            return Err(format!("CSV width {i}"));
        }
        let mut m = Map::new();
        for (j, h) in header.iter().enumerate() {
            m.insert(h.clone(), V::String(values[j].clone()));
        }
        out.push(V::Object(m));
    }
    Ok(out)
}

pub fn identity(name: &str) -> String {
    let cleaned: String = name
        .to_uppercase()
        .chars()
        .map(|c| if c.is_ascii_uppercase() || c.is_ascii_digit() || c == ' ' { c } else { ' ' })
        .collect();
    let mut parts: Vec<&str> = cleaned
        .split_whitespace()
        .filter(|x| !["JR", "SR", "II", "III", "IV"].contains(x))
        .collect();
    parts.sort_unstable();
    parts.join(" ")
}

fn boolean(v: &V) -> bool {
    js_string(v).to_lowercase() == "true"
}

fn vote_count(value: Option<&V>) -> Result<i64, String> {
    let n = js_number(value);
    if !(n.is_finite() && n.fract() == 0.0 && n.abs() <= 9_007_199_254_740_991.0 && n >= 0.0) {
        return Err("Invalid vote count".into());
    }
    Ok(n as i64)
}

fn major(value: &str) -> Option<&'static str> {
    match value {
        "REPUBLICAN" => Some("REPUBLICAN"),
        "DEMOCRAT" => Some("DEMOCRAT"),
        _ => None,
    }
}

struct CandidateAcc {
    id: String,
    name: String,
    parties: Vec<String>,
    lines: IndexMap<String, IndexMap<String, i64>>,
}

// Ballot lines of the same person belong to that person. A fusion line is not a separate voter.
fn candidate_totals(rows: &[&V]) -> Result<Vec<V>, String> {
    let mut candidates: IndexMap<String, CandidateAcc> = IndexMap::new();
    for row in rows {
        let id = identity(field_str(row, "candidate"));
        if id.is_empty() {
            continue;
        }
        let entry = candidates.entry(id.clone()).or_insert_with(|| CandidateAcc {
            id,
            name: js_string(row.get("candidate").unwrap_or(&V::Null)),
            parties: Vec::new(),
            lines: IndexMap::new(),
        });
        if let Some(p) = major(field_str(row, "party_simplified")) {
            if !entry.parties.iter().any(|x| x == p) {
                entry.parties.push(p.to_string());
            }
        }
        let line = format!("{}\u{0}writein={}", js_template(row.get("party_detailed")), boolean(&row["writein"]));
        let mode = {
            let m = or_empty(row.get("mode"));
            if m.is_empty() { "total".to_string() } else { m }
        };
        let modes = entry.lines.entry(line).or_default();
        if modes.contains_key(&mode) {
            return Err("Duplicate candidate ballot-line/mode".into());
        }
        modes.insert(mode, vote_count(row.get("candidatevotes"))?);
    }
    Ok(candidates
        .into_values()
        .map(|c| {
            let votes: i64 = c
                .lines
                .values()
                .map(|modes| {
                    if let Some(t) = modes.get("total") {
                        *t
                    } else {
                        modes.values().sum()
                    }
                })
                .sum();
            json!({"id": c.id, "name": c.name, "parties": c.parties, "votes": votes})
        })
        .collect())
}

pub struct PresidentTables {
    pub states: IndexMap<String, V>,
    pub national: IndexMap<i64, V>,
    pub years: Vec<i64>,
}

pub fn presidential_tables(rows: &[V]) -> Result<PresidentTables, String> {
    let mut grouped: IndexMap<String, Vec<&V>> = IndexMap::new();
    for r in rows {
        if field_str(r, "office") != "US PRESIDENT" {
            continue;
        }
        let sp = field_str(r, "state_po");
        if !(sp.len() == 2 && sp.bytes().all(|b| b.is_ascii_uppercase())) {
            continue;
        }
        let key = format!("{}:{}", field_str(r, "year"), sp);
        grouped.entry(key).or_default().push(r);
    }
    let mut states: IndexMap<String, V> = IndexMap::new();
    let mut national_votes: IndexMap<i64, (i64, i64, i64)> = IndexMap::new();
    for (key, group) in &grouped {
        let candidates = candidate_totals(group)?;
        if candidates
            .iter()
            .any(|c| c["parties"].as_array().map(|a| a.len() > 1).unwrap_or(false))
        {
            return Err("Ambiguous presidential candidate party".into());
        }
        let republican: i64 = candidates
            .iter()
            .filter(|c| c["parties"].as_array().is_some_and(|a| a.iter().any(|p| p == "REPUBLICAN")))
            .map(|c| c["votes"].as_i64().unwrap_or(0))
            .sum();
        let democratic: i64 = candidates
            .iter()
            .filter(|c| c["parties"].as_array().is_some_and(|a| a.iter().any(|p| p == "DEMOCRAT")))
            .map(|c| c["votes"].as_i64().unwrap_or(0))
            .sum();
        if republican == 0 || democratic == 0 {
            return Err("Missing presidential major party".into());
        }
        let mut it = key.split(':');
        let year_text = it.next().unwrap_or("");
        let state = it.next().unwrap_or("");
        let year = js_number_str(year_text);
        states.insert(
            key.clone(),
            json!({
                "year": num(year),
                "state": state,
                "republican": republican,
                "democratic": democratic,
                "margin": num((republican as f64 - democratic as f64) / (republican as f64 + democratic as f64)),
            }),
        );
        let y = year as i64;
        let n = national_votes.entry(y).or_insert((0, 0, 0));
        n.0 += republican;
        n.1 += democratic;
        n.2 += 1;
    }
    let mut national: IndexMap<i64, V> = IndexMap::new();
    for (year, n) in &national_votes {
        if n.2 != 51 {
            return Err(format!("Incomplete presidential states {}:{}", year, n.2));
        }
        national.insert(
            *year,
            json!({
                "republican": n.0,
                "democratic": n.1,
                "states": n.2,
                "year": *year,
                "margin": num((n.0 as f64 - n.1 as f64) / (n.0 as f64 + n.1 as f64)),
            }),
        );
    }
    let mut years: Vec<i64> = national.keys().copied().collect();
    years.sort_unstable();
    Ok(PresidentTables { states, national, years })
}

pub struct SenateRaces {
    pub races: IndexMap<String, V>,
    pub exclusions: Vec<V>,
}

pub fn senate_races(rows: &[V]) -> SenateRaces {
    let mut grouped: IndexMap<String, Vec<&V>> = IndexMap::new();
    let mut exclusions: Vec<V> = Vec::new();
    for r in rows {
        if field_str(r, "office") != "US SENATE"
            || field_str(r, "stage").to_lowercase() != "gen"
            || boolean(&r["special"])
            || boolean(&r["unofficial"])
        {
            continue;
        }
        let key = format!("{}:{}", field_str(r, "year"), field_str(r, "state_po"));
        grouped.entry(key).or_default().push(r);
    }
    let mut races: IndexMap<String, V> = IndexMap::new();
    for (key, group) in &grouped {
        match senate_race(key, group) {
            Ok(race) => {
                races.insert(key.clone(), race);
            }
            Err(reason) => exclusions.push(json!({"key": key, "reason": reason})),
        }
    }
    SenateRaces { races, exclusions }
}

fn ranked(candidates: &[V], party: &str) -> Vec<V> {
    let mut v: Vec<V> = candidates
        .iter()
        .filter(|c| c["parties"].as_array().is_some_and(|a| a.iter().any(|p| p == party)))
        .cloned()
        .collect();
    v.sort_by(|a, b| b["votes"].as_i64().cmp(&a["votes"].as_i64()));
    v
}

fn senate_race(key: &str, group: &[&V]) -> Result<V, String> {
    let candidates = candidate_totals(group)?;
    if candidates
        .iter()
        .any(|c| c["parties"].as_array().map(|a| a.len() > 1).unwrap_or(false))
    {
        return Err("cross_major_party_fusion".into());
    }
    let total = group
        .iter()
        .map(|r| vote_count(r.get("totalvotes")))
        .collect::<Result<Vec<i64>, _>>()?
        .into_iter()
        .max()
        .unwrap_or(0);
    if total < 100 {
        return Err("uncontested_or_invalid_total".into());
    }
    let r = ranked(&candidates, "REPUBLICAN");
    let d = ranked(&candidates, "DEMOCRAT");
    if r.first().map(|c| c["votes"].as_i64().unwrap_or(0)).unwrap_or(0) == 0
        || d.first().map(|c| c["votes"].as_i64().unwrap_or(0)).unwrap_or(0) == 0
    {
        return Err("missing_major_party".into());
    }
    if (r.get(1).map(|c| c["votes"].as_i64().unwrap_or(0)).unwrap_or(0) as f64) / total as f64 >= 0.01
        || (d.get(1).map(|c| c["votes"].as_i64().unwrap_or(0)).unwrap_or(0) as f64) / total as f64 >= 0.01
    {
        return Err("multiple_material_major_candidates".into());
    }
    let outside: Vec<&V> = candidates
        .iter()
        .filter(|c| c["parties"].as_array().map(|a| a.is_empty()).unwrap_or(true))
        .collect();
    if outside.iter().any(|c| {
        c["votes"].as_i64().unwrap_or(0) >= r[0]["votes"].as_i64().unwrap_or(0).max(d[0]["votes"].as_i64().unwrap_or(0))
    }) || outside.iter().map(|c| c["votes"].as_i64().unwrap_or(0)).sum::<i64>() as f64 / total as f64 >= 0.20
    {
        return Err("material_independent_competition".into());
    }
    if candidates.iter().map(|c| c["votes"].as_i64().unwrap_or(0)).sum::<i64>() as f64 > total as f64 * 1.001 {
        return Err("candidate_total_exceeds_reported_total".into());
    }
    let mut it = key.split(':');
    let year = js_number_str(it.next().unwrap_or(""));
    let state = it.next().unwrap_or("").to_string();
    let winner = if r[0]["votes"].as_i64().unwrap_or(0) > d[0]["votes"].as_i64().unwrap_or(0) {
        r[0].clone()
    } else {
        d[0].clone()
    };
    Ok(json!({
        "year": num(year),
        "state": state,
        "republican": r[0],
        "democratic": d[0],
        "margin": num((r[0]["votes"].as_i64().unwrap_or(0) as f64 - d[0]["votes"].as_i64().unwrap_or(0) as f64)
            / (r[0]["votes"].as_i64().unwrap_or(0) as f64 + d[0]["votes"].as_i64().unwrap_or(0) as f64)),
        "total": total,
        "winner": winner,
    }))
}

fn prior_president(tables: &PresidentTables, year: f64) -> Result<i64, String> {
    match tables.years.iter().copied().filter(|y| (*y as f64) < year).last() {
        Some(y) => Ok(y),
        None => Err("Missing strictly prior president".into()),
    }
}

pub fn race_features(args: &V, senate: &SenateRaces, president: &PresidentTables) -> Result<Option<V>, String> {
    let year = js_number(args.get("year"));
    let state = field_str(args, "state");
    let pres_year = prior_president(president, year)?;
    let nat = president.national.get(&pres_year).cloned().unwrap_or(V::Null);
    let state_pres = president.states.get(&format!("{}:{}", pres_year, state));
    let prior = senate.races.get(&format!("{}:{}", year as i64 - 6, state));
    let (Some(state_pres), Some(prior)) = (state_pres, prior) else {
        return Ok(None);
    };
    let old_pres_year = prior_president(president, js_number(prior.get("year")))?;
    let old_national = president.national.get(&old_pres_year).cloned().unwrap_or(V::Null);
    let r_id = match args.get("republican") {
        Some(V::String(s)) => identity(s),
        v => identity(field_str(&v.cloned().unwrap_or(V::Null), "name")),
    };
    let d_id = match args.get("democratic") {
        Some(V::String(s)) => identity(s),
        v => identity(field_str(&v.cloned().unwrap_or(V::Null), "name")),
    };
    let winner_id = field_str(&prior["winner"], "id");
    let incumbent = if winner_id == r_id {
        1
    } else if winner_id == d_id {
        -1
    } else {
        0
    };
    Ok(Some(json!({
        "x": [
            V::from(1),
            num(state_pres["margin"].as_f64().unwrap_or(f64::NAN) - nat["margin"].as_f64().unwrap_or(f64::NAN)),
            num(prior["margin"].as_f64().unwrap_or(f64::NAN) - old_national["margin"].as_f64().unwrap_or(f64::NAN)),
            V::from(incumbent),
        ],
        "clocks": {
            "presYear": pres_year,
            "priorSeatYear": prior["year"],
            "priorSeatPresYear": old_pres_year,
        },
        "nationalBaseline": nat["margin"],
        "stateMargin": state_pres["margin"],
        "priorSeatMargin": prior["margin"],
        "incumbent": incumbent,
        "incumbentNameMatch": incumbent != 0,
    })))
}

pub fn build_rows(senate: &SenateRaces, president: &PresidentTables, minimum_year: f64) -> Result<Vec<V>, String> {
    let mut out: Vec<V> = Vec::new();
    for r in senate.races.values() {
        if js_number(r.get("year")) < minimum_year {
            continue;
        }
        if let Some(f) = race_features(r, senate, president)? {
            out.push(json!({
                "key": format!("{}:{}", js_template(r.get("year")), js_template(r.get("state"))),
                "year": r["year"],
                "state": r["state"],
                "x": f["x"],
                "y": num(r["margin"].as_f64().unwrap_or(f64::NAN) - f["nationalBaseline"].as_f64().unwrap_or(f64::NAN)),
                "actualMargin": r["margin"],
                "nationalBaseline": f["nationalBaseline"],
                "clocks": f["clocks"],
            }));
        }
    }
    out.sort_by(|a, b| {
        js_number(a.get("year"))
            .partial_cmp(&js_number(b.get("year")))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| field_str(a, "state").cmp(field_str(b, "state")))
    });
    Ok(out)
}

fn solve(matrix: Vec<Vec<f64>>, vector: Vec<f64>) -> Result<Vec<f64>, String> {
    let n = vector.len();
    let mut a: Vec<Vec<f64>> = matrix
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let mut r = r.clone();
            r.push(vector[i]);
            r
        })
        .collect();
    for col in 0..n {
        let mut pivot = col;
        for i in (col + 1)..n {
            if a[i][col].abs() > a[pivot][col].abs() {
                pivot = i;
            }
        }
        a.swap(pivot, col);
        if a[col][col].abs() < 1e-12 {
            return Err("Singular regression".into());
        }
        let scale = a[col][col];
        for j in col..=n {
            a[col][j] /= scale;
        }
        for i in 0..n {
            if i != col {
                let factor = a[i][col];
                for j in col..=n {
                    a[i][j] -= factor * a[col][j];
                }
            }
        }
    }
    Ok(a.iter().map(|r| r[n]).collect())
}

pub fn fit_ridge(rows: &[V], lambda: f64) -> Result<V, String> {
    require_that(rows.len() >= 20, "rows.length >= 20")?;
    require_that(
        lambda.is_finite() && lambda > 0.0,
        "Number.isFinite(lambda) && lambda > 0",
    )?;
    let width = rows[0]["x"].as_array().map(|a| a.len()).unwrap_or(0);
    let mut means = vec![0.0];
    let mut scales = vec![1.0];
    for j in 1..width {
        let mean = rows.iter().map(|r| r["x"][j].as_f64().unwrap_or(f64::NAN)).sum::<f64>() / rows.len() as f64;
        let sd = (rows
            .iter()
            .map(|r| (r["x"][j].as_f64().unwrap_or(f64::NAN) - mean).powi(2))
            .sum::<f64>()
            / rows.len() as f64)
            .sqrt();
        means.push(mean);
        scales.push(if sd > 1e-9 { sd } else { 1.0 });
    }
    let mut matrix = vec![vec![0.0; width]; width];
    let mut vector = vec![0.0; width];
    for r in rows {
        let x = x_of(r);
        let x: Vec<f64> = x.iter().enumerate().map(|(j, v)| (v - means[j]) / scales[j]).collect();
        let y = r["y"].as_f64().unwrap_or(f64::NAN);
        for j in 0..width {
            vector[j] += x[j] * y;
            for k in 0..width {
                matrix[j][k] += x[j] * x[k];
            }
        }
    }
    for j in 1..width {
        matrix[j][j] += lambda;
    }
    let coefficients = solve(matrix, vector)?;
    Ok(json!({
        "lambda": num(lambda),
        "n": rows.len(),
        "trainedThroughYear": num(rows.iter().map(|r| js_number(r.get("year"))).fold(f64::NEG_INFINITY, f64::max)),
        "means": means.iter().map(|v| num(*v)).collect::<Vec<V>>(),
        "scales": scales.iter().map(|v| num(*v)).collect::<Vec<V>>(),
        "coefficients": coefficients.iter().map(|v| num(*v)).collect::<Vec<V>>(),
    }))
}

pub fn predict(model: &V, x: &[f64]) -> f64 {
    let coefs = model["coefficients"].as_array();
    let means = model["means"].as_array();
    let scales = model["scales"].as_array();
    x.iter()
        .enumerate()
        .map(|(j, v)| {
            coefs.and_then(|c| c.get(j)).and_then(V::as_f64).unwrap_or(f64::NAN)
                * (*v - means.and_then(|m| m.get(j)).and_then(V::as_f64).unwrap_or(f64::NAN))
                / scales.and_then(|s| s.get(j)).and_then(V::as_f64).unwrap_or(f64::NAN)
        })
        .sum()
}

fn x_of(r: &V) -> Vec<f64> {
    r["x"]
        .as_array()
        .map(|a| a.iter().map(|v| v.as_f64().unwrap_or(f64::NAN)).collect())
        .unwrap_or_default()
}

pub fn expanding_check(rows: &[V], lambda: f64, last_year: f64, minimum_train_rows: usize) -> Result<Vec<V>, String> {
    let mut predictions = Vec::new();
    let mut years: Vec<f64> = Vec::new();
    for r in rows {
        let y = js_number(r.get("year"));
        if !years.contains(&y) {
            years.push(y);
        }
    }
    for year in years.iter().copied().filter(|y| *y <= last_year) {
        let train: Vec<V> = rows.iter().filter(|r| js_number(r.get("year")) < year).cloned().collect();
        if train.len() < minimum_train_rows {
            continue;
        }
        let model = fit_ridge(&train, lambda)?;
        for r in rows.iter().filter(|r| js_number(r.get("year")) == year) {
            let p = predict(&model, &x_of(r));
            predictions.push(json!({
                "key": r["key"],
                "year": num(year),
                "trainedThroughYear": model["trainedThroughYear"],
                "predictedMargin": num(p + r["nationalBaseline"].as_f64().unwrap_or(f64::NAN)),
                "actualMargin": r["actualMargin"],
                "residual": num(r["y"].as_f64().unwrap_or(f64::NAN) - p),
                "clocks": r["clocks"],
            }));
        }
    }
    Ok(predictions)
}

pub fn metrics(predictions: &[V]) -> Result<V, String> {
    require_that(!predictions.is_empty(), "predictions.length > 0")?;
    let n = predictions.len() as f64;
    let mse = predictions
        .iter()
        .map(|r| r["residual"].as_f64().unwrap_or(f64::NAN).powi(2))
        .sum::<f64>()
        / n;
    let mut cycles: Vec<f64> = Vec::new();
    for r in predictions {
        let y = js_number(r.get("year"));
        if !cycles.contains(&y) {
            cycles.push(y);
        }
    }
    Ok(json!({
        "n": predictions.len(),
        "rmse": num(mse.sqrt()),
        "mae": num(predictions.iter().map(|r| r["residual"].as_f64().unwrap_or(f64::NAN).abs()).sum::<f64>() / n),
        "directionAccuracy": num(predictions
            .iter()
            .filter(|r| {
                (r["predictedMargin"].as_f64().unwrap_or(f64::NAN) > 0.0)
                    == (r["actualMargin"].as_f64().unwrap_or(f64::NAN) > 0.0)
            })
            .count() as f64
            / n),
        "cycles": cycles.iter().map(|v| num(*v)).collect::<Vec<V>>(),
    }))
}

// Fixed t4 distribution: CDF(t) = 1/2 + 3z/4 - z^3/4, z=t/sqrt(4+t^2).
pub fn probability_from_margin(margin: f64, standard_deviation: f64) -> Result<f64, String> {
    require_that(
        margin.is_finite() && standard_deviation.is_finite() && standard_deviation > 0.0,
        "Number.isFinite(margin) && Number.isFinite(standardDeviation) && standardDeviation > 0",
    )?;
    let t = margin / (standard_deviation / std::f64::consts::SQRT_2);
    let z = t / (4.0 + t * t).sqrt();
    Ok((0.5 + 0.75 * z - 0.25 * z.powi(3)).clamp(0.01, 0.99))
}

pub fn scenarios(args: &V) -> Result<V, String> {
    let model = &args["model"];
    let features = &args["features"];
    let environment = &args["environment"];
    let assessment_at_ms = js_number(args.get("assessmentAtMs"));
    let residual_rms = js_number(args.get("residualRms"));
    let definition = &args["definition"];
    require_that(assessment_at_ms.is_finite(), "Number.isFinite(assessmentAtMs)")?;
    let source_at_ms = date_parse(field_str(environment, "sourceAt"));
    let observed_at_ms = date_parse(field_str(environment, "observedAt"));
    require_that(
        source_at_ms.is_finite() && observed_at_ms.is_finite(),
        "Number.isFinite(sourceAtMs) && Number.isFinite(observedAtMs)",
    )?;
    require_that(
        source_at_ms <= observed_at_ms && observed_at_ms <= assessment_at_ms,
        "Causal polling clocks",
    )?;
    require_that(
        assessment_at_ms - source_at_ms <= js_number(environment.get("maximumAgeMs")),
        "Expired current environment",
    )?;
    let rep = js_number(environment.get("republicanPercent"));
    let dem = js_number(environment.get("democraticPercent"));
    require_that(rep > 0.0 && dem > 0.0, "environment.republicanPercent > 0 && environment.democraticPercent > 0")?;
    let national_margin = (rep - dem) / (rep + dem);
    let center = predict(model, &x_of(features)) + national_margin;
    let residual_sd = residual_rms.max(0.10);
    let mut variants: Vec<V> = Vec::new();
    for error in definition["pollingErrorSensitivityPoints"].as_array().cloned().unwrap_or_default() {
        for bias in definition["additionalNationalBiasStressPoints"].as_array().cloned().unwrap_or_default() {
            let e = js_number(Some(&error));
            let b = js_number(Some(&bias));
            let sd = (residual_sd.powi(2) + (e / 100.0).powi(2)).sqrt();
            let margin = center + b / 100.0;
            variants.push(json!({
                "nationalBiasPoints": num(b),
                "pollingRmsPoints": num(e),
                "margin": num(margin),
                "standardDeviation": num(sd),
                "winProbability": num(probability_from_margin(margin, sd)?),
            }));
        }
    }
    let lower = variants
        .iter()
        .map(|v| v["winProbability"].as_f64().unwrap_or(f64::NAN))
        .fold(f64::INFINITY, f64::min);
    let upper = variants
        .iter()
        .map(|v| v["winProbability"].as_f64().unwrap_or(f64::NAN))
        .fold(f64::NEG_INFINITY, f64::max);
    Ok(json!({
        "meanMargin": num(center),
        "nationalMargin": num(national_margin),
        "residualSd": num(residual_sd),
        "variants": variants,
        "lower": num(lower),
        "upper": num(upper),
        "recordedAt": iso(assessment_at_ms),
        "sourceAt": environment["sourceAt"],
        "expiresAt": iso(source_at_ms + js_number(environment.get("maximumAgeMs"))),
        "limitations": [
            "Scenario range is not a calibrated confidence interval.",
            "Uniform national swing transport and residual distribution are assumptions.",
            "Current polling has market dependence only if its stated sources do; this model does not import DDHQ win-probability outputs.",
        ],
    }))
}
