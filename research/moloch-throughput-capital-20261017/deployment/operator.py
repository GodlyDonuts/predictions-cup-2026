#!/usr/bin/env python3
"""PRIVATE REVIEW-ONLY controlled binary-only ACTIVE-overlay rebase successor. NO MODES EXECUTED. Owner must freeze all None pins; no incident-recovery/bridge/replay operation."""
import pathlib,json,hashlib,subprocess,datetime,os,sys,shutil,fcntl,time,uuid,copy
assert __debug__
helper=pathlib.Path('/tmp/moloch-throughput-rollout.py')
assert hashlib.sha256(helper.read_bytes()).hexdigest()=='1e0da4b462dd0627dc597b725fc592530d6ae3c91babe09d4ecb641d2ce87133'
prefix=helper.read_text().split('command=sys.argv[1]')[0]
assert 'def verified_dispatch(report):' in prefix and 'elif command' not in prefix
exec(compile(prefix,'pinned-helper-prefix','exec'),globals())
OP='moloch-throughput-capital-20261017-r11-rebase-draft'
# Pending actual owner-frozen source/build/review pins: no placeholders count as PASS.
SOURCE='027abb60eef2c1c5e03f077b9eb9d0778aa53b9b'
VALIDATION_SHA='84dcef2f1f716814dbd8eb13b3b0cfa3dbd4665015c988140e9de715a01d51d6'
PRIOR_SHA='889c0a47f311c71bfa3e18830c4f706761579fcf6eaef528664550afd4b2a0b8'
PRIOR_STAGE_PATH='/tmp/moloch-paper7-generation-recovered-10701-r4-staging.json'
PRIOR_STAGE_SHA='3e9fda7c83bb64d0127f68351216cc3bf4b4935bedbd0259f56061a8eb52d67a'
REVIEW_BUNDLE_SHA='7e781ecaf7c57f5f1fd10aeb4f29382d80dbef145e563f14ce2bfceacc00aec8'
EXPECTED_TEST_RESULTS={'native-library': [['90', '0', '1', '0', '0']], 'sdk-library': [['233', '0', '0', '0', '0']], 'runtime-library': [['230', '0', '0', '0', '0']], 'worker': [['119', '0', '0', '0', '0']], 'order_snapshot': [['6', '0', '0', '0', '0']], 'quote_priority': [['15', '0', '0', '0', '0']], 'maker_policy': [['33', '0', '0', '0', '0']], 'execution_distribution': [['25', '0', '0', '0', '0']], 'fundamental_acquisition': [['10', '0', '0', '0', '0']], 'fundamental_policy': [['111', '0', '0', '0', '0']], 'release': None, 'engine-probe-build': None, 'engine-probe-run': None}
ENGINE='b5b28a476fc057de93f5784f7b3600116418aa7e775f25576cfd2906ecd77713'
OLD_PID=577770
priorfile=pathlib.Path('/tmp/moloch-paper7-generation-recovered-10701-r4-production-rollout.json')
assert SOURCE and VALIDATION_SHA and PRIOR_SHA and PRIOR_STAGE_PATH and PRIOR_STAGE_SHA and REVIEW_BUNDLE_SHA and EXPECTED_TEST_RESULTS, 'OWNER_FINAL_FREEZE_REQUIRED'
priorstage=pathlib.Path(PRIOR_STAGE_PATH)
assert sha(priorfile)==PRIOR_SHA and sha(priorstage)==PRIOR_STAGE_SHA
prior=load(priorfile);priors=load(priorstage)
assert prior['newPid']==OLD_PID and prior['engineIdentity']==ENGINE and prior['financialBridgeEntries']==13
OLD_BINARY=prior['binarySha256'];OLD_CONTROL=pathlib.Path(prior['control']);OLD_CONTROL_SHA=prior['controlSha256'];OLD_RELEASE=prior['release']
assert OLD_CONTROL==shared/'moloch-live-paper7-generation-20261016.json'
OLD_UNIT_SHA=priors['pins'][priors['unitCandidate']]
STAGE=pathlib.Path('/tmp/moloch-throughput-entry-r10-20261017')
TARGET=pathlib.Path('/tmp/moloch-paper7-generation-live-20261016/target')
meta=pathlib.Path('/tmp/moloch-throughput-capital-r11-20261017-staging.json')
boundfile=pathlib.Path('/tmp/moloch-throughput-capital-r11-20261017-boundary.json')
receiptfile=pathlib.Path('/tmp/moloch-throughput-capital-r11-20261017-production-rollout.json')
requestfile=root/'throughput-capital-r11-20261017-stop-request.json'
stopfile=root/'EXECUTION_STOP';globalstop=shared/'EXECUTION_STOP'
unit=pathlib.Path.home()/'.config/systemd/user/moloch-live.service'
dropin=unit.parent/'moloch-live.service.d/10-bounded-restart.conf'
controlfile=shared/'moloch-live-throughput-capital-r11-20261017.json'
settingsfile=shared/'fundamental-paper7-expanded-20261017/settings-root-reviewed.json'
reviewbundle=STAGE/'root-reviewed-settings-validation.json'
EXPANDED_EXCHANGES={'872','885','993'}
GENERATION_SHA='9b2c1dc671e0438dc5af8c217fa7e2a2a6c62bd57ccec0c65b2253292bee0a68'
ACTIVE_OVERLAY_SHA='32be077587d31069734e13cb1b834a60b625cad6781c00b0ff2851f5cf8985a0'
ORIGINAL_REQUEST_SHA='9b2c1dc671e0438dc5af8c217fa7e2a2a6c62bd57ccec0c65b2253292bee0a68'
assert GENERATION_SHA and ACTIVE_OVERLAY_SHA and ORIGINAL_REQUEST_SHA, 'OWNER_GENERATION_FREEZE_REQUIRED'
activefile=shared/'fundamental-paper7-expanded-20261017/next-control-root-reviewed.json'
generation_request=shared/'fundamental-paper7-expanded-20261017/owner-request-001.json'

