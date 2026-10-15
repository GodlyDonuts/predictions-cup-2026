use baal_transport_service::{Config,Endpoint,Transport,ReadOnlyAuthority,SystemClock,DisabledCredentials,WindowsDpapiCredentials,CredentialProvider};
use serde_json::json;
use std::io::{BufRead,Write};
fn main(){if run().is_err(){eprintln!("TRANSPORT_SERVICE_START_FAILED");std::process::exit(1);}}
fn run()->Result<(),Box<dyn std::error::Error>>{
 let args:Vec<String>=std::env::args().collect();if args.len()<3||args[1]!="--config"{return Err("USAGE: --config PATH [--approved-dpapi-reads | --approved-file-reads ABS_FILE]".into());}
 let mut config:Config=serde_json::from_slice(&std::fs::read(&args[2])?)?;config.writes_enabled=false;
 // Credential access is opt-in; the default executable cannot make an authenticated call.
 let credential:Box<dyn CredentialProvider>=if args.len()==3{Box::new(DisabledCredentials)}else if args.len()==4&&args[3]=="--approved-dpapi-reads"{Box::new(WindowsDpapiCredentials::approved_default()?)}else if args.len()==5&&args[3]=="--approved-file-reads"{
 #[cfg(unix)] {Box::new(baal_transport_service::credentials::UnixFileCredentials::approved_file(std::path::PathBuf::from(&args[4]))?)}
 #[cfg(not(unix))] {return Err("UNIX_CREDENTIAL_STORAGE_REQUIRED".into())}
}else{return Err("UNRECOGNIZED_CREDENTIAL_OPTION".into())};
 let mut transport=Transport::new(config,credential,Box::new(SystemClock),Box::new(ReadOnlyAuthority))?;
 for line in std::io::stdin().lock().lines(){let value=match line{Ok(s)=>serde_json::from_str::<Endpoint>(&s),Err(_)=>break};let response=match value{Ok(e)=>match transport.request(e){Ok(r)=>json!({"ok":true,"receipt":r}),Err(e)=>json!({"ok":false,"error":e})},Err(_)=>json!({"ok":false,"error":{"code":"INVALID_ENDPOINT_COMMAND"}})};println!("{}",response);std::io::stdout().flush()?;}
 Ok(())
}
