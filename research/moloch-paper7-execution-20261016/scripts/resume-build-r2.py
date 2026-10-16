import datetime as dt,hashlib,json,os,pathlib,subprocess,time,re
root=pathlib.Path('/tmp/moloch-paper7-7f5caaa0-20261016');fix=pathlib.Path('/tmp/moloch-joint-maker-fixture-862dedea-20261016')
sha=lambda p:hashlib.sha256(pathlib.Path(p).read_bytes()).hexdigest()
m=json.loads((root/'integrated-source-manifest.json').read_bytes());prior=json.loads((root/'checkpoint.json').read_bytes())
assert prior['checks']['maker_policy']['exitCode']!=0
for n in ['native-library','runtime-library','worker']:assert prior['checks'][n]['exitCode']==0
for p,h in m['sourceHashes'].items():assert sha(root/p)==h,p
assert sha(fix/'maker_policy.rs')=='6fc5cfd77e548864601073a92c13dc63ea3793c1c1585f9702fb1c032106209b'
target=root/'target';cwd=root/'moloch/live-runtime'
env=dict(os.environ,CARGO_BUILD_JOBS='2',CARGO_TARGET_DIR=str(target),CARGO_INCREMENTAL='0',
 CARGO_PROFILE_DEV_DEBUG='0',CARGO_PROFILE_TEST_DEBUG='0',PATH=str(pathlib.Path.home()/'.cargo/bin')+os.pathsep+os.environ['PATH'])
v={'sourceCommit':m['sourceCommit'],'sourceManifestSha256':sha(root/'integrated-source-manifest.json'),
 'productionSourcesUnchanged':True,'fixtureSourceCommit':'862dedea0cd8206154c9aa8e0c728dedb3df8e0b',
 'fixtureSha256':sha(fix/'maker_policy.rs'),'fixtureOnlyChange':'hash original typed Config serialization, not Value reserialization',
 'originalFixtureFailureRetained':True,'initialWrapperMissingChronoFailureRetained':True,'priorValidation':prior,'productionChanged':False,'startedAt':dt.datetime.now(dt.timezone.utc).isoformat(),'checks':{}}
def run(name,args,tests=False,where=cwd):
 start=time.monotonic();log=root/(name+'.log')
 with log.open('xb')as f:p=subprocess.run(['nice','-n','15',*args],cwd=where,env=env,stdout=f,stderr=subprocess.STDOUT)
 row={'exitCode':p.returncode,'seconds':time.monotonic()-start,'logSha256':sha(log),'logBytes':log.stat().st_size}
 if tests:row['testResults']=re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out;',log.read_text())
 v['checks'][name]=row;(root/'resume-checkpoint.json').write_text(json.dumps(v,indent=2)+'\n');print(json.dumps({'group':name,**row}),flush=True)
 assert p.returncode==0,(name,log.read_text()[-4500:])
 if tests:assert row['testResults'] and all(int(r[1])==0 for r in row['testResults'])
run('maker-fixture-lock-r2',['cargo','generate-lockfile','--offline'],where=fix)
run('maker-fixture-typed-config-r2',['cargo','test','--locked','--offline','--test','maker_policy','--','--test-threads=2'],True,fix)
v['makerFixtureCargoTomlSha256']=sha(fix/'Cargo.toml');v['makerFixtureCargoLockSha256']=sha(fix/'Cargo.lock')
for name in ['execution_distribution','fundamental_acquisition','fundamental_policy']:
 run(name,['cargo','test','--locked','--offline','--test',name,'--','--test-threads=2'],True)
run('release',['cargo','build','--locked','--offline','--release','--bin','moloch-live-runtime','--message-format=json'])
binary=target/'release/moloch-live-runtime'
artifacts=[json.loads(l)for l in(root/'release.log').read_text().splitlines()if l.startswith('{')]
rlibs=[p for a in artifacts if a.get('reason')=='compiler-artifact'and a['target']['name']=='moloch_replay'for p in a['filenames']if p.endswith('.rlib')];assert len(rlibs)==1
probe=root/'engine-identity-probe.rs';probe.write_text('fn main(){println!("{}",moloch_replay::session::engine_identity());}\n')
run('engine-probe-build',['rustc','--edition=2021','-C','lto=thin',str(probe),'-L','dependency='+str(target/'release/deps'),'--extern','moloch_replay='+rlibs[0],'-o',str(root/'engine-identity-probe')])
run('engine-probe-run',[str(root/'engine-identity-probe')])
identity=(root/'engine-probe-run.log').read_text().strip();assert identity==m['expectedEngineIdentity']
for p,h in m['sourceHashes'].items():assert sha(root/p)==h,p
v.update(engineIdentity=identity,binarySha256=sha(binary),binaryBytes=binary.stat().st_size,nativeRlib=rlibs[0],nativeRlibSha256=sha(rlibs[0]),finishedAt=dt.datetime.now(dt.timezone.utc).isoformat())
(root/'validation.json').write_text(json.dumps(v,indent=2)+'\n')
print(json.dumps({'allChecksPassed':True,'binarySha256':v['binarySha256'],'engineIdentity':identity,'validationSha256':sha(root/'validation.json')}),flush=True)