def cleanpath(p):
 p=pathlib.Path(p);assert p.is_absolute()
 for a in [p,*p.parents]:assert not a.is_symlink(),str(a)
 return p
def immutable(p,data,mode=0o400):
 p=cleanpath(p);p.parent.mkdir(parents=True,exist_ok=True,mode=0o700)
 if p.exists():assert p.read_bytes()==data;return
 fd=os.open(p,os.O_WRONLY|os.O_CREAT|os.O_EXCL,mode)
 with os.fdopen(fd,'wb') as f:f.write(data);f.flush();os.fsync(f.fileno())
 fd=os.open(p.parent,os.O_RDONLY|os.O_DIRECTORY);os.fsync(fd);os.close(fd)
def enc(v):return (json.dumps(v,indent=2,ensure_ascii=False)+'\n').encode()
def immutable_json(p,v):immutable(p,enc(v))
def pins_ok(pins):
 for p,h in pins.items():assert sha(cleanpath(p))==h,p
def original_process():
 p=pathlib.Path('/proc')/str(OLD_PID)
 assert pid()==OLD_PID and sha(p/'exe')==OLD_BINARY and os.readlink(p/'exe')==OLD_RELEASE
 assert (p/'cmdline').read_bytes().split(b'\0')==[OLD_RELEASE.encode(),b'run',str(OLD_CONTROL).encode(),b'']
def service_pins():
 assert sha(unit)==OLD_UNIT_SHA
 assert sha(dropin)=='ff4237fb5e0b5f2a68159e4fa43d737eefc68ebdd422f56eea068c4ffba0e795'
 backup=dropin.with_name('10-bounded-restart.conf.before-exec-condition-20261016')
 assert set(p.name for p in dropin.parent.iterdir())=={dropin.name,backup.name}
 assert sha(backup)=='ddbe320f3fcd657d719a2ec28f67282ce2eb0c90ff8bf03eff9308871121936e'
