"""Build isolated source-projection scheduling; never starts an account client."""
from pathlib import Path
import datetime
import hashlib
import json
import os
import subprocess
import sys

ROOT = Path('/tmp/moloch-source-projection-r84-20261022')
NATIVE = Path('/tmp/moloch-recurring-cohort-20261021')
RUNTIME = Path('/tmp/moloch-exit-audit-r81-20261022')
WORKER = Path('/tmp/moloch-local-retirement-sweep-r83-20261022')
RUST = '/home/archon/.rustup/toolchains/stable-aarch64-unknown-linux-gnu/bin/rustc'


def sha(p):
    return hashlib.sha256(p.read_bytes()).hexdigest()


def main():
    expected=json.loads((ROOT/'source-hashes.json').read_bytes())
    changed=[]
    for p in (WORKER/'source').rglob('*'):
        if p.is_file():
            relative=p.relative_to(WORKER/'source')
            if sha(p)!=sha(ROOT/'source'/relative):changed.append(str(relative))
    assert set(changed)==set(expected),changed
    for p,h in expected.items():
        assert sha(WORKER/'source'/p)==h['before']
        assert sha(ROOT/'source'/p)==h['after']
    assert sha(NATIVE/'libmoloch_replay_release.rlib')=='fb56b89e5a6af2788baf4d858fcca02e675f0752a3cabf355aac791b767ba287'
    assert sha(RUNTIME/'libmoloch_live_runtime_release.rlib')=='95211bc67bf59a27dfa5f0394d4c186be52b2da0369234dc139009724798b5e1'
    checks={}
    stages=[(NATIVE,'native-release','native',None),(NATIVE,'native-tests-build','native',[]),
            (RUNTIME,'runtime-release','live-runtime',None),(RUNTIME,'runtime-fixture','live-runtime',None),
            (WORKER,'worker-test','live-runtime',[]),
            (WORKER,'worker-release','live-runtime',None)]
    assert sys.argv[1:] in ([],['--resume-worker'])
    if sys.argv[1:]:
        retained=json.loads((ROOT/'completed-library-builds.json').read_bytes())
        assert retained['sourceHashes']==expected
        for path,digest in retained['fileHashes'].items():assert sha(ROOT/path)==digest,path
        checks['native-tests-build']=retained['nativeTests']
        assert '136 passed; 0 failed' in checks['native-tests-build']
        stages=stages[3:]
    for old,name,crate,tests in stages:
        args=json.loads((old/(name+'-command.json')).read_bytes())
        args=[value.replace(str(old/'source'),str(ROOT/'source'))
              .replace(str(NATIVE/'libmoloch_replay_release.rlib'),str(ROOT/'libmoloch_replay_release.rlib'))
              .replace(str(RUNTIME/'libmoloch_live_runtime_release.rlib'),str(ROOT/'libmoloch_live_runtime_release.rlib'))
              .replace(str(RUNTIME/'libmoloch_live_runtime_fixture.rlib'),str(ROOT/'libmoloch_live_runtime_fixture.rlib')) for value in args]
        output=args.index('-o')+1;args[output]=str(ROOT/Path(args[output]).name);executable=args[output]
        args=['-L','dependency='+str(ROOT),*args]
        (ROOT/(name+'-command.json')).write_text(json.dumps(args))
        env=dict(os.environ,CARGO_MANIFEST_DIR=str(ROOT/'source/moloch'/crate))
        with (ROOT/(name+'-build.log')).open('w') as log:
            subprocess.run(['nice','-n','15',RUST,*args],env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
        print(name,'compiled',flush=True)
        if tests is not None:
            with (ROOT/(name+'-results.log')).open('w') as log:
                subprocess.run(['nice','-n','15',executable,*tests,'--test-threads=1'],stdout=log,stderr=subprocess.STDOUT,check=True)
            checks[name]=(ROOT/(name+'-results.log')).read_text().splitlines()[-2]
            print(checks[name],flush=True)
    binary=ROOT/'moloch-live-runtime'
    cli=subprocess.run([str(binary)],capture_output=True,text=True)
    assert 'usage:' in (cli.stdout+cli.stderr).lower()
    proof=dict(observedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),
        changedSourcePaths=changed,sourceHashes=expected,checks=checks,
        binarySha256=sha(binary),nativeLibrarySha256=sha(ROOT/'libmoloch_replay_release.rlib'),
        runtimeLibrarySha256=sha(ROOT/'libmoloch_live_runtime_release.rlib'),
        runtimeFixtureSha256=sha(ROOT/'libmoloch_live_runtime_fixture.rlib'),
        cliLoads=True,accountClientsCreated=0,productionChanged=False,
        endToEndSpeedupEstablished=False)
    (ROOT/'qualified.public.json').write_text(json.dumps(proof,indent=2)+'\n')
    print(json.dumps(proof),flush=True)


if __name__=='__main__':
    main()
