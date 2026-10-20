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
}
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Backfill { output:PathBuf, from:String, maximum_requests:usize, #[serde(default)] kind:HistoryKind }
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
    if let Some(job)=&plan.backfill {
        let start=chrono::DateTime::parse_from_rfc3339(&job.from)?.timestamp_millis();
        require(start>=0&&start%60000==0&&start<end,"HISTORY_BACKFILL_RANGE")?;
        require(job.output.is_absolute()&&job.maximum_requests>0&&job.maximum_requests<=50000,"HISTORY_BACKFILL_BOUND")?;
        require(plan.response_evidence.is_none(),"BACKFILL_HAS_PER_PAGE_EVIDENCE")?;
    }
    let shared=binding.root.parent().ok_or("ORIGINAL_SHARED_ROOT")?;
    let guard=Guard { control:plan.control_file.clone(),control_hash:plan.control_sha256.clone(),
        scope_hash:hash(&serde_json::to_vec(&binding.scope)?),network_stop:shared.join("NETWORK_DISABLED"),
        global_stop:shared.join("EXECUTION_STOP"),execution_stop:binding.root.join("EXECUTION_STOP"),job_stop:plan.job_stop.clone() };
    let report=json!({"classification":"single_GET_history_preflight_not_recovery_or_training",
        "readBoundaryCurrentlyOpen":guard.can_read(&binding.scope),"writesEnabled":false,"maximumHttpRequests":plan.backfill.as_ref().map_or(1,|j|j.maximum_requests),"backfillAllOriginalExchanges":plan.backfill.is_some(),
        "readAdmissionsPerMinute":100,"readCompletionPacingMs":600,"endpointKind":"history",
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
    let mut sdk=api::Transport::new_with_read_budget(config,Box::new(credentials),Box::new(api::SystemClock),Box::new(guard),100,600)?;
    if let Some(job)=&plan.backfill {
        return if job.kind==HistoryKind::Trades{trade_backfill(plan,job,exchanges,&mut sdk)}else{backfill(plan,job,exchanges,&mut sdk)}
    }
    // A fixed History endpoint has no pagination; maximum_pages is also1.
    let receipt=if let Some(path)=&plan.response_evidence {
        sdk.request_history_with_evidence(plan.exchange_id.clone(),plan.through.clone(),path)?
    } else { sdk.request(Endpoint::History { exchange_id:plan.exchange_id.clone(),to:plan.through.clone() })? };
    Ok(json!({"receipt":receipt,"historicalBooksReconstructed":false,"trainingAdmitted":false,"productionFinancialStateChanged":false}))
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
fn trade_backfill(plan:&Plan,job:&Backfill,exchanges:Vec<String>,sdk:&mut api::Transport)->Result<Value>{
    use std::{io::Write,collections::BTreeSet};
    fs::create_dir(&job.output)?;
    let mut index=fs::OpenOptions::new().write(true).create_new(true).open(job.output.join("index.jsonl"))?;
    let(mut requests,mut trades,mut complete,mut bytes)=(0usize,0usize,0usize,0u64);
    for exchange in exchanges {
        let mut cursor:Option<String>=None;let mut cursors=BTreeSet::new();let mut identities=BTreeSet::new();
        loop {
            require(requests<job.maximum_requests&&bytes<8*1024*1024*1024,"HISTORY_TRADE_ACQUISITION_BUDGET_REACHED")?;
            let n=requests;requests+=1;let evidence=job.output.join(format!("page-{n:06}-parsed.json"));
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
    let result=json!({"classification":"historical_public_trade_tape_not_owned_fills_or_books","requests":requests,"trades":trades,
        "exchangesComplete":complete,"fromInclusive":job.from,"throughExclusive":plan.through,"trainingAdmitted":false,
        "financialExecution":false,"receiptBytes":bytes});
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
    fn fixture()->(PathBuf,Plan){
        let p=std::env::temp_dir().join(format!("sig-history-read-{}-{}",std::process::id(),NEXT.fetch_add(1,Ordering::Relaxed)));
        fs::create_dir(&p).unwrap();let p=p.canonicalize().unwrap();let live=p.join("live-production-v1");fs::create_dir(&live).unwrap();
        fs::write(live.join("EXECUTION_STOP"),b"preserve").unwrap();
        let control=p.join("control.json");let raw=serde_json::to_vec(&json!({"root":live,"credentials":p.join("missing-key"),"accountScope":"original-scope","scope":{"tournamentId":"00000000-0000-0000-0000-000000000001","tournamentSlug":"cup","exchanges":{"2":"1"}}})).unwrap();fs::write(&control,&raw).unwrap();
        let plan=Plan {control_file:control,control_sha256:hash(&raw),binary_sha256:"0".repeat(64),job_stop:p.join("STOP"),exchange_id:"2".into(),through:"2026-10-12T05:00:00Z".into(),response_evidence:None,backfill:None};(p,plan)
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