def original_bridge():
 d=seal(bank/'engine-upgrade-v1.json')
 assert sha(bank/'engine-upgrade-v1.json')==prior['bridgeSha256'] and len(d['upgrades'])==13
 assert d['upgrades'][-1]['toEngineIdentity']==ENGINE
 return d
def snapshot():
 h,r,m=(seal(bank/n) for n in ['head.json','report.json','manifest.json'])
 assert h['sequence']==r['sequence'] and h['previous']==r['previous'] and h['manifestHash']==r['manifestHash']==m['hash']
 assert m['hash']=='78acaef952ed95b9bff24a3349ece669ccbe88ef7294b27dd9c2aa7d09cae325'
 original_bridge();state_guards(r);d=verified_dispatch(r)
 return {'head':h,'reportSha256':sha(bank/'report.json'),'prefixBytes':(bank/'events.jsonl').stat().st_size,
 'prefixSha256':sha(bank/'events.jsonl'),'dispatch':d,'pins':{str(p):sha(p) for p in [bank/'manifest.json',bank/'engine-upgrade-v1.json',root/'dispatch.ndjson',root/'fundamental.ndjson',root/'fundamental.initialized.json',root/'package-dispatch/package-dispatch.ndjson']},'fills':len(r['fills']),'reportRealized':r['report']['realized']}
def held():
 assert pid()==0 and not pathlib.Path('/proc',str(OLD_PID)).exists() and not globalstop.exists()
 f=cleanpath(shared/'capture.lock').open('r+')
 try:fcntl.flock(f,fcntl.LOCK_EX|fcntl.LOCK_NB)
 except BaseException:f.close();raise
 return f
def reviewed_validation():
 assert sha(STAGE/'validation.json')==VALIDATION_SHA
 v=load(STAGE/'validation.json')
 expected=set(EXPECTED_TEST_RESULTS)
 assert expected and {'runtime-library','worker','sdk-library','release','engine-probe-build','engine-probe-run'}<=expected
 assert set(v['checks'])==expected and v['sourceCommit']==SOURCE and v['engineIdentity']==ENGINE
 for name,row in v['checks'].items():
  log=STAGE/(name+'.log');assert row['exitCode']==0 and sha(log)==row['logSha256'] and log.stat().st_size==row['logBytes']
  assert row.get('testResults')==EXPECTED_TEST_RESULTS[name]
  if name in expected-{'release','engine-probe-build','engine-probe-run'}:
   assert row.get('testResults') and all(int(x[0])>0 and x[1]=='0' and int(x[2])<=(1 if name=='native-library' else 0) for x in row['testResults'])
 assert (STAGE/'engine-probe-run.log').read_text().strip()==ENGINE
 assert sha(TARGET/'release/moloch-live-runtime')==v['binarySha256']
 return v
def validate_stage(s):
 assert s['sourceCommit']==SOURCE and s['engineIdentity']==ENGINE
 pins_ok(s['pins']);service_pins();original_bridge()
 active=active_generation();after=load(controlfile);check=copy.deepcopy(after)
 check['activation']['binarySha256']=active['activation']['binarySha256']
 assert check==active, 'ONLY_ACTIVE_BINARY_REBASE_ALLOWED'
 assert after['fundamentalPolicy']==reviewed_settings(load(OLD_CONTROL))
 assert after['activation']['binarySha256']==s['binarySha256']
 v=load(STAGE/'validation.json')
 assert v['sourceCommit']==SOURCE and v['engineIdentity']==ENGINE and v['binarySha256']==s['binarySha256'] and all(row['exitCode']==0 for row in v['checks'].values())
 assert sha(TARGET/'release/moloch-live-runtime')==s['binarySha256']
