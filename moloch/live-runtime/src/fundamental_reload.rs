//! Opt-in, local OS-owner fundamental generations. This is not a financial
//! mandate reload: the original Control remains pinned for the worker's life.
//! Linux peer credentials authenticate the existing OS owner, not a JSON claim.
use crate::{feedback::Result,fundamental_policy::{Settings,LoadedSettings,Cohorts},dispatch::DispatchLog};
use moloch_replay::session::{Session,ExclusiveLock};
use serde::{Serialize,Deserialize};
use serde_json::{Value,json};
use sha2::{Sha256,Digest};
use std::{fs::{self,File},path::{Path,PathBuf},io::{Read,Write},sync::Arc};
pub const ZERO:&str="0000000000000000000000000000000000000000000000000000000000000000";
const MAX_CONTROL:u64=2*1024*1024;
fn need(ok:bool,why:&'static str)->Result<()>{if ok{Ok(())}else{Err(why.into())}}
pub fn sha(bytes:&[u8])->String{format!("{:x}",Sha256::digest(bytes))}
fn digest<T:Serialize+?Sized>(v:&T)->Result<String>{Ok(sha(&serde_json::to_vec(v)?))}
fn hex(s:&str)->bool{s.len()==64&&s.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b))}
/// Comparisons use the ORIGINAL parsed JSON, including unknown fields. The
/// worker additionally deserializes its strict Control before admitting it.
pub fn only_fundamental_changed(base:&[u8],next:&[u8])->Result<Settings>{
    let mut a:Value=serde_json::from_slice(base)?;let mut b:Value=serde_json::from_slice(next)?;
    need(a.is_object()&&b.is_object(),"RELOAD_CONTROL_OBJECT")?;
    let policy=b.as_object_mut().unwrap().remove("fundamentalPolicy").ok_or("RELOAD_POLICY_REQUIRED")?;
    a.as_object_mut().unwrap().remove("fundamentalPolicy");
    need(a==b,"RELOAD_FINANCIAL_CONTROL_CHANGED")?;
    let settings:Settings=serde_json::from_value(policy)?;
    need(!settings.allow_initialization,"RELOAD_INITIALIZATION_FORBIDDEN")?;Ok(settings)
}
/// Recovery-only comparison against the ORIGINAL pinned ACTIVE overlay. This
/// never approves a new model or financial mandate and never rewrites history.
fn only_binary_changed(active:&[u8],next_base:&[u8])->Result<Settings>{
    let mut a:Value=serde_json::from_slice(active)?;let mut b:Value=serde_json::from_slice(next_base)?;
    need(a.is_object()&&b.is_object(),"RELOAD_REBASE_CONTROL_OBJECT")?;
    for v in [&mut a,&mut b]{
        let activation=v.get_mut("activation").and_then(Value::as_object_mut).ok_or("RELOAD_REBASE_BINARY_REQUIRED")?;
        let binary=activation.remove("binarySha256").ok_or("RELOAD_REBASE_BINARY_REQUIRED")?;
        need(binary.as_str().is_some_and(hex),"RELOAD_REBASE_BINARY_HASH")?;
    }
    need(a==b,"RELOAD_REBASE_NONBINARY_CONTROL_CHANGED")?;
    let settings:Settings=serde_json::from_value(a.get("fundamentalPolicy").cloned().ok_or("RELOAD_POLICY_REQUIRED")?)?;
    need(!settings.allow_initialization,"RELOAD_INITIALIZATION_FORBIDDEN")?;Ok(settings)
}
pub struct ControlPin{file:File,path:PathBuf,identity:Value,bytes:u64,hash:String,version:Value}
impl ControlPin{
    fn open(path:&Path,expected:&str)->Result<Self>{
        need(path.is_absolute()&&hex(expected),"RELOAD_CONTROL_PIN")?;
        let m=fs::symlink_metadata(path)?;need(m.is_file()&&!m.file_type().is_symlink()&&m.len()<=MAX_CONTROL,"RELOAD_CONTROL_FILE")?;
        let file=File::open(path)?;let identity=identity(&file)?;
        let version=file_version(&file)?;let out=Self{file,path:path.into(),identity,bytes:m.len(),hash:expected.into(),version};out.validate()?;Ok(out)
    }
    fn read(&self)->Result<Vec<u8>>{
        let mut file=self.file.try_clone()?;use std::io::{Seek,SeekFrom};file.seek(SeekFrom::Start(0))?;
        let mut bytes=Vec::new();file.take(MAX_CONTROL+1).read_to_end(&mut bytes)?;need(bytes.len() as u64==self.bytes&&sha(&bytes)==self.hash,"RELOAD_CONTROL_CHANGED")?;Ok(bytes)
    }
    fn validate_metadata(&self)->Result<()>{need(file_version(&self.file)?==self.version,"RELOAD_CONTROL_CHANGED")?;let m=fs::symlink_metadata(&self.path)?;need(!m.file_type().is_symlink()&&identity(&File::open(&self.path)?)?==self.identity,"RELOAD_CONTROL_CHANGED")}
    pub fn validate(&self)->Result<()>{
        self.validate_metadata()?;let m=fs::symlink_metadata(&self.path)?;
        need(m.is_file()&&!m.file_type().is_symlink()&&m.len()==self.bytes&&identity(&File::open(&self.path)?)?==self.identity&&sha(&self.read()?)==self.hash,"RELOAD_CONTROL_CHANGED")
    }
}
#[cfg(unix)]fn identity(file:&File)->Result<Value>{use std::os::unix::fs::MetadataExt;let m=file.metadata()?;Ok(json!({"device":m.dev(),"inode":m.ino()}))}
#[cfg(not(unix))]fn identity(_: &File)->Result<Value>{Err("RELOAD_LINUX_REQUIRED".into())}
#[cfg(unix)]fn file_version(file:&File)->Result<Value>{use std::os::unix::fs::MetadataExt;let m=file.metadata()?;Ok(json!({"device":m.dev(),"inode":m.ino(),"bytes":m.len(),"mtime":m.mtime(),"mtimeNs":m.mtime_nsec(),"ctime":m.ctime(),"ctimeNs":m.ctime_nsec()}))}
#[cfg(not(unix))]fn file_version(_: &File)->Result<Value>{Err("RELOAD_LINUX_REQUIRED".into())}
#[derive(Serialize,Deserialize)]#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Request{
    pub version:u32,pub request_id:String,pub worker_pid:u32,pub worker_start_ticks:u64,pub owner_lease:Value,
    pub base_control_sha256:String,pub bank_manifest_sha256:String,pub run_id:String,
    pub previous_generation_sha256:String,pub control_path:PathBuf,pub control_sha256:String,
    pub issued_at_ms:u64,pub expires_at_ms:u64,
}
/// Only a live authenticated socket can construct this capability. A retained
/// generation is recovered separately from the ORIGINAL locked sidecar.
pub struct Approved{request:Request,raw_sha:String,raw:String,peer:Value,pin:ControlPin}
impl Approved{
    pub fn generation_sha(&self)->&str{&self.raw_sha}
    pub fn control_bytes(&self)->Result<Vec<u8>>{self.pin.validate()?;self.pin.read()}
    pub fn load(&self,base:&[u8])->Result<LoadedSettings>{only_fundamental_changed(base,&self.control_bytes()?)?.load()}
    pub(crate) fn validate_loaded(&self,loaded:&LoadedSettings,base:&[u8])->Result<()>{
        let settings=only_fundamental_changed(base,&self.control_bytes()?)?;
        need(loaded.settings_sha()==digest(&settings)?,"RELOAD_SETTINGS_NOT_APPROVED")?;loaded.validate_files()
    }
    pub fn validate(&self,bank:&Session,log:&DispatchLog,owner:&ExclusiveLock,base:&[u8],previous:&str,now:u64)->Result<()>{
        self.pin.validate()?;let r=&self.request;
        need(r.version==1&&!r.request_id.is_empty()&&r.request_id.len()<=128&&r.request_id.bytes().all(|b|b.is_ascii_alphanumeric()||b"-_".contains(&b))
            &&r.base_control_sha256==sha(base)&&r.bank_manifest_sha256==bank.manifest_hash().as_str().ok_or("RELOAD_MANIFEST")?
            &&bank.live_account_seed().is_some_and(|s|s["runId"]==r.run_id)&&r.previous_generation_sha256==previous
            &&hex(previous)&&r.issued_at_ms<=now&&now<r.expires_at_ms&&r.expires_at_ms.checked_sub(r.issued_at_ms).is_some_and(|d|d<=60_000)
            &&owner.descriptor_identity()?==r.owner_lease,"RELOAD_REQUEST_BINDING_OR_EXPIRED")?;
        quiescent(log)?;Ok(())
    }
    pub(crate) fn metadata(&self)->Value{json!({"version":1,"generationSha256":self.raw_sha,"request":self.request,"requestBodyText":self.raw,"peer":self.peer,"controlIdentity":self.pin.identity})}
}
/// No unresolved submission/cancellation is hidden by switching review routing.
pub fn quiescent(log:&DispatchLog)->Result<()>{
    log.require_resolved_attempts()?;
    need(!log.acknowledgments.keys().any(|id|log.cancel_attempted(*id)&&!log.closed.contains(id)),"RELOAD_UNRESOLVED_CANCEL")
}
/// Immutable installed overlay. Constructed only after the held Cohorts log has
/// accepted or replayed the exact generation. No Deserialize or public mint.
pub struct Active{pin:ControlPin,metadata:Value,recovery:Value,exchanges:std::collections::BTreeSet<String>,files:crate::fundamental_policy::GenerationFiles}
impl Active{
    pub fn validate(&self)->Result<()>{self.pin.validate_metadata()?;self.files.validate()}
    pub fn validate_full(&self)->Result<()>{self.pin.validate()?;self.files.validate_full()}
    pub fn generation_sha(&self)->&str{self.metadata["generationSha256"].as_str().unwrap()}
    /// Diagnostic provenance only: the current Control/binary still require the
    /// existing independent owner rollout/ProductionAuthority checks.
    pub fn recovery_receipt(&self)->&Value{&self.recovery}
    pub fn previous_sha(&self)->&str{self.metadata["request"]["previousGenerationSha256"].as_str().unwrap()}
    pub fn exchanges(&self)->&std::collections::BTreeSet<String>{&self.exchanges}
    pub fn recover(store:&Cohorts,base:&[u8])->Result<Option<(Self,LoadedSettings)>>{
        let Some(meta)=store.active_generation()else{return Ok(None)};
        let request:Request=serde_json::from_value(meta["request"].clone())?;
        let current_base=sha(base);
        let pin=ControlPin::open(&request.control_path,&request.control_sha256)?;
        need(pin.identity==meta["controlIdentity"],"RELOAD_CONTROL_REPLACED")?;
        let original_active=pin.read()?;
        let rebased=request.base_control_sha256!=current_base;
        let settings=if rebased{only_binary_changed(&original_active,base)?}else{only_fundamental_changed(base,&original_active)?};
        let loaded=settings.load()?;store.validate_generation_settings(&loaded)?;pin.validate()?;
        let recovery=json!({"originalBaseSha256":request.base_control_sha256,"currentBaseSha256":current_base,"recoveredGenerationSha256":meta["generationSha256"],"binaryOnlyRebase":rebased});
        Ok(Some((Self{pin,metadata:meta.clone(),recovery,exchanges:store.managed_exchanges(),files:loaded.retained_files()?},loaded)))
    }
    pub fn installed(store:&Cohorts,approved:Approved,loaded:&LoadedSettings)->Result<Self>{
        need(store.active_generation()==Some(&approved.metadata()),"RELOAD_GENERATION_NOT_DURABLE")?;
        let recovery=json!({"originalBaseSha256":approved.request.base_control_sha256,"currentBaseSha256":approved.request.base_control_sha256,"recoveredGenerationSha256":approved.raw_sha,"binaryOnlyRebase":false});
        Ok(Self{pin:approved.pin,metadata:store.active_generation().unwrap().clone(),recovery,exchanges:store.managed_exchanges(),files:loaded.retained_files()?})
    }
}
#[cfg(target_os="linux")]
mod linux{
    use super::*;use std::os::{unix::{net::{UnixListener,UnixStream},fs::{MetadataExt,PermissionsExt,FileTypeExt}},fd::AsRawFd};
    fn start(pid:u32)->Result<u64>{let s=fs::read_to_string(format!("/proc/{pid}/stat"))?;Ok(s.rsplit_once(") ").ok_or("RELOAD_PROCESS_STAT")?.1.split_whitespace().nth(19).ok_or("RELOAD_PROCESS_START")?.parse()?)}
    fn uid()->u32{unsafe{libc::geteuid()}}
    fn peer(stream:&UnixStream)->Result<Value>{
        let mut p:libc::ucred=unsafe{std::mem::zeroed()};let mut len=std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        // Linux supplies these credentials for the connected peer; no claimed
        // UID/PID from request JSON is accepted as authentication.
        let result=unsafe{libc::getsockopt(stream.as_raw_fd(),libc::SOL_SOCKET,libc::SO_PEERCRED,(&mut p as *mut libc::ucred).cast(),&mut len)};
        need(result==0&&len as usize==std::mem::size_of::<libc::ucred>()&&p.uid==uid()&&p.pid>0,"RELOAD_OS_OWNER_REQUIRED")?;
        Ok(json!({"uid":p.uid,"gid":p.gid,"pid":p.pid,"startTicks":start(p.pid as u32)?}))
    }
    pub struct Channel{listener:UnixListener,path:PathBuf,lease:Arc<ExclusiveLock>,worker_start:u64,identity:Value}
    pub struct Pending{stream:UnixStream,pub approved:Option<Approved>}
    impl Pending{pub fn reply(&mut self,value:&Value)->Result<()>{self.stream.write_all(&serde_json::to_vec(value)?)?;self.stream.write_all(b"\n")?;Ok(())}}
    impl Channel{
        pub fn bind(root:&Path,lease:Arc<ExclusiveLock>)->Result<Self>{
            let directory=root.join("fundamental-reload");match fs::create_dir(&directory){Ok(())=>fs::set_permissions(&directory,fs::Permissions::from_mode(0o700))?,Err(e)if e.kind()==std::io::ErrorKind::AlreadyExists=>{},Err(e)=>return Err(e.into())}
            let m=fs::symlink_metadata(&directory)?;need(m.is_dir()&&!m.file_type().is_symlink()&&m.uid()==uid()&&m.mode()&0o077==0,"RELOAD_PRIVATE_DIRECTORY")?;
            lease.descriptor_identity()?;let path=directory.join("owner.sock");
            if path.exists(){let m=fs::symlink_metadata(&path)?;need(m.file_type().is_socket()&&m.uid()==uid(),"RELOAD_SOCKET_PATH")?;
                match UnixStream::connect(&path){Err(e)if e.kind()==std::io::ErrorKind::ConnectionRefused=>fs::remove_file(&path)?,_=>return Err("RELOAD_SOCKET_ALREADY_ACTIVE".into())}}
            let listener=UnixListener::bind(&path)?;fs::set_permissions(&path,fs::Permissions::from_mode(0o600))?;listener.set_nonblocking(true)?;
            let m=fs::symlink_metadata(&path)?;let identity=json!({"device":m.dev(),"inode":m.ino()});
            Ok(Self{listener,path,lease,worker_start:start(std::process::id())?,identity})
        }
        pub fn status(&self,base:&[u8],bank:&Session,previous:&str)->Result<Value>{Ok(json!({"version":1,"socket":self.path,"workerPid":std::process::id(),"workerStartTicks":self.worker_start,"ownerLease":self.lease.descriptor_identity()?,"baseControlSha256":sha(base),"bankManifestSha256":bank.manifest_hash(),"runId":bank.live_account_seed().ok_or("RELOAD_BANK")?["runId"],"previousGenerationSha256":previous,"requestMaximumLifetimeMs":60000,"authentication":"linux_same_uid_peer_credentials"}))}
        pub fn poll(&self)->Result<Option<Pending>>{
            let(mut stream,_)=match self.listener.accept(){Ok(v)=>v,Err(e)if e.kind()==std::io::ErrorKind::WouldBlock=>return Ok(None),Err(e)=>return Err(e.into())};
            stream.set_read_timeout(Some(std::time::Duration::from_millis(200)))?;stream.set_write_timeout(Some(std::time::Duration::from_millis(200)))?;
            let peer=peer(&stream)?;let mut length=[0u8;4];stream.read_exact(&mut length)?;
            let length=u32::from_be_bytes(length) as usize;need(length>0&&length<16*1024,"RELOAD_REQUEST_BOUND")?;
            let mut bytes=vec![0;length];stream.read_exact(&mut bytes)?;
            let request:Request=serde_json::from_slice(&bytes)?;
            need(request.worker_pid==std::process::id()&&request.worker_start_ticks==self.worker_start&&request.owner_lease==self.lease.descriptor_identity()?,"RELOAD_WRONG_WORKER")?;
            need(start(peer["pid"].as_u64().ok_or("RELOAD_PEER")? as u32)?==peer["startTicks"].as_u64().ok_or("RELOAD_PEER")?,"RELOAD_PEER_CHANGED")?;
            let pin=ControlPin::open(&request.control_path,&request.control_sha256)?;
            Ok(Some(Pending{stream,approved:Some(Approved{request,raw_sha:sha(&bytes),raw:String::from_utf8(bytes)?,peer,pin})}))
        }
    }
    impl Drop for Channel{fn drop(&mut self){if fs::symlink_metadata(&self.path).is_ok_and(|m|json!({"device":m.dev(),"inode":m.ino()})==self.identity){let _=fs::remove_file(&self.path);}}}
    pub fn send(root:&Path,request_path:&Path,expected:&str)->Result<Value>{
        need(request_path.is_absolute()&&hex(expected),"RELOAD_REQUEST_PIN")?;let bytes=fs::read(request_path)?;
        need(bytes.len()<16*1024&&sha(&bytes)==expected,"RELOAD_REQUEST_HASH_OR_BOUND")?;
        let path=root.join("fundamental-reload/owner.sock");let m=fs::symlink_metadata(&path)?;need(m.file_type().is_socket()&&m.uid()==uid()&&m.mode()&0o077==0,"RELOAD_PRIVATE_SOCKET")?;
        let mut stream=UnixStream::connect(path)?;let server=peer(&stream)?;let r:Request=serde_json::from_slice(&bytes)?;
        need(server["pid"]==r.worker_pid&&server["startTicks"]==r.worker_start_ticks,"RELOAD_SERVER_CHANGED")?;
        stream.set_read_timeout(Some(std::time::Duration::from_secs(65)))?;stream.write_all(&(bytes.len() as u32).to_be_bytes())?;stream.write_all(&bytes)?;
        let mut response=Vec::new();stream.take(64*1024).read_to_end(&mut response)?;Ok(serde_json::from_slice(&response)?)
    }
}
#[cfg(target_os="linux")]pub use linux::{Channel,Pending,send};
#[cfg(not(target_os="linux"))]pub struct Channel;
#[cfg(not(target_os="linux"))]pub struct Pending{pub approved:Option<Approved>}
#[cfg(not(target_os="linux"))]impl Pending{pub fn reply(&mut self,_:&Value)->Result<()>{Err("RELOAD_LINUX_REQUIRED".into())}}
#[cfg(not(target_os="linux"))]impl Channel{
    pub fn bind(_: &Path,_:Arc<ExclusiveLock>)->Result<Self>{Err("RELOAD_LINUX_REQUIRED".into())}
    pub fn poll(&self)->Result<Option<Pending>>{Err("RELOAD_LINUX_REQUIRED".into())}
    pub fn status(&self,_:&[u8],_:&Session,_:&str)->Result<Value>{Err("RELOAD_LINUX_REQUIRED".into())}
}
#[cfg(not(target_os="linux"))]pub fn send(_: &Path,_:&Path,_:&str)->Result<Value>{Err("RELOAD_LINUX_REQUIRED".into())}

