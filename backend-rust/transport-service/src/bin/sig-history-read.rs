//! One historical candle GET through the original account queue. Preflight is
//! the default. This binary cannot remove stops, place orders or mint tokens.
use baal_transport_service::{self as api, Authority, Config, Endpoint, Permit, RequestSpec, Scope};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{fs, path::{Path, PathBuf}};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn hash(raw: &[u8]) -> String { format!("{:x}", Sha256::digest(raw)) }
fn require(ok: bool, code: &str) -> Result<()> { if ok { Ok(()) } else { Err(code.into()) } }

#[derive(Deserialize)]
#[serde(rename_all="camelCase", deny_unknown_fields)]
struct Plan {
    control_file: PathBuf,
    control_sha256: String,
    binary_sha256: String,
    job_stop: PathBuf,
    exchange_id: String,
    through: String,
    #[serde(default)]
    response_evidence: Option<PathBuf>,
    #[serde(default)]
    backfill: Option<Backfill>,
    #[serde(default)]
    forward: Option<Forward>,
}
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Forward {output:PathBuf,until:String,maximum_requests:usize,node:PathBuf,stream_entry:PathBuf,stream_sha256:String}
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Backfill { output:PathBuf, from:String, maximum_requests:usize, #[serde(default)] kind:HistoryKind,
    #[serde(default)] resume_completed:Option<ResumeCompleted> }
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct ResumeCompleted { directory:PathBuf, index_sha256:String }
#[derive(Deserialize,Default,PartialEq)]
#[serde(rename_all="snake_case")]
enum HistoryKind { #[default] Candles, Trades }
#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
struct Binding { root: PathBuf, credentials: PathBuf, account_scope: String, scope: Scope }

struct Guard {
    control: PathBuf, control_hash: String, scope_hash: String,
    network_stop: PathBuf, global_stop: PathBuf, execution_stop: PathBuf, job_stop: PathBuf,
}
impl Authority for Guard {
    fn can_read(&self, scope: &Scope) -> bool {
        // Exists errors must fail closed rather than treating inaccessible
        // stop files as absent. Execution STOP must stay a regular file.
        let absent=|p:&Path|matches!(fs::symlink_metadata(p),Err(e)if e.kind()==std::io::ErrorKind::NotFound);
        absent(&self.network_stop) && absent(&self.global_stop) && absent(&self.job_stop)
            && fs::symlink_metadata(&self.execution_stop).is_ok_and(|s|s.is_file()&&!s.file_type().is_symlink())
            && fs::read(&self.control).is_ok_and(|raw|hash(&raw)==self.control_hash)
            && serde_json::to_vec(scope).is_ok_and(|raw|hash(&raw)==self.scope_hash)
    }
    fn authorize(&self,_:&Scope,_:&RequestSpec,_:&Permit,_:bool,_:u64)->api::Result<()> {
        Err(api::TransportError { code:"HISTORY_READ_ONLY".into(),status:None,may_have_executed:false,
            retry_after_ms:0,order_id:None,api_code:None })
    }
    fn owns_order(&self,_:&str)->bool { false }
}

fn prepare(plan: &Plan) -> Result<(Config, Guard, PathBuf, Value)> {
    require(plan.control_file.is_absolute()&&plan.job_stop.is_absolute(),"ABSOLUTE_READ_PLAN_PATHS")?;
    let metadata=fs::symlink_metadata(&plan.control_file)?;
    require(metadata.is_file()&&!metadata.file_type().is_symlink(),"ORIGINAL_REGULAR_CONTROL_REQUIRED")?;
    let raw=fs::read(&plan.control_file)?;
    require(hash(&raw)==plan.control_sha256,"ORIGINAL_CONTROL_HASH_CHANGED")?;
    let binding:Binding=serde_json::from_slice(&raw)?;
    require(binding.root.is_absolute()&&binding.root.canonicalize()?==binding.root,"ORIGINAL_CANONICAL_STATE_ROOT")?;
    // A copied/fabricated control must not redirect the global stop checks or
    // the account lock to a fresh, empty state directory.
    #[cfg(not(test))]
    require(binding.root==Path::new("/home/archon/apps/moloch/shared/live-production-v1"),"ORIGINAL_PRODUCTION_ROOT_REQUIRED")?;
    require(binding.credentials.is_absolute()&&!binding.account_scope.is_empty(),"ORIGINAL_CREDENTIAL_AND_ACCOUNT_BINDING")?;
    require(binding.scope.exchanges.contains_key(&plan.exchange_id),"ORIGINAL_EXCHANGE_SCOPE_REQUIRED")?;
    let end=chrono::DateTime::parse_from_rfc3339(&plan.through)?.timestamp_millis();
    require(end>=0&&end%60000==0&&(end as u64)<=api::Clock::now_ms(&api::SystemClock),"PAST_MINUTE_ALIGNED_HISTORY_END")?;
    if let Some(forward)=&plan.forward {
        require(plan.backfill.is_none()&&plan.response_evidence.is_none(),"ONE_DATA_ONLY_CAPTURE_MODE")?;
        forward_bound(forward,api::Clock::now_ms(&api::SystemClock))?;
        require(forward.node.is_absolute()&&forward.stream_entry.is_absolute()
            &&hash(&fs::read(&forward.stream_entry)?)==forward.stream_sha256,"PINNED_STREAM_ENTRY")?;
    }
    if let Some(job)=&plan.backfill {
        let start=chrono::DateTime::parse_from_rfc3339(&job.from)?.timestamp_millis();
        require(start>=0&&start%60000==0&&start<end,"HISTORY_BACKFILL_RANGE")?;
        require(job.output.is_absolute()&&job.maximum_requests>0&&job.maximum_requests<=50000,"HISTORY_BACKFILL_BOUND")?;
        require(plan.response_evidence.is_none(),"BACKFILL_HAS_PER_PAGE_EVIDENCE")?;
        require(job.resume_completed.is_none()||job.kind==HistoryKind::Trades,"TRADE_RESUME_ONLY")?;
    }
    let shared=binding.root.parent().ok_or("ORIGINAL_SHARED_ROOT")?;
    let guard=Guard { control:plan.control_file.clone(),control_hash:plan.control_sha256.clone(),
        scope_hash:hash(&serde_json::to_vec(&binding.scope)?),network_stop:shared.join("NETWORK_DISABLED"),
        global_stop:shared.join("EXECUTION_STOP"),execution_stop:binding.root.join("EXECUTION_STOP"),job_stop:plan.job_stop.clone() };
    let report=json!({"classification":"data_only_preflight_not_recovery_or_training",
        "readBoundaryCurrentlyOpen":guard.can_read(&binding.scope),"writesEnabled":false,"maximumHttpRequests":plan.forward.as_ref().map_or_else(||plan.backfill.as_ref().map_or(1,|j|j.maximum_requests),|j|j.maximum_requests),"backfillAllOriginalExchanges":plan.backfill.is_some(),
        "readAdmissionsPerMinute":100,"readCompletionPacingMs":600,"endpointKind":if plan.forward.is_some(){"public_forward_capture_and_stream_auth"}else{"history"},
        "exchangeId":plan.exchange_id,"throughExclusive":plan.through,"maximumCandles":500,
        "controlSha256":plan.control_sha256,"scopeSha256":guard.scope_hash,"scopeExchanges":binding.scope.exchanges.len(),
        "networkDisabledPresent":guard.network_stop.try_exists().ok(),
        "globalExecutionStopPresent":guard.global_stop.try_exists().ok(),
        "executionStopPresent":guard.execution_stop.try_exists().ok(),
        "accountQueueOpened":false,"credentialsLoaded":false,"networkRequests":0,"productionFinancialStateChanged":false});
    let config=Config { queue_dir:binding.root.join("account-queue"),account_scope:binding.account_scope,
        scope:binding.scope,maximum_pages:1,writes_enabled:false };
    Ok((config,guard,binding.credentials,report))
}

#[cfg(unix)]
fn execute(plan:&Plan,config:Config,guard:Guard,credential:PathBuf)->Result<Value> {
    require(guard.can_read(&config.scope),"HISTORY_READ_BOUNDARY_CLOSED")?;
    require(hash(&fs::read(std::env::current_exe()?)?)==plan.binary_sha256,"CHECKED_HISTORY_BINARY_REQUIRED")?;
    let credentials=api::credentials::UnixFileCredentials::approved_file(credential)?;
    let exchanges:Vec<String>=config.scope.exchanges.keys().cloned().collect();
    let scope=config.scope.clone();
    let mut sdk=api::Transport::new_with_read_budget(config,Box::new(credentials),Box::new(api::SystemClock),Box::new(guard),100,600)?;
    if let Some(forward)=&plan.forward{return forward_capture(forward,&scope,&mut sdk)}
    if let Some(job)=&plan.backfill {
        return if job.kind==HistoryKind::Trades{trade_backfill(plan,job,exchanges,&mut sdk)}else{backfill(plan,job,exchanges,&mut sdk)}
    }
    // A fixed History endpoint has no pagination; maximum_pages is also1.
    let receipt=if let Some(path)=&plan.response_evidence {
        sdk.request_history_with_evidence(plan.exchange_id.clone(),plan.through.clone(),path)?
    } else { sdk.request(Endpoint::History { exchange_id:plan.exchange_id.clone(),to:plan.through.clone() })? };
    Ok(json!({"receipt":receipt,"historicalBooksReconstructed":false,"trainingAdmitted":false,"productionFinancialStateChanged":false}))
}

fn forward_bound(forward:&Forward,now:u64)->Result<u64>{
    let end=chrono::DateTime::parse_from_rfc3339(&forward.until)?.timestamp_millis();
    require(forward.output.is_absolute()&&end>0&&(end as u64)>now
        &&(end as u64)-now<=15*60*1000&&forward.maximum_requests>0&&forward.maximum_requests<=1500,
        "BOUNDED_FUTURE_PUBLIC_CAPTURE")?;Ok(end as u64)
}

#[cfg(unix)]
struct StreamChild(std::process::Child);
#[cfg(unix)]
impl Drop for StreamChild {
    fn drop(&mut self){
        use std::io::Write;
        if let Some(mut stdin)=self.0.stdin.take(){let _=stdin.write_all(b"{\"stop\":true}\n");let _=stdin.flush();}
        for _ in 0..50 {if matches!(self.0.try_wait(),Ok(Some(_))){return}std::thread::sleep(std::time::Duration::from_millis(100));}
        let _=self.0.kill();let _=self.0.wait();
    }
}

#[cfg(unix)]
fn forward_capture(job:&Forward,scope:&Scope,sdk:&mut api::Transport)->Result<Value>{
    use std::{io::Write,process::{Command,Stdio},collections::BTreeSet};
    let end=forward_bound(job,api::Clock::now_ms(&api::SystemClock))?;
    fs::create_dir(&job.output)?;disk_headroom(&job.output)?;
    require(hash(&fs::read(&job.stream_entry)?)==job.stream_sha256,"PINNED_STREAM_ENTRY")?;
    let auth=sdk.realtime_token()?;
    let mut child=StreamChild(Command::new(&job.node).arg(&job.stream_entry).stdin(Stdio::piped())
        .stdout(Stdio::null()).stderr(Stdio::from(fs::OpenOptions::new().create_new(true).write(true).open(job.output.join("stream-error.log"))?)).spawn()?);
    let pairs:Vec<Value>=scope.exchanges.iter().map(|(ex,m)|json!({"exchangeId":ex,"marketId":m})).collect();
    // Session auth goes only through the child pipe, never a plan, receipt or log.
    let init=json!({"auth":auth,"pairs":pairs,"tournamentId":scope.tournament_id,"aggregate":true,"outputDirectory":job.output.join("stream")});
    serde_json::to_writer(child.0.stdin.as_mut().ok_or("STREAM_STDIN")?,&init)?;
    child.0.stdin.as_mut().unwrap().write_all(b"\n")?;child.0.stdin.as_mut().unwrap().flush()?;drop(init);
    let mut receipts=fs::OpenOptions::new().create_new(true).write(true).open(job.output.join("receipts.jsonl"))?;
    let markets:Vec<String>=scope.exchanges.values().cloned().collect::<BTreeSet<_>>().into_iter().collect();
    let mut requests=1usize;let mut books=0usize;let mut metadata=0usize;let mut cycle=0usize;
    'capture:loop {
        for market in &markets {
            if api::Clock::now_ms(&api::SystemClock)>=end||requests>=job.maximum_requests{break 'capture}
            require(child.0.try_wait()?.is_none(),"FORWARD_STREAM_ENDED")?;disk_headroom(&job.output)?;
            let endpoint=if cycle==0{Endpoint::Market{market_id:market.clone()}}else{Endpoint::OrderBook{market_id:market.clone(),depth:200}};
            let receipt=sdk.request(endpoint)?;requests+=1;
            let kind=if cycle==0{"market"}else{"book"};
            writeln!(receipts,"{}",serde_json::to_string(&json!({"kind":kind,"source":"official_sig_api_v1","marketId":market,
                "tournamentId":scope.tournament_id,"endpoint":receipt.endpoint,"requestStartedAt":receipt.request_started_at,
                "receivedAt":receipt.received_at,"status":receipt.status,"data":receipt.data}))?)?;
            receipts.sync_all()?;if cycle==0{metadata+=1}else{books+=1}
        }
        cycle+=1;
    }
    drop(child);
    let result=json!({"classification":"prospective_public_SIG_capture_not_financial_execution","requests":requests,"marketReceipts":metadata,
        "bookReceipts":books,"until":job.until,"trainingAdmitted":false,"financialExecution":false,"streamGapsMustBeAudited":true});
    let mut out=fs::OpenOptions::new().create_new(true).write(true).open(job.output.join("complete.json"))?;
    out.write_all(&serde_json::to_vec(&result)?)?;out.sync_all()?;Ok(result)
}

fn page_cursor(receipt:&Value,requested:&str,floor:i64)->Result<Option<String>> {
    let requested=chrono::DateTime::parse_from_rfc3339(requested)?.timestamp_millis();
    let rows=receipt["candles"].as_array().ok_or("HISTORY_CANDLES")?;
    let mut earliest=None;
    for row in rows {
        let t=chrono::DateTime::parse_from_rfc3339(row["time"].as_str().ok_or("CANDLE_CLOCK")?)?.timestamp_millis();
        require(t>=0&&t%60000==0&&t<requested,"BACKFILL_CURSOR_CLOCK")?;
        earliest=Some(earliest.map_or(t,|old:i64|old.min(t)));
    }
    Ok(match earliest {
        Some(t)if t>floor=>Some(chrono::DateTime::from_timestamp_millis(t).ok_or("CANDLE_CLOCK")?.to_rfc3339_opts(chrono::SecondsFormat::Millis,true)),
        _=>None,
    })
}

#[cfg(unix)]
fn disk_headroom(path:&Path)->Result<()> {
    let output=std::process::Command::new("df").args(["--output=avail","-B1","--"]).arg(path).output()?;
    require(output.status.success(),"HISTORY_DISK_QUERY_FAILED")?;
    let text=std::str::from_utf8(&output.stdout)?;
    let free:u64=text.lines().last().ok_or("HISTORY_DISK_VALUE")?.trim().parse()?;
    require(free>4*1024*1024*1024,"HISTORY_MINIMUM_FOUR_GIB_HEADROOM")
}

// Copy only fully completed scopes from a hash-pinned previous index. The
// incomplete scope is reread from its beginning; its original evidence remains.
fn completed_prefix(plan:&Plan,job:&Backfill,exchanges:&[String])->Result<Vec<Value>> {
    let Some(source)=&job.resume_completed else{return Ok(Vec::new())};
    require(source.directory.is_absolute()&&source.directory.canonicalize()?==source.directory,"CANONICAL_RESUME_DIRECTORY")?;
    let raw=fs::read(source.directory.join("index.jsonl"))?;
    require(hash(&raw)==source.index_sha256&&raw.ends_with(b"\n"),"PINNED_COMPLETE_RESUME_INDEX")?;
    let mut result=Vec::new();let mut pending=Vec::new();let mut scope=0usize;
    let mut cursor:Option<String>=None;let mut seen=std::collections::BTreeSet::new();
    for (n,line) in std::str::from_utf8(&raw)?.lines().enumerate() {
        let row:Value=serde_json::from_str(line)?;
        require(scope<exchanges.len()&&row["page"]==n&&row["exchangeId"]==exchanges[scope]
            &&row["fromInclusive"]==job.from&&row["throughExclusive"]==plan.through
            &&row["requestCursor"]==json!(cursor),"RESUME_EXACT_SCOPE_AND_CURSOR_CHAIN")?;
        let name=format!("page-{n:06}-validated.json");
        require(row["file"]==name,"RESUME_EXACT_RECEIPT_NAME")?;
        let path=source.directory.join(&name);
        require(!fs::symlink_metadata(&path)?.file_type().is_symlink(),"REGULAR_RESUME_RECEIPT")?;
        let bytes=fs::read(path)?;require(row["sha256"]==hash(&bytes),"RESUME_RECEIPT_HASH")?;
        let receipt:Value=serde_json::from_slice(&bytes)?;
        let trades=receipt["data"]["data"].as_array().ok_or("RESUME_TRADE_ARRAY")?;
        require(row["trades"]==trades.len()&&row["nextCursor"]==receipt["data"]["pagination"]["nextCursor"],"RESUME_RECEIPT_COUNTS_AND_CURSOR")?;
        let next=row["nextCursor"].as_str().map(str::to_string);
        require(if let Some(c)=&next {receipt["data"]["pagination"]["hasMore"]==true&&seen.insert(c.clone())}
                else {row["nextCursor"].is_null()&&receipt["data"]["pagination"]["hasMore"]==false},"RESUME_CURSOR_PROGRESS")?;
        pending.push(row);cursor=next;
        if cursor.is_none(){result.append(&mut pending);scope+=1;seen.clear();}
    }
    Ok(result)
}

#[cfg(unix)]
fn trade_backfill(plan:&Plan,job:&Backfill,exchanges:Vec<String>,sdk:&mut api::Transport)->Result<Value>{
    use std::{io::Write,collections::BTreeSet};
    let inherited=completed_prefix(plan,job,&exchanges)?;
    fs::create_dir(&job.output)?;
    disk_headroom(&job.output)?;
    let mut index=fs::OpenOptions::new().write(true).create_new(true).open(job.output.join("index.jsonl"))?;
    let(mut requests,mut trades,mut complete,mut bytes)=(0usize,0usize,0usize,0u64);
    if let Some(source)=&job.resume_completed {
        for row in &inherited {
            let n=row["page"].as_u64().ok_or("RESUME_PAGE")?;
            for suffix in ["parsed","validated"] {
                let name=format!("page-{n:06}-{suffix}.json");let old=source.directory.join(&name);
                require(!fs::symlink_metadata(&old)?.file_type().is_symlink(),"REGULAR_RESUME_SOURCE")?;
                // Same-filesystem immutable receipt links avoid needless copies.
                fs::hard_link(&old,job.output.join(&name))?;bytes+=fs::metadata(&old)?.len();
            }
            trades+=row["trades"].as_u64().ok_or("RESUME_TRADES")? as usize;
            if row["nextCursor"].is_null(){complete+=1;}
            writeln!(index,"{}",serde_json::to_string(row)?)?;
        }
        index.sync_all()?;
    }
    let inherited_complete=complete;
    for exchange in exchanges.into_iter().skip(inherited_complete) {
        let mut cursor:Option<String>=None;let mut cursors=BTreeSet::new();let mut identities=BTreeSet::new();
        loop {
            require(requests<job.maximum_requests&&bytes<8*1024*1024*1024,"HISTORY_TRADE_ACQUISITION_BUDGET_REACHED")?;
            disk_headroom(&job.output)?;
            let n=inherited.len()+requests;requests+=1;let evidence=job.output.join(format!("page-{n:06}-parsed.json"));
            let receipt=sdk.request_historical_trade_page(exchange.clone(),job.from.clone(),plan.through.clone(),cursor.as_deref(),&evidence)?;
            let rows=receipt.data["data"].as_array().ok_or("HISTORY_TRADE_ARRAY")?;
            for row in rows {require(identities.insert(hash(&serde_json::to_vec(row)?)),"HISTORY_TRADE_CURSOR_OVERLAP")?;}
            trades+=rows.len();let pg=&receipt.data["pagination"];
            let next=pg["nextCursor"].as_str().map(str::to_string);
            require(if pg["hasMore"]==true{next.as_ref().is_some_and(|s|cursors.insert(s.clone()))}else{next.is_none()},"HISTORY_TRADE_CURSOR_REPEATED")?;
            let raw=serde_json::to_vec(&receipt)?;let name=format!("page-{n:06}-validated.json");
            let mut out=fs::OpenOptions::new().write(true).create_new(true).open(job.output.join(&name))?;out.write_all(&raw)?;out.sync_all()?;
            bytes+=raw.len() as u64+fs::metadata(&evidence)?.len();
            writeln!(index,"{}",serde_json::to_string(&json!({"page":n,"exchangeId":exchange,"fromInclusive":job.from,
                "throughExclusive":plan.through,"requestCursor":cursor,"nextCursor":next,"file":name,"sha256":hash(&raw),
                "trades":rows.len(),"retrievedAt":receipt.received_at,"trainingAdmitted":false,"bookDepthReconstructed":false}))?)?;index.sync_all()?;
            match next{Some(c)=>cursor=Some(c),None=>{complete+=1;break}}
        }
    }
    let result=json!({"classification":"historical_public_trade_tape_not_owned_fills_or_books","requests":inherited.len()+requests,"newRequests":requests,"trades":trades,
        "exchangesComplete":complete,"fromInclusive":job.from,"throughExclusive":plan.through,"trainingAdmitted":false,
        "financialExecution":false,"receiptBytes":bytes,"inheritedPages":inherited.len(),"inheritedCompleteExchanges":inherited_complete});
    let mut out=fs::OpenOptions::new().write(true).create_new(true).open(job.output.join("complete.json"))?;
    out.write_all(&serde_json::to_vec(&result)?)?;out.sync_all()?;Ok(result)
}

#[cfg(unix)]
fn backfill(plan:&Plan,job:&Backfill,exchanges:Vec<String>,sdk:&mut api::Transport)->Result<Value>{
    use std::io::Write;
    // A new output directory prevents clobbering failed or completed evidence.
    fs::create_dir(&job.output)?;
    let floor=chrono::DateTime::parse_from_rfc3339(&job.from)?.timestamp_millis();
    let mut index=fs::OpenOptions::new().write(true).create_new(true).open(job.output.join("index.jsonl"))?;
    let mut requests=0usize;let mut candles=0usize;let mut complete=0usize;
    for exchange in exchanges {
        let mut through=plan.through.clone();
        loop {
            require(requests<job.maximum_requests,"HISTORY_BACKFILL_REQUEST_BUDGET_REACHED")?;
            let n=requests;requests+=1;
            let evidence=job.output.join(format!("page-{n:06}-parsed.json"));
            let receipt=sdk.request_history_with_evidence(exchange.clone(),through.clone(),&evidence)?;
            let rows=receipt.data["candles"].as_array().ok_or("HISTORY_CANDLES")?;candles+=rows.len();
            let next=page_cursor(&receipt.data,&through,floor)?;
            let raw=serde_json::to_vec(&receipt)?;
            let name=format!("page-{n:06}-validated.json");let mut out=fs::OpenOptions::new().write(true).create_new(true).open(job.output.join(&name))?;
            out.write_all(&raw)?;out.sync_all()?;
            writeln!(index,"{}",serde_json::to_string(&json!({"page":n,"exchangeId":exchange,"requestedThroughExclusive":through,
                "file":name,"sha256":hash(&raw),"candles":rows.len(),"nextThroughExclusive":next,"retrievedAt":receipt.received_at,
                "trainingAdmitted":false,"bookDepthReconstructed":false}))?)?;index.sync_all()?;
            match next{Some(t)=>through=t,None=>{complete+=1;break}}
        }
    }
    let result=json!({"classification":"historical_sparse_trade_candles_not_books_not_training_admission","requests":requests,
        "candles":candles,"exchangesComplete":complete,"fromInclusive":job.from,"throughExclusive":plan.through,
        "financialExecution":false,"trainingAdmitted":false});
    let mut out=fs::OpenOptions::new().write(true).create_new(true).open(job.output.join("complete.json"))?;
    out.write_all(&serde_json::to_vec(&result)?)?;out.sync_all()?;Ok(result)
}
#[cfg(not(unix))]
fn execute(_:&Plan,_:Config,_:Guard,_:PathBuf)->Result<Value> { Err("UNIX_EXISTING_CREDENTIAL_STORAGE_REQUIRED".into()) }

fn run()->Result<()> {
    let args:Vec<String>=std::env::args().collect();
    require(args.len()==3||args.len()==4,"USAGE_PLAN_AND_OPTIONAL_EXECUTE_READ_ONLY")?;
    require(args[1]=="--plan"&&(args.len()==3||args[3]=="--execute-read-only"),"UNKNOWN_HISTORY_ARGUMENT")?;
    let plan:Plan=serde_json::from_slice(&fs::read(&args[2])?)?;
    let(config,guard,credential,report)=prepare(&plan)?;
    let result=if args.len()==3 { report } else { execute(&plan,config,guard,credential)? };
    println!("{}",serde_json::to_string(&result)?);Ok(())
}
fn main(){if let Err(error)=run(){eprintln!("SIG_HISTORY_READ_FAILED: {error}");std::process::exit(1)}}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64,Ordering};
    static NEXT:AtomicU64=AtomicU64::new(0);
    #[test]fn forward_capture_is_short_bounded_and_public_only(){
        let(p,_)=fixture();let mut job=Forward{output:p.join("forward"),until:"1970-01-01T00:10:00Z".into(),maximum_requests:1000,node:p.join("node"),stream_entry:p.join("entry.mjs"),stream_sha256:"0".repeat(64)};
        assert_eq!(forward_bound(&job,1).unwrap(),600000);
        assert!(forward_bound(&job,600000).is_err());job.until="1970-01-01T01:00:00Z".into();assert!(forward_bound(&job,1).is_err());
        job.until="1970-01-01T00:10:00Z".into();job.maximum_requests=1501;assert!(forward_bound(&job,1).is_err());cleanup(p);
    }
    #[test]fn resume_imports_only_complete_scopes_and_rejects_tampered_receipts(){
        let(p,plan)=fixture();let source=p.join("old");fs::create_dir(&source).unwrap();
        let exchanges=vec!["2".to_string(),"3".to_string()];let mut lines=String::new();
        for (n,next) in [Value::Null,json!("next")].into_iter().enumerate(){
            let receipt=serde_json::to_vec(&json!({"data":{"data":[],"pagination":{"nextCursor":next,"hasMore":!next.is_null()}}})).unwrap();
            let name=format!("page-{n:06}-validated.json");fs::write(source.join(&name),&receipt).unwrap();
            lines.push_str(&format!("{}\n",json!({"page":n,"exchangeId":exchanges[n],"fromInclusive":"2026-09-01T00:00:00Z","throughExclusive":plan.through,"requestCursor":null,"nextCursor":next,"file":name,"sha256":hash(&receipt),"trades":0})));
        }
        fs::write(source.join("index.jsonl"),&lines).unwrap();
        let job=Backfill{output:p.join("new"),from:"2026-09-01T00:00:00Z".into(),maximum_requests:10,kind:HistoryKind::Trades,
            resume_completed:Some(ResumeCompleted{directory:source.clone(),index_sha256:hash(lines.as_bytes())})};
        let rows=completed_prefix(&plan,&job,&exchanges).unwrap();assert_eq!(rows.len(),1);assert_eq!(rows[0]["exchangeId"],"2");
        fs::write(source.join("page-000000-validated.json"),b"tampered").unwrap();
        assert!(completed_prefix(&plan,&job,&exchanges).unwrap_err().to_string().contains("RECEIPT_HASH"));cleanup(p);
    }
    #[test]fn resume_index_hash_and_scope_must_match(){
        let(p,plan)=fixture();let source=p.join("old");fs::create_dir(&source).unwrap();
        let raw=b"{}\n";fs::write(source.join("index.jsonl"),raw).unwrap();
        let mut job=Backfill{output:p.join("new"),from:"2026-09-01T00:00:00Z".into(),maximum_requests:10,kind:HistoryKind::Trades,
            resume_completed:Some(ResumeCompleted{directory:source,index_sha256:"0".repeat(64)})};
        assert!(completed_prefix(&plan,&job,&["2".into()]).unwrap_err().to_string().contains("PINNED_COMPLETE"));
        job.resume_completed.as_mut().unwrap().index_sha256=hash(raw);
        assert!(completed_prefix(&plan,&job,&["2".into()]).unwrap_err().to_string().contains("SCOPE_AND_CURSOR"));cleanup(p);
    }
    fn fixture()->(PathBuf,Plan){
        let p=std::env::temp_dir().join(format!("sig-history-read-{}-{}",std::process::id(),NEXT.fetch_add(1,Ordering::Relaxed)));
        fs::create_dir(&p).unwrap();let p=p.canonicalize().unwrap();let live=p.join("live-production-v1");fs::create_dir(&live).unwrap();
        fs::write(live.join("EXECUTION_STOP"),b"preserve").unwrap();
        let control=p.join("control.json");let raw=serde_json::to_vec(&json!({"root":live,"credentials":p.join("missing-key"),"accountScope":"original-scope","scope":{"tournamentId":"00000000-0000-0000-0000-000000000001","tournamentSlug":"cup","exchanges":{"2":"1"}}})).unwrap();fs::write(&control,&raw).unwrap();
        let plan=Plan {control_file:control,control_sha256:hash(&raw),binary_sha256:"0".repeat(64),job_stop:p.join("STOP"),exchange_id:"2".into(),through:"2026-10-12T05:00:00Z".into(),response_evidence:None,backfill:None,forward:None};(p,plan)
    }
    fn cleanup(p:PathBuf){let resolved=p.canonicalize().unwrap();assert_eq!(resolved.parent().unwrap(),std::env::temp_dir().canonicalize().unwrap());assert!(resolved.file_name().unwrap().to_string_lossy().starts_with("sig-history-read-"));fs::remove_dir_all(resolved).unwrap();}
    #[test]fn preflight_opens_no_queue_or_credential(){let(p,plan)=fixture();let(c,g,_,v)=prepare(&plan).unwrap();assert!(g.can_read(&c.scope));assert!(!c.writes_enabled&&!c.queue_dir.exists());assert_eq!(v["networkRequests"],0);assert_eq!(v["credentialsLoaded"],false);cleanup(p);}
    #[test]fn backward_cursor_is_exclusive_and_stops_at_floor(){
        let t="2026-10-12T05:00:00Z";let floor=chrono::DateTime::parse_from_rfc3339("2026-10-01T00:00:00Z").unwrap().timestamp_millis();
        assert_eq!(page_cursor(&json!({"candles":[{"time":"2026-10-08T00:01:00Z"}]}),t,floor).unwrap().unwrap(),"2026-10-08T00:01:00.000Z");
        assert!(page_cursor(&json!({"candles":[{"time":"2026-10-01T00:00:00Z"}]}),t,floor).unwrap().is_none());
        assert!(page_cursor(&json!({"candles":[]}),t,floor).unwrap().is_none());
        assert!(page_cursor(&json!({"candles":[{"time":t}]}),t,floor).is_err());
    }
    #[test]fn all_global_and_job_stops_close_reads(){let(p,plan)=fixture();let(c,g,_,_)=prepare(&plan).unwrap();for f in [&g.network_stop,&g.global_stop,&g.job_stop]{fs::write(f,b"stop").unwrap();assert!(!g.can_read(&c.scope));fs::remove_file(f).unwrap();}assert!(g.can_read(&c.scope));cleanup(p);}
    #[test]fn execution_stop_removal_and_control_change_close_reads(){let(p,plan)=fixture();let(c,g,_,_)=prepare(&plan).unwrap();fs::remove_file(&g.execution_stop).unwrap();assert!(!g.can_read(&c.scope));fs::write(&g.execution_stop,b"preserve").unwrap();fs::write(&g.control,b"changed").unwrap();assert!(!g.can_read(&c.scope));assert!(prepare(&plan).is_err());cleanup(p);}
    #[test]fn changed_scope_or_unaligned_end_is_rejected(){let(p,mut plan)=fixture();let(mut c,g,_,_)=prepare(&plan).unwrap();c.scope.tournament_slug="other".into();assert!(!g.can_read(&c.scope));plan.through="2026-10-12T05:00:01Z".into();assert!(prepare(&plan).is_err());cleanup(p);}
    #[cfg(unix)]
    #[test]fn dangling_network_stop_still_blocks(){let(p,plan)=fixture();let(c,g,_,_)=prepare(&plan).unwrap();std::os::unix::fs::symlink(p.join("absent-target"),&g.network_stop).unwrap();assert!(!g.can_read(&c.scope));cleanup(p);}
    #[cfg(unix)]
    #[test]fn blocked_execution_never_creates_queue_or_loads_missing_key(){let(p,plan)=fixture();let(c,g,key,_)=prepare(&plan).unwrap();fs::write(&g.network_stop,b"stop").unwrap();let queue=c.queue_dir.clone();assert_eq!(execute(&plan,c,g,key).unwrap_err().to_string(),"HISTORY_READ_BOUNDARY_CLOSED");assert!(!queue.exists());cleanup(p);}
}