def utc(value):return datetime.datetime.fromisoformat(value.replace('Z','+00:00')).timestamp()
def fresh_trading():
 p=load(root/'progress.json');age=time.time()-utc(p['at'])
 assert p['pid']==OLD_PID and p['phase']=='trading' and p['realtimeHealthy'] is True and 0<=age<=120, 'OLD_WORKER_NOT_FRESH_TRADING'
 # Reversible staging cannot acquire the live Transport's lifetime SDK lock.
 # No unlocked/derived SDK state is treated as quiet evidence here. Only the
 # existing worker may finish its operation; mandatory quiet() runs under held()
 # AFTER the old PID is gone, and again immediately before promotion.
def active_generation():
 assert sha(activefile)==ACTIVE_OVERLAY_SHA and sha(generation_request)==ORIGINAL_REQUEST_SHA==GENERATION_SHA
 request=load(generation_request);active=load(activefile);before=load(OLD_CONTROL)
 assert request['baseControlSha256']==OLD_CONTROL_SHA and request['controlPath']==str(activefile) and request['controlSha256']==ACTIVE_OVERLAY_SHA
 assert request['workerPid']==OLD_PID and request['runId']==before['activation']['runId']
 check=copy.deepcopy(active);check['fundamentalPolicy']=before['fundamentalPolicy']
 assert check==before, 'ORIGINAL_OVERLAY_FINANCIAL_DRIFT'
 assert sha(reviewbundle)==REVIEW_BUNDLE_SHA
 approved=load(reviewbundle);pins_ok(approved['pins'])
 assert approved['generationSha256']==GENERATION_SHA and approved['activeOverlaySha256']==ACTIVE_OVERLAY_SHA and approved['oldBaseSha256']==OLD_CONTROL_SHA
 previous='0'*64;sequence=0;generations=[]
 import re
 with (root/'fundamental.ndjson').open('rb') as f:
  for raw in f:
   assert len(raw)<=8*1024*1024
   row=json.loads(raw);unsigned,n=re.subn(rb'"hash":"'+row['hash'].encode()+rb'",',b'',raw.strip(),count=1)
   assert n==1 and hashlib.sha256(unsigned).hexdigest()==row['hash'] and row['sequence']==sequence+1 and row['previous']==previous
   previous=row['hash'];sequence=row['sequence']
   if row['event']['kind']=='review_generation':generations.append((raw,row['event']))
 assert len(generations)==1, 'UNEXPECTED_GENERATION_CHANGE'
 raw,event=generations[0];meta=event['generation']
 assert hashlib.sha256(raw).hexdigest()==approved['generationRowRawSha256']
 assert meta==approved['originalGenerationMetadata'] and meta['generationSha256']==GENERATION_SHA
 assert meta['request']==request and meta['requestBodyText'].encode()==generation_request.read_bytes()
 assert {r['review']['exchangeId'] for r in event['reviews']}==set(approved['allExchangeIds'])
 identity=meta['controlIdentity'];st=activefile.stat()
 assert identity==approved['originalControlIdentity']
 # The exact platform descriptor schema is owner-frozen; pin original held inode/device.
 assert st.st_dev==approved['activeOverlayDevice'] and st.st_ino==approved['activeOverlayInode'] and st.st_size==approved['activeOverlayBytes']
 return active

def state_guards(r):
 active_generation();d=verified_dispatch(r)
 assert set(r['report']['ordinaryEntryExclusions']['managedExchangeIds'])==set(load(reviewbundle)['allExchangeIds'])
 assert '10701' in d['provedAborted']
 assert sha(reviewbundle)==REVIEW_BUNDLE_SHA
 recovery=load(reviewbundle)['q10701Recovery']
 assert recovery['qid']==10701 and recovery['sdkArchiveReverified'] is True and recovery['networkRequests']==0 and recovery['noRetry'] is True and recovery['verificationExitCode']==0
 assert recovery['archiveSha256']==d['provedAborted']['10701']
 pins_ok(recovery['pins'])
 q=next(q for q in r['orders'] if q['id']==10701)
 assert q['retired'] is True and q['filled']==0 and not any(f['orderId']==10701 for f in r['fills'])
 # All nine scopes are ALREADY managed by the original committed generation.
 # Do not require them to remain flat or discard fills/ACKs accepted since install.
 cancel_attempts=set();cancel_acks=set();closed=set()
 for raw in (root/'dispatch.ndjson').open('rb'):
  e=json.loads(raw)['event']
  if e['kind']=='closed':closed.add(e['qid'])
  if e['kind']=='cancel_attempt':cancel_attempts.add(e['qid'])
  if e['kind']=='cancel_ack':cancel_acks.add(e['qid'])
 assert not (cancel_attempts-cancel_acks-closed), 'UNRESOLVED_CANCEL_ATTEMPT'

