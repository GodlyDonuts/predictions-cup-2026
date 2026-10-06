use baal_portfolio_engine::{PortfolioEngine, js_stringify};
use serde_json::{Value as V, json};
use std::io::{self, BufRead, Write};
fn main() {
    let mut engine: Option<PortfolioEngine> = None;
    let mut dual_bank=V::Null;let mut dual_state=V::Null;let mut dual_v1=false;
    let mut output = io::BufWriter::new(io::stdout().lock());
    for line in io::stdin().lock().lines() {
        let result = (|| -> Result<V, String> {
            let q: V = serde_json::from_str(&line.map_err(|_| "INPUT")?).map_err(|_| "INPUT")?;
            match q["op"].as_str().unwrap_or("") {
                "init" => {
                    let next = PortfolioEngine::new(q["bank"].clone())?;
                    let state = next.state().clone();
                    engine = Some(next);
                    Ok(json!({"state":state}))
                }
                "restore" => {
                    let next = PortfolioEngine::restore(q["bank"].clone(), q["state"].clone())?;
                    let state = next.state().clone();
                    engine = Some(next);
                    Ok(json!({"state":state}))
                }
                "advance" => Ok(
                    json!({"state":engine.as_mut().ok_or("NOT_INITIALIZED")?.apply_with_entry_permission(&q["row"],q["availableAt"].as_str().ok_or("TIME")?,if q["accountEligibility"].is_null(){None}else{Some(&q["accountEligibility"])},if q["allowNewEntries"].is_null(){true}else{q["allowNewEntries"].as_bool().ok_or("ENTRY_PERMISSION")?})?}),
                ),
                "batch"=>{let current=engine.as_ref().ok_or("NOT_INITIALIZED")?;let mut next=PortfolioEngine::restore(current.bank().clone(),current.state().clone())?;for row in q["rows"].as_array().ok_or("ROWS")?{next.apply_with_entry_permission(row,q["availableAt"].as_str().ok_or("TIME")?,if q["accountEligibility"].is_null(){None}else{Some(&q["accountEligibility"])},if q["allowNewEntries"].is_null(){true}else{q["allowNewEntries"].as_bool().ok_or("ENTRY_PERMISSION")?})?;}let state=next.state().clone();engine=Some(next);Ok(json!({"state":state}))},
                "import"=>{let(next,evidence)=baal_portfolio_engine::import_history(q["bank"].clone(),std::path::Path::new(q["journalPath"].as_str().ok_or("PATH")?),q["journalBytes"].as_u64().ok_or("BYTES")?,q["journalSha"].as_str().ok_or("SHA")?,std::path::Path::new(q["publicPath"].as_str().ok_or("PATH")?),q["publicBytes"].as_u64().ok_or("BYTES")?,q["publicSha"].as_str().ok_or("SHA")?)?;let state=next.state().clone();engine=Some(next);Ok(json!({"state":state,"evidence":evidence}))},
                "import_historical_overlay_base"=>{let(next,evidence)=baal_portfolio_engine::import_historical_base_for_overlay(q["bank"].clone(),std::path::Path::new(q["journalPath"].as_str().ok_or("PATH")?),q["journalBytes"].as_u64().ok_or("BYTES")?,q["journalSha"].as_str().ok_or("SHA")?,std::path::Path::new(q["publicPath"].as_str().ok_or("PATH")?),q["publicBytes"].as_u64().ok_or("BYTES")?,q["publicSha"].as_str().ok_or("SHA")?,&q["opening"])?;let state=next.state().clone();engine=Some(next);Ok(json!({"historicalState":state,"evidence":evidence}))},
                "capacity" => Ok(
                    json!({"capacity":baal_portfolio_engine::dual_capacity(&q["review"],&q["policy"],q["books"].as_array().ok_or("BOOKS")?,if q["accountEligibility"].is_null(){None}else{Some(&q["accountEligibility"])},q["at"].as_str().ok_or("TIME")?)?}),
                ),
                "report" => Ok(
                    json!({"report":engine.as_ref().ok_or("NOT_INITIALIZED")?.report(q["at"].as_str().ok_or("TIME")?)?}),
                ),
                "dual_init"=>{let bank=q["bank"].clone();let v1=bank["mode"]=="dual_no_sequential_paper_v1";let next=if v1{baal_portfolio_engine::dual_v1::create(&bank)?}else{baal_portfolio_engine::dual_no::create(&bank)?};dual_bank=bank;dual_state=next;dual_v1=v1;Ok(json!({"state":dual_state}))},
                "dual_advance"=>{let next=if dual_v1{baal_portfolio_engine::dual_v1::advance(&dual_state,&q["event"],&dual_bank)?}else{baal_portfolio_engine::dual_no::advance(&dual_state,&q["event"],&dual_bank)?};dual_state=next;Ok(json!({"state":dual_state}))},
                "dual_report"=>Ok(json!({"report":if dual_v1{baal_portfolio_engine::dual_v1::report(&dual_state,&dual_bank,&q["at"])?}else{baal_portfolio_engine::dual_no::report(&dual_state,&dual_bank,&q["at"])?}})),
                _ => Err("OPERATION".into()),
            }
        })();
        let r = result.unwrap_or_else(|e| json!({"error":e}));
        writeln!(output, "{}", js_stringify(&r)).unwrap();
        output.flush().unwrap();
    }
}
