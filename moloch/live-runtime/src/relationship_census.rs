//! Optional startup discovery using the sole existing account transport.
//!
//! No response here authorizes a trade. Failure withholds relationship evidence;
//! it must not disable ordinary order/fill reconciliation or clear a STOP.
use baal_transport_service::{Endpoint,Receipt,Scope,Transport,TransportError};
use baal_transport_service::relationships::{self,BuyingPower,Collateral,Graph,RelationshipList};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest,Sha256};
use std::{collections::{BTreeMap,BTreeSet},fs::{self,OpenOptions},io::Write,path::{Path,PathBuf},sync::atomic::{AtomicU64,Ordering},time::{SystemTime,UNIX_EPOCH}};

#[derive(Debug,Clone,Serialize)]
#[serde(rename_all="snake_case")]
pub enum Availability { Available,Empty,Forbidden,Unavailable,Invalid,NotRequested }
#[derive(Debug,Clone,Serialize)]
#[serde(rename_all="camelCase")]
pub struct SourceEvidence {
    /// Hash of serde-serialized Receipt (including clocks and exact JSON Number
    /// lexemes), not a claim to hash pre-parse HTTP framing or original bytes.
    pub receipt_sha256:String,pub private_file:String,
    pub request_started_at:String,pub received_at:String,pub status:u16,
}
#[derive(Debug,Clone,Serialize)]
#[serde(rename_all="camelCase")]
pub struct Surface {
    pub availability:Availability,pub row_count:Option<usize>,
    pub source:Option<SourceEvidence>,pub error_code:Option<String>,
    pub http_status:Option<u16>,pub retry_after_ms:Option<u64>,
}
impl Surface {
    fn not_requested()->Self{Self{availability:Availability::NotRequested,row_count:None,source:None,error_code:None,http_status:None,retry_after_ms:None}}
    fn invalid(code:&str)->Self{Self{availability:Availability::Invalid,row_count:None,source:None,error_code:Some(code.into()),http_status:None,retry_after_ms:None}}
    fn error(error:&TransportError)->Self{
        let availability=match error.status {Some(401|403)=>Availability::Forbidden,Some(429|500..=599)=>Availability::Unavailable,_=>match error.code.as_str(){"CONNECTION_UNCONFIRMED"|"BODY_UNCONFIRMED"|"INVALID_RESPONSE_JSON"=>Availability::Unavailable,_=>Availability::Invalid}};
        Self{availability,row_count:None,source:None,error_code:Some(error.code.clone()),http_status:error.status,retry_after_ms:Some(error.retry_after_ms)}
    }
    fn success(count:usize,source:SourceEvidence)->Self{Self{availability:if count==0{Availability::Empty}else{Availability::Available},row_count:Some(count),http_status:Some(source.status),source:Some(source),error_code:None,retry_after_ms:None}}
}
#[derive(Debug,Clone,Serialize)]
#[serde(rename_all="camelCase")]
pub struct CensusReport {
    pub version:u32,pub started_at_ms:u64,pub completed_at_ms:u64,
    pub tournament_id:String,pub scope_sha256:String,
    pub relationships:Surface,pub constraints:Surface,pub collateral:Surface,
    pub account:Surface,pub graphs:BTreeMap<String,Surface>,
    pub active_relationship_count:usize,pub exhaustive_relationship_count:usize,
    pub known_floor_component_count:usize,pub unknown_floor_component_count:usize,
    pub feed_complete:bool,pub transport_guard_observed:bool,
    pub execution_authority:bool,
}
/// Intentionally not Serialize: account/profile-bearing receipts live only in
/// private evidence files; callers publish the redacted CensusReport.
pub struct CensusCache {
    pub scope:Scope,pub relationships:Option<RelationshipList>,
    pub constraints:Option<Value>,pub collateral:Option<Collateral>,
    pub buying_power:Option<BuyingPower>,pub graphs:BTreeMap<String,Graph>,
    pub private_evidence_directory:PathBuf,
}
pub struct Census {pub report:CensusReport,pub cache:CensusCache}

fn now_ms()->u64{SystemTime::now().duration_since(UNIX_EPOCH).map(|d|d.as_millis() as u64).unwrap_or(0)}
fn hash(bytes:&[u8])->String{format!("{:x}",Sha256::digest(bytes))}
fn guarded(error:&TransportError)->bool{
    error.may_have_executed||matches!(error.code.as_str(),"STOPPED_OR_OWNER_LOST"|"SCOPED_LEASE_LOST"|"COMPLETION_DURABILITY_UNCERTAIN"|"DURABILITY_FAILURE"|"EXPLICIT_COMPACTION_REQUIRED")
}
trait Reader {fn read(&mut self,endpoint:Endpoint)->Result<Receipt,TransportError>;}
impl Reader for Transport {fn read(&mut self,endpoint:Endpoint)->Result<Receipt,TransportError>{self.request(endpoint)}}

