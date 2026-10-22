"""Qualify a worker cohort-event index plus final-packet projection candidate; no live changes."""
from pathlib import Path
import datetime,hashlib,json,os,subprocess
W=Path('/tmp/moloch-cohort-index-r87-20261022')
P=Path('/tmp/moloch-final-packet-r86-20261022')
LIBS=Path('/tmp/moloch-local-guard-cohort-r85-20261022')
N=Path('/tmp/moloch-source-projection-r84-20261022')
RUST='/home/archon/.rustup/toolchains/stable-aarch64-unknown-linux-gnu/bin/rustc'
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def main():
    # This offline compiler is the preferred OOM victim, never the live worker.
    Path('/proc/self/oom_score_adj').write_text('1000');os.nice(15)
    q=json.loads((P/'qualified.public.json').read_bytes());expected=json.loads((W/'source-hashes.json').read_bytes())
    paths={str(p.relative_to(P/'source')) for p in (P/'source').rglob('*') if p.is_file()}|{str(p.relative_to(W/'source')) for p in (W/'source').rglob('*') if p.is_file()}
    changed=[]
    for file in paths:
        old=P/'source'/file;new=W/'source'/file
        if not old.exists() or sha(old)!=sha(new):changed.append(file)
    assert set(changed)==set(expected)
    for file,h in expected.items():
        assert sha(W/'source'/file)==h['after']
        assert (sha(P/'source'/file) if (P/'source'/file).exists() else None)==h['before']
    assert sha(LIBS/'libmoloch_live_runtime_release.rlib')==q['runtimeLibrarySha256']
    assert sha(N/'libmoloch_replay_release.rlib')==q['nativeLibrarySha256']
    checks={}
    for name,command in [('worker-tests','worker-tests'),('worker-release','worker-release')]:
        args=json.loads((P/(command+'-command.json')).read_bytes());args=[a.replace(str(P/'source'),str(W/'source')) for a in args];output=W/('worker-tests' if name=='worker-tests' else 'moloch-live-runtime');args[args.index('-o')+1]=str(output)
        (W/(name+'-command.json')).write_text(json.dumps(args))
        with (W/(name+'-build.log')).open('w') as log:subprocess.run([RUST,*args],env=dict(os.environ,CARGO_MANIFEST_DIR=str(W/'source/moloch/live-runtime')),stdout=log,stderr=subprocess.STDOUT,check=True)
        print(name,'compiled',flush=True)
        if name=='worker-tests':
            with (W/'worker-tests-results.log').open('w') as log:subprocess.run([str(output),'--test-threads=1'],stdout=log,stderr=subprocess.STDOUT,check=True)
            checks[name]=(W/'worker-tests-results.log').read_text().splitlines()[-2];assert '249 passed; 0 failed' in checks[name];print(checks[name],flush=True)
    cli=subprocess.run([str(W/'moloch-live-runtime')],capture_output=True,text=True);assert 'usage:' in (cli.stdout+cli.stderr).lower()
    proof=dict(observedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),changedSourcePaths=changed,sourceHashes=expected,
        binarySha256=sha(W/'moloch-live-runtime'),nativeLibrarySha256=q['nativeLibrarySha256'],runtimeLibrarySha256=q['runtimeLibrarySha256'],
        checks=checks,cliLoads=True,productionChanged=False,accountClientsCreated=0,measuredEndToEndSpeedup=False)
    (W/'qualified.public.json').write_text(json.dumps(proof,indent=2)+'\n');print(json.dumps(proof),flush=True)
if __name__=='__main__':main()
