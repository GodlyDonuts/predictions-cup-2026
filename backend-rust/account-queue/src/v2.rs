//! Isolated v2 prototype. Selection and migration are explicit offline operations.
//! The existing v1 Store and SDK remain unchanged.
use super::*;

const MAX_ROW_BYTES: u64 = 64 * 1024;
const MAX_POISON_BYTES:u64 = 4096;
const MAX_V2_CHECKPOINT_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_BINDINGS: usize = 2_000_000;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Operational {
    schema_version:u32, scope:String, last_clock:u64, next:u64, cooldown:u64,
    manual_review:bool, reads:Vec<u64>, writes:Vec<u64>, active:Option<Flight>,
    read_profile:Option<ReadProfile>,
}
impl Operational {
    fn capture(s:&State)->Self {Self{schema_version:s.schema_version,scope:s.scope.clone(),last_clock:s.last_clock,next:s.next,cooldown:s.cooldown,manual_review:s.manual_review,reads:s.reads.clone(),writes:s.writes.clone(),active:s.active.clone(),read_profile:s.read_profile.clone()}}
    fn install(&self,s:&mut State){s.schema_version=self.schema_version;s.scope=self.scope.clone();s.last_clock=self.last_clock;s.next=self.next;s.cooldown=self.cooldown;s.manual_review=self.manual_review;s.reads=self.reads.clone();s.writes=self.writes.clone();s.active=self.active.clone();s.read_profile=self.read_profile.clone();}
    fn empty_state(&self)->State{let mut s=State::new(self.scope.clone());self.install(&mut s);s}
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Delta {
    operational:Operational, bindings:Vec<Binding>, unknown_put:Vec<Binding>, unknown_remove:Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag="kind", content="command", rename_all="snake_case", deny_unknown_fields)]
enum Transition { Command(Command), Recovery }
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeltaRecord {schema_version:u32,sequence:u64,previous:String,transition:Transition,delta:Delta,hash:String}
fn delta_hash(sequence:u64,previous:&str,t:&Transition,d:&Delta)->String{hash(&(2u32,sequence,previous,t,d))}

// Manual review must remain persistable even when an otherwise legal input
// expands past the delta-row cap. Resolve scope/active identity from the prior
// durable state rather than repeating padded command fields or full State.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag="cause",rename_all="snake_case",deny_unknown_fields)]
enum PoisonWitness {
    ClockRegression{now:u64},
    CompletionOverflow{now:u64,status:u16,outcome:String,request_in_flight:bool},
    UnrepresentableCommand,
}
#[derive(Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
struct PoisonRecord {schema_version:u32,sequence:u64,previous:String,poison:PoisonWitness,command_sha256:String,hash:String}
#[derive(Serialize,Deserialize)]
#[serde(untagged)]
enum StoredRecord {Delta(DeltaRecord),Poison(PoisonRecord)}
fn poison_hash(sequence:u64,previous:&str,w:&PoisonWitness,command_sha256:&str)->String{hash(&(2u32,sequence,previous,"manual_review",w,command_sha256))}
fn prepare_poison(s:&State,w:&PoisonWitness)->Result<Delta>{
    need(!s.manual_review,"V2_POISON_ALREADY_DURABLE")?;
    let c=match w{
        PoisonWitness::ClockRegression{now}=>Command{op:"status".into(),scope:s.scope.clone(),now:*now,..Default::default()},
        PoisonWitness::CompletionOverflow{now,status,outcome,request_in_flight}=>{
            need(matches!(status,429|503),"V2_POISON_WITNESS")?;
            let request=s.active.as_ref().ok_or("V2_POISON_ACTIVE_REQUIRED")?.request.clone();
            // Every Retry-After overflow takes the same manual-review branch;
            // the raw header's padding has no subsequent economic effect.
            Command{op:"complete".into(),scope:s.scope.clone(),now:*now,request,status:*status,outcome:outcome.clone(),retry_after:"99999999999999999999999".into(),api_code:if *request_in_flight{"REQUEST_IN_FLIGHT".into()}else{String::new()},..Default::default()}
        },
        PoisonWitness::UnrepresentableCommand=>{
            let mut operational=Operational::capture(s);operational.manual_review=true;
            return Ok(Delta{operational,bindings:vec![],unknown_put:vec![],unknown_remove:vec![]});
        },
    };
    let (delta,_)=prepare(s,&Transition::Command(c));
    let delta=delta.ok_or("V2_POISON_WITNESS")?;
    need(delta.operational.manual_review,"V2_POISON_WITNESS")?;Ok(delta)
}
fn manual_review_witness(s:&State,c:&Command,d:&Delta)->Result<PoisonWitness>{
    let witness=if c.scope==s.scope && c.now<=MAX_TIME && c.now<s.last_clock{
        PoisonWitness::ClockRegression{now:c.now}
    }else if c.op=="complete" && matches!(c.status,429|503){
        PoisonWitness::CompletionOverflow{now:c.now,status:c.status,outcome:c.outcome.clone(),request_in_flight:c.api_code=="REQUEST_IN_FLIGHT"}
    }else{return Err("V2_POISON_WITNESS".into());};
    need(prepare_poison(s,&witness)?==*d,"V2_POISON_ORACLE_MISMATCH")?;Ok(witness)
}

