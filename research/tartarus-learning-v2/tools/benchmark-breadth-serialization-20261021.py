"""Isolated diagnostic recorder benchmark; never opens a bank or account API."""
from pathlib import Path
import datetime
import hashlib
import json
import os
import statistics
import subprocess

ROOT=Path('/tmp/moloch-breadth-serialization-20261021')
OLD=Path('/tmp/moloch-hint-batch-r76-20261021')
RUNTIME=Path('/home/archon/apps/moloch/shared/live-production-v1')
RUST='/home/archon/.rustup/toolchains/stable-aarch64-unknown-linux-gnu/bin/rustc'


def sha(p): return hashlib.sha256(p.read_bytes()).hexdigest()


def main():
    original=OLD/'source/moloch/live-runtime/src/breadth.rs'
    (ROOT/'original.rs').write_bytes(original.read_bytes())
    args=json.loads((OLD/'runtime-fixture-command.json').read_bytes())
    deps=[]
    for i,arg in enumerate(args):
        if arg in ['-L','--extern']: deps.extend(args[i:i+2])
    def build(source, output, test=False):
        command=['nice','-n','10',RUST,'--edition=2024',str(source),*deps,
                 '-C','opt-level=2','-C','lto=thin','-C','codegen-units=1',
                 '-C','embed-bitcode=yes','-o',str(output)]
        if test: command.append('--test')
        with output.with_suffix('.build.log').open('w') as log:
            subprocess.run(command,stdout=log,stderr=subprocess.STDOUT,check=True)
    prelude='mod feedback {pub type Result<T> = ResultAlias<T>; type ResultAlias<T> = std::result::Result<T,Box<dyn std::error::Error>>;}\n'
    for name in ['original','candidate']:
        harness=prelude+f'#[path="{name}.rs"] mod breadth;\n'+r'''
use serde_json::Value;
use sha2::{Digest,Sha256};
fn main(){
 let a:Vec<String>=std::env::args().collect();
 let payload:Value=serde_json::from_slice(&std::fs::read(&a[1]).unwrap()).unwrap();
 let count:u64=a[3].parse().unwrap();
 let start=std::time::Instant::now();
 let mut r=breadth::Recorder::new(std::path::Path::new(&a[2])).unwrap();
 for _ in 0..count{r.observe("markets_batch",&payload,"2026-10-21T22:42:00Z");assert!(r.error.is_none(),"{:?}",r.error);}
 let elapsed=start.elapsed().as_secs_f64();drop(r);
 let bytes=std::fs::read(&a[2]).unwrap();
 println!("{}",serde_json::json!({"seconds":elapsed,"frames":count,"bytes":bytes.len(),"sha256":format!("{:x}",Sha256::digest(&bytes))}));
}
'''
        source=ROOT/f'{name}-harness.rs'; source.write_text(harness)
        build(source,ROOT/f'{name}-benchmark')
        if name=='candidate':
            build(source,ROOT/'candidate-tests',True)
            result=subprocess.run([str(ROOT/'candidate-tests'),'--test-threads=2'],capture_output=True,text=True,check=True)
            (ROOT/'tests.log').write_text(result.stdout+result.stderr)
            tests=[x for x in result.stdout.splitlines() if x.startswith('test result:')]
            print(tests,flush=True)
    # Largest complete retained public frame in a bounded recorder tail.
    frames=[]
    for p in sorted(RUNTIME.glob('breadth-1038328-*.jsonl'),key=lambda p:p.stat().st_mtime)[-2:]:
        with p.open('rb') as f:
            offset=max(0,p.stat().st_size-8*1024*1024);f.seek(offset)
            if offset: f.readline()
            for line in f:
                if not line.endswith(b'\n'): continue
                r=json.loads(line)['event']
                if r['event']=='markets_batch': frames.append((len(line),r['payload']))
    assert frames
    payload=max(frames,key=lambda x:x[0])[1]
    (ROOT/'payload.json').write_text(json.dumps(payload,separators=(',',':')))
    assert (ROOT/'payload.json').stat().st_size<4*1024*1024
    runs=[]
    for i in range(3):
        for name in (['original','candidate'] if i%2==0 else ['candidate','original']):
            output=ROOT/f'{name}-{i}.jsonl'; assert not output.exists()
            r=subprocess.run(['nice','-n','10',str(ROOT/f'{name}-benchmark'),str(ROOT/'payload.json'),str(output),'16'],capture_output=True,text=True,check=True)
            value=json.loads(r.stdout); value.update(variant=name,round=i);runs.append(value)
            print(json.dumps(value),flush=True)
    assert len({r['sha256'] for r in runs})==1 and len({r['bytes'] for r in runs})==1
    medians={n:statistics.median(r['seconds'] for r in runs if r['variant']==n) for n in ['original','candidate']}
    result=dict(observedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),
        sourceSha256={n:sha(ROOT/f'{n}.rs') for n in ['original','candidate']},
        payloadSha256=sha(ROOT/'payload.json'),payloadBytes=(ROOT/'payload.json').stat().st_size,
        runs=runs,medianSeconds=medians,relativeReduction=1-medians['candidate']/medians['original'],
        allCaptureBytesIdentical=True,tests=tests,productionChanged=False,
        accountRequests=0,liveCadenceImprovementEstablished=False,
        limitation='Isolated recorder only, low-priority under concurrent original-ledger restoration; not whole-worker performance.')
    (ROOT/'benchmark.public.json').write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps(result),flush=True)


if __name__=='__main__': main()
