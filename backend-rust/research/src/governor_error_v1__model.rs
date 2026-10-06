// research/governor-error-v1/model.mjs — governor poll archive observations
// verified against an independent election-outcome archive.
use crate::election_baseline_v1__model::identity;
use crate::js::{field_str, js_number, js_string, js_template, js_truthy, map_key, num, num_str, require_that, safe_integer};
use indexmap::IndexMap;
use serde_json::{json, Value as V};

fn name(s: &str) -> String {
    identity(s).split(' ').filter(|t| t.len() > 1).collect::<Vec<_>>().join(" ")
}
const DAY: f64 = 86_400_000.0;
fn date(s: &str) -> f64 {
    let b = s.as_bytes();
    let ok = b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter().enumerate().all(|(i, c)| (i == 4 || i == 7) || c.is_ascii_digit());
    if !ok {
        return f64::NAN;
    }
    chrono::DateTime::parse_from_rfc3339(&format!("{s}T12:00:00Z"))
        .map(|d| d.timestamp_millis() as f64)
        .unwrap_or(f64::NAN)
}
fn digits(v: Option<&V>) -> bool {
    let s = js_template(v);
    !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit())
}

struct Cand {
    name: String,
    votes: i64,
    parties: Vec<V>,
    winner: bool,
    index: usize,
}

struct GovRace {
    d_name: String,
    r_name: String,
    d_votes: i64,
    r_votes: i64,
    total: i64,
    democratic_won: bool,
    source_urls: Vec<V>,
}

pub struct GovernorPollObservations {
    pub rows: Vec<V>,
    pub excluded: IndexMap<String, i64>,
    pub verified_outcome_races: usize,
}

