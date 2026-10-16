import datetime as dt,hashlib,json,os,shutil,subprocess,time,zipfile,re
from pathlib import Path
root=Path('/tmp/moloch-paper7-7f5caaa0-20261016')
archive=Path('/tmp/moloch-paper7-7f5caaa0-source-overlay-20261016.zip')
basezip=Path('/tmp/moloch-turnover-20543435-source-20261016.zip')
sha=lambda p:hashlib.sha256(Path(p).read_bytes()).hexdigest()
assert sha(archive)=='215b163662aec2222539a99cd738cfcf6b08b6181ffbaa7a8562b4c5809502e0'
assert sha(basezip)=='0b856c3c18577af5d08d6e4327be68107800eb1a753909a00bceeba910363c16'
assert shutil.disk_usage('/tmp').free>16*1024**3
root.mkdir(mode=0o700,exist_ok=False)
with zipfile.ZipFile(basezip) as z:
 assert all(not n.startswith('/') and '..' not in Path(n).parts for n in z.namelist());z.extractall(root)
with zipfile.ZipFile(archive) as z:
 assert all(not n.startswith('/') and '..' not in Path(n).parts for n in z.namelist());z.extractall(root)
old=json.loads((root/'source-manifest.json').read_bytes());overlay=json.loads((root/'paper7-overlay-manifest.json').read_bytes())
hashes=old['sourceHashes'].copy();hashes.update(overlay['overlays'])
for p,h in hashes.items():assert sha(root/p)==h,p
m={'sourceCommit':overlay['sourceCommit'],'baseSourceCommit':overlay['baseSourceCommit'],
 'expectedEngineIdentity':overlay['expectedNativeIdentity'],'overlayArchiveSha256':sha(archive),
 'sourceHashes':hashes,'productionChanged':False}
(root/'integrated-source-manifest.json').write_text(json.dumps(m,indent=2)+'\n')
target=root/'target';cwd=root/'moloch/live-runtime'
env=dict(os.environ,CARGO_BUILD_JOBS='2',CARGO_TARGET_DIR=str(target),CARGO_INCREMENTAL='0',
 CARGO_PROFILE_DEV_DEBUG='0',CARGO_PROFILE_TEST_DEBUG='0',PATH=str(Path.home()/'.cargo/bin')+os.pathsep+os.environ['PATH'])
v={'sourceCommit':m['sourceCommit'],'sourceManifestSha256':sha(root/'integrated-source-manifest.json'),
 'sourceFiles':len(hashes),'expectedEngineIdentity':m['expectedEngineIdentity'],'productionChanged':False,
 'startedAt':dt.datetime.now(dt.timezone.utc).isoformat(),'checks':{}}
def run(name,args,tests=False,where=cwd):
 start=time.monotonic();log=root/(name+'.log')
 with log.open('xb') as f:p=subprocess.run(['nice','-n','15',*args],cwd=where,env=env,stdout=f,stderr=subprocess.STDOUT)
 row={'exitCode':p.returncode,'seconds':time.monotonic()-start,'logSha256':sha(log),'logBytes':log.stat().st_size}
 if tests:row['testResults']=re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out;',log.read_text())
 v['checks'][name]=row;(root/'checkpoint.json').write_text(json.dumps(v,indent=2)+'\n');print(json.dumps({'group':name,**row}),flush=True)
 assert p.returncode==0,(name,log.read_text()[-4500:])
 if tests:assert row['testResults'] and all(int(r[1])==0 for r in row['testResults'])
run('native-library',['cargo','test','--locked','--offline','--lib','--','--test-threads=2'],True,root/'moloch/native')
run('runtime-library',['cargo','test','--locked','--offline','--lib','--','--test-threads=2'],True)
run('worker',['cargo','test','--locked','--offline','--bin','moloch-live-runtime','--','--test-threads=2'],True)
for name in ['maker_policy','execution_distribution','fundamental_acquisition','fundamental_policy']:
 run(name,['cargo','test','--locked','--offline','--test',name,'--','--test-threads=2'],True)
run('release',['cargo','build','--locked','--offline','--release','--bin','moloch-live-runtime','--message-format=json'])
binary=target/'release/moloch-live-runtime'
artifacts=[json.loads(l)for l in(root/'release.log').read_text().splitlines()if l.startswith('{')]
rlibs=[p for a in artifacts if a.get('reason')=='compiler-artifact'and a['target']['name']=='moloch_replay'for p in a['filenames']if p.endswith('.rlib')];assert len(rlibs)==1
probe=root/'engine-identity-probe.rs';probe.write_text('fn main(){println!("{}",moloch_replay::session::engine_identity());}\n')
run('engine-probe-build',['rustc','--edition=2021','-C','lto=thin',str(probe),'-L','dependency='+str(target/'release/deps'),'--extern','moloch_replay='+rlibs[0],'-o',str(root/'engine-identity-probe')])
run('engine-probe-run',[str(root/'engine-identity-probe')])
identity=(root/'engine-probe-run.log').read_text().strip();assert identity==m['expectedEngineIdentity']
for p,h in hashes.items():assert sha(root/p)==h,p
v.update(engineIdentity=identity,binarySha256=sha(binary),binaryBytes=binary.stat().st_size,
 nativeRlib=rlibs[0],nativeRlibSha256=sha(rlibs[0]),finishedAt=dt.datetime.now(dt.timezone.utc).isoformat())
(root/'validation.json').write_text(json.dumps(v,indent=2)+'\n')
print(json.dumps({'allChecksPassed':True,'sourceCommit':v['sourceCommit'],'binarySha256':v['binarySha256'],'engineIdentity':identity,'validationSha256':sha(root/'validation.json')}),flush=True)