fn evidence_directory(path:&Path)->Result<(),&'static str>{
    if !path.is_absolute(){return Err("CENSUS_EVIDENCE_ABSOLUTE_PATH_REQUIRED");}
    if let Ok(meta)=fs::symlink_metadata(path){if meta.file_type().is_symlink()||!meta.is_dir(){return Err("CENSUS_EVIDENCE_DIRECTORY");}}
    fs::create_dir_all(path).map_err(|_|"CENSUS_EVIDENCE_DIRECTORY")?;
    #[cfg(unix)] {use std::os::unix::fs::PermissionsExt;fs::set_permissions(path,fs::Permissions::from_mode(0o700)).map_err(|_|"CENSUS_EVIDENCE_PERMISSIONS")?;}
    Ok(())
}
fn persist(path:&Path,kind:&str,receipt:&Receipt,scope:&Scope)->Result<SourceEvidence,&'static str>{
    // These endpoints lack a tournament echo in some response shapes. Bind the
    // cache to the exact explicit request scope rather than label defaults.
    let scope_query=format!("tournamentId={}",scope.tournament_id);
    if !receipt.endpoint.split('?').nth(1).is_some_and(|q|q.split('&').any(|part|part==scope_query)) {return Err("CENSUS_RECEIPT_SCOPE");}
    if !(200..300).contains(&receipt.status){return Err("CENSUS_RECEIPT_STATUS");}
    static SEQUENCE:AtomicU64=AtomicU64::new(0);
    let name=format!("relationship-census-{}-{}-{}-{kind}.json",now_ms(),std::process::id(),SEQUENCE.fetch_add(1,Ordering::SeqCst));
    let bytes=serde_json::to_vec(receipt).map_err(|_|"CENSUS_EVIDENCE_SERIALIZATION")?;
    let mut options=OpenOptions::new();options.write(true).create_new(true);
    #[cfg(unix)] {use std::os::unix::fs::OpenOptionsExt;options.mode(0o600);}
    let mut file=options.open(path.join(&name)).map_err(|_|"CENSUS_EVIDENCE_CREATE")?;
    file.write_all(&bytes).and_then(|_|file.sync_all()).map_err(|_|"CENSUS_EVIDENCE_DURABILITY")?;
    #[cfg(unix)] {fs::File::open(path).and_then(|f|f.sync_all()).map_err(|_|"CENSUS_EVIDENCE_DURABILITY")?;}
    Ok(SourceEvidence{receipt_sha256:hash(&bytes),private_file:name,request_started_at:receipt.request_started_at.clone(),received_at:receipt.received_at.clone(),status:receipt.status})
}

