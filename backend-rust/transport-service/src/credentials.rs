use std::path::PathBuf;
use zeroize::Zeroizing;
use crate::{CredentialProvider,Result,fail,need};

pub struct DisabledCredentials;
impl CredentialProvider for DisabledCredentials {fn load(&self)->Result<Zeroizing<String>>{Err(fail("CREDENTIAL_ACCESS_DISABLED"))}}
pub struct WindowsDpapiCredentials {file:PathBuf}
impl WindowsDpapiCredentials {pub fn approved_default()->Result<Self>{let root=std::env::var_os("LOCALAPPDATA").ok_or_else(||fail("APPROVED_STORAGE_UNAVAILABLE"))?;Ok(Self{file:PathBuf::from(root).join("SigTradingCup").join("api-key.dpapi")})}}
impl CredentialProvider for WindowsDpapiCredentials {
 fn load(&self)->Result<Zeroizing<String>> {
  #[cfg(not(windows))] {let _=&self.file;Err(fail("WINDOWS_DPAPI_REQUIRED"))}
  #[cfg(windows)] {load_windows(&self.file)}
 }
}
#[cfg(windows)]
fn load_windows(file:&std::path::Path)->Result<Zeroizing<String>> {
 use windows_sys::Win32::{Security::Cryptography::{CryptUnprotectData,CRYPT_INTEGER_BLOB,CRYPTPROTECT_UI_FORBIDDEN},Foundation::LocalFree};
 // Deployment ACL audit is a native security check, not a shell command.
 verify_restricted_acl(file)?;verify_restricted_acl(file.parent().ok_or_else(||fail("APPROVED_STORAGE_PATH"))?)?;
 let meta=std::fs::symlink_metadata(file).map_err(|_|fail("APPROVED_CREDENTIAL_UNAVAILABLE"))?;need(meta.is_file()&&!meta.file_type().is_symlink()&&meta.len()<=8192,"APPROVED_CREDENTIAL_FORMAT")?;
 let text=Zeroizing::new(std::fs::read_to_string(file).map_err(|_|fail("APPROVED_CREDENTIAL_UNAVAILABLE"))?);let text=text.trim();need(text.len()%2==0&&text.bytes().all(|b|b.is_ascii_hexdigit()),"APPROVED_CREDENTIAL_FORMAT")?;
 let mut cipher=Zeroizing::new(Vec::new());for pair in text.as_bytes().chunks_exact(2){let s=std::str::from_utf8(pair).map_err(|_|fail("APPROVED_CREDENTIAL_FORMAT"))?;cipher.push(u8::from_str_radix(s,16).map_err(|_|fail("APPROVED_CREDENTIAL_FORMAT"))?);}
 let input=CRYPT_INTEGER_BLOB{cbData:cipher.len() as u32,pbData:cipher.as_mut_ptr()};let mut output=CRYPT_INTEGER_BLOB{cbData:0,pbData:std::ptr::null_mut()};
 unsafe{if CryptUnprotectData(&input,std::ptr::null_mut(),std::ptr::null(),std::ptr::null(),std::ptr::null_mut(),CRYPTPROTECT_UI_FORBIDDEN,&mut output)==0{return Err(fail("DPAPI_DECRYPTION_FAILED"));}}
 if output.pbData.is_null()||output.cbData==0||output.cbData>4096 {if !output.pbData.is_null(){unsafe{std::ptr::write_bytes(output.pbData,0,output.cbData as usize);LocalFree(output.pbData as _);}}return Err(fail("DPAPI_PLAINTEXT_FORMAT"));}
 let bytes=unsafe{std::slice::from_raw_parts(output.pbData,output.cbData as usize)};let decoded=if bytes.len()%2==0{let words=Zeroizing::new(bytes.chunks_exact(2).map(|p|u16::from_le_bytes([p[0],p[1]])).collect::<Vec<u16>>());String::from_utf16(&words).map_err(|_|fail("DPAPI_PLAINTEXT_FORMAT"))}else{Err(fail("DPAPI_PLAINTEXT_FORMAT"))};
 unsafe{std::ptr::write_bytes(output.pbData,0,output.cbData as usize);LocalFree(output.pbData as _);}
 let key=Zeroizing::new(decoded?);need(key.starts_with("ace_")&&key.len()<=200&&key.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'_'),"DPAPI_PLAINTEXT_FORMAT")?;Ok(key)
}
#[cfg(windows)]
fn verify_restricted_acl(file:&std::path::Path)->Result<()> {
 use std::{os::windows::ffi::OsStrExt,ptr::null_mut};
 use windows_sys::Win32::{Foundation::{CloseHandle,LocalFree},System::Threading::{GetCurrentProcess,OpenProcessToken},Security::{ACL,ACE_HEADER,ACCESS_ALLOWED_ACE,DACL_SECURITY_INFORMATION,OWNER_SECURITY_INFORMATION,GetAce,EqualSid,CreateWellKnownSid,WinLocalSystemSid,WinBuiltinAdministratorsSid,GetTokenInformation,TokenUser,TOKEN_QUERY,TOKEN_USER,Authorization::{GetNamedSecurityInfoW,SE_FILE_OBJECT}}};
 let name=file.as_os_str().encode_wide().chain(Some(0)).collect::<Vec<_>>();let mut owner=null_mut();let mut dacl:*mut ACL=null_mut();let mut descriptor=null_mut();
 unsafe {need(GetNamedSecurityInfoW(name.as_ptr(),SE_FILE_OBJECT,DACL_SECURITY_INFORMATION|OWNER_SECURITY_INFORMATION,&mut owner,null_mut(),&mut dacl,null_mut(),&mut descriptor)==0,"CREDENTIAL_ACL_UNAVAILABLE")?;}
 let result=(||{
  need(!owner.is_null()&&!dacl.is_null(),"CREDENTIAL_ACL_UNRESTRICTED")?;let mut token=null_mut();unsafe{need(OpenProcessToken(GetCurrentProcess(),TOKEN_QUERY,&mut token)!=0,"WINDOWS_USER_UNAVAILABLE")?;}
  let mut size=0;unsafe{GetTokenInformation(token,TokenUser,null_mut(),0,&mut size);}
  if size==0||size>8192{unsafe{CloseHandle(token);}return Err(fail("WINDOWS_USER_UNAVAILABLE"));}
  let mut bytes=vec![0usize;(size as usize+std::mem::size_of::<usize>()-1)/std::mem::size_of::<usize>()];let obtained=unsafe{GetTokenInformation(token,TokenUser,bytes.as_mut_ptr() as _,size,&mut size)};unsafe{CloseHandle(token);}need(obtained!=0,"WINDOWS_USER_UNAVAILABLE")?;
  let current=unsafe{(*(bytes.as_ptr() as *const TOKEN_USER)).User.Sid};need(unsafe{EqualSid(owner,current)}!=0,"CREDENTIAL_OWNER_MISMATCH")?;
  let mut system=[0u8;128];let mut admin=[0u8;128];let mut system_size=128;let mut admin_size=128;
  unsafe{need(CreateWellKnownSid(WinLocalSystemSid,null_mut(),system.as_mut_ptr() as _,&mut system_size)!=0&&CreateWellKnownSid(WinBuiltinAdministratorsSid,null_mut(),admin.as_mut_ptr() as _,&mut admin_size)!=0,"WINDOWS_SID_UNAVAILABLE")?;}
  for index in 0..unsafe{(*dacl).AceCount} {let mut ace=null_mut();unsafe{need(GetAce(dacl,index as u32,&mut ace)!=0,"CREDENTIAL_ACL_UNAVAILABLE")?;}let kind=unsafe{(*(ace as *const ACE_HEADER)).AceType};
   if kind==1 {continue;}need(kind==0,"CREDENTIAL_ACL_UNSUPPORTED")?;let allowed=ace as *const ACCESS_ALLOWED_ACE;let sid=unsafe{&(*allowed).SidStart as *const u32 as *mut std::ffi::c_void};
   need(unsafe{EqualSid(sid,current)!=0||EqualSid(sid,system.as_ptr() as _)!=0||EqualSid(sid,admin.as_ptr() as _)!=0},"CREDENTIAL_ACL_TOO_BROAD")?;
  }Ok(())
 })();unsafe{LocalFree(descriptor);}result
}

