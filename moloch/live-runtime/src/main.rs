#[cfg(unix)]
mod worker;
fn main(){
 #[cfg(unix)]
 if let Err(e)=worker::main(){eprintln!("moloch-live stopped: {e}");std::process::exit(1)}
 #[cfg(not(unix))]
 {eprintln!("moloch-live requires the approved Unix runtime");std::process::exit(1)}
}