pub fn governor_poll_observations(polls: &[V], outcomes: &[V]) -> Result<GovernorPollObservations, String> {
    let mut grouped: IndexMap<String, IndexMap<String, Vec<&V>>> = IndexMap::new();
    let mut races: IndexMap<String, GovRace> = IndexMap::new();
    let mut excluded: IndexMap<String, i64> = IndexMap::new();
    let mut seen: IndexMap<String, V> = IndexMap::new();
    let mut rows: Vec<V> = Vec::new();
    let mut reject = |r: &str| {
        *excluded.entry(r.to_string()).or_insert(0) += 1;
    };
    for r in outcomes {
        if field_str(r, "office_name") != "Governor"
            || field_str(r, "stage") != "general"
            || field_str(r, "special") != "false"
            || js_truthy(r.get("ranked_choice_round"))
            || js_number(r.get("cycle")) > 2022.0
        {
            continue;
        }
        let k = format!("{}:{}", js_template(r.get("cycle")), js_template(r.get("state_abbrev")));
        grouped
            .entry(k)
            .or_default()
            .entry(map_key(r.get("race_id")))
            .or_default()
            .push(r);
    }
    for (key, groups) in &grouped {
        if groups.len() != 1 {
            continue;
        }
        let all = &groups.values().next().unwrap();
        if all.iter().any(|r| !digits(r.get("votes"))) {
            continue;
        }
        let total = all.iter().map(|r| js_number(r.get("votes"))).sum::<f64>();
        if !safe_integer(total) || total <= 0.0 {
            continue;
        }
        let total = total as i64;
        let mut candidates: IndexMap<String, Cand> = IndexMap::new();
        for r in all.iter().copied() {
            if !js_truthy(r.get("candidate_name")) || !js_truthy(r.get("candidate_id")) {
                continue;
            }
            let cid = map_key(r.get("candidate_id"));
            let idx = candidates.len();
            let c = candidates.entry(cid).or_insert_with(|| Cand {
                name: name(&js_string(r.get("candidate_name").unwrap_or(&V::Null))),
                votes: 0,
                parties: Vec::new(),
                winner: false,
                index: idx,
            });
            let nm = name(&js_string(r.get("candidate_name").unwrap_or(&V::Null)));
            require_that(c.name == nm, "CONFLICTING_GOVERNOR_CANDIDATE")?;
            c.votes += js_number(r.get("votes")) as i64;
            let party = r.get("ballot_party").cloned().unwrap_or(V::Null);
            if !c.parties.iter().any(|x| *x == party) {
                c.parties.push(party);
            }
            // JS: c.winner ||= r.winner === 'true' (strict string equality)
            if r["winner"].as_str() == Some("true") {
                c.winner = true;
            }
        }
        let values: Vec<&Cand> = candidates.values().collect();
        let ds: Vec<usize> = values
            .iter()
            .filter(|c| c.parties.iter().any(|x| x == "DEM"))
            .map(|c| c.index)
            .collect();
        let rs: Vec<usize> = values
            .iter()
            .filter(|c| c.parties.iter().any(|x| x == "REP"))
            .map(|c| c.index)
            .collect();
        let winners: Vec<usize> = values.iter().filter(|c| c.winner).map(|c| c.index).collect();
        let max_votes = values.iter().map(|c| c.votes).max().unwrap_or(0);
        if ds.len() != 1
            || rs.len() != 1
            || winners.len() != 1
            || ![ds[0], rs[0]].contains(&winners[0])
            || candidates.values().nth(winners[0]).map(|c| c.votes) != Some(max_votes)
        {
            continue;
        }
        let d_idx = ds[0];
        let r_idx = rs[0];
        let w_idx = winners[0];
        let d_cand = candidates.values().nth(d_idx).unwrap();
        let r_cand = candidates.values().nth(r_idx).unwrap();
        let (d_name, d_votes) = (d_cand.name.clone(), d_cand.votes);
        let (r_name, r_votes) = (r_cand.name.clone(), r_cand.votes);
        let mut source_urls: Vec<V> = Vec::new();
        for r in all.iter().copied() {
            let s = r.get("source").cloned().unwrap_or(V::Null);
            if js_truthy(Some(&s)) && !source_urls.contains(&s) {
                source_urls.push(s);
            }
        }
        races.insert(
            key.clone(),
            GovRace {
                d_name,
                r_name,
                d_votes,
                r_votes,
                total,
                democratic_won: w_idx == d_idx,
                source_urls,
            },
        );
    }
    for p in polls {
        if field_str(p, "type_simple") != "Gov-G" {
            continue;
        }
        let year = js_number(p.get("cycle"));
        let key = format!("{}:{}", num_str(year), js_template(p.get("location")));
        let race = races.get(&key);
        if !safe_integer(year) || year > 2022.0 || race.is_none() {
            reject("regular_final_outcome_unavailable");
            continue;
        }
        let race = race.unwrap();
        if field_str(p, "cand1_party") != "DEM"
            || field_str(p, "cand2_party") != "REP"
            || name(field_str(p, "cand1_name")) != race.d_name
            || name(field_str(p, "cand2_name")) != race.r_name
        {
            reject("final_candidate_identity_unmatched");
            continue;
        }
        if js_truthy(p.get("partisan")) && field_str(p, "partisan") != "NA" {
            reject("partisan_or_internal");
            continue;
        }
        let field_at = date(field_str(p, "polldate"));
        let election_at = date(field_str(p, "electiondate"));
        let horizon = (election_at - field_at) / DAY;
        if !horizon.is_finite()
            || horizon <= 0.0
            || !field_str(p, "electiondate").starts_with(&format!("{}-11-", num_str(year)))
            || (horizon - js_number(p.get("time_to_election"))).abs() > 1.0
        {
            reject("election_or_field_clock_invalid");
            continue;
        }
        let d = js_number(p.get("cand1_pct"));
        let r = js_number(p.get("cand2_pct"));
        let ad = js_number(p.get("cand1_actual"));
        let ar = js_number(p.get("cand2_actual"));
        if ![d, r, ad, ar].iter().all(|x| x.is_finite() && *x > 0.0 && *x < 100.0) || d + r > 100.001 {
            reject("invalid_vote_shares");
            continue;
        }
        let vd = 100.0 * race.d_votes as f64 / race.total as f64;
        let vr = 100.0 * race.r_votes as f64 / race.total as f64;
        if (ad - vd).abs() > 0.25
            || (ar - vr).abs() > 0.25
            || ((ad > ar) != race.democratic_won)
        {
            reject("separate_outcome_archive_disagrees");
            continue;
        }
        if ![p.get("poll_id"), p.get("question_id"), p.get("pollster_rating_id")]
            .iter()
            .all(|x| digits(*x))
        {
            reject("source_identity_missing");
            continue;
        }
        let id = format!("{}:{}", js_template(p.get("race_id")), js_template(p.get("question_id")));
        let row = json!({
            "id": id,
            "key": key,
            "year": num(year),
            "state": p["location"],
            "pollId": p["poll_id"],
            "questionId": p["question_id"],
            "pollsterId": p["pollster_rating_id"],
            "pollster": p["pollster"],
            "fieldDate": p["polldate"],
            "fieldAt": num(field_at),
            "electionDate": p["electiondate"],
            "electionAt": num(election_at),
            "horizonDays": num(horizon),
            "democratic": num(d),
            "republican": num(r),
            "remaining": num(100.0 - d - r),
            "pollMargin": num(d - r),
            "actualMargin": num(vd - vr),
            "democraticWon": race.democratic_won,
            "population": "not_in_archive",
            "sourceFinalMargin": num(ad - ar),
            "finalSourceUrls": race.source_urls,
        });
        if let Some(old) = seen.get(&id) {
            require_that(*old == row, "conflicting_source_question")?;
            reject("identical_duplicate");
            continue;
        }
        seen.insert(id, row.clone());
        rows.push(row);
    }
    rows.sort_by(|a, b| {
        js_number(a.get("year"))
            .partial_cmp(&js_number(b.get("year")))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| field_str(a, "key").cmp(field_str(b, "key")))
            .then_with(|| field_str(a, "id").cmp(field_str(b, "id")))
    });
    Ok(GovernorPollObservations {
        rows,
        excluded,
        verified_outcome_races: races.len(),
    })
}