// A sparse State is an exact projection for State::apply's current access set.
// Unknown has a global-empty gate, so include a sentinel actual binding even
// when it differs from the requested binding. Never use an empty projection
// to answer that gate. The differential tests guard additions to State::apply.
fn projection(s:&State,c:Option<&Command>)->State{
    let mut p=Operational::capture(s).empty_state();
    if let Some(b)=c.and_then(|c|c.binding.as_ref()){
        if let Some(v)=s.intents.get(&b.intent){p.intents.insert(b.intent.clone(),v.clone());}
        if let Some(v)=s.keys.get(&b.key){p.keys.insert(b.key.clone(),v.clone());}
        if let Some(v)=s.unknown.get(&b.intent){p.unknown.insert(b.intent.clone(),v.clone());}
    }
    if let Some((k,v))=s.unknown.first_key_value(){p.unknown.insert(k.clone(),v.clone());}
    p
}
fn changes(before:&State,after:&State)->Delta{
    Delta{operational:Operational::capture(after),
        bindings:after.intents.iter().filter(|(k,v)|before.intents.get(*k)!=Some(*v)).map(|(_,v)|v.clone()).collect(),
        unknown_put:after.unknown.iter().filter(|(k,v)|before.unknown.get(*k)!=Some(*v)).map(|(_,v)|v.clone()).collect(),
        unknown_remove:before.unknown.keys().filter(|k|!after.unknown.contains_key(*k)).cloned().collect()}
}
fn prepare(s:&State,t:&Transition)->(Option<Delta>,Result<Value>){
    let mut p=projection(s,match t{Transition::Command(c)=>Some(c),_=>None});
    let before=p.clone(); // At most two unknowns and one referenced intent/key.
    match t {
        Transition::Command(c)=>{
            let mut c=c.clone();if matches!(c.op.as_str(),"summary"|"lookup_intent"){c.op="status".into();}
            let result=p.apply(&c);
            let delta=if result.is_ok() || p.manual_review {Some(changes(&before,&p))}else{None};
            (delta,result)
        },
        Transition::Recovery=>{p.recover();let delta=if p!=before{Some(changes(&before,&p))}else{None};(delta,Ok(json!({"recovered":true})))}
    }
}
fn validate_delta(s:&State,d:&Delta)->Result<()>{
    need(d.operational.scope==s.scope && d.operational.schema_version==1,"V2_DELTA_SCOPE")?;
    need(d.bindings.len()<=1 && d.unknown_put.len()<=1 && d.unknown_remove.len()<=1,"V2_DELTA_CARDINALITY")?;
    need(s.intents.len().saturating_add(d.bindings.len())<=MAX_BINDINGS,"V2_BINDING_LIMIT")?;
    for b in &d.bindings{need(!s.intents.contains_key(&b.intent) && !s.keys.contains_key(&b.key),"V2_BINDING_REPLACEMENT")?;}
    for b in &d.unknown_put{need(s.intents.get(&b.intent)==Some(b)||d.bindings.contains(b),"V2_UNKNOWN_UNBOUND")?;}
    for k in &d.unknown_remove{need(s.unknown.contains_key(k),"V2_UNKNOWN_REMOVE_MISSING")?;}
    Ok(())
}
fn install(s:&mut State,d:&Delta){
    d.operational.install(s);
    for b in &d.bindings{s.intents.insert(b.intent.clone(),b.clone());s.keys.insert(b.key.clone(),b.clone());}
    for b in &d.unknown_put{s.unknown.insert(b.intent.clone(),b.clone());}
    for k in &d.unknown_remove{s.unknown.remove(k);}
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(super) struct MigrationProof {
    schema_version:u32,scope:String,sequence:u64,previous:String,state_sha256:String,
    source_journal:String,source_bytes:u64,source_sha256:String,checkpoint_hash:Option<String>,manifest_sha256:Option<String>,
}
impl MigrationProof {
    pub(super) fn capture(s:&Store)->Result<Self>{
        s.owned()?;let (source_bytes,source_sha256)=source_digest(&s.dir.join(&s.journal_name))?;
        need(source_bytes==s.active_bytes,"SOURCE_JOURNAL_CHANGED")?;
        let manifest_path=s.dir.join("account-queue-manifest.json");
        let manifest_sha256=if manifest_path.exists(){
            need(fs::metadata(&manifest_path).map_err(|_|"MANIFEST_UNAVAILABLE")?.len()<=4096,"MANIFEST_TOO_LARGE")?;
            let bytes=fs::read(&manifest_path).map_err(|_|"MANIFEST_UNAVAILABLE")?;
            let m:Manifest=serde_json::from_slice(&bytes).map_err(|_|"MANIFEST_INVALID")?;
            need(m.schema_version==1 && m.scope==s.state.scope && m.journal==s.journal_name && Some(&m.checkpoint_hash)==s.checkpoint_hash.as_ref(),"V2_MIGRATION_MANIFEST_CHANGED")?;
            Some(format!("{:x}",Sha256::digest(&bytes)))
        }else{need(s.checkpoint_hash.is_none(),"GENERATION_MANIFEST_REQUIRED")?;None};
        Ok(Self{schema_version:1,scope:s.state.scope.clone(),sequence:s.sequence,previous:s.previous.clone(),state_sha256:hash(&s.state),source_journal:s.journal_name.clone(),source_bytes,source_sha256,checkpoint_hash:s.checkpoint_hash.clone(),manifest_sha256})
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {schema_version:u32,anchor:Anchor,operational:Operational,migration:MigrationProof,bindings:usize,unknowns:usize}
#[derive(Serialize, Deserialize)]
#[serde(tag="kind", content="value",rename_all="snake_case",deny_unknown_fields)]
enum Entry {Header(Header),Binding(Binding),Unknown(Binding),Footer{bindings:usize,unknowns:usize}}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CheckpointRow {index:u64,previous:String,entry:Entry,hash:String}
fn row_hash(index:u64,previous:&str,entry:&Entry)->String{hash(&(2u32,index,previous,entry))}

fn manifest_bytes(dir:&Path)->Result<Option<Vec<u8>>>{
    let file=match File::open(dir.join("account-queue-manifest.json")){
        Ok(file)=>file,Err(e) if e.kind()==std::io::ErrorKind::NotFound=>return Ok(None),Err(_)=>return Err("MANIFEST_UNAVAILABLE".into())};
    let mut bytes=Vec::new();file.take(4097).read_to_end(&mut bytes).map_err(|_|"MANIFEST_UNAVAILABLE")?;
    need(bytes.len()<=4096,"MANIFEST_TOO_LARGE")?;Ok(Some(bytes))
}
fn selected_manifest(dir:&Path,version:u32,scope:&str,journal:&str,checkpoint:Option<&str>)->Result<Option<Vec<u8>>>{
    let bytes=manifest_bytes(dir)?;
    if let Some(bytes)=&bytes{
        let m:Manifest=serde_json::from_slice(bytes).map_err(|_|"MANIFEST_INVALID")?;
        need(m.schema_version==version && m.scope==scope && m.journal==journal && Some(m.checkpoint_hash.as_str())==checkpoint,"V2_MANIFEST_CHANGED")?;
    }else{need(version==1 && checkpoint.is_none(),"GENERATION_MANIFEST_REQUIRED")?;}
    Ok(bytes)
}

fn pin_directory(dir:&Path)->Result<()>{
    #[cfg(unix)] {File::open(dir).and_then(|f|f.sync_all()).map_err(|_|"V2_DIRECTORY_DURABILITY_FAILURE".into())}
    // No pretend durability on unsupported platforms. Windows fixture tests
    // exercise staging/semantics; durable migration requires Unix for now.
    #[cfg(not(unix))] {let _=dir;Err("V2_DIRECTORY_FSYNC_UNSUPPORTED".into())}
}
fn bounded_line(reader:&mut impl BufRead)->Result<Option<Vec<u8>>>{
    let mut bytes=Vec::new();
    let n=reader.take(MAX_ROW_BYTES+1).read_until(b'\n',&mut bytes).map_err(|_|"V2_READ_FAILED")?;
    if n==0{return Ok(None);}
    need(n as u64<=MAX_ROW_BYTES,"V2_ROW_TOO_LARGE")?;
    need(bytes.last()==Some(&b'\n'),"UNTERMINATED_JOURNAL_RECORD")?;Ok(Some(bytes))
}
fn validate_maps(s:&State)->Result<()>{
    need(s.intents.len()==s.keys.len() && s.intents.len()<=MAX_BINDINGS,"V2_BINDING_MAP_SHAPE")?;
    for (k,b) in &s.intents{need(k==&b.intent && s.keys.get(&b.key)==Some(b),"V2_BINDING_MAP_SHAPE")?;}
    for (k,b) in &s.unknown{need(k==&b.intent && s.intents.get(k)==Some(b),"V2_UNKNOWN_UNBOUND")?;}
    if let Some(f)=&s.active{if f.write{let b=f.binding.as_ref().ok_or("V2_ACTIVE_UNBOUND")?;need(s.intents.get(&b.intent)==Some(b),"V2_ACTIVE_UNBOUND")?;}}
    Ok(())
}
// Streaming rows avoid a full serialized State allocation and avoid repeating
// the key map on disk. Every key is reconstructed and uniqueness-checked.
fn write_checkpoint(path:&Path,state:&State,header:Header)->Result<String>{
    validate_maps(state)?;
    let mut file=OpenOptions::new().write(true).create_new(true).open(path).map_err(|_|"COMPACTION_FILE_CREATE_FAILED")?;
    let mut index=0;let mut previous=String::new();let mut total=0u64;
    let mut write=|entry:Entry|->Result<()>{
        index+=1;let digest=row_hash(index,&previous,&entry);
        let row=CheckpointRow{index,previous:previous.clone(),entry,hash:digest.clone()};
        let mut bytes=serde_json::to_vec(&row).map_err(|_|"V2_SERIALIZATION_FAILED")?;bytes.push(b'\n');
        need(bytes.len() as u64<=MAX_ROW_BYTES,"V2_ROW_TOO_LARGE")?;
        total=total.checked_add(bytes.len() as u64).ok_or("V2_CHECKPOINT_TOO_LARGE")?;
        need(total<=MAX_V2_CHECKPOINT_BYTES,"V2_CHECKPOINT_TOO_LARGE")?;
        file.write_all(&bytes).map_err(|_|"COMPACTION_DURABILITY_FAILURE")?;previous=digest;Ok(())
    };
    write(Entry::Header(header))?;
    for b in state.intents.values(){write(Entry::Binding(b.clone()))?;}
    for b in state.unknown.values(){write(Entry::Unknown(b.clone()))?;}
    write(Entry::Footer{bindings:state.intents.len(),unknowns:state.unknown.len()})?;
    drop(write);file.sync_all().map_err(|_|"COMPACTION_DURABILITY_FAILURE")?;Ok(previous)
}
fn read_checkpoint(path:&Path,scope:&str,expected:&str)->Result<(State,Header)>{
    need(fs::metadata(path).map_err(|_|"CHECKPOINT_UNAVAILABLE")?.len()<=MAX_V2_CHECKPOINT_BYTES,"V2_CHECKPOINT_TOO_LARGE")?;
    let mut reader=BufReader::new(File::open(path).map_err(|_|"CHECKPOINT_UNAVAILABLE")?);
    let mut index=0;let mut previous=String::new();let mut header=None;let mut state=None;let mut footer=false;let mut unknown_phase=false;
    while let Some(bytes)=bounded_line(&mut reader)?{
        need(!footer,"V2_AFTER_CHECKPOINT_FOOTER")?;
        let row:CheckpointRow=serde_json::from_slice(&bytes).map_err(|_|"CHECKPOINT_INVALID")?;
        index+=1;need(row.index==index && row.previous==previous && row.hash==row_hash(index,&previous,&row.entry),"CHECKPOINT_INTEGRITY_OR_SCOPE_FAILURE")?;
        previous=row.hash;
        match row.entry{
            Entry::Header(h)=>{need(index==1 && h.schema_version==2 && h.operational.schema_version==1 && h.operational.scope==scope && h.migration.schema_version==1 && h.migration.scope==scope && h.anchor.sequence>=h.migration.sequence && (h.anchor.sequence!=h.migration.sequence || h.anchor.previous==h.migration.previous) && h.bindings<=MAX_BINDINGS && h.unknowns<=h.bindings && local_name(&h.anchor.source_journal) && local_name(&h.migration.source_journal),"V2_CHECKPOINT_HEADER")?;state=Some(h.operational.empty_state());header=Some(h);},
            Entry::Binding(b)=>{let s=state.as_mut().ok_or("V2_CHECKPOINT_HEADER_REQUIRED")?;need(!unknown_phase && s.intents.len()<MAX_BINDINGS && !s.intents.contains_key(&b.intent) && !s.keys.contains_key(&b.key),"V2_CHECKPOINT_BINDING_DUPLICATE")?;s.keys.insert(b.key.clone(),b.clone());s.intents.insert(b.intent.clone(),b);},
            Entry::Unknown(b)=>{unknown_phase=true;let s=state.as_mut().ok_or("V2_CHECKPOINT_HEADER_REQUIRED")?;need(s.intents.get(&b.intent)==Some(&b) && !s.unknown.contains_key(&b.intent),"V2_CHECKPOINT_UNKNOWN")?;s.unknown.insert(b.intent.clone(),b);},
            Entry::Footer{bindings,unknowns}=>{let s=state.as_ref().ok_or("V2_CHECKPOINT_HEADER_REQUIRED")?;let h=header.as_ref().unwrap();need(bindings==s.intents.len() && unknowns==s.unknown.len() && bindings==h.bindings && unknowns==h.unknowns,"V2_CHECKPOINT_COUNTS")?;footer=true;},
        }
    }
    need(footer && previous==expected,"V2_CHECKPOINT_FOOTER_OR_HASH")?;
    let state=state.ok_or("V2_CHECKPOINT_HEADER_REQUIRED")?;validate_maps(&state)?;Ok((state,header.unwrap()))
}

pub(super) struct V2Store {
    file:File,state:State,sequence:u64,previous:String,lock_path:PathBuf,identity:Vec<u8>,poisoned:bool,
    dir:PathBuf,journal_name:String,checkpoint_hash:String,migration:MigrationProof,active_bytes:u64,maximum_bytes:u64,
}
impl V2Store {
    fn owned(&self)->Result<()>{need(!self.poisoned,"DURABILITY_FAILURE")?;need(fs::read(&self.lock_path).ok().as_deref()==Some(&self.identity),"SCOPED_LEASE_LOST")}
    pub(super) fn open(dir:&Path,scope:&str,token:&str)->Result<Self>{
        need(!scope.is_empty() && !token.is_empty(),"SCOPE_TOKEN_REQUIRED")?;
        let lock_path=dir.join("offline-account-queue.lock");
        let identity=serde_json::to_vec(&json!({"scope":scope,"token":token,"pid":std::process::id()})).unwrap();
        let mut lock=OpenOptions::new().write(true).create_new(true).open(&lock_path).map_err(|_|"EXCLUSIVE_QUEUE_LOCK_REQUIRED")?;
        if lock.write_all(&identity).and_then(|_|lock.sync_all()).is_err(){let _=fs::remove_file(&lock_path);return Err("LOCK_DURABILITY_FAILURE".into());}
        let load=||->Result<Self>{
            pin_directory(dir)?;
            need(fs::read(dir.join("account-queue-generation.guard")).ok().as_deref()==Some(scope.as_bytes()),"GENERATION_GUARD_SCOPE_FAILURE")?;
            let manifest_path=dir.join("account-queue-manifest.json");
            need(fs::metadata(&manifest_path).map_err(|_|"GENERATION_MANIFEST_REQUIRED")?.len()<=4096,"MANIFEST_TOO_LARGE")?;
            let m:Manifest=serde_json::from_slice(&fs::read(&manifest_path).map_err(|_|"MANIFEST_UNAVAILABLE")?).map_err(|_|"MANIFEST_INVALID")?;
            need(m.schema_version==2 && m.scope==scope && local_name(&m.journal) && local_name(&m.checkpoint),"MANIFEST_SCOPE_OR_PATH_FAILURE")?;
            let (mut state,h)=read_checkpoint(&dir.join(&m.checkpoint),scope,&m.checkpoint_hash)?;
            let mut sequence=h.anchor.sequence;let mut previous=h.anchor.previous;
            let path=dir.join(&m.journal);let active_bytes=fs::metadata(&path).map_err(|_|"ACTIVE_JOURNAL_UNAVAILABLE")?.len();
            need(active_bytes<=MAX_ACTIVE_JOURNAL_BYTES,"ACTIVE_JOURNAL_TOO_LARGE")?;
            let mut reader=BufReader::new(File::open(&path).map_err(|_|"JOURNAL_READ_FAILED")?);
            while let Some(bytes)=bounded_line(&mut reader)?{
                let record:StoredRecord=serde_json::from_slice(&bytes).map_err(|_|"JOURNAL_INVALID")?;
                match record{
                    StoredRecord::Delta(r)=>{
                        need(r.schema_version==2 && r.sequence==sequence.checked_add(1).ok_or("V2_SEQUENCE_OVERFLOW")? && r.previous==previous && r.hash==delta_hash(r.sequence,&r.previous,&r.transition,&r.delta),"JOURNAL_INTEGRITY_OR_SCOPE_FAILURE")?;
                        let (expected,_)=prepare(&state,&r.transition);
                        need(expected.as_ref()==Some(&r.delta),"V2_TRANSITION_MISMATCH")?;validate_delta(&state,&r.delta)?;
                        install(&mut state,&r.delta);sequence=r.sequence;previous=r.hash;
                    },
                    StoredRecord::Poison(r)=>{
                        need(bytes.len() as u64<=MAX_POISON_BYTES && r.schema_version==2 && r.sequence==sequence.checked_add(1).ok_or("V2_SEQUENCE_OVERFLOW")? && r.previous==previous && r.command_sha256.len()==64 && r.command_sha256.bytes().all(|b|b.is_ascii_hexdigit()&&!b.is_ascii_uppercase()) && r.hash==poison_hash(r.sequence,&r.previous,&r.poison,&r.command_sha256),"JOURNAL_INTEGRITY_OR_SCOPE_FAILURE")?;
                        let delta=prepare_poison(&state,&r.poison)?;validate_delta(&state,&delta)?;install(&mut state,&delta);sequence=r.sequence;previous=r.hash;
                    },
                }
            }
            // Older, unpromoted prototype tails might not leave the new compact
            // poison reserve. Refuse them before exposing a command interface;
            // never erase a clock poison merely because compaction is needed.
            need(state.manual_review || active_bytes.saturating_add(MAX_POISON_BYTES)<=MAX_ACTIVE_JOURNAL_BYTES,"V2_POISON_RESERVE_REQUIRED")?;
            let file=OpenOptions::new().append(true).open(path).map_err(|_|"JOURNAL_OPEN_FAILED")?;
            Ok(Self{file,state,sequence,previous,lock_path:lock_path.clone(),identity:identity.clone(),poisoned:false,dir:dir.into(),journal_name:m.journal,checkpoint_hash:m.checkpoint_hash,migration:h.migration,active_bytes,maximum_bytes:MAX_ACTIVE_JOURNAL_BYTES})
        };
        match load(){Ok(mut s)=>{let t=Transition::Recovery;let (d,_)=prepare(&s.state,&t);if let Some(d)=d{s.persist(t,d)?;}Ok(s)},Err(e)=>{if fs::read(&lock_path).ok().as_deref()==Some(&identity){let _=fs::remove_file(lock_path);}Err(e)}}
    }
    fn persist(&mut self,t:Transition,d:Delta)->Result<()>{self.persist_with_hook(t,d,|_|Ok(()))}
    fn persist_poison(&mut self,witness:PoisonWitness,command_sha256:String)->Result<()>{
        self.owned()?;let delta=prepare_poison(&self.state,&witness)?;validate_delta(&self.state,&delta)?;
        let sequence=self.sequence.checked_add(1).ok_or("V2_SEQUENCE_OVERFLOW")?;
        let digest=poison_hash(sequence,&self.previous,&witness,&command_sha256);
        let record=PoisonRecord{schema_version:2,sequence,previous:self.previous.clone(),poison:witness,command_sha256,hash:digest.clone()};
        let mut bytes=serde_json::to_vec(&record).map_err(|_|"V2_SERIALIZATION_FAILED")?;bytes.push(b'\n');
        need(bytes.len() as u64<=MAX_POISON_BYTES,"V2_POISON_TOO_LARGE")?;
        if self.active_bytes.saturating_add(bytes.len() as u64)>self.maximum_bytes{self.poisoned=true;return Err("V2_POISON_RESERVE_EXHAUSTED".into());}
        self.poisoned=true;
        self.file.write_all(&bytes).and_then(|_|self.file.sync_all()).map_err(|_|"DURABILITY_FAILURE")?;
        need(fs::read(&self.lock_path).ok().as_deref()==Some(&self.identity),"SCOPED_LEASE_LOST")?;
        install(&mut self.state,&delta);self.sequence=sequence;self.previous=digest;self.active_bytes+=bytes.len() as u64;self.poisoned=false;Ok(())
    }
    fn persist_with_hook(&mut self,t:Transition,d:Delta,mut hook:impl FnMut(&str)->Result<()>)->Result<()>{
        self.owned()?;validate_delta(&self.state,&d)?;
        let sequence=self.sequence.checked_add(1).ok_or("V2_SEQUENCE_OVERFLOW")?;
        let digest=delta_hash(sequence,&self.previous,&t,&d);
        let record=DeltaRecord{schema_version:2,sequence,previous:self.previous.clone(),transition:t,delta:d,hash:digest.clone()};
        let mut bytes=serde_json::to_vec(&record).map_err(|_|"V2_SERIALIZATION_FAILED")?;bytes.push(b'\n');
        if bytes.len() as u64>MAX_ROW_BYTES{
            // The attempted operation is never installed. Preserve the previous
            // active/unknown state and durably require review before returning
            // a format error; restart can still recover an existing flight.
            if !self.state.manual_review{self.persist_poison(PoisonWitness::UnrepresentableCommand,hash(&record.transition))?;}
            return Err("V2_ROW_TOO_LARGE".into());
        }
        // A normal transition reserves both a recovery row and a compact poison
        // row. Recovery preserves the poison reserve unless review is already
        // durable. Poison can consume its reserve without a normal cap refusal.
        let reserve=if matches!(record.transition,Transition::Recovery){if self.state.manual_review{0}else{MAX_POISON_BYTES}}else{MAX_ROW_BYTES+MAX_POISON_BYTES};
        need(self.active_bytes.saturating_add(bytes.len() as u64).saturating_add(reserve)<=self.maximum_bytes,"EXPLICIT_COMPACTION_REQUIRED")?;
        self.poisoned=true;
        hook("before_append")?;
        self.file.write_all(&bytes).and_then(|_|self.file.sync_all()).map_err(|_|"DURABILITY_FAILURE")?;
        hook("append_synced")?;
        need(fs::read(&self.lock_path).ok().as_deref()==Some(&self.identity),"SCOPED_LEASE_LOST")?;
        install(&mut self.state,&record.delta);self.sequence=sequence;self.previous=digest;self.active_bytes+=bytes.len() as u64;self.poisoned=false;Ok(())
    }
    pub(super) fn command(&mut self,c:Command)->Result<Value>{
        self.owned()?;
        if c.op=="compact"{need(c.scope==self.state.scope,"SCOPE_MISMATCH")?;need(c.now>=self.state.last_clock && c.now<=MAX_TIME,"COMPACTION_CLOCK_INVALID")?;return self.compact_with_hook(|_|Ok(()),pin_directory);}
        let full=c.op=="status";let summary=c.op=="summary";let lookup=if c.op=="lookup_intent"{Some(c.request.clone())}else{None};
        let t=Transition::Command(c.clone());let (d,result)=prepare(&self.state,&t);
        if let Some(d)=d{
            if d.operational.manual_review && !self.state.manual_review{
                match manual_review_witness(&self.state,&c,&d){
                    Ok(witness)=>self.persist_poison(witness,hash(&c))?,
                    Err(e)=>{self.persist_poison(PoisonWitness::UnrepresentableCommand,hash(&c))?;return Err(e);},
                }
            }else{self.persist(t,d)?;}
        }
        self.owned()?;
        match result{Ok(_) if full=>Ok(json!({"state":self.state})),Ok(_) if summary=>Ok(self.summary()),Ok(_) if lookup.is_some()=>self.lookup_intent(lookup.as_deref().unwrap()),r=>r}
    }
    fn summary(&self)->Value{
        json!({"summarySchemaVersion":1,"globalUnknownCount":self.state.unknown.len(),"hasUnknown":!self.state.unknown.is_empty(),"state":Operational::capture(&self.state),"immutable_bindings":self.state.intents.len(),"idempotency_keys":self.state.keys.len(),"unknown_acceptances":self.state.unknown.len(),"sequence":self.sequence,"previous":self.previous,"active_journal_bytes":self.active_bytes,"schema_version":2})
    }
    pub(super) fn lookup_intent(&self,intent:&str)->Result<Value>{self.owned()?;need(!intent.is_empty(),"INTENT_ID_REQUIRED")?;Ok(json!({"lookupSchemaVersion":1,"intent":intent,"binding":self.state.intents.get(intent),"unknownBinding":self.state.unknown.get(intent),"globalUnknownCount":self.state.unknown.len(),"hasUnknown":!self.state.unknown.is_empty()}))}
    fn compact_with_hook(&mut self,mut hook:impl FnMut(&str)->Result<()>,mut pin:impl FnMut(&Path)->Result<()>)->Result<Value>{
        self.owned()?;self.poisoned=true;
        let expected_manifest=selected_manifest(&self.dir,2,&self.state.scope,&self.journal_name,Some(&self.checkpoint_hash))?;
        self.file.sync_all().map_err(|_|"COMPACTION_DURABILITY_FAILURE")?;
        let (source_bytes,source_sha256)=source_digest(&self.dir.join(&self.journal_name))?;
        need(source_bytes==self.active_bytes,"SOURCE_JOURNAL_CHANGED")?;
        let anchor=Anchor{sequence:self.sequence,previous:self.previous.clone(),source_journal:self.journal_name.clone(),source_bytes,source_sha256,prior_checkpoint_hash:Some(self.checkpoint_hash.clone())};
        let h=Header{schema_version:2,anchor,operational:Operational::capture(&self.state),migration:self.migration.clone(),bindings:self.state.intents.len(),unknowns:self.state.unknown.len()};
        let (journal_name,checkpoint_hash)=switch_generation(&self.dir,&self.lock_path,&self.identity,&self.state,h,expected_manifest.as_deref(),&mut hook,&mut pin)?;
        self.file=OpenOptions::new().append(true).open(self.dir.join(&journal_name)).map_err(|_|"ACTIVE_JOURNAL_OPEN_FAILED")?;
        self.journal_name=journal_name;self.checkpoint_hash=checkpoint_hash;self.active_bytes=0;self.poisoned=false;self.owned()?;Ok(json!({"compacted":true,"sequence":self.sequence,"previous":self.previous,"immutable_bindings":self.state.intents.len(),"unknown_acceptances":self.state.unknown.len(),"active_journal_bytes":0}))
    }
    pub(super) fn migrate_v1(old:Store,proof:&MigrationProof)->Result<Self>{Self::migrate_with_hook(old,proof,|_|Ok(()),pin_directory)}
    fn migrate_with_hook(mut old:Store,proof:&MigrationProof,mut hook:impl FnMut(&str)->Result<()>,mut pin:impl FnMut(&Path)->Result<()>)->Result<Self>{
        old.owned()?;need(!old.recovery_compaction_required && old.state.active.is_none(),"V2_MIGRATION_RECONCILE_FIRST")?;
        need(&MigrationProof::capture(&old)?==proof,"V2_MIGRATION_SOURCE_PROOF_MISMATCH")?;validate_maps(&old.state)?;
        let expected_manifest=selected_manifest(&old.dir,1,&old.state.scope,&old.journal_name,old.checkpoint_hash.as_deref())?;
        let expected_hash=expected_manifest.as_ref().map(|bytes|format!("{:x}",Sha256::digest(bytes)));
        need(expected_hash==proof.manifest_sha256,"V2_MIGRATION_SOURCE_PROOF_MISMATCH")?;
        let anchor=Anchor{sequence:old.sequence,previous:old.previous.clone(),source_journal:old.journal_name.clone(),source_bytes:proof.source_bytes,source_sha256:proof.source_sha256.clone(),prior_checkpoint_hash:old.checkpoint_hash.clone()};
        let h=Header{schema_version:2,anchor,operational:Operational::capture(&old.state),migration:proof.clone(),bindings:old.state.intents.len(),unknowns:old.state.unknown.len()};
        old.poisoned=true;
        let (journal_name,checkpoint_hash)=switch_generation(&old.dir,&old.lock_path,&old.identity,&old.state,h,expected_manifest.as_deref(),&mut hook,&mut pin)?;
        let file=OpenOptions::new().append(true).open(old.dir.join(&journal_name)).map_err(|_|"ACTIVE_JOURNAL_OPEN_FAILED")?;
        // Transfer this exact lease; old Drop must not remove the successor lock.
        let identity=std::mem::take(&mut old.identity);
        let state=std::mem::replace(&mut old.state,State::new(String::new()));
        Ok(Self{file,state,sequence:old.sequence,previous:old.previous.clone(),lock_path:old.lock_path.clone(),identity,poisoned:false,dir:old.dir.clone(),journal_name,checkpoint_hash,migration:proof.clone(),active_bytes:0,maximum_bytes:MAX_ACTIVE_JOURNAL_BYTES})
    }
}
impl Drop for V2Store {fn drop(&mut self){if fs::read(&self.lock_path).ok().as_deref()==Some(&self.identity){let _=fs::remove_file(&self.lock_path);}}}

fn switch_generation(dir:&Path,lock:&Path,identity:&[u8],state:&State,h:Header,expected_manifest:Option<&[u8]>,hook:&mut impl FnMut(&str)->Result<()>,pin:&mut impl FnMut(&Path)->Result<()>)->Result<(String,String)>{
    let nonce=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|_|"COMPACTION_NONCE_FAILED")?.as_nanos();
    let checkpoint_name=format!("checkpoint-v2-{}-{nonce}.jsonl",h.anchor.sequence);
    let journal_name=format!("queue-v2-{}-{nonce}.jsonl",h.anchor.sequence);
    let source_path=dir.join(&h.anchor.source_journal);let source_expected=(h.anchor.source_bytes,h.anchor.source_sha256.clone());
    let digest=write_checkpoint(&dir.join(&checkpoint_name),state,h)?;hook("checkpoint_synced")?;
    need(fs::read(lock).ok().as_deref()==Some(identity),"SCOPED_LEASE_LOST")?;
    write_new_synced(&dir.join(&journal_name),&[])?;hook("journal_synced")?;
    let m=Manifest{schema_version:2,scope:state.scope.clone(),journal:journal_name.clone(),checkpoint:checkpoint_name,checkpoint_hash:digest.clone()};
    let temp=format!("manifest-v2-{nonce}.tmp");write_new_synced(&dir.join(&temp),&serde_json::to_vec(&m).unwrap())?;hook("manifest_synced")?;
    need(source_digest(&source_path)?==source_expected,"SOURCE_JOURNAL_CHANGED")?;
    let guard=dir.join("account-queue-generation.guard");
    if !guard.exists(){write_new_synced(&guard,state.scope.as_bytes())?;}else{need(fs::read(&guard).ok().as_deref()==Some(state.scope.as_bytes()),"GENERATION_GUARD_SCOPE_FAILURE")?;}
    // Pin all targets AND the guard before selecting them; interruption before
    // the first manifest is fail-closed, never an old-history fallback.
    pin(dir)?;hook("generation_pinned")?;
    need(fs::read(lock).ok().as_deref()==Some(identity),"SCOPED_LEASE_LOST")?;
    // The lease is necessary but insufficient if an unrelated maintenance
    // process replaces the selection without honoring it. Compare the exact
    // captured bytes (including expected absence) immediately before rename.
    // This detects a changed selection; it is not a filesystem compare-and-swap.
    need(manifest_bytes(dir)?.as_deref()==expected_manifest,"V2_MANIFEST_CHANGED")?;
    fs::rename(dir.join(&temp),dir.join("account-queue-manifest.json")).map_err(|_|"MANIFEST_ATOMIC_SWITCH_FAILED")?;
    hook("manifest_switched")?;pin(dir)?;hook("manifest_pinned")?;
    need(fs::read(lock).ok().as_deref()==Some(identity),"SCOPED_LEASE_LOST")?;Ok((journal_name,digest))
}

pub(super) fn run(args:&[String])->Result<()>{
    need(args.len()==5 || (args.len()==6 && args[1]=="--offline-migrate-v2"),"USAGE: MODE DIRECTORY SCOPE TOKEN [PROOF_FILE]")?;
    if args[1]=="--offline-v1-proof"{
        let st=Store::open(Path::new(&args[2]),&args[3],&args[4])?;
        need(st.state.active.is_none(),"V2_MIGRATION_RECONCILE_FIRST")?;
        println!("{}",serde_json::to_string(&MigrationProof::capture(&st)?).unwrap());return Ok(());
    }
    if args[1]=="--offline-migrate-v2"{
        need(args.len()==6,"V2_PROOF_FILE_REQUIRED")?;
        let path=Path::new(&args[5]);need(fs::metadata(path).map_err(|_|"V2_PROOF_UNAVAILABLE")?.len()<=16*1024,"V2_PROOF_TOO_LARGE")?;
        let proof:MigrationProof=serde_json::from_slice(&fs::read(path).map_err(|_|"V2_PROOF_UNAVAILABLE")?).map_err(|_|"V2_PROOF_INVALID")?;
        let st=Store::open(Path::new(&args[2]),&args[3],&args[4])?;let st=V2Store::migrate_v1(st,&proof)?;println!("{}",st.summary());return Ok(());
    }
    let mut st=V2Store::open(Path::new(&args[2]),&args[3],&args[4])?;
    let stdin=std::io::stdin();let mut stdout=std::io::stdout().lock();let mut input=BufReader::new(stdin.lock());
    while let Some(bytes)=bounded_line(&mut input)?{
        let response=match serde_json::from_slice::<Command>(&bytes){Ok(c)=>match st.command(c){Ok(v)=>json!({"ok":true,"result":v}),Err(e)=>json!({"ok":false,"error":e})},Err(_)=>json!({"ok":false,"error":"INVALID_COMMAND"})};
        writeln!(stdout,"{}",response).and_then(|_|stdout.flush()).map_err(|_|"OUTPUT_FAILURE")?;
    }Ok(())
}

#[cfg(test)]
#[path="v2_tests.rs"]
mod tests;