def reviewed_settings(before):
 assert sha(reviewbundle)==REVIEW_BUNDLE_SHA
 v=load(reviewbundle);assert v['executionOwnerReviewed'] is True and v['typedSettingsVerified'] is True and v['sourceCommit']==SOURCE
 assert v['settingsPath']==str(settingsfile) and sha(settingsfile)==v['settingsRawSha256']
 assert v['originallyAddedExchanges']==sorted(EXPANDED_EXCHANGES) and v['withheldExchanges']==['1004']
 settings=load(settingsfile);old=before['fundamentalPolicy']
 assert settings['version']==old['version']==1 and settings['allowInitialization'] is False and len(settings['reviews'])==9 and len(old['reviews'])==6
 assert all(row in settings['reviews'] for row in old['reviews'])
 assert len({row['path'] for row in settings['reviews']})==9
 assert sha(settings['authorityPath'])==settings['authoritySha256']
 pins_ok(v['pins'])
 actual=set()
 for row in settings['reviews']:
  assert sha(row['path'])==row['fileSha256'];review=load(row['path']);actual.add(review['exchangeId'])
  for source in row['sourceFiles']:assert sha(source['path'])==source['sha256']
 assert settings==load(activefile)['fundamentalPolicy']
 assert actual==set(v['allExchangeIds']) and len(actual)==9 and EXPANDED_EXCHANGES<=actual and '1004' not in actual
 # Actual Rust offline verification output/log is separately frozen by root.
 assert v['verificationExitCode']==0 and sha(v['verificationLogPath'])==v['verificationLogSha256']
 return settings
def stopped_causally(request):
 status=load(root/'status.json');stop=load(stopfile)
 assert request['operator']==OP and request['oldPid']==OLD_PID and request['reason']=='CONTROLLED_THROUGHPUT_CAPITAL_UPGRADE'
 assert status['pid']==OLD_PID and status['mode']=='stopped' and status['reason'] in ['LIVE_EXECUTION_STOP','STOPPED_OR_OWNER_LOST','LIVE_EXIT_REFRESH_CONTROL_OR_STOP']
 # refresh_controls maps a failed can_read (including this owner STOP) to
 # LIVE_EXIT_REFRESH_CONTROL_OR_STOP. The generic reason alone is not a witness:
 # stage/control/source pins, the separate exact root request and bound hashes,
 # old PID exit, quiet SDK and independent approval are all still required.
 assert request['stagingSha256']==sha(meta) and len(request['token'])==32 and all(c in '0123456789abcdef' for c in request['token'])
 assert not globalstop.exists()
 assert stop['reason']==status['reason'] and utc(request['at'])<=utc(stop['at'])<=utc(status['at'])<=time.time()
 return True

