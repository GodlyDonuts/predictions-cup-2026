//! Retain parsed historical response evidence before strict validation.
//! This is a typed GET using the original transport and account admission.
use super::*;
use std::{fs::OpenOptions, io::Write, path::Path};

pub(crate) fn valid_window(v:&Value,start:u64,end:u64,requested:u64,now:u64)->bool {
    let terminal_empty=start==end&&v["coverage"]["complete"]==true&&v["candles"].as_array().is_some_and(|r|r.is_empty());
    (start<end||terminal_empty)&&end<=requested&&end<=now&&end%60000==0
}

/// A weighted average is an observation, not exact cash or an order price.
/// Compare its original decimal lexeme to [0,1] without f64 or rounding.
pub(crate) fn validate_price(value:&Value)->Result<()> {validate_observation(value,true)}
pub(crate) fn validate_volume(value:&Value)->Result<()> {validate_observation(value,false)}
fn validate_observation(value:&Value,unit_interval:bool)->Result<()> {
    need(value.is_number(),"HISTORY_OBSERVATION_NUMBER")?;
    let text=value.to_string();need(text.len()<=128,"HISTORY_OBSERVATION_BOUND")?;
    let negative=text.starts_with('-');let unsigned=text.strip_prefix('-').unwrap_or(&text);
    let mut exponent_parts=unsigned.split(['e','E']);let mantissa=exponent_parts.next().unwrap();
    let exponent=exponent_parts.next().map_or(Ok(0i64),|v|v.parse::<i64>().map_err(|_|fail("HISTORY_OBSERVATION_BOUND")))?;
    need(exponent_parts.next().is_none(),"HISTORY_OBSERVATION_BOUND")?;
    let mut parts=mantissa.split('.');let whole=parts.next().unwrap();let fraction=parts.next().unwrap_or("");
    need(parts.next().is_none()&&!whole.is_empty()&&whole.bytes().chain(fraction.bytes()).all(|b|b.is_ascii_digit()),"HISTORY_OBSERVATION_BOUND")?;
    let joined=format!("{whole}{fraction}");let significant=joined.trim_start_matches('0');
    if significant.is_empty(){return Ok(())}
    need(!negative,"HISTORY_OBSERVATION_BOUND")?;
    let order=exponent.checked_sub(fraction.len() as i64).and_then(|x|x.checked_add(significant.len() as i64)).ok_or_else(||fail("HISTORY_OBSERVATION_BOUND"))?;
    need(!unit_interval||order<=0||(order==1&&significant.starts_with('1')&&significant[1..].bytes().all(|b|b==b'0')),"HISTORY_OBSERVATION_BOUND")
}

impl Transport {
    /// One documented public trade-tape page, retaining original observational
    /// decimals; no conversion into order ticks, financial fills or money.
    pub fn request_historical_trade_page(&mut self,exchange_id:String,from:String,to:String,cursor:Option<&str>,output:&Path)->Result<Receipt>{
        need(output.is_absolute()&&!output.exists(),"NEW_ABSOLUTE_HISTORY_EVIDENCE_REQUIRED")?;
        let endpoint=Endpoint::Trades{exchange_id:exchange_id.clone(),from:from.clone(),to:to.clone()};
        let path=self.path(&endpoint,cursor)?;let observed_endpoint=path.clone();
        self.perform(RequestSpec{method:"GET".into(),endpoint:path,body:None,body_json:None},None,|s,v|{
            retain(output,&observed_endpoint,s.clock.now_ms(),v)?;
            validate_trade_page(&s.config.scope,&exchange_id,&from,&to,v)?;
            s.coverage(&v["coverage"],true)
        })
    }
    pub fn request_history_with_evidence(&mut self, exchange_id:String, to:String, output:&Path)->Result<Receipt>{
        need(output.is_absolute()&&!output.exists(),"NEW_ABSOLUTE_HISTORY_EVIDENCE_REQUIRED")?;
        let endpoint=Endpoint::History{exchange_id,to};
        let path=self.path(&endpoint,None)?;
        let observed_endpoint=path.clone();
        self.perform(RequestSpec{method:"GET".into(),endpoint:path,body:None,body_json:None},None,|s,v|{
            retain(output,&observed_endpoint,s.clock.now_ms(),v)?;
            s.validate(&endpoint,v)
        })
    }
}

fn validate_trade_page(scope:&Scope,exchange:&str,from:&str,to:&str,v:&Value)->Result<()> {
    let start=parse_clock(&json!(from)).ok_or_else(||fail("HISTORY_TRADE_CLOCK"))?;
    let end=parse_clock(&json!(to)).ok_or_else(||fail("HISTORY_TRADE_CLOCK"))?;
    need(start<end&&v["exchangeId"]==exchange&&scope.exchanges.get(exchange).is_some_and(|m|v["marketId"]==*m)
        &&parse_clock(&v["from"])==Some(start)&&parse_clock(&v["to"])==Some(end),"HISTORY_TRADE_SCOPE")?;
    pagination(v)?;let rows=v["data"].as_array().ok_or_else(||fail("HISTORY_TRADE_ARRAY"))?;
    need(rows.len()<=200,"HISTORY_TRADE_PAGE_BOUND")?;
    let p=&v["pagination"];
    need(if p["hasMore"]==true{!rows.is_empty()&&p["nextCursor"].as_str().is_some_and(|s|!s.is_empty()&&s.len()<=4096)}else{p["nextCursor"].is_null()},"HISTORY_TRADE_CURSOR")?;
    let mut seen=BTreeSet::new();
    for row in rows {
        need(row["id"].as_str().is_some_and(numeric)&&parse_clock(&row["createdAt"]).is_some_and(|t|start<=t&&t<end)
            &&row["size"].as_u64().is_some_and(|n|n>0)&&row["side"].as_str().is_some_and(|s|s=="YES"||s=="NO"),"HISTORY_TRADE_ROW")?;
        need(row.get("price").is_some(),"HISTORY_TRADE_PRICE_FIELD")?;
        if !row["price"].is_null(){validate_price(&row["price"])?;}
        validate_volume(&row["volume"])?;
        // Legacy and engine IDs occupy separate spaces. Preserve full facts;
        // do not conflate different rows solely because their ID matches.
        need(seen.insert(serde_json::to_string(row).map_err(|_|fail("HISTORY_TRADE_ROW"))?),"HISTORY_TRADE_DUPLICATE_ROW")?;
    }Ok(())
}

