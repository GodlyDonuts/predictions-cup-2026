// live-paper/dip-init.mjs — one-time diversified-dip cohort initializer.
use serde_json::{json, Map, Value as V};
use std::fs;
use std::io::Write;
use std::path::Path;

fn arg<'a>(args: &'a V, name: &str) -> Result<&'a str, String> {
    args.get(name).and_then(V::as_str).ok_or_else(|| format!("Missing --{name}"))
}
fn read_json(file: &str) -> Result<V, String> {
    serde_json::from_str(&fs::read_to_string(file).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn write_new(file: &Path, text: &str) -> Result<(), String> {
    let mut f = fs::OpenOptions::new().write(true).create_new(true).open(file).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(file, fs::Permissions::from_mode(0o600)).map_err(|e| e.to_string())?;
    }
    f.write_all(text.as_bytes()).map_err(|e| e.to_string())
}

/// dipInit — the JS CLI takes process.argv flags; the port takes them as an
/// options object: {runtime, universe, identities, config, journal} paths plus
/// scaled/responsive booleans. The embedded dip-policy.json mirrors the JS
/// `new URL('./dip-policy.json', import.meta.url)` module-relative read.
pub fn dip_init(args: &V) -> Result<V, String> {
    let scaled = args.get("scaled") == Some(&V::Bool(true));
    let responsive = args.get("responsive") == Some(&V::Bool(true));
    if scaled && responsive { return Err("Choose one cohort".into()); }
    let name = if responsive { "dip-responsive" } else if scaled { "dip-scale" } else { "dip" };
    let dir = arg(args, "runtime")?;
    let universe = read_json(arg(args, "universe")?)?;
    let identity_input = read_json(arg(args, "identities")?)?;
    let base = read_json(arg(args, "config")?)?;
    let mut policy: V = serde_json::from_str(include_str!("../../../live-paper/dip-policy.json"))
        .map_err(|e| e.to_string())?;
    if scaled {
        let p = policy.as_object_mut().ok_or("Invalid dip policy")?;
        p.insert("profile".into(), json!("scale"));
        p.insert("maxOpenMarkets".into(), json!(100));
        p.insert("maxPerState".into(), json!(4));
        p.insert("maxPerOffice".into(), json!(50));
        p.insert("maxMarketCost".into(), json!("1000.000000"));
    }
    if responsive {
        let p = policy.as_object_mut().ok_or("Invalid dip policy")?;
        p.insert("executionPolicy".into(), json!("responsive_v2"));
        p.insert("requireScheduledEntry".into(), json!(true));
        p.insert("maxOpenMarkets".into(), json!(3));
    }
    if Path::new(dir).join(format!("{name}-bank.json")).exists()
        || Path::new(dir).join(format!("{name}-state.json")).exists()
    {
        return Err("Refusing to replace existing dip accounting".into());
    }
    let mut c = base.as_object().cloned().unwrap_or_default();
    c.insert("feeBps".into(), json!(0));
    c.insert("strategyUniverse".into(),
        json!(if scaled { "diversified-dip-scale-v1" } else { "diversified-dip-v1" }));
    let mut seen = std::collections::BTreeSet::new();
    let mut market_ids = Vec::new();
    for x in universe["exchanges"].as_array().cloned().unwrap_or_default() {
        let key = baal_service_runtime::json_text::js_stringify(&x["marketId"]);
        if seen.insert(key) { market_ids.push(x["marketId"].clone()); }
    }
    c.insert("marketIds".into(), json!(market_ids));
    c.insert("orderShares".into(), json!(if scaled { 1000 } else { 25 }));
    c.insert("maxPositionShares".into(), json!(if scaled { 2000 } else { 50 }));
    c.insert("maxExposure".into(), json!(if scaled { "50000.000000" } else { "5000.000000" }));
    c.insert("cashReserve".into(), json!(if scaled { "50000.000000" } else { "95000.000000" }));
    c.insert("stopLoss".into(), policy["stopLoss"].clone());
    c.insert("exitAfterMs".into(), policy["maximumHoldMs"].clone());
    let c = V::Object(c);
    if identity_input["tournamentId"] != c["tournamentId"]
        || identity_input["kind"].as_str() != Some("derived_cached_market_identity")
    {
        return Err("Identity input scope mismatch".into());
    }
    let mut identities = Map::new();
    for m in identity_input["markets"].as_array().cloned().unwrap_or_default() {
        let node = &m["node"];
        identities.insert(
            baal_service_runtime::json_text::js_stringify(&m["marketId"]),
            json!({
                "title": m["title"],
                "state": if baal_service_runtime::json_text::js_truthy(node.get("usState")) { node["usState"].clone() } else { V::Null },
                "office": if baal_service_runtime::json_text::js_truthy(node.get("officeLevel")) { node["officeLevel"].clone() } else { V::Null },
                "party": if node["resolutionType"].as_str() == Some("Party Winner") { node["winnerName"].clone() } else { V::Null },
                "identitySource": "cached_structured_metadata",
                "resolutionRulesVerified": false,
            }),
        );
    }
    let out = crate::dip_shadow::create_dip(
        &fs::read_to_string(arg(args, "journal")?).map_err(|e| e.to_string())?,
        &c, &universe, &policy, &V::Object(identities))?;
    let bank = &out["bank"];
    let state = &out["state"];
    write_new(&Path::new(dir).join(format!("{name}-state.json")),
        &serde_json::to_string_pretty(state).map_err(|e| e.to_string())?)?;
    write_new(&Path::new(dir).join(format!("{name}-bank.json")),
        &serde_json::to_string_pretty(bank).map_err(|e| e.to_string())?)?;
    Ok(json!({
        "mode": "diversified_dip_paper", "profile": name,
        "markets": bank["universe"]["exchanges"].as_array().map(|a| a.len()).unwrap_or(0),
        "cutoff": bank["cutoff"],
        "bookHistoriesSeeded": state["bookHistory"].as_object().map(|o| o.len()).unwrap_or(0),
        "historicalCandlesSeeded": state["histories"].as_object().map(|o| o.len()).unwrap_or(0),
        "feeBps": 0,
        "observations": state["wallet"]["observations"],
        "competitionOrders": 0,
    }))
}
