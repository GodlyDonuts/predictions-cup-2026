use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs::{self, File, OpenOptions}, io::{BufRead, BufReader, Read, Seek, SeekFrom, Write}, path::{Path, PathBuf}};

const MAX_TIME: u64 = 8_640_000_000_000_000;
const MAX_SAFE: u64 = 9_007_199_254_740_991;
const MAX_ACTIVE_JOURNAL_BYTES: u64 = 64 * 1024 * 1024;
const MAX_CHECKPOINT_BYTES: u64 = 256 * 1024 * 1024;
type Result<T> = std::result::Result<T, String>;
fn need(ok: bool, error: &str) -> Result<()> { if ok { Ok(()) } else { Err(error.into()) } }
fn hash<T: Serialize>(v: &T) -> String { format!("{:x}", Sha256::digest(serde_json::to_vec(v).expect("serializable state"))) }

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Binding { intent: String, key: String, payload_hash: String }
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Flight { request: String, write: bool, binding: Option<Binding>, network_started: bool }
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct State {
    schema_version: u32, scope: String, last_clock: u64, next: u64, cooldown: u64,
    manual_review: bool, reads: Vec<u64>, writes: Vec<u64>, active: Option<Flight>,
    unknown: BTreeMap<String, Binding>, intents: BTreeMap<String, Binding>, keys: BTreeMap<String, Binding>,
}
impl State {
    fn new(scope: String) -> Self { Self { schema_version:1, scope, last_clock:0, next:0, cooldown:0, manual_review:false, reads:vec![], writes:vec![], active:None, unknown:BTreeMap::new(), intents:BTreeMap::new(), keys:BTreeMap::new() } }
    fn clock(&mut self, now: u64) -> Result<()> {
        need(now <= MAX_TIME, "INVALID_CLOCK")?;
        if now < self.last_clock { self.manual_review=true; return Err("CLOCK_REGRESSION".into()); }
        self.last_clock=now;
        self.reads.retain(|x| now.saturating_sub(*x)<=60_000);
        self.writes.retain(|x| now.saturating_sub(*x)<=60_000);
        Ok(())
    }
    fn time(&self, write: bool) -> u64 {
        let samples=if write { &self.writes } else { &self.reads };
        let cap=if write {24} else {60};
        let rolling=if samples.len()>=cap {samples[0].saturating_add(60_001)} else {0};
        self.next.max(self.cooldown).max(rolling)
    }
    fn apply(&mut self, c: &Command) -> Result<Value> {
        need(c.scope==self.scope, "SCOPE_MISMATCH")?;
        self.clock(c.now)?;
        if c.op=="status" { return Ok(json!({"state":self})); }
        need(!self.manual_review, "MANUAL_REVIEW_REQUIRED")?;
        match c.op.as_str() {
            "admit" => {
                need(self.active.is_none(), "SERIAL_QUEUE_BUSY")?;
                need(!c.request.is_empty(), "REQUEST_ID_REQUIRED")?;
                if c.write {
                    need(self.unknown.is_empty(), "UNKNOWN_ACCEPTANCE_RECONCILE_FIRST")?;
                    need(c.authorized && c.permit_valid && c.lease_valid, "WRITE_FENCE_REQUIRED")?;
                    let b=c.binding.as_ref().ok_or("IMMUTABLE_BINDING_REQUIRED")?;
                    need(!b.intent.is_empty() && !b.key.is_empty() && b.payload_hash.len()==64 && b.payload_hash.bytes().all(|x| x.is_ascii_hexdigit() && !x.is_ascii_uppercase()), "INVALID_BINDING")?;
                    need(self.intents.get(&b.intent).is_none_or(|old|old==b), "LOGICAL_REQUEST_CHANGED")?;
                    need(self.keys.get(&b.key).is_none_or(|old|old==b), "IDEMPOTENCY_PAYLOAD_CHANGED")?;
                    need(!self.intents.contains_key(&b.intent), "KNOWN_INTENT_RECONCILE_FIRST")?;
                }
                let at=self.time(c.write);
                if c.now<at {return Ok(json!({"admitted":false,"wait_until":at}));}
                let interval=if c.write {2500} else {1000};
                self.next=c.now.checked_add(interval).filter(|x|*x<=MAX_TIME).ok_or("CLOCK_OVERFLOW")?;
                (if c.write {&mut self.writes} else {&mut self.reads}).push(c.now);
                if let Some(b)=c.binding.as_ref().filter(|_|c.write) {self.intents.insert(b.intent.clone(),b.clone()); self.keys.insert(b.key.clone(),b.clone());}
                self.active=Some(Flight {request:c.request.clone(),write:c.write,binding:if c.write {c.binding.clone()} else {None},network_started:false});
                Ok(json!({"admitted":true,"next":self.next}))
            },
            "network" => {
                let f=self.active.as_mut().ok_or("NO_ACTIVE_REQUEST")?;
                need(f.request==c.request, "REQUEST_MISMATCH")?;
                need(!f.network_started, "DUPLICATE_NETWORK_DISPATCH")?;
                if f.write {need(c.authorized && c.permit_valid && c.lease_valid && c.binding==f.binding, "NETWORK_WRITE_FENCE_REQUIRED")?;}
                f.network_started=true;
                Ok(json!({"network_permitted":true}))
            },
            "complete" => {
                let f=self.active.as_ref().ok_or("NO_ACTIVE_REQUEST")?.clone();
                need(f.request==c.request, "REQUEST_MISMATCH")?;
                need(["success","not_sent","unknown","rejected"].contains(&c.outcome.as_str()), "INVALID_OUTCOME")?;
                need(c.outcome!="not_sent" || !f.network_started, "DISPATCH_CANNOT_BE_NOT_SENT")?;
                need(!["success","rejected","unknown"].contains(&c.outcome.as_str()) || f.network_started, "NETWORK_NOT_STARTED")?;
                if matches!(c.status,429|503) {
                    let retry=retry_after(&c.retry_after,c.now);
                    let delay=retry.max(if c.status==429 {60_000} else {10_000});
                    if retry==MAX_SAFE || c.now.checked_add(delay).is_none_or(|x|x>MAX_TIME) {self.manual_review=true;} else {self.cooldown=self.cooldown.max(c.now+delay);}
                }
                if c.api_code=="REQUEST_IN_FLIGHT" {self.cooldown=self.cooldown.max(c.now.checked_add(90_000).filter(|x|*x<=MAX_TIME).ok_or("CLOCK_OVERFLOW")?);}
                if f.write && c.outcome=="unknown" {let b=f.binding.clone().unwrap();self.unknown.insert(b.intent.clone(),b);}
                let interval=if f.write {2500} else {1000};
                self.next=self.next.max(c.now.checked_add(interval).filter(|x|*x<=MAX_TIME).ok_or("CLOCK_OVERFLOW")?).max(self.cooldown);
                self.active=None;
                Ok(json!({"completed":true,"next":self.next,"manual_review":self.manual_review}))
            },
            "reconcile" => {
                need(c.reconciled && c.lease_valid && c.permit_valid, "SCOPED_RECONCILIATION_REQUIRED")?;
                let b=c.binding.as_ref().ok_or("BINDING_REQUIRED")?;
                need(self.unknown.get(&b.intent)==Some(b), "UNKNOWN_BINDING_MISMATCH")?;
                // No replacement or same-key retry is released here. Bindings remain permanently reserved.
                self.unknown.remove(&b.intent);
                Ok(json!({"reconciled":true,"retry_permitted":false}))
            },
            _=>Err("UNKNOWN_OPERATION".into()),
        }
    }
    fn recover(&mut self) {
        if let Some(f)=self.active.take() {
            // Crash between durable network fence and response has unknown acceptance.
            if f.write {let b=f.binding.unwrap();self.unknown.insert(b.intent.clone(),b);}
            let recovery_wait=if f.write {2500} else if f.network_started {21_000} else {1000};
            self.next=self.next.max(self.last_clock.saturating_add(recovery_wait));
        }
    }
}
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Command {
    op:String, scope:String, now:u64, request:String, write:bool,
    binding:Option<Binding>, authorized:bool, permit_valid:bool, lease_valid:bool,
    outcome:String, status:u16, retry_after:String, api_code:String, reconciled:bool,
}
fn retry_after(header: &str, now:u64)->u64 {
    let text=header.trim();
    if text.is_empty(){return 0;}
    let mut dots=0;let mut valid=true;
    for b in text.bytes(){if b==b'.'{dots+=1;}else if !b.is_ascii_digit(){valid=false;}}
    let numeric=valid && dots<=1 && !text.starts_with('.') && !text.ends_with('.');
    if numeric {let n=text.parse::<f64>().unwrap_or(f64::INFINITY)*1000.;return if !n.is_finite() || n>=MAX_SAFE as f64 {MAX_SAFE} else {n.ceil() as u64};}
    if !text.to_ascii_uppercase().ends_with("GMT"){return 0;}
    httpdate::parse_http_date(text).ok().and_then(|d|d.duration_since(std::time::UNIX_EPOCH).ok()).map(|d|(d.as_millis().min(MAX_TIME as u128) as u64).saturating_sub(now)).unwrap_or(0)
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {sequence:u64, previous:String, state:State, hash:String}
fn record_hash(sequence:u64,previous:&str,state:&State)->String {hash(&(sequence,previous,state))}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Anchor {sequence:u64, previous:String, source_journal:String, source_bytes:u64, source_sha256:String, prior_checkpoint_hash:Option<String>}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Checkpoint {schema_version:u32, anchor:Anchor, state:State, hash:String}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {schema_version:u32, scope:String, journal:String, checkpoint:String, checkpoint_hash:String}
fn checkpoint_hash(anchor:&Anchor,state:&State)->String {hash(&(1u32,anchor,state))}
fn local_name(name:&str)->bool {!name.is_empty() && name.len()<160 && name.bytes().all(|x|x.is_ascii_alphanumeric() || x==b'-' || x==b'.') && !name.contains("..")}
fn source_digest(path:&Path)->Result<(u64,String)> {let mut file=File::open(path).map_err(|_|"SOURCE_JOURNAL_UNAVAILABLE")?;let mut digest=Sha256::new();let mut size=0u64;let mut buffer=[0u8;65536];loop{let n=file.read(&mut buffer).map_err(|_|"SOURCE_JOURNAL_UNAVAILABLE")?;if n==0{break;}size+=n as u64;digest.update(&buffer[..n]);}Ok((size,format!("{:x}",digest.finalize())))}
fn write_new_synced(path:&Path,bytes:&[u8])->Result<()> {let mut file=OpenOptions::new().write(true).create_new(true).open(path).map_err(|_|"COMPACTION_FILE_CREATE_FAILED")?;file.write_all(bytes).and_then(|_|file.sync_all()).map_err(|_|"COMPACTION_DURABILITY_FAILURE".into())}
struct Store {file:File, state:State, sequence:u64, previous:String, lock_path:PathBuf, identity:Vec<u8>, poisoned:bool, dir:PathBuf, journal_name:String, checkpoint_hash:Option<String>, active_bytes:u64, maximum_bytes:u64, recovery_compaction_required:bool}
impl Store {
    fn open(dir:&Path,scope:&str,token:&str)->Result<Self>{
        Self::open_with_limit(dir,scope,token,MAX_ACTIVE_JOURNAL_BYTES)
    }
    fn open_with_limit(dir:&Path,scope:&str,token:&str,maximum_bytes:u64)->Result<Self>{
        need(!scope.is_empty() && !token.is_empty(),"SCOPE_TOKEN_REQUIRED")?;
        fs::create_dir_all(dir).map_err(|_|"STORE_CREATE_FAILED")?;
        let lock_path=dir.join("offline-account-queue.lock");
        let identity=serde_json::to_vec(&json!({"scope":scope,"token":token,"pid":std::process::id()})).unwrap();
        let mut lock=OpenOptions::new().write(true).create_new(true).open(&lock_path).map_err(|_|"EXCLUSIVE_QUEUE_LOCK_REQUIRED")?;
        lock.write_all(&identity).and_then(|_|lock.sync_all()).map_err(|_|"LOCK_DURABILITY_FAILURE")?;
        let load=||->Result<(State,u64,String,File,String,Option<String>,u64)>{
            let mut state=State::new(scope.into());let mut sequence=0;let mut previous=String::new();let mut journal_name="account-queue.jsonl".to_string();let mut checkpoint_hash=None;
            let manifest_path=dir.join("account-queue-manifest.json");
            let guard_path=dir.join("account-queue-generation.guard");
            if guard_path.exists(){need(fs::read(&guard_path).ok().as_deref()==Some(scope.as_bytes()),"GENERATION_GUARD_SCOPE_FAILURE")?;need(manifest_path.exists(),"GENERATION_MANIFEST_REQUIRED")?;}
            if manifest_path.exists(){
                need(guard_path.exists(),"GENERATION_GUARD_REQUIRED")?;
                need(fs::metadata(&manifest_path).map_err(|_|"MANIFEST_UNAVAILABLE")?.len()<=4096,"MANIFEST_TOO_LARGE")?;
                let manifest:Manifest=serde_json::from_slice(&fs::read(&manifest_path).map_err(|_|"MANIFEST_UNAVAILABLE")?).map_err(|_|"MANIFEST_INVALID")?;
                need(manifest.schema_version==1 && manifest.scope==scope && local_name(&manifest.journal) && local_name(&manifest.checkpoint),"MANIFEST_SCOPE_OR_PATH_FAILURE")?;
                let checkpoint_path=dir.join(&manifest.checkpoint);
                need(fs::metadata(&checkpoint_path).map_err(|_|"CHECKPOINT_UNAVAILABLE")?.len()<=MAX_CHECKPOINT_BYTES,"CHECKPOINT_TOO_LARGE")?;
                let checkpoint:Checkpoint=serde_json::from_slice(&fs::read(&checkpoint_path).map_err(|_|"CHECKPOINT_UNAVAILABLE")?).map_err(|_|"CHECKPOINT_INVALID")?;
                need(checkpoint.schema_version==1 && checkpoint.state.schema_version==1 && checkpoint.state.scope==scope && checkpoint.hash==manifest.checkpoint_hash && checkpoint.hash==self::checkpoint_hash(&checkpoint.anchor,&checkpoint.state) && local_name(&checkpoint.anchor.source_journal),"CHECKPOINT_INTEGRITY_OR_SCOPE_FAILURE")?;
                state=checkpoint.state;sequence=checkpoint.anchor.sequence;previous=checkpoint.anchor.previous;checkpoint_hash=Some(checkpoint.hash);journal_name=manifest.journal;
                // A manifest-referenced journal is never recreated if missing.
                need(dir.join(&journal_name).exists(),"ACTIVE_JOURNAL_UNAVAILABLE")?;
            }
            let path=dir.join(&journal_name);
            let active_bytes=fs::metadata(&path).map(|m|m.len()).unwrap_or(0);
            need(active_bytes<=maximum_bytes,"ACTIVE_JOURNAL_TOO_LARGE")?;
            if path.exists(){
                let mut journal=File::open(&path).map_err(|_|"JOURNAL_READ_FAILED")?;
                let size=journal.metadata().map_err(|_|"JOURNAL_READ_FAILED")?.len();
                if size>0 {journal.seek(SeekFrom::End(-1)).map_err(|_|"JOURNAL_READ_FAILED")?;let mut end=[0u8];journal.read_exact(&mut end).map_err(|_|"JOURNAL_READ_FAILED")?;need(end[0]==b'\n',"UNTERMINATED_JOURNAL_RECORD")?;journal.seek(SeekFrom::Start(0)).map_err(|_|"JOURNAL_READ_FAILED")?;}
                for line in BufReader::new(journal).lines(){
                let line=line.map_err(|_|"JOURNAL_READ_FAILED")?;need(!line.is_empty(),"EMPTY_JOURNAL_RECORD")?;
                let r:Record=serde_json::from_str(&line).map_err(|_|"JOURNAL_INVALID")?;
                need(r.sequence==sequence+1 && r.previous==previous && r.hash==record_hash(r.sequence,&r.previous,&r.state) && r.state.scope==scope && r.state.schema_version==1,"JOURNAL_INTEGRITY_OR_SCOPE_FAILURE")?;
                state=r.state;sequence=r.sequence;previous=r.hash;
            }}
            let file=OpenOptions::new().append(true).create(true).open(path).map_err(|_|"JOURNAL_OPEN_FAILED")?;
            Ok((state,sequence,previous,file,journal_name,checkpoint_hash,active_bytes))
        };
        match load(){Ok((state,sequence,previous,file,journal_name,checkpoint_hash,active_bytes))=>{let mut s=Self{file,state,sequence,previous,lock_path,identity,poisoned:false,dir:dir.into(),journal_name,checkpoint_hash,active_bytes,maximum_bytes,recovery_compaction_required:false};let mut recovered=s.state.clone();recovered.recover();if recovered!=s.state{match s.persist(recovered.clone()){Ok(())=>{},Err(e) if e=="EXPLICIT_COMPACTION_REQUIRED"=>{s.state=recovered;s.recovery_compaction_required=true;},Err(e)=>return Err(e)}}Ok(s)},Err(e)=>{let _=fs::remove_file(&lock_path);Err(e)}}
    }
    fn owned(&self)->Result<()> {need(!self.poisoned,"DURABILITY_FAILURE")?;need(fs::read(&self.lock_path).ok().as_deref()==Some(&self.identity),"SCOPED_LEASE_LOST")}
    fn persist(&mut self,state:State)->Result<()> {
        self.owned()?;let sequence=self.sequence+1;let hash=record_hash(sequence,&self.previous,&state);
        let record=Record{sequence,previous:self.previous.clone(),state:state.clone(),hash:hash.clone()};
        let mut bytes=serde_json::to_vec(&record).unwrap();bytes.push(b'\n');
        need(self.active_bytes.saturating_add(bytes.len() as u64)<=self.maximum_bytes,"EXPLICIT_COMPACTION_REQUIRED")?;
        if self.file.write_all(&bytes).and_then(|_|self.file.sync_all()).is_err(){self.poisoned=true;return Err("DURABILITY_FAILURE".into());}
        self.active_bytes+=bytes.len() as u64;self.state=state;self.sequence=sequence;self.previous=hash;Ok(())
    }
    fn compact_with_hook(&mut self,mut hook:impl FnMut(&str)->Result<()>)->Result<Value>{
        self.owned()?;
        self.file.sync_all().map_err(|_|"COMPACTION_DURABILITY_FAILURE")?;
        let (source_bytes,source_sha256)=source_digest(&self.dir.join(&self.journal_name))?;
        need(source_bytes==self.active_bytes,"SOURCE_JOURNAL_CHANGED")?;
        let anchor=Anchor{sequence:self.sequence,previous:self.previous.clone(),source_journal:self.journal_name.clone(),source_bytes,source_sha256,prior_checkpoint_hash:self.checkpoint_hash.clone()};
        let checkpoint_digest=checkpoint_hash(&anchor,&self.state);
        let checkpoint=Checkpoint{schema_version:1,anchor,state:self.state.clone(),hash:checkpoint_digest.clone()};
        let bytes=serde_json::to_vec(&checkpoint).unwrap();need(bytes.len() as u64<=MAX_CHECKPOINT_BYTES,"CHECKPOINT_TOO_LARGE")?;
        let nonce=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|_|"COMPACTION_NONCE_FAILED")?.as_nanos();
        let checkpoint_name=format!("checkpoint-{}-{nonce}.json",self.sequence);
        let journal_name=format!("queue-{}-{nonce}.jsonl",self.sequence);
        write_new_synced(&self.dir.join(&checkpoint_name),&bytes)?;hook("checkpoint_synced")?;self.owned()?;
        write_new_synced(&self.dir.join(&journal_name),&[])?;hook("journal_synced")?;self.owned()?;
        let manifest=Manifest{schema_version:1,scope:self.state.scope.clone(),journal:journal_name.clone(),checkpoint:checkpoint_name,checkpoint_hash:checkpoint_digest.clone()};
        let temp_name=format!("manifest-{nonce}.tmp");
        write_new_synced(&self.dir.join(&temp_name),&serde_json::to_vec(&manifest).unwrap())?;hook("manifest_synced")?;self.owned()?;
        let guard_path=self.dir.join("account-queue-generation.guard");
        // A first-generation guard without a manifest requires manual recovery.
        // Never permit this process to keep appending after such an interrupted switch.
        self.poisoned=true;
        if !guard_path.exists(){write_new_synced(&guard_path,self.state.scope.as_bytes())?;}else{need(fs::read(&guard_path).ok().as_deref()==Some(self.state.scope.as_bytes()),"GENERATION_GUARD_SCOPE_FAILURE")?;}
        hook("generation_guard_synced")?;need(fs::read(&self.lock_path).ok().as_deref()==Some(&self.identity),"SCOPED_LEASE_LOST")?;
        fs::rename(self.dir.join(&temp_name),self.dir.join("account-queue-manifest.json")).map_err(|_|"MANIFEST_ATOMIC_SWITCH_FAILED")?;
        hook("manifest_switched")?;
        let file=OpenOptions::new().append(true).open(self.dir.join(&journal_name)).map_err(|_|"ACTIVE_JOURNAL_OPEN_FAILED")?;
        self.file=file;self.journal_name=journal_name.clone();self.checkpoint_hash=Some(checkpoint_digest.clone());self.active_bytes=0;self.poisoned=false;self.recovery_compaction_required=false;self.owned()?;
        Ok(json!({"compacted":true,"sequence":self.sequence,"previous":self.previous,"checkpoint_hash":checkpoint_digest,"source_bytes":source_bytes,"active_journal_bytes":0,"immutable_bindings":self.state.intents.len(),"unknown_acceptances":self.state.unknown.len()}))
    }
    fn command(&mut self,c:Command)->Result<Value>{self.owned()?;
        if c.op=="compact"{need(c.scope==self.state.scope,"SCOPE_MISMATCH")?;need(c.now>=self.state.last_clock && c.now<=MAX_TIME,"COMPACTION_CLOCK_INVALID")?;return self.compact_with_hook(|_|Ok(()));}
        need(!self.recovery_compaction_required,"RECOVERY_COMPACTION_REQUIRED")?;
        let mut candidate=self.state.clone();let result=candidate.apply(&c);if result.is_ok() || candidate.manual_review {self.persist(candidate)?;}self.owned()?;result}
}
impl Drop for Store {fn drop(&mut self){if fs::read(&self.lock_path).ok().as_deref()==Some(&self.identity){let _=fs::remove_file(&self.lock_path);}}}
fn run()->Result<()> {
    let args:Vec<String>=std::env::args().collect();need(args.len()==5 && args[1]=="--offline-jsonl","USAGE: --offline-jsonl DIRECTORY SCOPE TOKEN")?;
    let mut store=Store::open(Path::new(&args[2]),&args[3],&args[4])?;
    let stdin=std::io::stdin();let mut stdout=std::io::stdout().lock();
    for line in stdin.lock().lines(){let line=line.map_err(|_|"INPUT_FAILURE")?;
        let response=match serde_json::from_str::<Command>(&line){Ok(c)=>match store.command(c){Ok(v)=>json!({"ok":true,"result":v}),Err(e)=>json!({"ok":false,"error":e})},Err(_)=>json!({"ok":false,"error":"INVALID_COMMAND"})};
        writeln!(stdout,"{}",response).and_then(|_|stdout.flush()).map_err(|_|"OUTPUT_FAILURE")?;
    }Ok(())
}
fn main(){if let Err(e)=run(){eprintln!("{}",e);std::process::exit(1);}}

