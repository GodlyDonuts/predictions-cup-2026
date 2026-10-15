#![allow(dead_code)]
// Reuse the checked durable Rust core directly; no child process or Node adapter.
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../account-queue/src/main.rs"));

pub struct AccountAdmission { store:Store }
impl AccountAdmission {
    pub fn configure_reads(&mut self,cap:usize,gap_ms:u64)->std::result::Result<(),String>{
        need((1..=100).contains(&cap)&&(600..=60_000).contains(&gap_ms),"READ_PROFILE_BOUNDS")?;
        let expected=ReadProfile{cap,gap_ms};
        if let Some(existing)=&self.store.state.read_profile {return need(*existing==expected,"READ_PROFILE_IMMUTABLE")}
        need(self.store.sequence==0,"READ_PROFILE_REQUIRES_FRESH_QUEUE")?;
        let mut state=self.store.state.clone();state.read_profile=Some(expected);self.store.persist(state)
    }
    pub fn open(directory:&Path,scope:&str,token:&str)->std::result::Result<Self,String>{Ok(Self{store:Store::open(directory,scope,token)?})}
    pub fn command(&mut self,command:Value)->std::result::Result<Value,String>{let c:Command=serde_json::from_value(command).map_err(|_|"INVALID_COMMAND".to_string())?;self.store.command(c)}
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
        if compact{let result=self.store.command(c)?;if pin(&self.store.dir).is_err(){self.store.poisoned=true;return Err("COMPACTION_DURABILITY_FAILURE".into())}return Ok(result)}
        match self.store.command(c){Err(code) if !compact&&matches!(code.as_str(),"EXPLICIT_COMPACTION_REQUIRED"|"RECOVERY_COMPACTION_REQUIRED")=>{
            // Preserve a regressed command unchanged so its manual-review poison
            // can still be durably recorded after making local storage space.
            self.command(serde_json::json!({"op":"compact","scope":scope,"now":now.max(self.store.state.last_clock)}))?;
            if pin(&self.store.dir).is_err(){self.store.poisoned=true;return Err("COMPACTION_DURABILITY_FAILURE".into())}
            self.command(command)
        },result=>result}
    }
    pub fn seed_legacy_headroom(&mut self,now:u64,until:u64,manual_review:bool)->std::result::Result<(),String>{
        need((self.store.sequence==0||self.store.sequence==1&&self.store.state.read_profile.is_some())&&self.store.state.last_clock==0&&self.store.state.reads.is_empty()&&self.store.state.writes.is_empty()&&self.store.state.active.is_none()&&self.store.state.intents.is_empty(),"MIGRATION_BOUNDARY_REQUIRES_FRESH_QUEUE")?;
        need(now<=until&&until<=MAX_TIME,"INVALID_MIGRATION_CLOCK")?;let mut state=self.store.state.clone();state.last_clock=now;state.next=until;state.cooldown=until;state.manual_review=manual_review;self.store.persist(state)
    }
}