def mode_stage():
 assert not meta.exists() and not boundfile.exists() and not requestfile.exists() and not receiptfile.exists()
 original_process();fresh_trading();service_pins();original_bridge();state_guards(seal(bank/'report.json'))
 assert sha(OLD_CONTROL)==OLD_CONTROL_SHA and not stopfile.exists() and not globalstop.exists()
 v=reviewed_validation();m=load(STAGE/'integrated-source-manifest.json')
 assert v['sourceCommit']==SOURCE and v['engineIdentity']==ENGINE and m['nativeSourceBytesUnchanged']
 assert v['sourceManifestSha256']==sha(STAGE/'integrated-source-manifest.json') and all(r['exitCode']==0 for r in v['checks'].values())
 for p,h in m['sourceHashes'].items():assert sha(STAGE/p)==h,p
 binary=v['binarySha256'];release=base/'releases'/('throughput-capital-'+binary[:12])/'moloch-live-runtime'
 immutable(release,(TARGET/'release/moloch-live-runtime').read_bytes(),0o500);assert sha(release)==binary
 before=load(OLD_CONTROL);after=copy.deepcopy(active_generation())
 assert after['fundamentalPolicy']==reviewed_settings(before)
 after['activation']['binarySha256']=binary
 immutable_json(controlfile,after)
 text=unit.read_text();oldline='ExecStart='+OLD_RELEASE+' run '+str(OLD_CONTROL);assert text.count(oldline)==1
 candidate=STAGE/'unit-candidate.service';immutable(candidate,text.replace(oldline,'ExecStart='+str(release)+' run '+str(controlfile)).encode())
 paths=[activefile,generation_request,settingsfile,reviewbundle,controlfile,release,candidate,STAGE/'validation.json',STAGE/'integrated-source-manifest.json',OLD_CONTROL,priorfile,priorstage]
 s={'at':now(),'operator':OP,'sourceCommit':SOURCE,'engineIdentity':ENGINE,'binarySha256':binary,'release':str(release),'unitCandidate':str(candidate),
 'controlChanges':['activation.binarySha256 relative to ORIGINAL active overlay'],'generationSha256':GENERATION_SHA,'activeOverlaySha256':ACTIVE_OVERLAY_SHA,'oldBaseSha256':OLD_CONTROL_SHA,'newBaseSha256':sha(controlfile),'financialBridgeChanged':False,
 'pins':{str(p):sha(p) for p in paths},'priorBridgeSha256':prior['bridgeSha256'],'financialBridgeEntries':13}
 validate_stage(s);immutable_json(meta,s);print(json.dumps({'staged':True,'sourceCommit':SOURCE,'stagingSha256':sha(meta),'productionChanged':False}))
def mode_stop(expected):
 assert sha(meta)==expected and not boundfile.exists() and not requestfile.exists() and not receiptfile.exists()
 s=load(meta);validate_stage(s);original_process();fresh_trading();state_guards(seal(bank/'report.json'));assert not stopfile.exists() and not globalstop.exists()
 request={'operator':OP,'token':uuid.uuid4().hex,'at':now(),'oldPid':OLD_PID,'reason':'CONTROLLED_THROUGHPUT_CAPITAL_UPGRADE','stagingSha256':expected,'action':'finish current operation and stop; preserve all original orders and bank'}
 immutable_json(requestfile,request);immutable_json(stopfile,request)
 for _ in range(240):
  if pid()==0:break
  time.sleep(.5)
 with held():
  assert stopped_causally(load(requestfile))
  snap=snapshot();q=quiet();assert snapshot()==snap
  immutable_json(boundfile,{'at':now(),'operator':OP,'stagingSha256':expected,'snapshot':snap,'requestSha256':sha(requestfile),'stopSha256':sha(stopfile),'workerStatusSha256':sha(root/'status.json'),'workerStatus':load(root/'status.json'),'queueQuiet':{'globalUnknownCount':q['globalUnknownCount'],'active':q['state']['active'],'manualReview':q['state']['manual_review']}})
 print(json.dumps({'stopped':True,'boundarySha256':sha(boundfile),'sequence':snap['head']['sequence'],'fills':snap['fills']}))