fn retain(path:&Path,endpoint:&str,received_ms:u64,data:&Value)->Result<()> {
    let bytes=serde_json::to_vec(&json!({"classification":"parsed_history_response_not_validated_not_raw_wire_bytes",
        "endpoint":endpoint,"receivedAt":utc(received_ms)?,"data":data,"trainingAdmitted":false})).map_err(|_|fail("HISTORY_EVIDENCE_SERIALIZE"))?;
    let mut options=OpenOptions::new();options.write(true).create_new(true);
    #[cfg(unix)] {use std::os::unix::fs::OpenOptionsExt;options.mode(0o600);}
    let mut file=options.open(path).map_err(|_|fail("HISTORY_EVIDENCE_CREATE"))?;
    file.write_all(&bytes).and_then(|_|file.sync_all()).map_err(|_|fail("HISTORY_EVIDENCE_PERSIST"))
}

#[cfg(test)] mod tests {
    use super::*;
    #[test]fn historical_trade_pages_preserve_observations_and_scope(){
        let scope=Scope{tournament_id:"cup".into(),tournament_slug:"cup".into(),exchanges:BTreeMap::from([("2".into(),"1".into())])};
        let from="2026-10-01T00:00:00Z";let to="2026-10-02T00:00:00Z";
        let mut v=json!({"exchangeId":"2","marketId":"1","from":from,"to":to,"data":[{"id":"4","createdAt":from,"price":0.0600000000000001,"volume":0.6000000000000001,"size":10,"side":"NO"}],"pagination":{"limit":200,"hasMore":false,"nextCursor":null}});
        validate_trade_page(&scope,"2",from,to,&v).unwrap();
        v["data"][0]["createdAt"]=json!(to);assert!(validate_trade_page(&scope,"2",from,to,&v).is_err());
        v["data"][0]["createdAt"]=json!(from);v["marketId"]=json!("3");assert!(validate_trade_page(&scope,"2",from,to,&v).is_err());
        v["marketId"]=json!("1");v["pagination"]["hasMore"]=json!(true);assert!(validate_trade_page(&scope,"2",from,to,&v).is_err());
    }
    #[test] fn only_complete_empty_pages_allow_equal_bounds(){
        let empty=json!({"candles":[],"coverage":{"complete":true}});
        assert!(valid_window(&empty,60000,60000,60000,60000));
        assert!(!valid_window(&empty,60000,60000,0,60000));
        assert!(!valid_window(&empty,60000,60000,60000,0));
        assert!(!valid_window(&json!({"candles":[{}],"coverage":{"complete":true}}),60000,60000,60000,60000));
        assert!(!valid_window(&json!({"candles":[],"coverage":{"complete":false}}),60000,60000,60000,60000));
    }
    #[test] fn weighted_averages_keep_precision_without_relaxing_cash(){
        for raw in ["0.0600000000000001","0.0649999999999999","0.8500000000000001","0.8630991379310345","0","1.0000000","1e-20","-0.0"]{
            validate_price(&serde_json::from_str(raw).unwrap()).unwrap();
        }
        for raw in ["1.00000000000000001","-0.0000000000001","1e100","null","\"0.5\""]{
            assert!(validate_price(&serde_json::from_str(raw).unwrap()).is_err(),"{raw}");
        }
        assert!(decimal_units("0.8630991379310345").is_err());
        validate_volume(&serde_json::from_str("1000.0000000000001").unwrap()).unwrap();
        assert!(validate_volume(&serde_json::from_str("-0.0000000001").unwrap()).is_err());
    }
    #[test] fn evidence_preserves_arbitrary_decimal_and_refuses_overwrite(){
        let root=std::env::temp_dir().join(format!("sig-history-evidence-{}",std::process::id()));
        std::fs::create_dir(&root).unwrap();let path=root.join("receipt.json");
        let data:Value=serde_json::from_str("{\"vwap\":0.1234567890123456789}").unwrap();
        retain(&path,"/exchanges/840/price-history",1,&data).unwrap();
        let saved=std::fs::read(&path).unwrap();let v:Value=serde_json::from_slice(&saved).unwrap();
        assert_eq!(v["data"]["vwap"].to_string(),"0.1234567890123456789");
        assert_eq!(v["trainingAdmitted"],false);
        assert!(retain(&path,"same",2,&data).is_err());assert_eq!(saved,std::fs::read(&path).unwrap());
        assert_eq!(root.canonicalize().unwrap().parent().unwrap(),std::env::temp_dir().canonicalize().unwrap());
        std::fs::remove_file(path).unwrap();std::fs::remove_dir(root).unwrap();
    }
}
