// Deterministic cross-language accounting fixtures. No network or account API.
use crate::{
    engine::Wallet,
    money::{money, price},
    types::*,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{fs, path::Path, sync::Arc};
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    config: Config,
    started_at: String,
    operations: Vec<Value>,
}
fn field<'a>(v: &'a Value, k: &str) -> Result<&'a str, String> {
    v[k].as_str().ok_or_else(|| "INVALID_FIXTURE".into())
}
fn book(v: &Value, c: &Config) -> Result<Book, String> {
    let list = |key: &str| -> Result<Vec<Level>, String> {
        v[key]
            .as_array()
            .ok_or("INVALID_FIXTURE")?
            .iter()
            .map(|l| {
                let q = l["quantity"].as_i64().ok_or("INVALID_FIXTURE")?;
                if q < 1 || q > 9_007_199_254_740_991 {
                    return Err("INVALID_FIXTURE".into());
                }
                Ok(Level {
                    price: price(field(l, "price")?)?,
                    quantity: q,
                })
            })
            .collect()
    };
    let bids = list("bids")?;
    let asks = list("asks")?;
    if bids.is_empty()
        || asks.is_empty()
        || bids.windows(2).any(|x| x[0].price <= x[1].price)
        || asks.windows(2).any(|x| x[0].price >= x[1].price)
        || bids[0].price >= asks[0].price
    {
        return Err("INVALID_FIXTURE_DEPTH".into());
    }
    let market = field(v, "marketId")?;
    let exchange = field(v, "exchangeId")?;
    if !c.market_ids.iter().any(|x| x == market)
        || exchange.is_empty()
        || !exchange.bytes().all(|x| x.is_ascii_digit())
        || field(v, "instrument")? != format!("{exchange}:YES")
    {
        return Err("INVALID_FIXTURE_SCOPE".into());
    }
    let received = instant(field(v, "receivedAt")?)?;
    let requested = instant(field(v, "requestStartedAt")?)?;
    let source = nanos(field(v, "sourceAt")?)?;
    if requested > received
        || received - requested > c.max_data_age_ms
        || source / 1_000_000 > received + 2000
        || received - source / 1_000_000 > c.max_data_age_ms
    {
        return Err("INVALID_FIXTURE_CLOCK".into());
    }
    Ok(Book {
        market_id: market.into(),
        exchange_id: exchange.into(),
        instrument: field(v, "instrument")?.into(),
        received_at: received,
        request_started_at: requested,
        source_ns: source,
        sequence: field(v, "sequence")?
            .parse()
            .map_err(|_| "INVALID_FIXTURE")?,
        bids,
        asks,
    })
}
pub fn run(path: &Path) -> Result<Value, String> {
    let f: Fixture = serde_json::from_slice(&fs::read(path).map_err(|_| "FIXTURE_OPEN_FAILED")?)
        .map_err(|_| "FIXTURE_JSON_INVALID")?;
    f.config.validate()?;
    let mut wallet = Wallet::new(&f.config, instant(&f.started_at)?)?;
    let mut points = Vec::new();
    for op in &f.operations {
        match field(op, "kind")? {
            "book" => {
                let b = Arc::new(book(&op["book"], &f.config)?);
                let horizon = op["horizonMs"].as_i64().ok_or("INVALID_FIXTURE")?;
                let margin = op["minimumEdge"].as_f64().ok_or("INVALID_FIXTURE")?;
                if let Some(profit) = op["takeProfit"].as_str() {
                    if let Some(policy) = op["exitPolicy"].as_str() {
                        let policy = match policy {
                            "snapshot_limit_v1" => crate::engine::DipExitPolicy::SnapshotLimit,
                            "profit_floor_stop_buffer_v1" => {
                                crate::engine::DipExitPolicy::ProfitFloorStopBuffer(
                                    op["stopBufferTicks"]
                                        .as_u64()
                                        .filter(|n| *n <= 5)
                                        .ok_or("INVALID_DIP_EXIT_POLICY")?
                                        as u8,
                                )
                            }
                            _ => return Err("INVALID_DIP_EXIT_POLICY".into()),
                        };
                        wallet.step_dip_policy(
                            b,
                            &f.config,
                            op["netEdge"].as_f64(),
                            horizon,
                            margin,
                            crate::money::amount(profit)?,
                            policy,
                        )?;
                    } else {
                        wallet.step_dip(
                            b,
                            &f.config,
                            op["netEdge"].as_f64(),
                            horizon,
                            margin,
                            crate::money::amount(profit)?,
                        )?;
                    }
                } else {
                    wallet.step(b, &f.config, op["netEdge"].as_f64(), horizon, margin)?;
                }
            }
            "invalidate" => wallet.invalidate(op["marketId"].as_str(), instant(field(op, "at")?)?),
            "point" => {
                wallet.verify()?;
                let p = wallet.point(&f.config, instant(field(op, "at")?)?)?;
                let mut v = serde_json::to_value(p).map_err(|_| "OUTPUT_INVALID")?;
                v.as_object_mut().unwrap().remove("signalMetrics");
                v.as_object_mut().unwrap().remove("unsupportedInstruments");
                points.push(v);
            }
            _ => return Err("INVALID_FIXTURE_OPERATION".into()),
        }
    }
    wallet.verify()?;
    Ok(
        json!({"mode":"accounting_fixture_not_profit_evidence","points":points,"fills":wallet.fills,"competitionOrders":0,"networkRequests":0,"moneyScale":money(1_000_000)}),
    )
}
