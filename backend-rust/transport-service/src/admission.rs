#![allow(dead_code)]
// Reuse the checked durable Rust core directly; no child process or Node adapter.
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../account-queue/src/main.rs"));

#[derive(Clone,Debug,Serialize,Deserialize,PartialEq,Eq)]
#[serde(deny_unknown_fields)]
pub struct ImmutableBinding {pub intent:String,pub key:String,pub payload_hash:String}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActiveRequest {pub request:String,pub write:bool,pub binding:Option<ImmutableBinding>,pub network_started:bool}
#[derive(Clone,Debug,Serialize,Deserialize,PartialEq,Eq)]
#[serde(deny_unknown_fields)]
pub struct ReadBudget {pub cap:usize,pub gap_ms:u64}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationalSummary {
    pub schema_version:u32,pub scope:String,pub last_clock:u64,pub next:u64,pub cooldown:u64,
    pub manual_review:bool,pub reads:Vec<u64>,pub writes:Vec<u64>,pub active:Option<ActiveRequest>,pub read_profile:Option<ReadBudget>,
}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdmissionSummary {
    #[serde(rename="summarySchemaVersion")]pub summary_schema_version:u32,
    #[serde(rename="globalUnknownCount")]pub global_unknown_count:usize,
    #[serde(rename="hasUnknown")]pub has_unknown:bool,
    pub state:OperationalSummary,pub immutable_bindings:usize,pub idempotency_keys:usize,pub unknown_acceptances:usize,
    pub sequence:u64,pub previous:String,pub active_journal_bytes:u64,pub schema_version:u32,
}
impl AdmissionSummary {
    pub fn parse(raw:Value,scope:&str)->std::result::Result<Self,String>{
        // Option fields are also required to be explicitly present. A missing
        // active/profile field is not evidence of an empty queue/default profile.
        need(raw["state"].get("active").is_some()&&raw["state"].get("read_profile").is_some(),"QUEUE_SUMMARY_FIELDS_REQUIRED")?;
        let s:Self=serde_json::from_value(raw).map_err(|_|"QUEUE_SUMMARY_INVALID")?;
        need(s.summary_schema_version==1 && matches!(s.schema_version,1|2) && s.state.schema_version==1 && s.state.scope==scope,"QUEUE_SUMMARY_SCHEMA_OR_SCOPE")?;
        need(s.has_unknown==(s.global_unknown_count>0)&&s.unknown_acceptances==s.global_unknown_count&&s.global_unknown_count<=s.immutable_bindings&&s.immutable_bindings==s.idempotency_keys,"QUEUE_SUMMARY_UNKNOWN_INCONSISTENT")?;
        need(s.state.last_clock<=MAX_TIME&&s.state.next<=MAX_TIME&&s.state.cooldown<=MAX_TIME,"QUEUE_SUMMARY_CLOCK")?;
        need((s.sequence==0&&s.previous.is_empty())||(s.sequence>0&&valid_hash(&s.previous)),"QUEUE_SUMMARY_HEAD")?;
        if let Some(p)=&s.state.read_profile{need((1..=100).contains(&p.cap)&&(600..=60_000).contains(&p.gap_ms),"READ_PROFILE_BOUNDS")?;}
        if let Some(a)=&s.state.active{need(!a.request.is_empty()&&a.write==a.binding.is_some(),"QUEUE_SUMMARY_ACTIVE")?;if let Some(b)=&a.binding{valid_binding(b)?;}}
        Ok(s)
    }
}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BindingLookup {
    #[serde(rename="lookupSchemaVersion")]pub lookup_schema_version:u32,pub intent:String,pub binding:Option<ImmutableBinding>,
    #[serde(rename="unknownBinding")]pub unknown_binding:Option<ImmutableBinding>,
    #[serde(rename="globalUnknownCount")]pub global_unknown_count:usize,#[serde(rename="hasUnknown")]pub has_unknown:bool,
}
impl BindingLookup {
    fn parse(raw:Value,intent:&str)->std::result::Result<Self,String>{
        need(raw.get("binding").is_some()&&raw.get("unknownBinding").is_some(),"QUEUE_LOOKUP_FIELDS_REQUIRED")?;
        let s:Self=serde_json::from_value(raw).map_err(|_|"QUEUE_LOOKUP_INVALID")?;
        need(s.lookup_schema_version==1&&s.intent==intent&&!intent.is_empty()&&s.has_unknown==(s.global_unknown_count>0),"QUEUE_LOOKUP_SCHEMA_OR_ID")?;
        if let Some(b)=&s.binding{valid_binding(b)?;need(b.intent==intent,"QUEUE_LOOKUP_BINDING_ID")?;}
        if let Some(b)=&s.unknown_binding{need(s.global_unknown_count>0&&s.binding.as_ref()==Some(b),"QUEUE_LOOKUP_UNKNOWN_INCONSISTENT")?;}
        Ok(s)
    }
}
fn valid_hash(s:&str)->bool{s.len()==64&&s.bytes().all(|b|b.is_ascii_hexdigit()&&!b.is_ascii_uppercase())}
fn valid_binding(b:&ImmutableBinding)->std::result::Result<(),String>{need(!b.intent.is_empty()&&!b.key.is_empty()&&valid_hash(&b.payload_hash),"INVALID_BINDING")}
enum Backend {V1(Store),V2(v2::V2Store)}
pub struct AccountAdmission {store:Backend,scope:String,directory:PathBuf}
impl AccountAdmission {
    pub fn configure_reads(&mut self,cap:usize,gap_ms:u64)->std::result::Result<(),String>{
        need((1..=100).contains(&cap)&&(600..=60_000).contains(&gap_ms),"READ_PROFILE_BOUNDS")?;
        let expected=ReadProfile{cap,gap_ms};
        match &mut self.store{
            Backend::V1(s)=>{s.owned()?;if let Some(existing)=&s.state.read_profile{return need(*existing==expected,"READ_PROFILE_IMMUTABLE")}
                need(s.sequence==0,"READ_PROFILE_REQUIRES_FRESH_QUEUE")?;let mut state=s.state.clone();state.read_profile=Some(expected);s.persist(state)},
            Backend::V2(s)=>{s.owned()?;let summary=AdmissionSummary::parse(s.summary(),&self.scope)?;
                let existing=summary.state.read_profile.ok_or("READ_PROFILE_REQUIRES_FRESH_QUEUE")?;need(existing.cap==cap&&existing.gap_ms==gap_ms,"READ_PROFILE_IMMUTABLE")},
        }
    }
    pub fn open(directory:&Path,scope:&str,token:&str)->std::result::Result<Self,String>{
        // Detection never opens a fallback store or changes the selection. Each
        // chosen reader validates the same manifest again under its queue lease.
        let version=match File::open(directory.join("account-queue-manifest.json")){
            Ok(file)=>{let mut bytes=Vec::new();file.take(4097).read_to_end(&mut bytes).map_err(|_|"MANIFEST_UNAVAILABLE")?;need(bytes.len()<=4096,"MANIFEST_TOO_LARGE")?;let m:Manifest=serde_json::from_slice(&bytes).map_err(|_|"MANIFEST_INVALID")?;need(m.scope==scope&&local_name(&m.journal)&&local_name(&m.checkpoint),"MANIFEST_SCOPE_OR_PATH_FAILURE")?;m.schema_version},
            Err(e) if e.kind()==std::io::ErrorKind::NotFound=>1,Err(_)=>return Err("MANIFEST_UNAVAILABLE".into()),
        };
        let store=match version{1=>Backend::V1(Store::open(directory,scope,token)?),2=>Backend::V2(v2::V2Store::open(directory,scope,token)?),_=>return Err("MANIFEST_SCOPE_OR_PATH_FAILURE".into())};
        Ok(Self{store,scope:scope.into(),directory:directory.into()})
    }
    pub fn is_v2(&self)->bool{matches!(self.store,Backend::V2(_))}
    fn raw_command(&mut self,mut c:Command)->std::result::Result<Value,String>{
        if c.op=="diagnostic_status"{c.op="status".into();return match &mut self.store{Backend::V1(s)=>s.command(c),Backend::V2(s)=>s.command(c)};}
        match &mut self.store{Backend::V1(s)=>s.command(c),Backend::V2(s)=>{need(c.op!="status","V2_TYPED_SUMMARY_REQUIRED")?;s.command(c)}}
    }
    pub fn command(&mut self,command:Value)->std::result::Result<Value,String>{let c:Command=serde_json::from_value(command).map_err(|_|"INVALID_COMMAND".to_string())?;self.raw_command(c)}
    pub fn summary(&self)->std::result::Result<AdmissionSummary,String>{
        let raw=match &self.store{
            Backend::V1(s)=>{s.owned()?;let state=&s.state;json!({"summarySchemaVersion":1,"globalUnknownCount":state.unknown.len(),"hasUnknown":!state.unknown.is_empty(),"state":{"schema_version":state.schema_version,"scope":state.scope,"last_clock":state.last_clock,"next":state.next,"cooldown":state.cooldown,"manual_review":state.manual_review,"reads":state.reads,"writes":state.writes,"active":state.active,"read_profile":state.read_profile},"immutable_bindings":state.intents.len(),"idempotency_keys":state.keys.len(),"unknown_acceptances":state.unknown.len(),"sequence":s.sequence,"previous":s.previous,"active_journal_bytes":s.active_bytes,"schema_version":1})},
            Backend::V2(s)=>{s.owned()?;s.summary()},
        };AdmissionSummary::parse(raw,&self.scope)
    }
    pub fn lookup_intent(&self,intent:&str)->std::result::Result<BindingLookup,String>{
        need(!intent.is_empty(),"INTENT_ID_REQUIRED")?;
        let raw=match &self.store{Backend::V1(s)=>{s.owned()?;json!({"lookupSchemaVersion":1,"intent":intent,"binding":s.state.intents.get(intent),"unknownBinding":s.state.unknown.get(intent),"globalUnknownCount":s.state.unknown.len(),"hasUnknown":!s.state.unknown.is_empty()})},Backend::V2(s)=>s.lookup_intent(intent)?};
        BindingLookup::parse(raw,intent)
    }
    fn poison(&mut self){match &mut self.store{Backend::V1(s)=>s.poisoned=true,Backend::V2(s)=>s.poisoned=true}}
    /// Retry only a rejected local append after explicit preserving compaction.
    /// This never sends or repeats an HTTP request; all active/unknown bindings,
    /// rate history and backoff survive the checked checkpoint transition.
    pub fn command_with_maintenance(&mut self,command:Value)->std::result::Result<Value,String>{
        self.command_with_maintenance_sync(command,|dir|{
            #[cfg(unix)]File::open(dir).and_then(|f|f.sync_all()).map_err(|_|"COMPACTION_DURABILITY_FAILURE".to_string())?;
            #[cfg(not(unix))]let _=dir;
            Ok(())
        })
    }
    fn command_with_maintenance_sync(&mut self,command:Value,mut pin:impl FnMut(&Path)->std::result::Result<(),String>)->std::result::Result<Value,String>{
        let c:Command=serde_json::from_value(command.clone()).map_err(|_|"INVALID_COMMAND".to_string())?;
        let scope=c.scope.clone();let now=c.now;let compact=c.op=="compact";
        if compact{let result=self.raw_command(c)?;if pin(&self.directory).is_err(){self.poison();return Err("COMPACTION_DURABILITY_FAILURE".into())}return Ok(result)}
        match self.raw_command(c){Err(code) if !compact&&matches!(code.as_str(),"EXPLICIT_COMPACTION_REQUIRED"|"RECOVERY_COMPACTION_REQUIRED")=>{
            // Preserve a regressed command unchanged so its manual-review poison
            // can still be durably recorded after making local storage space.
            let clock=self.summary()?.state.last_clock;
            self.command(serde_json::json!({"op":"compact","scope":scope,"now":now.max(clock)}))?;
            if pin(&self.directory).is_err(){self.poison();return Err("COMPACTION_DURABILITY_FAILURE".into())}
            self.command(command)
        },result=>result}
    }
    pub fn seed_legacy_headroom(&mut self,now:u64,until:u64,manual_review:bool)->std::result::Result<(),String>{
        let Backend::V1(s)=&mut self.store else{return Err("MIGRATION_BOUNDARY_REQUIRES_FRESH_QUEUE".into());};s.owned()?;
        need((s.sequence==0||s.sequence==1&&s.state.read_profile.is_some())&&s.state.last_clock==0&&s.state.reads.is_empty()&&s.state.writes.is_empty()&&s.state.active.is_none()&&s.state.intents.is_empty(),"MIGRATION_BOUNDARY_REQUIRES_FRESH_QUEUE")?;
        need(now<=until&&until<=MAX_TIME,"INVALID_MIGRATION_CLOCK")?;let mut state=s.state.clone();state.last_clock=now;state.next=until;state.cooldown=until;state.manual_review=manual_review;s.persist(state)
    }
    #[cfg(test)]fn v1(&self)->&Store{match &self.store{Backend::V1(s)=>s,_=>panic!("v1 fixture")}}
    #[cfg(test)]fn v1_mut(&mut self)->&mut Store{match &mut self.store{Backend::V1(s)=>s,_=>panic!("v1 fixture")}}
    #[cfg(test)]fn from_v1(store:Store)->Self{let scope=store.state.scope.clone();let directory=store.dir.clone();Self{store:Backend::V1(store),scope,directory}}
}