#[cfg(test)]
mod tests {
    use super::*;
    fn cmd(op:&str,now:u64,write:bool)->Command {Command{op:op.into(),scope:"fixture-scope".into(),now,request:format!("request-{now}"),write,binding:if write{Some(Binding{intent:format!("intent-{now}"),key:format!("key-{now}"),payload_hash:"a".repeat(64)})}else{None},authorized:true,permit_valid:true,lease_valid:true,..Default::default()}}
    fn complete(s:&mut State,c:&Command,now:u64,status:u16,outcome:&str){let mut network=cmd("network",c.now,c.write);network.request=c.request.clone();network.binding=c.binding.clone();s.apply(&network).unwrap();let mut done=cmd("complete",now,c.write);done.request=c.request.clone();done.status=status;done.outcome=outcome.into();s.apply(&done).unwrap();}
    fn fixture()->PathBuf {let p=std::env::temp_dir().join(format!("baal-offline-queue-test-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));fs::create_dir_all(&p).unwrap();p}
    #[test] fn completion_pacing_prevents_durable_delay_burst(){let mut s=State::new("fixture-scope".into());let c=cmd("admit",1000,false);s.apply(&c).unwrap();complete(&mut s,&c,26_000,200,"success");assert_eq!(s.next,27_000);let r=s.apply(&cmd("admit",26_001,false)).unwrap();assert_eq!(r["wait_until"],27_000);assert!(s.active.is_none());}
    #[test] fn writes_have_2500_ms_shared_pacing(){let mut s=State::new("fixture-scope".into());let c=cmd("admit",1000,true);s.apply(&c).unwrap();complete(&mut s,&c,1700,200,"success");assert_eq!(s.next,4200);assert_eq!(s.apply(&cmd("admit",2000,false)).unwrap()["wait_until"],4200);}
    #[test] fn serialized_read_and_write_admissions(){let mut s=State::new("fixture-scope".into());s.apply(&cmd("admit",0,false)).unwrap();assert_eq!(s.apply(&cmd("admit",1,true)).unwrap_err(),"SERIAL_QUEUE_BUSY");}
    #[test] fn every_inclusive_rolling_minute_retains_headroom(){for pattern in 0..4 {let mut s=State::new("fixture-scope".into());let mut now=0;let mut admissions=vec![];for i in 0..1000 {let write=match pattern{0=>false,1=>true,2=>i%2==0,_=>i%100<50};let mut c=cmd("admit",now,write);let mut r=s.apply(&c).unwrap();if r["admitted"]==false{now=r["wait_until"].as_u64().unwrap();c=cmd("admit",now,write);r=s.apply(&c).unwrap();}assert_eq!(r["admitted"],true);admissions.push((now,write));complete(&mut s,&c,now,200,"success");now=s.next;}for &(at,_) in &admissions {let window:Vec<_>=admissions.iter().filter(|(t,_)|*t>=at&&*t<=at+60000).collect();assert!(window.iter().filter(|(_,w)|!w).count()<=60);assert!(window.iter().filter(|(_,w)|*w).count()<=24);}}}
    #[test] fn retry_after_numeric_dates_invalid_and_overflow(){assert_eq!(retry_after("1.25",0),1250);assert_eq!(retry_after(".1",0),0);assert_eq!(retry_after("garbage",0),0);assert_eq!(retry_after("9999999999999999999999999",0),MAX_SAFE);assert_eq!(retry_after("Thu, 01 Jan 1970 00:02:00 GMT",60000),60000);assert_eq!(retry_after("Thu, 01 Jan 1970 00:00:00 GMT",60000),0);}
    #[test] fn persistent_429_and_503_backoff(){for status in [429,503]{let mut s=State::new("fixture-scope".into());let c=cmd("admit",1000,false);s.apply(&c).unwrap();complete(&mut s,&c,2000,status,"rejected");assert_eq!(s.cooldown,2000+if status==429 {60000}else{10000});assert_eq!(s.apply(&cmd("admit",3000,false)).unwrap()["wait_until"],s.cooldown);}}
    #[test] fn overflow_retry_requires_permanent_manual_review(){let mut s=State::new("fixture-scope".into());let c=cmd("admit",0,false);s.apply(&c).unwrap();let mut n=cmd("network",0,false);n.request=c.request.clone();s.apply(&n).unwrap();let mut d=cmd("complete",1,false);d.request=c.request.clone();d.outcome="rejected".into();d.status=429;d.retry_after="99999999999999999999".into();s.apply(&d).unwrap();assert!(s.manual_review);assert_eq!(s.apply(&cmd("admit",70000,false)).unwrap_err(),"MANUAL_REVIEW_REQUIRED");}
    #[test] fn clock_regression_is_durably_poisoned(){let dir=fixture();{let mut st=Store::open(&dir,"fixture-scope","token").unwrap();st.command(cmd("status",100,false)).unwrap();assert_eq!(st.command(cmd("status",99,false)).unwrap_err(),"CLOCK_REGRESSION");}let mut st=Store::open(&dir,"fixture-scope","token2").unwrap();assert!(st.state.manual_review);assert_eq!(st.command(cmd("admit",101,false)).unwrap_err(),"MANUAL_REVIEW_REQUIRED");drop(st);fs::remove_dir_all(dir).unwrap();}
    #[test] fn lease_loss_and_scope_mismatch_fail_closed(){let dir=fixture();let mut st=Store::open(&dir,"fixture-scope","token").unwrap();let mut c=cmd("admit",1,false);c.scope="different".into();assert_eq!(st.command(c).unwrap_err(),"SCOPE_MISMATCH");fs::write(&st.lock_path,b"replacement").unwrap();assert_eq!(st.command(cmd("admit",1,false)).unwrap_err(),"SCOPED_LEASE_LOST");drop(st);assert_eq!(fs::read(dir.join("offline-account-queue.lock")).unwrap(),b"replacement");fs::remove_dir_all(dir).unwrap();}
    #[test] fn overlapping_process_lock_is_not_a_ttl(){let dir=fixture();let st=Store::open(&dir,"fixture-scope","token").unwrap();assert!(Store::open(&dir,"fixture-scope","other").is_err());drop(st);let st=Store::open(&dir,"fixture-scope","new").unwrap();drop(st);fs::remove_dir_all(dir).unwrap();}
    #[test] fn network_fence_rechecks_authority_and_payload(){let mut s=State::new("fixture-scope".into());let c=cmd("admit",1,true);s.apply(&c).unwrap();let mut n=cmd("network",2,true);n.request=c.request.clone();n.binding=c.binding.clone();n.authorized=false;assert_eq!(s.apply(&n).unwrap_err(),"NETWORK_WRITE_FENCE_REQUIRED");n.authorized=true;n.binding.as_mut().unwrap().payload_hash="b".repeat(64);assert!(s.apply(&n).is_err());n.binding=c.binding.clone();s.apply(&n).unwrap();assert_eq!(s.apply(&n).unwrap_err(),"DUPLICATE_NETWORK_DISPATCH");}
    #[test] fn unknown_acceptance_blocks_replacements_until_exact_reconciliation(){let mut s=State::new("fixture-scope".into());let c=cmd("admit",1,true);s.apply(&c).unwrap();complete(&mut s,&c,2,0,"unknown");assert_eq!(s.apply(&cmd("admit",3000,true)).unwrap_err(),"UNKNOWN_ACCEPTANCE_RECONCILE_FIRST");let mut r=cmd("reconcile",3001,true);r.binding=c.binding.clone();r.reconciled=true;assert_eq!(s.apply(&r).unwrap()["retry_permitted"],false);let mut retry=cmd("admit",6000,true);retry.binding=c.binding.clone();assert_eq!(s.apply(&retry).unwrap_err(),"KNOWN_INTENT_RECONCILE_FIRST");}
    #[test] fn crash_restoration_preserves_uncertain_write_and_reservations(){let dir=fixture();let c=cmd("admit",1000,true);{let mut st=Store::open(&dir,"fixture-scope","token").unwrap();st.command(c).unwrap();}let mut st=Store::open(&dir,"fixture-scope","token2").unwrap();assert!(st.state.active.is_none());assert_eq!(st.state.unknown.len(),1);assert_eq!(st.state.writes,vec![1000]);assert_eq!(st.command(cmd("admit",5000,true)).unwrap_err(),"UNKNOWN_ACCEPTANCE_RECONCILE_FIRST");drop(st);fs::remove_dir_all(dir).unwrap();}
    #[test] fn torn_or_tampered_journal_never_resumes(){for torn in [false,true]{let dir=fixture();{let mut st=Store::open(&dir,"fixture-scope","token").unwrap();st.command(cmd("status",100,false)).unwrap();}let p=dir.join("account-queue.jsonl");if torn{let mut f=OpenOptions::new().append(true).open(&p).unwrap();f.write_all(b"{\"sequence\":").unwrap();}else{let x=fs::read_to_string(&p).unwrap().replace("fixture-scope","wrong-scope");fs::write(&p,x).unwrap();}assert!(Store::open(&dir,"fixture-scope","token2").is_err());fs::remove_dir_all(dir).unwrap();}}
    #[test] fn changed_intent_and_reused_key_are_rejected(){let mut s=State::new("fixture-scope".into());let c=cmd("admit",1,true);s.apply(&c).unwrap();complete(&mut s,&c,2,200,"success");let mut n=cmd("admit",3000,true);n.binding=c.binding.clone();n.binding.as_mut().unwrap().payload_hash="b".repeat(64);assert_eq!(s.apply(&n).unwrap_err(),"LOGICAL_REQUEST_CHANGED");n.binding=Some(Binding{intent:"new".into(),key:c.binding.unwrap().key,payload_hash:"b".repeat(64)});assert_eq!(s.apply(&n).unwrap_err(),"IDEMPOTENCY_PAYLOAD_CHANGED");}
    #[test] fn missing_final_newline_is_not_implicitly_repaired(){let dir=fixture();{let mut st=Store::open(&dir,"fixture-scope","token").unwrap();st.command(cmd("status",100,false)).unwrap();}let path=dir.join("account-queue.jsonl");let mut bytes=fs::read(&path).unwrap();bytes.pop();fs::write(&path,bytes).unwrap();assert!(Store::open(&dir,"fixture-scope","new").is_err());fs::remove_dir_all(dir).unwrap();}
    #[test] fn crashed_network_read_keeps_remote_timeout_headroom(){let mut s=State::new("fixture-scope".into());let c=cmd("admit",1000,false);s.apply(&c).unwrap();let mut n=cmd("network",1100,false);n.request=c.request;s.apply(&n).unwrap();s.recover();assert_eq!(s.next,22100);assert!(s.active.is_none());}
    #[test] fn request_in_flight_extends_shared_cooldown(){let mut s=State::new("fixture-scope".into());let c=cmd("admit",1000,false);s.apply(&c).unwrap();let mut n=cmd("network",1000,false);n.request=c.request.clone();s.apply(&n).unwrap();let mut d=cmd("complete",1100,false);d.request=c.request;d.status=409;d.outcome="rejected".into();d.api_code="REQUEST_IN_FLIGHT".into();s.apply(&d).unwrap();assert_eq!(s.next,91100);}
    #[test] fn compaction_retains_every_binding_unknown_and_source_anchor(){let dir=fixture();let source;let state;let anchor_sequence;let anchor_hash;{
        let mut st=Store::open(&dir,"fixture-scope","token").unwrap();let c=cmd("admit",1000,true);st.command(c).unwrap();
        // Recover the crash-conservative write state before compaction.
        let mut recovered=st.state.clone();recovered.recover();st.persist(recovered).unwrap();
        state=st.state.clone();source=source_digest(&dir.join("account-queue.jsonl")).unwrap();anchor_sequence=st.sequence;anchor_hash=st.previous.clone();
        let r=st.command(cmd("compact",1000,false)).unwrap();assert_eq!(r["immutable_bindings"],1);assert_eq!(r["unknown_acceptances"],1);assert_eq!(st.active_bytes,0);
        let manifest:Manifest=serde_json::from_slice(&fs::read(dir.join("account-queue-manifest.json")).unwrap()).unwrap();let ck:Checkpoint=serde_json::from_slice(&fs::read(dir.join(manifest.checkpoint)).unwrap()).unwrap();assert_eq!((ck.anchor.source_bytes,ck.anchor.source_sha256),source);assert_eq!(ck.anchor.sequence,anchor_sequence);assert_eq!(ck.anchor.previous,anchor_hash);
    }let mut st=Store::open(&dir,"fixture-scope","new").unwrap();assert_eq!(st.state,state);assert_eq!(st.sequence,anchor_sequence);assert_eq!(st.previous,anchor_hash);assert_eq!(st.command(cmd("admit",5000,true)).unwrap_err(),"UNKNOWN_ACCEPTANCE_RECONCILE_FIRST");assert_eq!(source_digest(&dir.join("account-queue.jsonl")).unwrap(),source);drop(st);fs::remove_dir_all(dir).unwrap();}
    #[test] fn compaction_faults_select_only_old_or_fully_written_new_generation(){for stage in ["checkpoint_synced","journal_synced","manifest_synced","manifest_switched"]{let dir=fixture();let expected;let switched;{
        let mut st=Store::open(&dir,"fixture-scope","token").unwrap();st.command(cmd("status",100,false)).unwrap();expected=st.state.clone();
        let err=st.compact_with_hook(|point|if point==stage{Err("INJECTED_PROCESS_CRASH".into())}else{Ok(())}).unwrap_err();assert_eq!(err,"INJECTED_PROCESS_CRASH");switched=stage=="manifest_switched";assert_eq!(st.poisoned,switched);
    }let st=Store::open(&dir,"fixture-scope","new").unwrap();assert_eq!(st.state,expected);assert_eq!(st.journal_name!="account-queue.jsonl",switched);drop(st);fs::remove_dir_all(dir).unwrap();}}
    #[test] fn repeated_compaction_keeps_logical_chain_continuity(){let dir=fixture();let sequence;let previous;{let mut st=Store::open(&dir,"fixture-scope","token").unwrap();st.command(cmd("status",1,false)).unwrap();st.command(cmd("compact",1,false)).unwrap();st.command(cmd("status",2,false)).unwrap();sequence=st.sequence;previous=st.previous.clone();let prior=st.checkpoint_hash.clone();st.command(cmd("compact",2,false)).unwrap();let manifest:Manifest=serde_json::from_slice(&fs::read(dir.join("account-queue-manifest.json")).unwrap()).unwrap();let ck:Checkpoint=serde_json::from_slice(&fs::read(dir.join(manifest.checkpoint)).unwrap()).unwrap();assert_eq!(ck.anchor.prior_checkpoint_hash,prior);assert_eq!(st.sequence,sequence);assert_eq!(st.previous,previous);}let st=Store::open(&dir,"fixture-scope","new").unwrap();assert_eq!(st.sequence,sequence);assert_eq!(st.previous,previous);drop(st);fs::remove_dir_all(dir).unwrap();}
    #[test] fn tampered_checkpoint_and_missing_active_journal_fail_closed(){for missing in [false,true]{let dir=fixture();{let mut st=Store::open(&dir,"fixture-scope","token").unwrap();st.command(cmd("status",100,false)).unwrap();st.command(cmd("compact",100,false)).unwrap();}let manifest:Manifest=serde_json::from_slice(&fs::read(dir.join("account-queue-manifest.json")).unwrap()).unwrap();if missing{fs::remove_file(dir.join(manifest.journal)).unwrap();}else{let p=dir.join(manifest.checkpoint);let raw=fs::read_to_string(&p).unwrap().replace("\"last_clock\":100","\"last_clock\":99");fs::write(p,raw).unwrap();}assert!(Store::open(&dir,"fixture-scope","new").is_err());fs::remove_dir_all(dir).unwrap();}}
    #[test] fn active_tail_cap_requires_explicit_compaction_without_losing_admission(){let dir=fixture();let mut st=Store::open(&dir,"fixture-scope","token").unwrap();st.command(cmd("status",1,false)).unwrap();st.maximum_bytes=st.active_bytes+10;let previous=st.state.clone();assert_eq!(st.command(cmd("admit",2000,true)).unwrap_err(),"EXPLICIT_COMPACTION_REQUIRED");assert_eq!(st.state,previous);st.command(cmd("compact",2000,false)).unwrap();st.maximum_bytes=MAX_ACTIVE_JOURNAL_BYTES;assert_eq!(st.command(cmd("admit",2000,true)).unwrap()["admitted"],true);drop(st);fs::remove_dir_all(dir).unwrap();}
    #[test] fn lease_loss_during_compaction_does_not_switch_manifest(){let dir=fixture();let mut st=Store::open(&dir,"fixture-scope","token").unwrap();st.command(cmd("status",1,false)).unwrap();let lock=st.lock_path.clone();assert_eq!(st.compact_with_hook(|point|{if point=="checkpoint_synced"{fs::write(&lock,b"replacement").unwrap();}Ok(())}).unwrap_err(),"SCOPED_LEASE_LOST");assert!(!dir.join("account-queue-manifest.json").exists());drop(st);assert_eq!(fs::read(&lock).unwrap(),b"replacement");fs::remove_dir_all(dir).unwrap();}
    #[test] fn manifest_cannot_escape_directory(){let dir=fixture();{let mut st=Store::open(&dir,"fixture-scope","token").unwrap();st.command(cmd("compact",0,false)).unwrap();}let p=dir.join("account-queue-manifest.json");let mut m:Manifest=serde_json::from_slice(&fs::read(&p).unwrap()).unwrap();m.journal="../outside.jsonl".into();fs::write(p,serde_json::to_vec(&m).unwrap()).unwrap();assert!(Store::open(&dir,"fixture-scope","new").is_err());fs::remove_dir_all(dir).unwrap();}
    #[test] fn compaction_with_active_write_keeps_crash_uncertainty(){let dir=fixture();{let mut st=Store::open(&dir,"fixture-scope","token").unwrap();st.command(cmd("admit",1000,true)).unwrap();st.command(cmd("compact",1000,false)).unwrap();assert!(st.state.active.is_some());}let st=Store::open(&dir,"fixture-scope","new").unwrap();assert_eq!(st.state.unknown.len(),1);assert_eq!(st.state.intents.len(),1);assert_eq!(st.state.keys.len(),1);assert_eq!(st.state.writes,vec![1000]);assert_eq!(st.state.next,3500);drop(st);fs::remove_dir_all(dir).unwrap();}
    #[test] fn missing_manifest_never_falls_back_to_older_intent_history(){let dir=fixture();{let mut st=Store::open(&dir,"fixture-scope","token").unwrap();st.command(cmd("status",100,false)).unwrap();st.command(cmd("compact",100,false)).unwrap();st.command(cmd("admit",1000,true)).unwrap();}fs::remove_file(dir.join("account-queue-manifest.json")).unwrap();assert!(Store::open(&dir,"fixture-scope","new").is_err());fs::remove_dir_all(dir).unwrap();}
    #[test] fn crash_after_first_generation_guard_fails_closed_without_erasing_sources(){let dir=fixture();{let mut st=Store::open(&dir,"fixture-scope","token").unwrap();st.command(cmd("admit",1000,true)).unwrap();assert_eq!(st.compact_with_hook(|stage|if stage=="generation_guard_synced"{Err("INJECTED_PROCESS_CRASH".into())}else{Ok(())}).unwrap_err(),"INJECTED_PROCESS_CRASH");}assert!(dir.join("account-queue.jsonl").exists());assert!(Store::open(&dir,"fixture-scope","new").is_err());fs::remove_dir_all(dir).unwrap();}
    #[test] fn restoration_after_compaction_ignores_archived_record_count(){let dir=fixture();let archive_bytes;{let mut st=Store::open(&dir,"fixture-scope","token").unwrap();for now in 1..=300{st.command(cmd("status",now,false)).unwrap();}archive_bytes=st.active_bytes;st.command(cmd("compact",300,false)).unwrap();}let st=Store::open(&dir,"fixture-scope","new").unwrap();assert_eq!(st.sequence,300);assert_eq!(st.active_bytes,0);assert_eq!(fs::metadata(dir.join("account-queue.jsonl")).unwrap().len(),archive_bytes);assert_eq!(st.state.last_clock,300);drop(st);fs::remove_dir_all(dir).unwrap();}
    #[test] fn restart_at_capacity_requires_explicit_preserving_recovery_compaction(){let dir=fixture();let cap;{let mut st=Store::open(&dir,"fixture-scope","token").unwrap();st.command(cmd("admit",1000,true)).unwrap();cap=st.active_bytes;}let mut st=Store::open_with_limit(&dir,"fixture-scope","new",cap).unwrap();assert!(st.recovery_compaction_required);assert_eq!(st.state.unknown.len(),1);assert_eq!(st.command(cmd("admit",5000,true)).unwrap_err(),"RECOVERY_COMPACTION_REQUIRED");st.command(cmd("compact",1000,false)).unwrap();assert!(!st.recovery_compaction_required);assert_eq!(st.state.unknown.len(),1);drop(st);let st=Store::open(&dir,"fixture-scope","third").unwrap();assert_eq!(st.state.unknown.len(),1);assert_eq!(st.state.intents.len(),1);assert_eq!(st.state.writes,vec![1000]);drop(st);fs::remove_dir_all(dir).unwrap();}
}