#[cfg(test)]mod maintenance_tests{
 use super::*;
 fn fixture()->PathBuf{let p=std::env::temp_dir().join(format!("sdk-maintenance-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));fs::create_dir(&p).unwrap();p}
 fn command(op:&str,now:u64)->Value{serde_json::json!({"op":op,"scope":"maintenance","now":now,"request":"request","write":true,"binding":{"intent":"intent","key":"key","payload_hash":"a".repeat(64)},"authorized":true,"lease_valid":true,"permit_valid":true,"outcome":"success","status":200})}
 fn opened(dir:&Path)->AccountAdmission{let mut a=AccountAdmission{store:Store::open(dir,"maintenance","first").unwrap()};for now in 1..10{a.command(serde_json::json!({"op":"status","scope":"maintenance","now":now})).unwrap();}a}
 #[test]fn capacity_rotation_replays_one_local_admission_without_forgetting_bindings(){let dir=fixture();{let mut a=opened(&dir);a.store.maximum_bytes=a.store.active_bytes+10;assert_eq!(a.command_with_maintenance(command("admit",2000)).unwrap()["admitted"],true);assert_eq!(a.store.state.intents.len(),1);assert_eq!(a.store.state.keys.len(),1);assert_eq!(a.store.state.writes,vec![2000]);assert!(dir.join("account-queue-manifest.json").exists());}fs::remove_dir_all(dir).unwrap();}
 #[test]fn capacity_rotation_before_network_marker_preserves_crash_uncertainty(){let dir=fixture();{let mut a=opened(&dir);a.command(command("admit",2000)).unwrap();a.store.maximum_bytes=a.store.active_bytes+10;a.command_with_maintenance(command("network",2000)).unwrap();assert!(a.store.state.active.as_ref().unwrap().network_started);}let a=AccountAdmission{store:Store::open(&dir,"maintenance","second").unwrap()};assert_eq!(a.store.state.unknown.len(),1);assert_eq!(a.store.state.intents.len(),1);drop(a);fs::remove_dir_all(dir).unwrap();}
 #[test]fn completion_rotation_persists_existing_outcome_without_a_second_network_admission(){let dir=fixture();{let mut a=opened(&dir);a.command(command("admit",2000)).unwrap();a.command(command("network",2000)).unwrap();a.store.maximum_bytes=a.store.active_bytes+10;a.command_with_maintenance(command("complete",2500)).unwrap();assert!(a.store.state.active.is_none());assert!(a.store.state.unknown.is_empty());assert_eq!(a.store.state.writes,vec![2000]);assert_eq!(a.store.state.intents.len(),1);assert_eq!(a.store.state.next,5000);}fs::remove_dir_all(dir).unwrap();}
 #[test]fn recovery_compaction_retains_unknown_and_blocks_a_new_write(){let dir=fixture();let cap;{let mut a=opened(&dir);a.command(command("admit",2000)).unwrap();a.command(command("network",2000)).unwrap();cap=a.store.active_bytes;}let mut a=AccountAdmission{store:Store::open_with_limit(&dir,"maintenance","second",cap).unwrap()};assert!(a.store.recovery_compaction_required);a.command_with_maintenance(serde_json::json!({"op":"status","scope":"maintenance","now":3000})).unwrap();assert_eq!(a.store.state.unknown.len(),1);assert_eq!(a.store.state.writes,vec![2000]);assert_eq!(a.command_with_maintenance(command("admit",5000)).unwrap_err(),"UNKNOWN_ACCEPTANCE_RECONCILE_FIRST");drop(a);fs::remove_dir_all(dir).unwrap();}
 #[test]fn lease_and_durability_failures_never_trigger_maintenance(){for poisoned in [true,false]{let dir=fixture();let mut a=opened(&dir);if poisoned{a.store.poisoned=true;}else{fs::write(&a.store.lock_path,b"other-owner").unwrap();}assert_eq!(a.command_with_maintenance(command("status",2000)).unwrap_err(),if poisoned{"DURABILITY_FAILURE"}else{"SCOPED_LEASE_LOST"});assert!(!dir.join("account-queue-manifest.json").exists());drop(a);fs::remove_dir_all(dir).unwrap();}}
 #[test]fn cap_cannot_hide_regressed_clock_manual_review(){let dir=fixture();{let mut a=opened(&dir);a.store.maximum_bytes=a.store.active_bytes+10;assert_eq!(a.command_with_maintenance(command("status",1)).unwrap_err(),"CLOCK_REGRESSION");assert!(a.store.state.manual_review);assert_eq!(a.store.state.last_clock,9);assert_eq!(a.command_with_maintenance(command("admit",2000)).unwrap_err(),"MANUAL_REVIEW_REQUIRED");}let a=AccountAdmission{store:Store::open(&dir,"maintenance","second").unwrap()};assert!(a.store.state.manual_review);drop(a);fs::remove_dir_all(dir).unwrap();}
 #[test]fn oversized_single_record_rotates_once_then_fails_without_looping(){let dir=fixture();{let mut a=opened(&dir);a.store.maximum_bytes=1;let sequence=a.store.sequence;let previous=a.store.state.clone();assert_eq!(a.command_with_maintenance(command("admit",2000)).unwrap_err(),"EXPLICIT_COMPACTION_REQUIRED");assert_eq!(a.store.sequence,sequence);assert_eq!(a.store.state,previous);assert_eq!(a.store.active_bytes,0);assert_eq!(fs::read_dir(&dir).unwrap().filter(|p|p.as_ref().unwrap().file_name().to_string_lossy().starts_with("checkpoint-")).count(),1);}fs::remove_dir_all(dir).unwrap();}
 #[test]fn failed_directory_pin_stops_before_original_admission_is_replayed(){let dir=fixture();{let mut a=opened(&dir);a.store.maximum_bytes=a.store.active_bytes+10;let previous=a.store.state.clone();let mut pins=0;assert_eq!(a.command_with_maintenance_sync(command("admit",2000),|_|{pins+=1;Err("injected".into())}).unwrap_err(),"COMPACTION_DURABILITY_FAILURE");assert_eq!(pins,1);assert_eq!(a.store.state,previous);assert!(a.store.poisoned);assert!(a.store.state.intents.is_empty());assert_eq!(a.command_with_maintenance(command("admit",2000)).unwrap_err(),"DURABILITY_FAILURE");}fs::remove_dir_all(dir).unwrap();}
 #[test]fn explicit_compaction_also_requires_directory_pin(){let dir=fixture();{let mut a=opened(&dir);assert_eq!(a.command_with_maintenance_sync(command("compact",2000),|_|Err("injected".into())).unwrap_err(),"COMPACTION_DURABILITY_FAILURE");assert!(a.store.poisoned);}fs::remove_dir_all(dir).unwrap();}
}