#[cfg(test)]
#[path="admission_v2_tests.rs"]
mod adapter_v2_tests;

#[cfg(test)]mod maintenance_tests{
 use super::*;
 fn fixture()->PathBuf{let p=std::env::temp_dir().join(format!("sdk-maintenance-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));fs::create_dir(&p).unwrap();p}
 fn command(op:&str,now:u64)->Value{serde_json::json!({"op":op,"scope":"maintenance","now":now,"request":"request","write":true,"binding":{"intent":"intent","key":"key","payload_hash":"a".repeat(64)},"authorized":true,"lease_valid":true,"permit_valid":true,"outcome":"success","status":200})}
 fn opened(dir:&Path)->AccountAdmission{let mut a=AccountAdmission::from_v1(Store::open(dir,"maintenance","first").unwrap());for now in 1..10{a.command(serde_json::json!({"op":"status","scope":"maintenance","now":now})).unwrap();}a}
 #[test]fn capacity_rotation_replays_one_local_admission_without_forgetting_bindings(){let dir=fixture();{let mut a=opened(&dir);a.v1_mut().maximum_bytes=a.v1().active_bytes+10;assert_eq!(a.command_with_maintenance(command("admit",2000)).unwrap()["admitted"],true);assert_eq!(a.v1().state.intents.len(),1);assert_eq!(a.v1().state.keys.len(),1);assert_eq!(a.v1().state.writes,vec![2000]);assert!(dir.join("account-queue-manifest.json").exists());}fs::remove_dir_all(dir).unwrap();}
 #[test]fn capacity_rotation_before_network_marker_preserves_crash_uncertainty(){let dir=fixture();{let mut a=opened(&dir);a.command(command("admit",2000)).unwrap();a.v1_mut().maximum_bytes=a.v1().active_bytes+10;a.command_with_maintenance(command("network",2000)).unwrap();assert!(a.v1().state.active.as_ref().unwrap().network_started);}let a=AccountAdmission::from_v1(Store::open(&dir,"maintenance","second").unwrap());assert_eq!(a.v1().state.unknown.len(),1);assert_eq!(a.v1().state.intents.len(),1);drop(a);fs::remove_dir_all(dir).unwrap();}
 #[test]fn completion_rotation_persists_existing_outcome_without_a_second_network_admission(){let dir=fixture();{let mut a=opened(&dir);a.command(command("admit",2000)).unwrap();a.command(command("network",2000)).unwrap();a.v1_mut().maximum_bytes=a.v1().active_bytes+10;a.command_with_maintenance(command("complete",2500)).unwrap();assert!(a.v1().state.active.is_none());assert!(a.v1().state.unknown.is_empty());assert_eq!(a.v1().state.writes,vec![2000]);assert_eq!(a.v1().state.intents.len(),1);assert_eq!(a.v1().state.next,5000);}fs::remove_dir_all(dir).unwrap();}
 #[test]fn recovery_compaction_retains_unknown_and_blocks_a_new_write(){let dir=fixture();let cap;{let mut a=opened(&dir);a.command(command("admit",2000)).unwrap();a.command(command("network",2000)).unwrap();cap=a.v1().active_bytes;}let mut a=AccountAdmission::from_v1(Store::open_with_limit(&dir,"maintenance","second",cap).unwrap());assert!(a.v1().recovery_compaction_required);a.command_with_maintenance(serde_json::json!({"op":"status","scope":"maintenance","now":3000})).unwrap();assert_eq!(a.v1().state.unknown.len(),1);assert_eq!(a.v1().state.writes,vec![2000]);assert_eq!(a.command_with_maintenance(command("admit",5000)).unwrap_err(),"UNKNOWN_ACCEPTANCE_RECONCILE_FIRST");drop(a);fs::remove_dir_all(dir).unwrap();}
 #[test]fn lease_and_durability_failures_never_trigger_maintenance(){for poisoned in [true,false]{let dir=fixture();let mut a=opened(&dir);if poisoned{a.v1_mut().poisoned=true;}else{fs::write(&a.v1().lock_path,b"other-owner").unwrap();}assert_eq!(a.command_with_maintenance(command("status",2000)).unwrap_err(),if poisoned{"DURABILITY_FAILURE"}else{"SCOPED_LEASE_LOST"});assert!(!dir.join("account-queue-manifest.json").exists());drop(a);fs::remove_dir_all(dir).unwrap();}}
 #[test]fn cap_cannot_hide_regressed_clock_manual_review(){let dir=fixture();{let mut a=opened(&dir);a.v1_mut().maximum_bytes=a.v1().active_bytes+10;assert_eq!(a.command_with_maintenance(command("status",1)).unwrap_err(),"CLOCK_REGRESSION");assert!(a.v1().state.manual_review);assert_eq!(a.v1().state.last_clock,9);assert_eq!(a.command_with_maintenance(command("admit",2000)).unwrap_err(),"MANUAL_REVIEW_REQUIRED");}let a=AccountAdmission::from_v1(Store::open(&dir,"maintenance","second").unwrap());assert!(a.v1().state.manual_review);drop(a);fs::remove_dir_all(dir).unwrap();}
 #[test]fn oversized_single_record_rotates_once_then_fails_without_looping(){let dir=fixture();{let mut a=opened(&dir);a.v1_mut().maximum_bytes=1;let sequence=a.v1().sequence;let previous=a.v1().state.clone();assert_eq!(a.command_with_maintenance(command("admit",2000)).unwrap_err(),"EXPLICIT_COMPACTION_REQUIRED");assert_eq!(a.v1().sequence,sequence);assert_eq!(a.v1().state,previous);assert_eq!(a.v1().active_bytes,0);assert_eq!(fs::read_dir(&dir).unwrap().filter(|p|p.as_ref().unwrap().file_name().to_string_lossy().starts_with("checkpoint-")).count(),1);}fs::remove_dir_all(dir).unwrap();}
 #[test]fn failed_directory_pin_stops_before_original_admission_is_replayed(){let dir=fixture();{let mut a=opened(&dir);a.v1_mut().maximum_bytes=a.v1().active_bytes+10;let previous=a.v1().state.clone();let mut pins=0;assert_eq!(a.command_with_maintenance_sync(command("admit",2000),|_|{pins+=1;Err("injected".into())}).unwrap_err(),"COMPACTION_DURABILITY_FAILURE");assert_eq!(pins,1);assert_eq!(a.v1().state,previous);assert!(a.v1().poisoned);assert!(a.v1().state.intents.is_empty());assert_eq!(a.command_with_maintenance(command("admit",2000)).unwrap_err(),"DURABILITY_FAILURE");}fs::remove_dir_all(dir).unwrap();}
 #[test]fn explicit_compaction_also_requires_directory_pin(){let dir=fixture();{let mut a=opened(&dir);assert_eq!(a.command_with_maintenance_sync(command("compact",2000),|_|Err("injected".into())).unwrap_err(),"COMPACTION_DURABILITY_FAILURE");assert!(a.v1().poisoned);}fs::remove_dir_all(dir).unwrap();}
}
