"""Compare exact production reader sections on a copied dispatch prefix only."""
from pathlib import Path
import datetime
import hashlib
import json
import os
import statistics
import subprocess

ROOT = Path('/tmp/moloch-closed-index-r77-20261021')
OLD = Path('/tmp/moloch-hint-batch-r76-20261021')
RUNTIME = Path('/home/archon/apps/moloch/shared/live-production-v1')
RUST = '/home/archon/.rustup/toolchains/stable-aarch64-unknown-linux-gnu/bin/rustc'


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    args = json.loads((OLD / 'runtime-fixture-command.json').read_bytes())
    deps = []
    for i, arg in enumerate(args):
        if arg in ['-L', '--extern']:
            deps.extend(args[i:i+2])
    deps.extend(['--extern', f'moloch_live_runtime={OLD}/libmoloch_live_runtime_fixture.rlib'])
    prelude = '''use moloch_live_runtime::{dispatch::DispatchLog,feedback::Result};
use serde::{Serialize,Deserialize}; use serde_json::{Value,json}; use sha2::{Digest,Sha256};
use std::{collections::{BTreeMap,BTreeSet},fs::{self,File,OpenOptions},io::{Read,Write},path::{Path,PathBuf}};
const MAX_DISPATCH_ROW:usize=8*1024*1024;
fn need(ok:bool,why:&'static str)->Result<()>{if ok{Ok(())}else{Err(why.into())}}
fn digest<T:Serialize+?Sized>(v:&T)->Result<String>{Ok(format!("{:x}",Sha256::digest(serde_json::to_vec(v)?)))}
fn bytes_sha(b:&[u8])->String{format!("{:x}",Sha256::digest(b))}
fn hex(s:&str)->bool{s.len()==64&&s.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b))}
'''
    driver = '''fn main(){
let a:Vec<String>=std::env::args().collect();let path=PathBuf::from(&a[1]);
assert!(path.components().any(|c|c.as_os_str()=="offline-closure-cache"));
assert!(!path.to_string_lossy().contains("/shared/live-production-v1/"));
let log=DispatchLog::open(&path).unwrap();let identity=identity(&File::open(&path).unwrap()).unwrap();
let sets:Vec<BTreeSet<u64>>=serde_json::from_slice(&fs::read(&a[2]).unwrap()).unwrap();
let cache=std::sync::Mutex::new(ClosedRowsCache::default());let mut rows=Vec::new();let start=std::time::Instant::now();
for wanted in &sets{let begin=std::time::Instant::now();let(found,pin)=original_closed_rows(&cache,&path,&identity,&log,wanted).unwrap();
rows.push(json!({"wanted":wanted,"seconds":begin.elapsed().as_secs_f64(),"rowsSha256":digest(&found).unwrap(),"originalSha256":pin.sha}));}
println!("{}",json!({"seconds":start.elapsed().as_secs_f64(),"selections":rows,"accountRequests":0}));
}
'''
    source_hashes = {}
    for name, source_root in [('original', OLD), ('candidate', ROOT)]:
        source_path = source_root / 'source/moloch/live-runtime/src/fundamental_policy.rs'
        source = source_path.read_text()
        source_hashes[name] = sha(source_path)
        reader = source[source.index('const CLOSED_CACHE_BYTES:'):source.index('#[cfg(all(test,unix))]', source.index('const CLOSED_CACHE_BYTES:'))]
        pins = source[source.index('#[cfg(unix)]fn identity'):source.index('#[derive(Serialize,Deserialize)]', source.index('impl FilePin{'))]
        harness = ROOT / f'{name}-reader-harness.rs'
        tests = ROOT / 'source/research/moloch-fundamental-closure-cache-20261017/tests.rs'
        harness.write_text(prelude + pins + reader + driver + (f'\n#[cfg(test)]#[path="{tests}"]mod tests;\n' if name == 'candidate' else ''))
        for testing in ([False, True] if name == 'candidate' else [False]):
            binary = ROOT / (f'{name}-reader-tests' if testing else f'{name}-reader-benchmark')
            command = ['nice', '-n', '10', RUST, '--edition=2024', str(harness), *deps,
                       '-C', 'opt-level=2', '-C', 'lto=thin', '-C', 'codegen-units=1',
                       '-C', 'embed-bitcode=yes', '-o', str(binary)]
            if testing:
                command.append('--test')
            with binary.with_suffix('.build.log').open('w') as log:
                subprocess.run(command, stdout=log, stderr=subprocess.STDOUT, check=True)
            if testing:
                with (ROOT / 'reader-tests.log').open('w') as log:
                    subprocess.run([str(binary), '--test-threads=2'], stdout=log, stderr=subprocess.STDOUT, check=True)
                print((ROOT / 'reader-tests.log').read_text().splitlines()[-2], flush=True)
    copied = ROOT / 'offline-closure-cache'
    copied.mkdir(exist_ok=False)
    raw = (RUNTIME / 'dispatch.ndjson').read_bytes()
    raw = raw[:raw.rfind(b'\n')+1]  # Complete original prefix; production remains untouched.
    path = copied / 'dispatch.ndjson'
    path.write_bytes(raw)
    original_sha = sha(path)
    closed = [json.loads(row)['event']['qid'] for row in raw.splitlines()
              if json.loads(row)['event']['kind'] == 'closed']
    assert len(closed) >= 12
    # Different closed-order selections on the same original head, as used by
    # different cohorts. Repeat the entire sequence to include existing hits.
    selected = sorted(closed[-12:])
    sets = [[qid] for qid in selected] + [selected[:6], selected[6:], selected]
    sets += sets
    selection_path = copied / 'selections.json'
    selection_path.write_text(json.dumps(sets))
    runs = []
    for i in range(3):
        for name in (['original', 'candidate'] if i % 2 == 0 else ['candidate', 'original']):
            run = subprocess.run(['nice', '-n', '10', str(ROOT / f'{name}-reader-benchmark'),
                                  str(path), str(selection_path)], capture_output=True, text=True, check=True)
            result = json.loads(run.stdout)
            assert sha(path) == original_sha
            result.update(variant=name, round=i)
            runs.append(result)
            print(json.dumps({k: result[k] for k in ['variant', 'round', 'seconds']}), flush=True)
    proofs = [[(r['wanted'], r['rowsSha256'], r['originalSha256']) for r in run['selections']] for run in runs]
    assert all(proof == proofs[0] for proof in proofs)
    median = {name: statistics.median(r['seconds'] for r in runs if r['variant'] == name)
              for name in ['original', 'candidate']}
    result = dict(observedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),
                  sourceSha256=source_hashes, copiedPrefixBytes=len(raw), copiedPrefixSha256=original_sha,
                  selectionsPerRun=len(sets), runs=runs, medianSeconds=median,
                  relativeReduction=1-median['candidate']/median['original'],
                  identicalOriginalRows=True, originalPrefixUnchanged=True, productionChanged=False,
                  accountRequests=0, wholeWorkerSpeedEstablished=False)
    (ROOT / 'benchmark.public.json').write_text(json.dumps(result, indent=2)+'\n')
    print(json.dumps({k: v for k, v in result.items() if k != 'runs'}), flush=True)


if __name__ == '__main__':
    main()