def mode_promote(approvalpath,approvalsha):
 assert sha(approvalpath)==approvalsha and not receiptfile.exists()
 s=load(meta);b=load(boundfile);a=load(approvalpath)
 assert a=={'operator':OP,'executionOwner':'Rhaegal-Viserion-paper6-20261016','executionOwnerReviewed':True,'stagingSha256':sha(meta),'boundarySha256':sha(boundfile),'requestSha256':sha(requestfile),'stopSha256':sha(stopfile),'workerStatusSha256':sha(root/'status.json'),'generationSha256':GENERATION_SHA,'activeOverlaySha256':ACTIVE_OVERLAY_SHA,'oldBaseSha256':OLD_CONTROL_SHA,'newBaseSha256':sha(controlfile),'reason':'root-reviewed-original-active-overlay-binary-only-rebase-controlled-stop-not-human-stop'}
 assert sha(requestfile)==b['requestSha256']
 assert b['stagingSha256']==sha(meta) and load(requestfile)['operator']==OP and load(requestfile)['oldPid']==OLD_PID
 with held():
  assert stopped_causally(load(requestfile))
  validate_stage(s);assert snapshot()==b['snapshot'] and sha(stopfile)==b['stopSha256'] and sha(root/'status.json')==b['workerStatusSha256']
  quiet();assert snapshot()==b['snapshot'];original_bridge()
  immutable(unit.with_name(unit.name+'.before-throughput-capital-20261017'),unit.read_bytes())
  atomic(unit,pathlib.Path(s['unitCandidate']).read_bytes());subprocess.run(['systemctl','--user','daemon-reload'],check=True)
  assert sha(unit)==s['pins'][s['unitCandidate']] and sha(bank/'engine-upgrade-v1.json')==prior['bridgeSha256']
  assert not globalstop.exists() and sha(stopfile)==b['stopSha256']
  archive=root/('EXECUTION_STOP.throughput-capital-'+load(requestfile)['token']+'.json');assert not archive.exists();os.rename(stopfile,archive);assert sha(archive)==b['stopSha256']
 try:
  assert pid()==0 and not pathlib.Path('/proc',str(OLD_PID)).exists() and not stopfile.exists() and not globalstop.exists()
  subprocess.run(['systemctl','--user','reset-failed','moloch-live.service'],check=True);subprocess.run(['systemctl','--user','start','moloch-live.service'],check=True)
  newpid=pid();p=pathlib.Path('/proc')/str(newpid);assert newpid>0 and newpid!=OLD_PID and sha(p/'exe')==s['binarySha256'] and os.readlink(p/'exe')==s['release']
  assert (p/'cmdline').read_bytes().split(b'\0')==[s['release'].encode(),b'run',str(controlfile).encode(),b'']
 except BaseException:
  if not stopfile.exists():save(stopfile,{'operator':OP,'reason':'THROUGHPUT_CAPITAL_START_FAILED_NO_RETRY','at':now()})
  raise
 result={'at':now(),'operator':OP,'oldPid':OLD_PID,'newPid':newpid,'sourceCommit':SOURCE,'binarySha256':s['binarySha256'],'engineIdentity':ENGINE,
 'release':s['release'],'control':str(controlfile),'controlSha256':sha(controlfile),'stagingSha256':sha(meta),'approvalSha256':approvalsha,
 'bridgeSha256':sha(bank/'engine-upgrade-v1.json'),'financialBridgeEntries':13,'boundary':b,'controlChanges':s['controlChanges'],'financialHistoryChangedByOperator':False,'freshTradingVerified':False,'profitabilityEstablished':False}
 immutable_json(receiptfile,result);print(json.dumps({'startedRestoring':True,'newPid':newpid,'binarySha256':s['binarySha256'],'engineIdentity':ENGINE}))
if __name__=='__main__':
 mode=sys.argv[1]
 if mode=='stage' and len(sys.argv)==2:mode_stage()
 elif mode=='stop' and len(sys.argv)==3:mode_stop(sys.argv[2])
 elif mode=='promote' and len(sys.argv)==4:mode_promote(sys.argv[2],sys.argv[3])
 else:raise ValueError('explicit stage|stop STAGE_SHA|promote OWNER_APPROVAL RAW_SHA required')