/// Explicitly provisioned Unix runtime storage. This provider never creates a key.
#[cfg(unix)]
pub struct UnixFileCredentials { file:PathBuf }
#[cfg(unix)]
impl UnixFileCredentials { pub fn approved_file(file:PathBuf)->Result<Self>{need(file.is_absolute(),"APPROVED_STORAGE_PATH")?;Ok(Self{file})} }
#[cfg(unix)]
impl CredentialProvider for UnixFileCredentials {
 fn load(&self)->Result<Zeroizing<String>> {
  use std::{fs,io::Read,os::unix::fs::MetadataExt};
  let parent=self.file.parent().ok_or_else(||fail("APPROVED_STORAGE_PATH"))?;
  let home=std::env::var_os("HOME").ok_or_else(||fail("APPROVED_STORAGE_UNAVAILABLE"))?;
  let uid=fs::metadata(home).map_err(|_|fail("APPROVED_STORAGE_UNAVAILABLE"))?.uid();
  let directory=fs::symlink_metadata(parent).map_err(|_|fail("APPROVED_CREDENTIAL_UNAVAILABLE"))?;
  need(directory.is_dir()&&!directory.file_type().is_symlink()&&directory.uid()==uid&&directory.mode()&0o077==0,"CREDENTIAL_DIRECTORY_PERMISSIONS")?;
  let before=fs::symlink_metadata(&self.file).map_err(|_|fail("APPROVED_CREDENTIAL_UNAVAILABLE"))?;
  need(before.is_file()&&!before.file_type().is_symlink()&&before.uid()==uid&&before.mode()&0o077==0&&before.len()<=4096,"CREDENTIAL_FILE_PERMISSIONS")?;
  let file=fs::File::open(&self.file).map_err(|_|fail("APPROVED_CREDENTIAL_UNAVAILABLE"))?;
  let opened=file.metadata().map_err(|_|fail("APPROVED_CREDENTIAL_UNAVAILABLE"))?;
  need(opened.ino()==before.ino()&&opened.dev()==before.dev()&&opened.uid()==uid&&opened.mode()&0o077==0&&opened.is_file(),"CREDENTIAL_FILE_CHANGED")?;
  let mut contents=Zeroizing::new(String::new());file.take(4097).read_to_string(&mut contents).map_err(|_|fail("APPROVED_CREDENTIAL_FORMAT"))?;
  let after=fs::symlink_metadata(&self.file).map_err(|_|fail("CREDENTIAL_FILE_CHANGED"))?;
  need(after.ino()==opened.ino()&&after.dev()==opened.dev()&&!after.file_type().is_symlink()&&after.uid()==uid&&after.mode()&0o077==0,"CREDENTIAL_FILE_CHANGED")?;
  let text=contents.trim();need((16..=256).contains(&text.len())&&text.starts_with("ace_")&&text.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'_'),"APPROVED_CREDENTIAL_FORMAT")?;
  Ok(Zeroizing::new(text.to_owned()))
 }
}
#[cfg(all(test,unix))]
mod unix_file_tests {
 use super::*;use std::{fs,os::unix::fs::{PermissionsExt,symlink},sync::atomic::{AtomicU64,Ordering}};
 static NEXT:AtomicU64=AtomicU64::new(0);
 fn root()->PathBuf{let p=std::env::temp_dir().join(format!("moloch-credential-test-{}-{}",std::process::id(),NEXT.fetch_add(1,Ordering::Relaxed)));fs::create_dir(&p).unwrap();fs::set_permissions(&p,fs::Permissions::from_mode(0o700)).unwrap();p}
 fn fake_key(p:&std::path::Path){fs::write(p,"ace_invented_fixture_only_12345678\n").unwrap();fs::set_permissions(p,fs::Permissions::from_mode(0o600)).unwrap();}
 #[test]fn restricted_file_loads_without_creating_storage(){let p=root();let f=p.join("key");let c=UnixFileCredentials::approved_file(f.clone()).unwrap();assert!(c.load().is_err());assert!(!f.exists());fake_key(&f);assert_eq!(c.load().unwrap().as_str(),"ace_invented_fixture_only_12345678");fs::remove_dir_all(p).unwrap();}
 #[test]fn public_permissions_and_symlinks_rejected(){let p=root();let f=p.join("key");fake_key(&f);let c=UnixFileCredentials::approved_file(f.clone()).unwrap();fs::set_permissions(&f,fs::Permissions::from_mode(0o644)).unwrap();assert!(c.load().is_err());fs::set_permissions(&f,fs::Permissions::from_mode(0o600)).unwrap();fs::set_permissions(&p,fs::Permissions::from_mode(0o755)).unwrap();assert!(c.load().is_err());fs::set_permissions(&p,fs::Permissions::from_mode(0o700)).unwrap();fs::rename(&f,p.join("actual")).unwrap();symlink(p.join("actual"),&f).unwrap();assert!(c.load().is_err());fs::remove_dir_all(p).unwrap();}
 #[test]fn malformed_and_oversized_values_rejected(){let p=root();let f=p.join("key");fake_key(&f);let c=UnixFileCredentials::approved_file(f.clone()).unwrap();for text in ["not-a-key".to_owned(),"ace_has spaces_inside_123".to_owned(),"ace_".to_owned()+&"x".repeat(5000)]{fs::write(&f,text).unwrap();assert!(c.load().is_err());}fs::remove_dir_all(p).unwrap();}
}