#[cfg(test)]mod tests{
 use super::*;
 fn settings()->Value{json!({"version":1,"authorityPath":"/immutable/authority.json","authoritySha256":"a".repeat(64),"reviews":[],"allowInitialization":false})}
 #[test]fn only_the_explicit_fundamental_overlay_may_change(){
  let base=json!({"activation":{"binarySha256":"b".repeat(64),"maximumCommitted":"95579.915000"},"accountScope":"account","scope":{"tournamentId":"cup"},"root":"/bank","fundamentalReload":true,"fundamentalPolicy":null});
  let bytes=serde_json::to_vec(&base).unwrap();let mut next=base.clone();next["fundamentalPolicy"]=settings();assert!(only_fundamental_changed(&bytes,&serde_json::to_vec(&next).unwrap()).is_ok());
  for key in ["activation","accountScope","scope","root","credentials","ownerCapitalAllocation","dailyEntryWindows","ordinarySideEdge","fundamentalReload"]{let mut bad=next.clone();bad[key]=json!("changed");assert!(only_fundamental_changed(&bytes,&serde_json::to_vec(&bad).unwrap()).is_err(),"{key}");}
  next["fundamentalPolicy"]["allowInitialization"]=json!(true);assert!(only_fundamental_changed(&bytes,&serde_json::to_vec(&next).unwrap()).is_err());
 }
 #[cfg(target_os="linux")]mod kernel{
  use super::*;use std::os::unix::{net::UnixStream,fs::PermissionsExt};
  fn root()->PathBuf{static NEXT:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(0);let path=std::env::temp_dir().join(format!("reload-kernel-{}-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(),NEXT.fetch_add(1,std::sync::atomic::Ordering::Relaxed)));fs::create_dir(&path).unwrap();path}
  #[test]fn local_owner_channel_is_exclusive_and_private(){let root=root();let lease=Arc::new(ExclusiveLock::acquire(&root.join("owner.lock")).unwrap());let channel=Channel::bind(&root,lease.clone()).unwrap();assert!(Channel::bind(&root,lease).is_err());assert_eq!(fs::metadata(root.join("fundamental-reload")).unwrap().permissions().mode()&0o777,0o700);drop(channel);fs::remove_dir_all(root).unwrap();}
  #[test]fn broadly_accessible_directory_cannot_be_an_owner_channel(){let root=root();let lease=Arc::new(ExclusiveLock::acquire(&root.join("owner.lock")).unwrap());fs::create_dir(root.join("fundamental-reload")).unwrap();fs::set_permissions(root.join("fundamental-reload"),fs::Permissions::from_mode(0o755)).unwrap();assert!(Channel::bind(&root,lease).is_err());fs::remove_dir_all(root).unwrap();}
  #[test]fn wrong_worker_and_truncated_owner_requests_never_mint_capability(){
   for truncated in [false,true]{let root=root();let lease=Arc::new(ExclusiveLock::acquire(&root.join("owner.lock")).unwrap());let channel=Channel::bind(&root,lease.clone()).unwrap();let mut stream=UnixStream::connect(root.join("fundamental-reload/owner.sock")).unwrap();
    if truncated{stream.write_all(&100u32.to_be_bytes()).unwrap();stream.write_all(b"{}").unwrap();stream.shutdown(std::net::Shutdown::Write).unwrap();}
    else{let request=Request{version:1,request_id:"wrong-worker".into(),worker_pid:std::process::id()+1,worker_start_ticks:0,owner_lease:lease.descriptor_identity().unwrap(),base_control_sha256:"a".repeat(64),bank_manifest_sha256:"b".repeat(64),run_id:"run".into(),previous_generation_sha256:ZERO.into(),control_path:root.join("unused"),control_sha256:"c".repeat(64),issued_at_ms:1,expires_at_ms:2};let bytes=serde_json::to_vec(&request).unwrap();stream.write_all(&(bytes.len() as u32).to_be_bytes()).unwrap();stream.write_all(&bytes).unwrap();}
    assert!(channel.poll().is_err());drop(stream);drop(channel);fs::remove_dir_all(root).unwrap();
   }
  }
  #[test]fn control_pin_rejects_equal_byte_path_replacement(){let root=root();let path=root.join("control.json");fs::write(&path,b"{}").unwrap();let pin=ControlPin::open(&path,&sha(b"{}")).unwrap();let other=root.join("replacement");fs::write(&other,b"{}").unwrap();fs::rename(other,&path).unwrap();assert!(pin.validate().is_err());drop(pin);fs::remove_dir_all(root).unwrap();}
 }
}
