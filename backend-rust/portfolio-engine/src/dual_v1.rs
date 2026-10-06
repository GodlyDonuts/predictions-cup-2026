use serde_json::{Value as V, json};
use sha2::{Digest, Sha256};
pub type Result<T> = std::result::Result<T, String>;
const SCALE: i64 = 1_000_000;
const TICK: i64 = 5_000;
fn need(ok: bool, c: &str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(format!("DUAL_NO_PAIR_{c}"))
    }
}
fn strv(v: &V) -> &str {
    v.as_str().unwrap_or("")
}
fn n(v: &V) -> i64 {
    v.as_i64().unwrap_or(0)
}
fn arr(v: &V) -> Vec<V> {
    v.as_array().cloned().unwrap_or_default()
}
fn index_key(k: &str) -> Option<u32> {
    let n = k.parse::<u32>().ok()?;
    if n < u32::MAX && n.to_string() == k {
        Some(n)
    } else {
        None
    }
}
fn obj(v: &V) -> Vec<(String, V)> {
    let mut a: Vec<_> = v
        .as_object()
        .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();
    a.sort_by(|(a, _), (b, _)| match (index_key(a), index_key(b)) {
        (Some(a), Some(b)) => a.cmp(&b),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.encode_utf16().cmp(b.encode_utf16()),
    });
    a
}
pub fn amount(v: &V) -> Result<i64> {
    let s = v.as_str().ok_or("MONEY")?;
    let neg = s.starts_with('-');
    let x = s.strip_prefix('-').unwrap_or(s);
    let parts: Vec<_> = x.split('.').collect();
    need(
        parts.len() <= 2
            && !parts[0].is_empty()
            && (parts[0] == "0" || !parts[0].starts_with('0'))
            && parts[0].bytes().all(|b| b.is_ascii_digit()),
        "MONEY",
    )?;
    let f = parts.get(1).copied().unwrap_or("");
    need(
        f.len() <= 6
            && (parts.len() == 1 || !f.is_empty())
            && f.bytes().all(|b| b.is_ascii_digit()),
        "MONEY",
    )?;
    let a = parts[0]
        .parse::<i64>()
        .map_err(|_| "MONEY")?
        .checked_mul(SCALE)
        .and_then(|a| a.checked_add(format!("{f:0<6}").parse::<i64>().ok()?))
        .ok_or("MONEY_OVERFLOW")?;
    Ok(if neg { -a } else { a })
}
pub fn money(a: i64) -> V {
    let x = a.unsigned_abs();
    json!(format!(
        "{}{}.{:06}",
        if a < 0 { "-" } else { "" },
        x / 1_000_000,
        x % 1_000_000
    ))
}
// Gregorian civil-date conversion uses integer arithmetic, independent of local timezone.
fn days(y: i64, m: i64, d: i64) -> i64 {
    let y = y - if m <= 2 { 1 } else { 0 };
    let era = y.div_euclid(400);
    let yo = y - era * 400;
    let mp = m + if m > 2 { -3 } else { 9 };
    era * 146097 + yo * 365 + yo / 4 - yo / 100 + (153 * mp + 2) / 5 + d - 1 - 719468
}
pub fn instant(v: &V) -> Result<i64> {
    let s = strv(v);
    need(
        s.len() == 24
            && s.as_bytes()[4] == b'-'
            && s.as_bytes()[7] == b'-'
            && s.as_bytes()[10] == b'T'
            && s.as_bytes()[13] == b':'
            && s.as_bytes()[16] == b':'
            && s.as_bytes()[19] == b'.'
            && s.ends_with('Z'),
        "TIME",
    )?;
    let p = |a, b| {
        s.get(a..b)
            .unwrap_or("")
            .parse::<i64>()
            .map_err(|_| "TIME".to_string())
    };
    let (y, m, d, h, mi, se, ms) = (
        p(0, 4)?,
        p(5, 7)?,
        p(8, 10)?,
        p(11, 13)?,
        p(14, 16)?,
        p(17, 19)?,
        p(20, 23)?,
    );
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let md = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    need(
        (1..=12).contains(&m)
            && d >= 1
            && d <= md[(m - 1) as usize]
            && (0..24).contains(&h)
            && (0..60).contains(&mi)
            && (0..60).contains(&se)
            && (0..1000).contains(&ms),
        "TIME",
    )?;
    Ok(days(y, m, d) * 86400000 + h * 3600000 + mi * 60000 + se * 1000 + ms)
}
fn iso(t: i64) -> V {
    let z = t.div_euclid(86400000) + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    y += if m <= 2 { 1 } else { 0 };
    let a = t.rem_euclid(86400000);
    json!(format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z",
        a / 3600000,
        a / 60000 % 60,
        a / 1000 % 60,
        a % 1000
    ))
}
fn canonical(v: &V, out: &mut String) {
    match v {
        V::Object(_) => {
            out.push('{');
            for (i, (k, v)) in obj(v).iter().enumerate() {
                if i > 0 {
                    out.push(',')
                }
                out.push_str(&serde_json::to_string(k).unwrap());
                out.push(':');
                canonical(v, out);
            }
            out.push('}')
        }
        V::Array(a) => {
            out.push('[');
            for (i, v) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',')
                }
                canonical(v, out)
            }
            out.push(']')
        }
        V::Number(n) => {
            let mut b = ryu_js::Buffer::new();
            out.push_str(b.format_finite(n.as_f64().unwrap()));
        }
        _ => out.push_str(&serde_json::to_string(v).unwrap()),
    }
}
pub fn hash(v: &V) -> String {
    let mut text = String::new();
    canonical(v, &mut text);
    format!("{:x}", Sha256::digest(text.as_bytes()))
}
pub fn policy() -> V {
    json!({"initialCash":"100.000000","quantity":1,"latencyMs":2000,"intentLifetimeMs":120000,"maxAgeMs":20000,"maxSkewMs":10000,"depthFractionBps":100,"adverseTicks":1,"maxOwnedAndReserved":"1.000000","cumulativeBuyBudget":"1.000000","minimumPackageMargin":"0.010000","takeProfit":"0.010000","packageStopLoss":"1.000000","maximumPairedHoldMs":3024000000i64,"cooldownMs":60000,"feePerShare":"0.000000"})
}
fn ids(b: &V) -> Vec<String> {
    arr(&b["review"]["legs"])
        .iter()
        .map(|l| strv(&l["exchangeId"]).to_owned())
        .collect()
}
fn valid_id(v: &V) -> bool {
    let s = strv(v);
    !s.is_empty()
        && !s.starts_with('0')
        && s.bytes().all(|b| b.is_ascii_digit())
        && s.parse::<u64>().is_ok_and(|n| n <= 9007199254740991)
}
fn hex(v: &V) -> bool {
    let s = strv(v);
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
pub fn review(r: &V) -> Result<()> {
    need(
        r["schemaVersion"] == 1
            && r["kind"] == "conditional_no_pair_review"
            && r["id"].is_string()
            && r["tournamentId"].is_string()
            && hex(&r["evidenceHash"]),
        "REVIEW",
    )?;
    need(
        instant(&r["expiresAt"])? > instant(&r["recordedAt"])?,
        "REVIEW_CLOCK",
    )?;
    let legs = arr(&r["legs"]);
    need(legs.len() == 2, "REVIEW_SCOPE")?;
    need(
        legs[0]["exchangeId"] != legs[1]["exchangeId"]
            && legs[0]["marketId"] != legs[1]["marketId"],
        "REVIEW_SCOPE",
    )?;
    let fields = [
        "raceId",
        "stageId",
        "usState",
        "raceName",
        "raceStage",
        "officeLevel",
        "contractType",
        "electionDate",
        "resolutionType",
    ];
    for l in &legs {
        let root = &l["root"];
        let p = &root["contract_details"];
        need(
            valid_id(&l["marketId"])
                && valid_id(&l["exchangeId"])
                && l["outcome"] == "NO"
                && hex(&l["rulesHash"]),
            "REVIEW_SCOPE",
        )?;
        need(
            root["node_type"] == "contract"
                && root["contract_type"] == "Election Outcome"
                && root.get("settled_with") == Some(&V::Null)
                && p.is_object()
                && l["captureMarketId"] == l["marketId"],
            "ROOT",
        )?;
        need(
            valid_id(&root["contract_id"]) && root["node_id"] == root["contract_id"],
            "CONTRACT_ID",
        )?;
        need(hash(root) == strv(&l["rulesHash"]), "ROOT_HASH")?;
        need(
            fields.iter().all(|k| !strv(&p[*k]).is_empty())
                && p["raceStage"] == "General"
                && p["resolutionType"] == "Party Winner"
                && p["contractType"] == "Election Outcome",
            "PREDICATE",
        )?;
        need(
            p["winnerName"] == "Democratic Party" || p["winnerName"] == "Republican Party",
            "PARTY",
        )?;
        need(
            strv(&p["electionDate"]).len() == 10
                && strv(&p["electionDate"]).bytes().enumerate().all(|(i, c)| {
                    if i == 4 || i == 7 {
                        c == b'-'
                    } else {
                        c.is_ascii_digit()
                    }
                })
                && instant(&root["settlement_date"])? > instant(&r["recordedAt"])?,
            "SETTLEMENT",
        )?;
    }
    need(
        legs[0]["root"]["contract_details"]["winnerName"]
            != legs[1]["root"]["contract_details"]["winnerName"],
        "DIFFERENT_PARTIES",
    )?;
    need(
        fields.iter().all(|k| {
            legs[0]["root"]["contract_details"][*k] == legs[1]["root"]["contract_details"][*k]
        }) && legs[0]["root"]["settlement_date"] == legs[1]["root"]["settlement_date"],
        "SAME_RACE_STAGE",
    )?;
    need(
        r["normalResolutionOnly"] == true && r["administrativeRefundGapAcknowledged"] == true,
        "EXCEPTION_ACK",
    )?;
    let scenarios = arr(&r["scenarios"]);
    let unique_scenarios: std::collections::HashSet<_> =
        scenarios.iter().map(|s| strv(&s["id"])).collect();
    need(unique_scenarios.len() == scenarios.len(), "SCENARIOS")?;
    need(
        scenarios.len() > 1
            && scenarios.iter().all(|s| {
                s["id"].is_string()
                    && (s["category"] == "ordinary" || s["category"] == "exception")
                    && arr(&s["payouts"]).len() == 2
                    && arr(&s["payouts"]).iter().all(|v| *v == 0 || *v == 1)
            })
            && scenarios
                .iter()
                .any(|s| s["category"] == "exception" && s["payouts"] == json!([0, 0])),
        "SCENARIOS",
    )?;
    for payouts in [json!([0, 1]), json!([1, 0]), json!([1, 1])] {
        need(
            scenarios
                .iter()
                .any(|s| s["category"] == "ordinary" && s["payouts"] == payouts),
            "NORMAL_SCENARIOS",
        )?;
    }
    need(
        arr(&r["assumptions"]).len() > 0
            && arr(&r["assumptions"]).iter().all(|v| !strv(v).is_empty()),
        "ASSUMPTIONS",
    )?;
    Ok(())
}
pub fn bank_check(b: &V) -> Result<()> {
    review(&b["review"])?;
    need(
        b["mode"] == "dual_no_sequential_paper_v1" && b["policy"] == policy(),
        "BANK",
    )?;
    instant(&b["cutoff"])?;
    Ok(())
}
pub fn create(b: &V) -> Result<V> {
    bank_check(b)?;
    Ok(
        json!({"bankHash":hash(b),"lastAt":b["cutoff"],"cash":b["policy"]["initialCash"],"realizedPnl":"0.000000","cumulativeBuys":"0.000000","positions":{},"books":{},"fills":[],"counts":{},"pending":null,"package":null,"anchor":null,"nextIntentId":1,"nextPackageId":1,"cooldownUntil":0,"roundtrips":[]}),
    )
}
fn inc(s: &mut V, k: &str) {
    let a = n(&s["counts"][k]);
    s["counts"][k] = json!(a + 1);
}
fn cost(s: &V) -> Result<i64> {
    obj(&s["positions"])
        .iter()
        .try_fold(0, |a, (_, p)| Ok(a + amount(&p["cost"])?))
}
fn reserve(s: &V) -> Result<i64> {
    if s["pending"]["action"] != "buy" {
        return Ok(0);
    }
    if s["pending"]["stage"] == "entry_first" {
        obj(&s["package"]["entryLimits"])
            .iter()
            .try_fold(0, |a, (_, p)| Ok(a + amount(p)?))
    } else {
        amount(&s["pending"]["limit"])
    }
}
pub fn assert_state(s: &V, b: &V) -> Result<()> {
    assert_state_frozen(s, b, &hash(b))
}
pub fn assert_state_frozen(s: &V, b: &V, bank_hash: &str) -> Result<()> {
    need(strv(&s["bankHash"]) == bank_hash, "BANK_CHANGED")?;
    let (mut buys, mut sells) = (0, 0);
    let mut qs = std::collections::HashMap::<String, i64>::new();
    let scope = ids(b);
    for f in arr(&s["fills"]) {
        let id = strv(&f["exchangeId"]);
        need(
            f["quantity"] == 1 && scope.iter().any(|x| x == id),
            "FILL_SCOPE",
        )?;
        let v = amount(&f["price"])?;
        let q = qs.entry(id.to_owned()).or_default();
        if f["action"] == "buy" {
            buys += v;
            *q += 1
        } else {
            need(f["action"] == "sell", "FILL_ACTION")?;
            sells += v;
            *q -= 1;
            need(*q >= 0, "OVERSELL")?;
        }
    }
    let owned = cost(s)?;
    let reserved = reserve(s)?;
    need(
        amount(&s["cash"])? == amount(&b["policy"]["initialCash"])? - buys + sells
            && amount(&s["cash"])? >= reserved,
        "CASH_CONSERVATION",
    )?;
    need(
        amount(&s["realizedPnl"])? == sells - buys + owned && amount(&s["cumulativeBuys"])? == buys,
        "FIFO_CONSERVATION",
    )?;
    need(
        owned + reserved <= amount(&b["policy"]["maxOwnedAndReserved"])?
            && buys + reserved <= amount(&b["policy"]["cumulativeBuyBudget"])?,
        "CAPITAL",
    )?;
    for id in scope {
        need(
            n(&s["positions"][&id]["quantity"]) == *qs.get(&id).unwrap_or(&0),
            "INVENTORY_CONSERVATION",
        )?;
    }
    need(
        obj(&s["positions"]).iter().all(|(_, p)| p["quantity"] == 1)
            && (s["pending"].is_null() || s["pending"]["quantity"] == 1),
        "QUANTITY",
    )?;
    let id = strv(&s["pending"]["exchangeId"]);
    if s["pending"]["action"] == "sell" {
        need(!s["positions"][id].is_null(), "COVERED_SELL")?;
    }
    if s["pending"]["action"] == "buy" {
        need(s["positions"][id].is_null(), "DUPLICATE_BUY")?;
    }
    need(
        !s["package"].is_null() == (owned > 0 || !s["pending"].is_null()),
        "PACKAGE_OWNERSHIP",
    )?;
    Ok(())
}
fn current(b: &V, at: &V) -> Result<bool> {
    let t = instant(at)?;
    Ok(t >= instant(&b["review"]["recordedAt"])? && t < instant(&b["review"]["expiresAt"])?)
}
fn fresh(book: &V, account: &V, at: &V, b: &V) -> bool {
    (|| -> Result<bool> {
        if book.is_null() || !account.is_object() {
            return Ok(false);
        }
        let t = instant(at)?;
        let req = instant(&book["requestStartedAt"])?;
        let keys = account.as_object().unwrap();
        if keys.len() != 4
            || account["schemaVersion"] != 1
            || !account["excludedExchangeIds"].is_array()
            || arr(&account["excludedExchangeIds"])
                .iter()
                .any(|v| !valid_id(v))
        {
            return Ok(false);
        }
        for k in ["positionsAt", "ordersAt"] {
            let a = instant(&account[k])?;
            if a > req || t - a > 20000 {
                return Ok(false);
            }
        }
        Ok(instant(&book["receivedAt"])? <= t
            && t - req <= n(&b["policy"]["maxAgeMs"])
            && !arr(&account["excludedExchangeIds"]).contains(&book["exchangeId"]))
    })()
    .unwrap_or(false)
}
fn pair(s: &V, a: &V, at: &V, b: &V) -> Result<Option<Vec<V>>> {
    let books: Vec<_> = ids(b).iter().map(|id| s["books"][id].clone()).collect();
    if !books.iter().all(|x| fresh(x, a, at, b)) {
        return Ok(None);
    }
    let max = books
        .iter()
        .map(|x| instant(&x["receivedAt"]))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .max()
        .unwrap();
    let min = books
        .iter()
        .map(|x| instant(&x["requestStartedAt"]))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .min()
        .unwrap();
    Ok(if max - min <= n(&b["policy"]["maxSkewMs"]) {
        Some(books)
    } else {
        None
    })
}
#[derive(Clone)]
struct Level {
    unit: i64,
    quantity: i64,
    index: usize,
    raw: V,
}
fn level(book: &V, action: &str, b: &V, ticks: i64) -> Result<Option<Level>> {
    for (index, l) in arr(&book[if action == "buy" { "bids" } else { "asks" }])
        .iter()
        .enumerate()
    {
        let q = n(&l["quantity"]) * n(&b["policy"]["depthFractionBps"]) / 10000;
        let unit = SCALE - amount(&l["price"])?
            + if action == "buy" {
                ticks * TICK
            } else {
                -ticks * TICK
            };
        if q >= 1 && (TICK..=SCALE - TICK).contains(&unit) {
            return Ok(Some(Level {
                unit,
                quantity: q,
                index,
                raw: l["price"].clone(),
            }));
        }
    }
    Ok(None)
}
fn unit(book: &V, action: &str, b: &V) -> Result<i64> {
    Ok(level(book, action, b, 1)?
        .map(|x| x.unit)
        .unwrap_or(if action == "buy" { SCALE } else { 0 }))
}
fn pending(
    s: &mut V,
    b: &V,
    id: &str,
    action: &str,
    stage: &str,
    limit: i64,
    at: &V,
) -> Result<()> {
    need(
        (TICK..=SCALE - TICK).contains(&limit) && limit % TICK == 0,
        "LIMIT",
    )?;
    let leg = arr(&b["review"]["legs"])
        .into_iter()
        .find(|l| l["exchangeId"] == id)
        .ok_or("LEG")?;
    s["pending"] = json!({"id":s["nextIntentId"],"packageId":s["package"]["id"],"marketId":leg["marketId"],"exchangeId":id,"action":action,"stage":stage,"quantity":1,"limit":money(limit),"createdAt":at,"expiresAt":iso(instant(at)?+n(&b["policy"]["intentLifetimeMs"])),"decisionVersion":s["books"][id]["versionKey"]});
    s["nextIntentId"] = json!(n(&s["nextIntentId"]) + 1);
    inc(s, &format!("intent_{stage}"));
    Ok(())
}
fn finish(s: &mut V, b: &V, at: &V, reason: &str) -> Result<()> {
    need(obj(&s["positions"]).is_empty(), "FINISH_OWNED")?;
    let p = s["package"].clone();
    s["roundtrips"].as_array_mut().unwrap().push(json!({"id":p["id"],"openedAt":p["createdAt"],"closedAt":at,"reason":reason,"buyCost":p["buyCost"],"saleProceeds":p["saleProceeds"],"realizedPnl":money(amount(&p["saleProceeds"])?-amount(&p["buyCost"])?),"completedPair":p["completedPair"]}));
    s["package"] = V::Null;
    s["pending"] = V::Null;
    s["anchor"] = V::Null;
    s["cooldownUntil"] = json!(instant(at)? + n(&b["policy"]["cooldownMs"]));
    inc(s, "closed_packages");
    Ok(())
}
fn orphan(s: &mut V, b: &V, at: &V, reason: &str) -> Result<()> {
    s["pending"] = V::Null;
    s["anchor"] = V::Null;
    if obj(&s["positions"]).is_empty() {
        return finish(s, b, at, reason);
    }
    s["package"]["stage"] = json!("orphan");
    s["package"]["orphanReason"] = json!(reason);
    inc(s, "orphan_obligation");
    Ok(())
}
fn expired(s: &mut V, b: &V, at: &V, reason: &str) -> Result<()> {
    let stage = s["pending"]["stage"].clone();
    need(!s["pending"].is_null(), "EXPIRY")?;
    s["pending"] = V::Null;
    inc(s, "intent_expired");
    match strv(&stage) {
        "entry_first" => finish(s, b, at, reason)?,
        "entry_second" | "exit_second" => orphan(s, b, at, reason)?,
        "exit_first" => {
            s["package"]["stage"] = json!("holding");
            s["package"]["exitLimits"] = V::Null;
        }
        "orphan" => {}
        _ => need(false, "EXPIRY_STAGE")?,
    }
    Ok(())
}
fn fill(s: &mut V, book: &V, at: &V, b: &V) -> Result<()> {
    let i = s["pending"].clone();
    if i.is_null()
        || i["exchangeId"] != book["exchangeId"]
        || book["versionKey"] == i["decisionVersion"]
        || instant(&book["requestStartedAt"])?
            < instant(&i["createdAt"])? + n(&b["policy"]["latencyMs"])
        || instant(&book["receivedAt"])? > instant(&i["expiresAt"])?
        || instant(at)? > instant(&i["expiresAt"])?
    {
        return Ok(());
    }
    let action = strv(&i["action"]);
    let Some(l) = level(book, action, b, 1)? else {
        return Ok(());
    };
    if (action == "buy" && l.unit > amount(&i["limit"])?)
        || (action == "sell" && l.unit < amount(&i["limit"])?)
    {
        return Ok(());
    }
    let id = strv(&i["exchangeId"]);
    if action == "buy" {
        need(s["positions"][id].is_null(), "DUPLICATE_POSITION")?;
        s["positions"][id] = json!({"marketId":book["marketId"],"exchangeId":book["exchangeId"],"quantity":1,"cost":money(l.unit),"enteredAt":at});
        s["cash"] = money(amount(&s["cash"])? - l.unit);
        s["cumulativeBuys"] = money(amount(&s["cumulativeBuys"])? + l.unit);
        s["package"]["buyCost"] = money(amount(&s["package"]["buyCost"])? + l.unit);
    } else {
        need(!s["positions"][id].is_null(), "COVERED_EXIT")?;
        s["cash"] = money(amount(&s["cash"])? + l.unit);
        s["realizedPnl"] =
            money(amount(&s["realizedPnl"])? + l.unit - amount(&s["positions"][id]["cost"])?);
        s["package"]["saleProceeds"] = money(amount(&s["package"]["saleProceeds"])? + l.unit);
        s["positions"].as_object_mut().unwrap().remove(id);
    }
    let mut f = i.clone();
    f["price"] = money(l.unit);
    f["at"] = at.clone();
    f["fees"] = json!("0.000000");
    f["model"] = json!("counterfactual_later_independent_book_not_proven_execution");
    f["source"] = json!({"requestStartedAt":book["requestStartedAt"],"receivedAt":book["receivedAt"],"version":book["versionKey"],"publicationSequence":book["publicationSequence"],"publicationHash":book["publicationHash"],"eligibleDepth":l.quantity,"levelIndex":l.index,"rawYesPrice":l.raw});
    s["fills"].as_array_mut().unwrap().push(f);
    inc(s, &format!("hypothetical_{action}"));
    s["pending"] = V::Null;
    s["anchor"] = V::Null;
    match strv(&i["stage"]) {
        "entry_first" => {
            s["package"]["stage"] = json!("entry_second");
            let id = strv(&s["package"]["second"]).to_owned();
            let lim = amount(&s["package"]["entryLimits"][&id])?;
            pending(s, b, &id, "buy", "entry_second", lim, at)?;
        }
        "entry_second" => {
            need(
                cost(s)? <= SCALE - amount(&b["policy"]["minimumPackageMargin"])?,
                "PACKAGE_MARGIN",
            )?;
            s["package"]["stage"] = json!("holding");
            s["package"]["completedPair"] = json!(true);
            s["package"]["completedAt"] = at.clone();
            inc(s, "completed_pairs");
        }
        "exit_first" => {
            s["package"]["stage"] = json!("exit_second");
            let id = strv(&s["package"]["secondExit"]).to_owned();
            let lim = amount(&s["package"]["exitLimits"][&id])?;
            pending(s, b, &id, "sell", "exit_second", lim, at)?;
        }
        "exit_second" | "orphan" => {
            let reason = strv(
                &s["package"][if i["stage"] == "orphan" {
                    "orphanReason"
                } else {
                    "exitReason"
                }],
            )
            .to_owned();
            finish(s, b, at, &reason)?;
        }
        _ => need(false, "FILL_STAGE")?,
    }
    Ok(())
}
pub fn advance(input: &V, e: &V, b: &V) -> Result<V> {
    bank_check(b)?;
    assert_state(input, b)?;
    advance_checked(input, e, b)
}
pub fn advance_checked(input: &V, e: &V, b: &V) -> Result<V> {
    advance_frozen(input, e, b, &hash(b))
}
// The worker owns this immutable bank and computes its binding once. Every
// event still verifies complete cash, FIFO, inventory and reserved capital.
pub fn advance_frozen(input: &V, e: &V, b: &V, bank_hash: &str) -> Result<V> {
    let mut s = input.clone();
    let at = &e["availableAt"];
    let t = instant(at)?;
    need(
        t >= instant(&s["lastAt"])? && t >= instant(&b["cutoff"])?,
        "CLOCK",
    )?;
    s["lastAt"] = at.clone();
    if !s["anchor"].is_null()
        && t - instant(&s["anchor"]["at"])? > n(&b["policy"]["intentLifetimeMs"])
    {
        s["anchor"] = V::Null;
        inc(&mut s, "confirmation_anchor_expired");
    }
    if e["kind"] == "boundary" {
        s["books"] = json!({});
        s["anchor"] = V::Null;
        if !s["pending"].is_null() {
            expired(&mut s, b, at, "source_boundary")?;
        }
        inc(&mut s, "source_boundary");
        assert_state_frozen(&s, b, bank_hash)?;
        return Ok(s);
    }
    let book = &e["book"];
    let a = &e["accountEligibility"];
    let scope = ids(b);
    need(
        e["kind"] == "book"
            && arr(&b["review"]["legs"]).iter().any(|l| {
                l["exchangeId"] == book["exchangeId"] && l["marketId"] == book["marketId"]
            }),
        "BOOK_SCOPE",
    )?;
    for (id, mark) in obj(&s["books"]) {
        if !fresh(&mark, a, at, b) {
            s["books"].as_object_mut().unwrap().remove(&id);
        }
    }
    for mark in arr(&e["pairBooks"]) {
        if arr(&b["review"]["legs"])
            .iter()
            .any(|l| l["exchangeId"] == mark["exchangeId"] && l["marketId"] == mark["marketId"])
            && fresh(&mark, a, at, b)
        {
            s["books"][strv(&mark["exchangeId"])] = mark.clone();
        }
    }
    let id = strv(&book["exchangeId"]);
    if fresh(book, a, at, b) {
        s["books"][id] = book.clone();
    } else {
        s["books"].as_object_mut().unwrap().remove(id);
    }
    let excluded = arr(&a["excludedExchangeIds"])
        .iter()
        .any(|v| scope.contains(&strv(v).to_owned()));
    if !s["pending"].is_null()
        && (t > instant(&s["pending"]["expiresAt"])?
            || (s["pending"]["action"] == "buy" && (!current(b, at)? || excluded)))
    {
        expired(
            &mut s,
            b,
            at,
            if excluded {
                "account_exposure"
            } else {
                "intent_or_review_expiry"
            },
        )?;
    }
    if fresh(book, a, at, b) {
        fill(&mut s, book, at, b)?;
    } else {
        inc(&mut s, "unavailable_or_excluded_book");
    }
    let books = pair(&s, a, at, b)?;
    let p = s["package"].clone();
    if p["stage"] == "orphan" && s["pending"].is_null() {
        let id = obj(&s["positions"])[0].0.clone();
        let mark = s["books"][&id].clone();
        if book["exchangeId"] == id && fresh(&mark, a, at, b) && unit(&mark, "sell", b)? >= TICK {
            let u = unit(&mark, "sell", b)?;
            pending(&mut s, b, &id, "sell", "orphan", u, at)?;
        }
    } else if p["stage"] == "holding" && s["pending"].is_null() && books.is_some() {
        let books = books.as_ref().unwrap();
        let units = books
            .iter()
            .map(|x| unit(x, "sell", b))
            .collect::<Result<Vec<_>>>()?;
        let proceeds: i64 = units.iter().sum();
        let basis = cost(&s)?;
        let holding = t - instant(&p["completedAt"])? >= n(&b["policy"]["maximumPairedHoldMs"]);
        let stop = holding || basis - proceeds >= amount(&b["policy"]["packageStopLoss"])?;
        if units.iter().all(|x| *x >= TICK)
            && (proceeds >= basis + amount(&b["policy"]["takeProfit"])? || stop)
        {
            s["package"]["exitReason"] =
                json!(if proceeds >= basis + amount(&b["policy"]["takeProfit"])? {
                    "paired_profit_target"
                } else if holding {
                    "paired_holding_limit"
                } else {
                    "paired_loss_limit"
                });
            let mut limits = json!({});
            for (i, x) in books.iter().enumerate() {
                limits[strv(&x["exchangeId"])] = money(units[i]);
            }
            s["package"]["exitLimits"] = limits;
            let mut positions = obj(&s["positions"]);
            positions.sort_by(|(_, a), (_, z)| {
                amount(&z["cost"])
                    .unwrap()
                    .cmp(&amount(&a["cost"]).unwrap())
            });
            let first = strv(&positions[0].1["exchangeId"]).to_owned();
            s["package"]["secondExit"] = json!(scope.iter().find(|x| **x != first).unwrap());
            s["package"]["stage"] = json!("exit_first");
            let lim = amount(&s["package"]["exitLimits"][&first])?;
            pending(&mut s, b, &first, "sell", "exit_first", lim, at)?;
            inc(&mut s, "paired_exit_obligation");
        }
    } else if p.is_null()
        && s["pending"].is_null()
        && t >= n(&s["cooldownUntil"])
        && books.is_some()
        && e["admission"]["positiveConditionalCapacity"] == true
        && current(b, at)?
        && books.as_ref().unwrap().iter().all(|x| {
            instant(&x["requestStartedAt"]).unwrap_or(i64::MIN) > instant(&b["cutoff"]).unwrap()
        })
    {
        let books = books.as_ref().unwrap();
        let units = books
            .iter()
            .map(|x| unit(x, "buy", b))
            .collect::<Result<Vec<_>>>()?;
        let sum: i64 = units.iter().sum();
        let versions: Vec<_> = books.iter().map(|x| x["versionKey"].clone()).collect();
        if units.iter().all(|v| (TICK..=SCALE - TICK).contains(v))
            && sum <= SCALE - amount(&b["policy"]["minimumPackageMargin"])?
            && sum <= amount(&b["policy"]["maxOwnedAndReserved"])?
            && amount(&s["cumulativeBuys"])? + sum <= amount(&b["policy"]["cumulativeBuyBudget"])?
            && amount(&s["cash"])? >= sum
        {
            if !s["anchor"].is_null()
                && versions
                    .iter()
                    .enumerate()
                    .all(|(i, v)| *v != s["anchor"]["versions"][i])
                && books.iter().all(|x| {
                    instant(&x["requestStartedAt"]).unwrap() > instant(&s["anchor"]["at"]).unwrap()
                })
            {
                let idx = if units[0] <= units[1] { 0 } else { 1 };
                let first = strv(&books[idx]["exchangeId"]);
                let second = scope.iter().find(|x| *x != first).unwrap();
                let mut limits = json!({});
                for (i, x) in books.iter().enumerate() {
                    limits[strv(&x["exchangeId"])] = money(units[i]);
                }
                s["package"] = json!({"id":s["nextPackageId"],"first":first,"second":second,"stage":"entry_first","createdAt":at,"completedPair":false,"buyCost":"0.000000","saleProceeds":"0.000000","entryLimits":limits,"entryVersions":versions});
                s["nextPackageId"] = json!(n(&s["nextPackageId"]) + 1);
                pending(&mut s, b, first, "buy", "entry_first", units[idx], at)?;
                s["anchor"] = V::Null;
                inc(&mut s, "paired_entry_obligation");
            } else if s["anchor"].is_null() {
                s["anchor"] = json!({"versions":versions,"at":at});
            }
        } else {
            s["anchor"] = V::Null;
            inc(&mut s, "no_package_capacity");
        }
    } else if p.is_null()
        && (!fresh(book, a, at, b) || excluded || !current(b, at)? || books.is_some())
    {
        s["anchor"] = V::Null;
    }
    assert_state_frozen(&s, b, bank_hash)?;
    Ok(s)
}
pub fn report(s: &V, b: &V, at: &V) -> Result<V> {
    assert_state(s, b)?;
    let mut liquidation = 0;
    let mut complete = true;
    let mut positions = vec![];
    for (_, mut p) in obj(&s["positions"]) {
        let book = &s["books"][strv(&p["exchangeId"])];
        let l = if !book.is_null() {
            level(book, "sell", b, 1)?
        } else {
            None
        };
        let fresh_mark = !book.is_null()
            && instant(at)? >= instant(&book["receivedAt"])?
            && instant(at)? - instant(&book["requestStartedAt"])? <= n(&b["policy"]["maxAgeMs"])
            && l.is_some();
        let value = if fresh_mark {
            let u = l.unwrap().unit;
            liquidation += u;
            money(u)
        } else {
            complete = false;
            V::Null
        };
        p["estimatedLiquidationProceeds"] = value;
        p["unsoldShares"] = json!(if fresh_mark { 0 } else { 1 });
        p["quoteReceivedAt"] = book["receivedAt"].clone();
        p["quoteRequestStartedAt"] = book["requestStartedAt"].clone();
        p["quoteAgeMs"] = if !book.is_null() {
            json!(instant(at)? - instant(&book["requestStartedAt"])?)
        } else {
            V::Null
        };
        positions.push(p);
    }
    let paired = positions.len() == 2 && s["package"]["completedPair"] == true;
    let fills = arr(&s["fills"]);
    let trips = arr(&s["roundtrips"]);
    Ok(
        json!({"schemaVersion":1,"mode":b["mode"],"cohort":b["name"],"bankHash":s["bankHash"],"cutoff":b["cutoff"],"updatedAt":at,"reviewExpiresAt":b["review"]["expiresAt"],"cash":s["cash"],"realizedPnl":s["realizedPnl"],"estimatedLiquidationPnl":if complete{money(amount(&s["cash"])?+liquidation-amount(&b["policy"]["initialCash"])?)}else{V::Null},"positions":positions,"pending":s["pending"],"package":s["package"],"reserved":money(reserve(s)?),"cumulativeBuys":s["cumulativeBuys"],"counts":s["counts"],"fills":fills.len(),"recentFills":fills.iter().skip(fills.len().saturating_sub(10)).collect::<Vec<_>>(),"closedPackages":trips.iter().filter(|x|amount(&x["buyCost"]).unwrap()>0).count(),"recentRoundtrips":trips.iter().skip(trips.len().saturating_sub(10)).collect::<Vec<_>>(),"fees":"0.000000","ordinaryConditionalSettlementSurplus":if paired{money(SCALE-cost(s)?)}else{V::Null},"zeroPayoutExceptionLoss":money(-cost(s)?),"reviewCurrent":current(b,at)?,"executionAuthority":false,"competitionOrders":0,"networkRequests":0,"profitabilityEstablished":false,"limitations":["Two sequential hypothetical fills are not atomic execution or evidence of queue priority.","Ordinary settlement surplus is conditional on reviewed rules and is never booked as realized or liquidation P&L.","A failed hedge or second exit creates an orphan obligation that may realize a loss.","Matched inventory may be held through the reviewed election horizon; no settlement payout is booked without explicit settlement evidence. Entry expiry does not settle inventory. All costs and losses remain attributed."]}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn money_exact_and_malformed() {
        for a in [-1234567, -1, 0, 5000, 995000, 100000000] {
            assert_eq!(amount(&money(a)).unwrap(), a)
        }
        for v in [
            json!("01.0"),
            json!("1.0000001"),
            json!(".5"),
            json!("1."),
            json!(0.5),
            json!("NaN"),
        ] {
            assert!(amount(&v).is_err())
        }
    }
    #[test]
    fn clock_roundtrip_and_invalid_dates() {
        for s in [
            "1970-01-01T00:00:00.000Z",
            "2026-10-05T21:27:58.001Z",
            "2000-02-29T23:59:59.999Z",
        ] {
            assert_eq!(iso(instant(&json!(s)).unwrap()), json!(s))
        }
        for s in [
            "2026-02-29T00:00:00.000Z",
            "2026-10-05T24:00:00.000Z",
            "2026-10-05T21:27:58Z",
        ] {
            assert!(instant(&json!(s)).is_err())
        }
    }
    #[test]
    fn javascript_canonical_integer_order() {
        let v = json!({"100":"a","3":"b","z":1e-7,"a":1e21});
        let mut s = String::new();
        canonical(&v, &mut s);
        assert_eq!(s, "{\"3\":\"b\",\"100\":\"a\",\"a\":1e+21,\"z\":1e-7}")
    }
}
