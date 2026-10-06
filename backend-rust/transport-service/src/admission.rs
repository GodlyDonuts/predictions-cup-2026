#![allow(dead_code)]
// Reuse the checked durable Rust core directly; no child process or Node adapter.
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../account-queue/src/main.rs"));

pub struct AccountAdmission { store:Store }
impl AccountAdmission {
    pub fn open(directory:&Path,scope:&str,token:&str)->std::result::Result<Self,String>{Ok(Self{store:Store::open(directory,scope,token)?})}
    pub fn command(&mut self,command:Value)->std::result::Result<Value,String>{let c:Command=serde_json::from_value(command).map_err(|_|"INVALID_COMMAND".to_string())?;self.store.command(c)}
    pub fn seed_legacy_headroom(&mut self,now:u64,until:u64,manual_review:bool)->std::result::Result<(),String>{
        need(self.store.sequence==0&&self.store.state.active.is_none()&&self.store.state.intents.is_empty(),"MIGRATION_BOUNDARY_REQUIRES_FRESH_QUEUE")?;
        need(now<=until&&until<=MAX_TIME,"INVALID_MIGRATION_CLOCK")?;let mut state=self.store.state.clone();state.last_clock=now;state.next=until;state.cooldown=until;state.manual_review=manual_review;self.store.persist(state)
    }
}