/// The only production entry point. Reuses the caller's mutable account worker.
/// Reads are synchronous and serialized by its existing account admission queue.
/// `graph_market_ids` must be unique known markets, maximum three. The list uses
/// Transport's configured maximum_pages; the helper never loops or retries reads.
pub fn capture(transport:&mut Transport,scope:&Scope,private_directory:&Path,graph_market_ids:&[String])->Census{
    capture_inner(transport,scope,private_directory,graph_market_ids)
}
fn capture_inner(reader:&mut impl Reader,scope:&Scope,directory:&Path,roots:&[String])->Census {
    let mut report=CensusReport{version:1,started_at_ms:now_ms(),completed_at_ms:0,tournament_id:scope.tournament_id.clone(),scope_sha256:hash(&serde_json::to_vec(scope).unwrap_or_default()),relationships:Surface::not_requested(),constraints:Surface::not_requested(),collateral:Surface::not_requested(),account:Surface::not_requested(),graphs:BTreeMap::new(),active_relationship_count:0,exhaustive_relationship_count:0,known_floor_component_count:0,unknown_floor_component_count:0,feed_complete:false,transport_guard_observed:false,execution_authority:false};
    let mut cache=CensusCache{scope:scope.clone(),relationships:None,constraints:None,collateral:None,buying_power:None,graphs:BTreeMap::new(),private_evidence_directory:directory.into()};
    let numeric=|s:&str|!s.is_empty()&&!s.starts_with('0')&&s.bytes().all(|b|b.is_ascii_digit())&&s.parse::<u64>().is_ok_and(|n|n<=9_007_199_254_740_991);
    let valid_scope=!scope.exchanges.is_empty()&&scope.exchanges.iter().all(|(e,m)|numeric(e)&&numeric(m));
    let valid_roots=roots.len()<=3&&roots.iter().collect::<BTreeSet<_>>().len()==roots.len()&&roots.iter().all(|m|scope.exchanges.values().any(|known|known==m));
    let setup=if !relationships::uuid(&scope.tournament_id)||!valid_scope||!valid_roots{Err("CENSUS_INVALID_SCOPE_OR_ROOTS")}else{evidence_directory(directory)};
    if let Err(code)=setup {report.relationships=Surface::invalid(code);report.completed_at_ms=now_ms();return Census{report,cache};}
    let jobs=vec![
        ("relationships".to_string(),Endpoint::Relationships{market_id:None,exchange_id:None,relationship_type:None,depth:4}),
        ("constraints".to_string(),Endpoint::RelationshipConstraints{market_id:None,relationship_id:None,violations_only:false,min_violation:Some("0".into())}),
        ("collateral".to_string(),Endpoint::Collateral),
        ("account".to_string(),Endpoint::Account),
    ].into_iter().chain(roots.iter().map(|m|(format!("graph-{m}"),Endpoint::RelationshipGraph{market_id:m.clone(),depth:4})));
    for (kind,endpoint) in jobs {
        if report.transport_guard_observed {break;}
        let surface=match reader.read(endpoint) {
            Err(error)=>{report.transport_guard_observed=guarded(&error);Surface::error(&error)},
            Ok(receipt)=>match persist(directory,&kind,&receipt,scope) {
                Err(code)=>Surface::invalid(code),
                Ok(source)=>{
                    let normalized:Result<usize,TransportError>=match kind.as_str() {
                        "relationships"=>relationships::parse_list(&receipt.data,scope).map(|list|{
                            report.active_relationship_count=list.data.iter().filter(|r|r.status==relationships::RelationshipStatus::Active).count();
                            report.exhaustive_relationship_count=list.data.iter().filter(|r|r.is_exhaustive).count();
                            let count=list.data.len();cache.relationships=Some(list);count
                        }),
                        "constraints"=>relationships::validate_constraints(&receipt.data,scope).map(|_|{let count=receipt.data["data"].as_array().map_or(0,Vec::len);cache.constraints=Some(receipt.data.clone());count}),
                        "collateral"=>relationships::parse_collateral(&receipt.data,scope).map(|value|{
                            report.known_floor_component_count=value.components.iter().filter(|c|c.guaranteed_payout_floor_units.is_some()).count();
                            report.unknown_floor_component_count=value.components.len()-report.known_floor_component_count;
                            let count=value.components.len();cache.collateral=Some(value);count
                        }),
                        "account"=>relationships::parse_buying_power(&receipt.data,scope).map(|value|{cache.buying_power=Some(value);1}),
                        _=>relationships::parse_graph(&receipt.data,scope).map(|graph|{let count=graph.relationships.len();cache.graphs.insert(kind.trim_start_matches("graph-").to_string(),graph);count}),
                    };
                    match normalized {Ok(count)=>Surface::success(count,source),Err(error)=>{let mut surface=Surface::error(&error);surface.source=Some(source);surface}}
                }
            }
        };
        match kind.as_str(){"relationships"=>report.relationships=surface,"constraints"=>report.constraints=surface,"collateral"=>report.collateral=surface,"account"=>report.account=surface,_=>{report.graphs.insert(kind.trim_start_matches("graph-").to_string(),surface);}}
    }
    report.feed_complete=cache.relationships.is_some()&&cache.constraints.is_some()&&cache.collateral.is_some()&&cache.buying_power.is_some()&&roots.iter().all(|m|cache.graphs.contains_key(m));
    report.completed_at_ms=now_ms();Census{report,cache}
}

