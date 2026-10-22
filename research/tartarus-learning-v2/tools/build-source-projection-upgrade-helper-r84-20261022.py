"""Build an original-ledger upgrade proposer; this script never opens a live bank."""
import hashlib
import json
from pathlib import Path
import subprocess

ROOT=Path('/tmp/moloch-source-projection-r84-20261022')
RUST='/home/archon/.rustup/toolchains/stable-aarch64-unknown-linux-gnu/bin/rustc'
SOURCE=r'''
use std::{env,fs::OpenOptions,io::Write,path::Path};
fn main()->Result<(),Box<dyn std::error::Error>>{
 let a:Vec<String>=env::args().collect();
 if a.len()==2&&a[1]=="identity"{println!("{}",moloch_replay::session::engine_identity());return Ok(())}
 if a.len()!=5{return Err("usage: proposer BANK OLD_BINARY_SHA APPROVED_UTC NEW_OUTPUT".into())}
 let q=moloch_replay::session::propose_live_engine_upgrade(Path::new(&a[1]),&a[2],&a[3])?;
 let file=OpenOptions::new().write(true).create_new(true).open(&a[4])?;
 let mut writer=std::io::BufWriter::new(file);serde_json::to_writer(&mut writer,&q)?;writer.write_all(b"\n")?;writer.flush()?;writer.get_ref().sync_all()?;
 println!("original-prefix proposal written; no production approval applied");Ok(())
}
'''


def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()


def main():
    proof=json.loads((ROOT/'qualified.public.json').read_bytes())
    native=ROOT/'libmoloch_replay_release.rlib';assert sha(native)==proof['nativeLibrarySha256']
    args=json.loads((ROOT/'native-release-command.json').read_bytes())
    deps=[]
    for i,x in enumerate(args[:-1]):
        if x=='-L':deps += [x,args[i+1]]
        if x=='--extern' and args[i+1].startswith('serde_json='):deps += [x,args[i+1]]
    source=ROOT/'propose-upgrade.rs';source.write_text(SOURCE)
    binary=ROOT/'propose-upgrade'
    # Retained release dependencies contain LLVM bitcode; link this helper with
    # the same thin-LTO settings as the qualified production executable.
    command=[RUST,'--edition=2024','-C','opt-level=3','-C','lto=thin',
        '-C','codegen-units=1','-C','embed-bitcode=yes',str(source),
        '--extern','moloch_replay='+str(native),*deps,'-o',str(binary)]
    subprocess.run(command,check=True)
    identity=subprocess.check_output([str(binary),'identity'],text=True).strip()
    assert len(identity)==64 and all(c in '0123456789abcdef' for c in identity)
    result=dict(engineIdentity=identity,helperSha256=sha(binary),helperSourceSha256=sha(source),
        nativeLibrarySha256=sha(native),productionBankOpened=False,accountRequests=0)
    (ROOT/'upgrade-helper.public.json').write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps(result))


if __name__=='__main__':main()
