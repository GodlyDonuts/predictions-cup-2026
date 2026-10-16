"""OFFLINE BUILD ONLY. No bank open/replay, bridge, account or production calls."""
import datetime as dt
import hashlib,json,os,re,shutil,signal,stat,subprocess,sys,time,zipfile
from pathlib import Path
BASE=Path('/tmp/moloch-buy-timestamp-recovery-20261016')
ROOT=Path('/tmp/moloch-native-flow-build-b2202660-20261016-r2')
ARCHIVE=Path('/tmp/moloch-native-flow-b2202660-build-inputs.zip')
ARCHIVE_SHA='50e7e4e85648c05faacc3e412782ea44bcc4a882e7fc3768707eace800c9e94e'
MANIFEST_SHA='ed55c5f60fbacf54dfc9049bafd02dd9ce0ebc2157b3fa4683a45340eeeef921'
RELEASE_SHA='03e227871d6f86d814343d81859c65e59b7954e0688b2c11c5fda43daad1cef3'
VALIDATION_SHA='446fc9a150388be86a9e6298c5fe2e841729a87963afe56a36bc3817b1b87885'
OLD='8acc461bc02fb9236100058fe464b203d5da005375632af05c176223c7246754'
INPUTS=tuple('src/'+n for n in ('engine.rs','engine_live.rs','reviewed_inventory_exit.rs','reviewed_fundamental_entry.rs','ordinary_entry_exclusions.rs','live_market_capacity.rs','engine_capital.rs','semantic_floor.rs','engine_packages.rs','probabilistic_packages.rs','engine_collateral.rs','lib.rs','legacy.rs','journal.rs','session.rs'))+('Cargo.lock',)
sha=lambda p:hashlib.sha256(Path(p).read_bytes()).hexdigest()
def need(ok,code):
 if not ok:raise ValueError(code)
def pinned(p,h=None,artifact=False):
 p=Path(p);s=p.lstat();need(p.is_absolute() and p.resolve()==p and stat.S_ISREG(s.st_mode) and (s.st_nlink==1 or artifact) and s.st_size<64*1024**2,'SOURCE_FILE')
 value={'bytes':s.st_size,'sha256':sha(p)};need(h is None or value['sha256']==h,'SHA_'+str(p));need(tuple(getattr(p.lstat(),k) for k in ('st_dev','st_ino','st_mode','st_uid','st_gid','st_size','st_mtime_ns','st_ctime_ns','st_nlink'))==tuple(getattr(s,k) for k in ('st_dev','st_ino','st_mode','st_uid','st_gid','st_size','st_mtime_ns','st_ctime_ns','st_nlink')),'SOURCE_CHANGED');return value
def save(name,value):
 with (ROOT/name).open('x',encoding='utf-8',newline='\n') as f:f.write(json.dumps(value,sort_keys=True,indent=2)+'\n');f.flush();os.fsync(f.fileno())
def artifacts(log,name,manifest=None):
 rows=[]
 for line in log.read_text().splitlines():
  try:a=json.loads(line)
  except ValueError:continue
  if a.get('reason')=='compiler-artifact' and a['target']['name']==name and (manifest is None or Path(a['manifest_path'])==manifest):
   libs=[Path(x) for x in a['filenames'] if x.endswith('.rlib')]
   if libs:need(len(libs)==1 and a['profile']['opt_level']=='3' and not a['profile']['test'] and not a['profile']['debug_assertions'],'OPTIMIZED_ARTIFACT');rows.append((libs[0],a))
 need(len(rows)==1,'UNIQUE_ACTUAL_ARTIFACT_'+name);return rows[0]
def identity(native):
 h=hashlib.sha256();pins={}
 for n in INPUTS:pins[n]=pinned(native/n);h.update((native/n).read_bytes());h.update(b'\0')
 return h.hexdigest(),pins