#[cfg(test)]mod tests{
    use super::*;
    use serde_json::json;
    struct Fake {reads:Vec<Endpoint>,responses:std::collections::VecDeque<Result<Receipt,TransportError>>}
    impl Reader for Fake {fn read(&mut self,endpoint:Endpoint)->Result<Receipt,TransportError>{self.reads.push(endpoint);self.responses.pop_front().expect("bounded expected reads")}}
    fn scope()->Scope{Scope{tournament_id:"bda92870-621e-47b0-bc3c-3602c5c26f55".into(),tournament_slug:"midterm-elections".into(),exchanges:BTreeMap::from([("994".into(),"305".into())])}}
    fn receipt(kind:&str,data:Value)->Result<Receipt,TransportError>{Ok(Receipt{endpoint:format!("/{kind}?tournamentId={}",scope().tournament_id),request_started_at:"2026-10-15T08:00:00.000Z".into(),received_at:"2026-10-15T08:00:01.000Z".into(),status:200,data})}
    fn directory()->PathBuf{static N:AtomicU64=AtomicU64::new(0);std::env::temp_dir().join(format!("moloch-census-test-{}-{}-{}",std::process::id(),now_ms(),N.fetch_add(1,Ordering::SeqCst)))}
    fn empty_responses()->std::collections::VecDeque<Result<Receipt,TransportError>>{[
        receipt("relationships",json!({"data":[],"pagination":{"limit":200,"hasMore":false,"nextCursor":null}})),
        receipt("relationships/constraints",json!({"data":[],"violationsCount":0,"computedAt":"2026-10-15T08:00:00Z"})),
        receipt("portfolio/collateral",json!({"data":[],"availableAdvance":0})),
        receipt("account",json!({"balance":100000,"availableAdvance":0,"buyingPower":100000,"email":"private-fixture@example.invalid"})),
    ].into()}
    #[test]fn empty_census_is_complete_evidence_not_trade_authority(){let dir=directory();let mut reader=Fake{reads:vec![],responses:empty_responses()};let c=capture_inner(&mut reader,&scope(),&dir,&[]);assert_eq!(reader.reads.len(),4);assert!(c.report.feed_complete);assert!(!c.report.execution_authority);assert!(matches!(c.report.relationships.availability,Availability::Empty));let public=serde_json::to_string(&c.report).unwrap();assert!(!public.contains("private-fixture"));assert!(!public.contains("100000"));for entry in fs::read_dir(&dir).unwrap(){let bytes=fs::read(entry.unwrap().path()).unwrap();let _:Receipt=serde_json::from_slice(&bytes).unwrap();}let evidence=c.report.account.source.unwrap();assert_eq!(hash(&fs::read(dir.join(evidence.private_file)).unwrap()),evidence.receipt_sha256);fs::remove_dir_all(dir).unwrap();}
    #[test]fn forbidden_optional_surface_does_not_prevent_other_reads(){let dir=directory();let mut responses=empty_responses();responses[0]=Err(TransportError{code:"OFFICIAL_REQUEST_REJECTED".into(),status:Some(403),may_have_executed:false,retry_after_ms:0,order_id:None,api_code:Some("FORBIDDEN".into())});let mut reader=Fake{reads:vec![],responses};let c=capture_inner(&mut reader,&scope(),&dir,&[]);assert_eq!(reader.reads.len(),4);assert!(!c.report.feed_complete);assert!(!c.report.transport_guard_observed);assert!(c.cache.buying_power.is_some());assert!(matches!(c.report.relationships.availability,Availability::Forbidden));fs::remove_dir_all(dir).unwrap();}
    #[test]fn invalid_roots_make_zero_requests_and_guard_failure_stops_census(){let dir=directory();let mut reader=Fake{reads:vec![],responses:empty_responses()};let c=capture_inner(&mut reader,&scope(),&dir,&["305/../../orders".into()]);assert!(reader.reads.is_empty());assert!(!c.report.feed_complete);assert!(!dir.exists());let mut responses=empty_responses();responses[0]=Err(TransportError{code:"STOPPED_OR_OWNER_LOST".into(),status:None,may_have_executed:false,retry_after_ms:0,order_id:None,api_code:None});let mut reader=Fake{reads:vec![],responses};let c=capture_inner(&mut reader,&scope(),&dir,&[]);assert_eq!(reader.reads.len(),1);assert!(c.report.transport_guard_observed);assert!(!c.report.feed_complete);fs::remove_dir_all(dir).unwrap();}
    #[test]fn source_scope_mismatch_keeps_cache_unavailable(){let dir=directory();let mut responses=empty_responses();responses[0].as_mut().unwrap().endpoint="/relationships?tournamentId=550e8400-e29b-41d4-a716-446655440000".into();let mut reader=Fake{reads:vec![],responses};let c=capture_inner(&mut reader,&scope(),&dir,&[]);assert_eq!(reader.reads.len(),4);assert!(c.cache.relationships.is_none());assert_eq!(c.report.relationships.error_code.as_deref(),Some("CENSUS_RECEIPT_SCOPE"));fs::remove_dir_all(dir).unwrap();}
}
