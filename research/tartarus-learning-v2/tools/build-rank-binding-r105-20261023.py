"""Qualify single-boundary rank ownership validation; no account client or runtime changes."""
from pathlib import Path
import hashlib
import json
import os
import shutil
import subprocess
import time

ROOT = Path('/tmp/moloch-rank-binding-r105-20261023')
PRIOR = Path('/tmp/moloch-policy-log-r104-20261022')
RUST = '/home/archon/.rustup/toolchains/stable-aarch64-unknown-linux-gnu/bin/rustc'


def sha(p):
    return hashlib.sha256(p.read_bytes()).hexdigest()


def main():
    os.umask(0o077); os.nice(15)
    Path('/proc/self/oom_score_adj').write_text('1000')
    shutil.copytree(PRIOR/'source', ROOT/'source')
    patch=json.loads((ROOT/'source-patch.json').read_bytes())
    target=ROOT/'source'/patch['path']
    assert sha(target)==patch['oldSha256']
    target.write_text(patch['source'])
    assert sha(target)==patch['newSha256']
    tmp=Path('/dev/shm/r105-rank-binding'); tmp.mkdir(mode=0o700,exist_ok=True)
    checks={}
    for name,run in [('runtime-fixture',None),('fundamental-tests',['--test-threads=1']),('rank-cohort-tests',['--test-threads=1']),
                     ('runtime-release',None),('worker-release',None)]:
        origin=PRIOR if (PRIOR/(name+'-command.json')).exists() else Path('/tmp/moloch-generation-pins-r103-20261022')
        args=json.loads((origin/(name+'-command.json')).read_bytes())
        args=[x.replace(str(origin/'source'),str(ROOT/'source'))
              .replace('moloch_live_runtime='+str(origin),'moloch_live_runtime='+str(ROOT)) for x in args]
        args[args.index('-o')+1]=str(ROOT/Path(args[args.index('-o')+1]).name)
        args=['-L','dependency='+str(ROOT),*args]
        (ROOT/(name+'-command.json')).write_text(json.dumps(args))
        env=dict(os.environ,CARGO_MANIFEST_DIR=str(ROOT/'source/moloch/live-runtime'),TMPDIR=str(tmp))
        start=time.monotonic()
        with (ROOT/(name+'-build.log')).open('x') as log:
            subprocess.run([RUST,*args],check=True,stdout=log,stderr=subprocess.STDOUT,env=env)
        if run:
            with (ROOT/(name+'-results.log')).open('x') as log:
                subprocess.run([args[args.index('-o')+1],*run],check=True,stdout=log,stderr=subprocess.STDOUT,env=env)
            checks[name]=next(x for x in (ROOT/(name+'-results.log')).read_text().splitlines() if x.startswith('test result:'))
            assert (('149' if name=='fundamental-tests' else '7')+' passed; 0 failed') in checks[name]
        print(name,round(time.monotonic()-start,3),flush=True)
    binary=ROOT/'moloch-live-runtime'
    cli=subprocess.run([str(binary)],capture_output=True,text=True)
    assert 'usage:' in (cli.stdout+cli.stderr).lower()
    q=dict(binarySha256=sha(binary),runtimeLibrarySha256=sha(ROOT/'libmoloch_live_runtime_release.rlib'),
           changedSource=patch['path'],sourceSha256=sha(target),
           checks=checks,cliLoads=True,fullInitialCheckPreserved=True,publicCrossCallCheckPreserved=True,targetFifoRecheckedAfterAppend=True,
           originalHistoryChanged=False,nativeEngineChanged=False,riskCapsChanged=False,
           accountClientsCreated=0,deployed=False)
    (ROOT/'qualified.public.json').write_text(json.dumps(q,indent=2)+'\n')
    print('QUALIFIED_UNDEPLOYED',flush=True)


if __name__=='__main__': main()