def build():
 need(sys.platform.startswith('linux') and len(sys.argv)==2 and sha(__file__)==sys.argv[1],'LINUX_REVIEWED_SCRIPT_SHA')
 need(not os.environ.get('RUSTFLAGS') and not os.environ.get('CARGO_ENCODED_RUSTFLAGS'),'UNREVIEWED_COMPILER_FLAGS')
 need(shutil.disk_usage('/tmp').free>8*1024**3,'SCRATCH_HEADROOM');pinned(ARCHIVE,ARCHIVE_SHA)
 pinned(BASE/'source-manifest.json',MANIFEST_SHA);pinned(BASE/'release.log',RELEASE_SHA);pinned(BASE/'validation.json',VALIDATION_SHA)
 manifest=json.loads((BASE/'source-manifest.json').read_bytes());validation=json.loads((BASE/'validation.json').read_bytes())
 need(len(manifest['sourceHashes'])==251 and manifest['sourceCommit']=='774f94fe6938f55ed447d078965cc583810cb3e7','BASE_SOURCE_SCOPE')
 for n,h in manifest['sourceHashes'].items():pinned(BASE/n,h)
 old_native=BASE/'moloch/native';old_id,old_sources=identity(old_native);need(old_id==OLD==validation['engineIdentity'],'BASE_IDENTITY')
 old_rlib,old_art=artifacts(BASE/'release.log','moloch_replay',old_native/'Cargo.toml');old_json,old_json_art=artifacts(BASE/'release.log','serde_json')
 pinned(old_rlib,validation['nativeRlibSha256'],artifact=True);pinned(old_json,artifact=True)
 need(sorted(old_json_art['features'])==['arbitrary_precision','default','std'],'BASE_JSON_FEATURES')
 ROOT.mkdir(mode=0o700)
 with zipfile.ZipFile(ARCHIVE) as z:
  need(set(z.namelist())=={'engine.rs','offline_probe.rs','package.json'},'EXACT_PACKAGE')
  package=json.loads(z.read('package.json'));need(package['candidateCommit']=='b2202660acdff64da566977b2b333af334df8b35' and package['offlineOnly'] and package['productionAuthorization'] is False,'PACKAGE_CLASS')
  for n in ['engine.rs','offline_probe.rs']:
   raw=z.read(n);need(len(raw)==package['files'][n]['bytes'] and hashlib.sha256(raw).hexdigest()==package['files'][n]['sha256'],'PACKAGE_BYTES');(ROOT/n).write_bytes(raw)
 native=ROOT/'moloch/native'
 for n,h in manifest['sourceHashes'].items():
  if n.startswith('moloch/native/'):
   dest=ROOT/n;dest.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(BASE/n,dest);pinned(dest,h)
 shutil.copyfile(ROOT/'engine.rs',native/'src/engine.rs')
 new_id,new_sources=identity(native);need(new_id!=OLD and [n for n in INPUTS if old_sources[n]!=new_sources[n]]==['src/engine.rs'],'ONLY_ENGINE_DELTA')
 env=dict(os.environ,CARGO_BUILD_JOBS='1',CARGO_TARGET_DIR='/tmp/moloch-native-flow-build-b2202660-20261016/target',PATH=str(Path.home()/'.cargo/bin')+os.pathsep+os.environ['PATH'])
 for p in native.rglob('*'):
  if p.is_file():os.utime(p,None)
 evidence={'offlineOnly':True,'productionAuthorization':False,'startedAt':dt.datetime.now(dt.timezone.utc).isoformat(),'archive':pinned(ARCHIVE),'sourceManifest':pinned(BASE/'source-manifest.json'),'baselineEngineIdentity':OLD,'candidateEngineIdentity':new_id,'baselineSourceFiles':old_sources,'candidateSourceFiles':new_sources,'checks':{},'buildFlags':{'cargoRelease':'opt3/thinLTO/codegen1 from exact Cargo.toml','probe':['--edition=2024','-C','opt-level=3','-C','lto=thin','-C','codegen-units=1'],'jobs':1,'nice':19}}
 def run(name,args,cwd=ROOT,seconds=1800):
  started=time.monotonic()
  with (ROOT/(name+'.log')).open('xb') as f:
   process=subprocess.Popen(['nice','-n','19',*map(str,args)],cwd=cwd,env=env,stdout=f,stderr=subprocess.STDOUT,start_new_session=True)
   try:code=process.wait(timeout=seconds)
   except subprocess.TimeoutExpired:
    os.killpg(process.pid,signal.SIGTERM)
    try:process.wait(timeout=5)
    except subprocess.TimeoutExpired:os.killpg(process.pid,signal.SIGKILL);process.wait()
    code='timeout'
  row={'exitCode':code,'seconds':time.monotonic()-started,'log':pinned(ROOT/(name+'.log'))};evidence['checks'][name]=row;save(name+'-receipt.json',row);need(code==0,'FAILED_'+name);return ROOT/(name+'.log')
 run('compiler',['rustc','-vV'],seconds=30)
 log=run('native-library',['cargo','test','--manifest-path',native/'Cargo.toml','--locked','--offline','--lib','--features','serde_json/arbitrary_precision','--','--test-threads=1'])
 summaries=re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out;',log.read_text())
 need(len(summaries)==1 and summaries[0][1]=='0' and summaries[0][3:] == ('0','0'),'FULL_LIB_TEST_SUMMARY');evidence['nativeTestSummary']=summaries[0]
 need(len(re.findall(r'test engine::activity_window_tests::[^ ]+ \.\.\. ok',log.read_text()))==5,'FIVE_FLOW_TESTS_ACTUALLY_PASS')
 release=run('native-release',['cargo','build','--manifest-path',native/'Cargo.toml','--locked','--offline','--release','--lib','--features','serde_json/arbitrary_precision','--message-format=json'])
 new_rlib,new_art=artifacts(release,'moloch_replay',native/'Cargo.toml');new_json,new_json_art=artifacts(release,'serde_json');need(new_json_art['features']==old_json_art['features'] and new_art['profile']==old_art['profile'],'MATCHED_NATIVE_JSON_PROFILES')
 evidence['artifacts']={}
 for role,rlib,serde,art,jart,source in [('baseline',old_rlib,old_json,old_art,old_json_art,old_native),('candidate',new_rlib,new_json,new_art,new_json_art,native)]:
  out=ROOT/(role+'-probe');run(role+'-probe-build',['rustc','--edition=2024','-C','opt-level=3','-C','lto=thin','-C','codegen-units=1',ROOT/'offline_probe.rs','-L','dependency='+str(rlib.parent),'--extern','moloch_replay='+str(rlib),'--extern','serde_json='+str(serde),'-o',out])
  found=run(role+'-identity',[out,'identity'],seconds=30).read_text().strip();need(found==(OLD if role=='baseline' else new_id),'ACTUAL_COMPILED_IDENTITY')
  evidence['artifacts'][role]={'probePath':str(out),'probe':pinned(out),'nativeRlibPath':str(rlib),'nativeRlib':pinned(rlib,artifact=True),'nativeCargoArtifact':art,'serdeCargoArtifact':jart,'nativeSourceRoot':str(source),'sourceFiles':old_sources if role=='baseline' else new_sources,'engineIdentity':found,'optimizedProfile':'release','buildEvidencePath':str(BASE/'release.log' if role=='baseline' else release),'buildEvidence':pinned(BASE/'release.log' if role=='baseline' else release)}
 for n,h in manifest['sourceHashes'].items():pinned(BASE/n,h)
 need(identity(native)[1]==new_sources,'CANDIDATE_SOURCE_UNCHANGED');evidence['probeSource']=pinned(ROOT/'offline_probe.rs');evidence['finishedAt']=dt.datetime.now(dt.timezone.utc).isoformat();save('build-validation.json',evidence)
 print(json.dumps({'buildOnly':True,'output':str(ROOT),'candidateEngineIdentity':new_id,'validation':pinned(ROOT/'build-validation.json'),'bankReplayRun':False,'bridgeProposed':False,'productionAuthorization':False}))
if __name__=='__main__':build()
