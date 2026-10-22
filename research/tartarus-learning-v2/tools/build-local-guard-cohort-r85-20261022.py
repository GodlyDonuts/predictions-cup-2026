"""Reproduce the R83 cohort stop offline, then build its fix with qualified R84."""
from pathlib import Path
import datetime, hashlib, json, os, subprocess

ROOT=Path('/tmp/moloch-local-guard-cohort-r85-20261022')
PRIOR=Path('/tmp/moloch-source-projection-r84-20261022')
BASE=Path('/tmp/moloch-exit-audit-r81-20261022')
RUST='/home/archon/.rustup/toolchains/stable-aarch64-unknown-linux-gnu/bin/rustc'

def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def compile_stage(name,old,source_name,baseline=False):
    args=json.loads((old/(source_name+'-command.json')).read_bytes())
    replacements=[(str(old/'source'),str(ROOT/'source')),
        ('/tmp/moloch-recurring-cohort-20261021/libmoloch_replay_release.rlib',str(PRIOR/'libmoloch_replay_release.rlib')),
        (str(BASE/'libmoloch_live_runtime_fixture.rlib'),str((PRIOR if baseline else ROOT)/'libmoloch_live_runtime_fixture.rlib')),
        (str(PRIOR/'libmoloch_live_runtime_fixture.rlib'),str((PRIOR if baseline else ROOT)/'libmoloch_live_runtime_fixture.rlib')),
        (str(PRIOR/'libmoloch_live_runtime_release.rlib'),str(ROOT/'libmoloch_live_runtime_release.rlib'))]
    for before,after in replacements:args=[a.replace(before,after) for a in args]
    output=ROOT/(name if name not in ['runtime-release','runtime-fixture','worker-release'] else {'runtime-release':'libmoloch_live_runtime_release.rlib','runtime-fixture':'libmoloch_live_runtime_fixture.rlib','worker-release':'moloch-live-runtime'}[name])
    args[args.index('-o')+1]=str(output);args=['-L','dependency='+str(ROOT),'-L','dependency='+str(PRIOR),*args]
    (ROOT/(name+'-command.json')).write_text(json.dumps(args))
    with (ROOT/(name+'-build.log')).open('w') as log:
        subprocess.run([RUST,*args],env=dict(os.environ,CARGO_MANIFEST_DIR=str(ROOT/'source/moloch/live-runtime')),stdout=log,stderr=subprocess.STDOUT,check=True)
    print(name,'compiled',flush=True);return output

def main():
    original=json.loads((PRIOR/'qualified.public.json').read_bytes());assert sha(PRIOR/'moloch-live-runtime')==original['binarySha256']
    assert sha(PRIOR/'libmoloch_replay_release.rlib')==original['nativeLibrarySha256']
    expected=json.loads((ROOT/'source-hashes.json').read_bytes())
    for name,h in expected.items():assert sha(ROOT/'source'/name)==h['after'] and sha(PRIOR/'source'/name)==h['before']
    baseline=compile_stage('fundamental-before',BASE,'fundamental',True)
    before=subprocess.run([str(baseline),'local_guard_','--test-threads=1'],capture_output=True,text=True)
    (ROOT/'regression-before.log').write_text(before.stdout+before.stderr)
    assert before.returncode!=0 and 'FUNDAMENTAL_ABANDON_POSITIVE_NOT_SENT_REQUIRED' in before.stdout,before.stdout+before.stderr
    print('Original missing-proof regression reproduced',flush=True)
    checks={}
    for name,old,command in [('runtime-release',PRIOR,'runtime-release'),('runtime-fixture',PRIOR,'runtime-fixture'),('fundamental-tests',BASE,'fundamental'),('worker-tests',PRIOR,'worker-test'),('worker-release',PRIOR,'worker-release')]:
        binary=compile_stage(name,old,command)
        if name.endswith('-tests'):
            with (ROOT/(name+'-results.log')).open('w') as log:subprocess.run([str(binary),'--test-threads=1'],stdout=log,stderr=subprocess.STDOUT,check=True)
            checks[name]=(ROOT/(name+'-results.log')).read_text().splitlines()[-2];print(checks[name],flush=True)
    cli=subprocess.run([str(ROOT/'moloch-live-runtime')],capture_output=True,text=True);assert 'usage:' in (cli.stdout+cli.stderr).lower()
    proof=dict(observedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),sourceHashes=expected,
        inheritedProjectionQualification=original,checks=checks,originalRegressionReproduced=True,
        binarySha256=sha(ROOT/'moloch-live-runtime'),runtimeLibrarySha256=sha(ROOT/'libmoloch_live_runtime_release.rlib'),
        nativeLibrarySha256=original['nativeLibrarySha256'],runtimeFixtureSha256=sha(ROOT/'libmoloch_live_runtime_fixture.rlib'),
        cliLoads=True,productionChanged=False,accountClientsCreated=0)
    (ROOT/'qualified.public.json').write_text(json.dumps(proof,indent=2)+'\n');print(json.dumps(proof),flush=True)

if __name__=='__main__':main()
