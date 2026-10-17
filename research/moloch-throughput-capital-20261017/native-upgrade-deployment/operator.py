#!/usr/bin/env python3
"""OWNER-FROZEN R13 native bridge+binary-only original-active-generation rebase. Each operational mode is explicit; no mode is executed by freezing."""
import pathlib,json,hashlib,subprocess,datetime,os,sys,shutil,fcntl,time,uuid,copy
assert __debug__
helper=pathlib.Path('/tmp/moloch-throughput-rollout.py')
assert hashlib.sha256(helper.read_bytes()).hexdigest()=='1e0da4b462dd0627dc597b725fc592530d6ae3c91babe09d4ecb641d2ce87133'
prefix=helper.read_text().split('command=sys.argv[1]')[0]
assert 'def verified_dispatch(report):' in prefix and 'elif command' not in prefix
exec(compile(prefix,'pinned-helper-prefix','exec'),globals())
OP='moloch-throughput-capital-20261017-r13-enginebridge'
# Pending actual owner-frozen source/build/review pins: no placeholders count as PASS.
SOURCE='7c11cd1c49579deb71ab98afe77114ccc6798f12'
VALIDATION_SHA='88fedde0c1d2e4167d6b41b48154172f2bbf8b9bccacb0411c8587f239a413b8'
PRIOR_SHA='6f06ac060fe79bac87a017501b28cab4b4a5e75b5ecc0f72d625f1c8caa960d3'
PRIOR_STAGE_PATH='/tmp/moloch-throughput-capital-r11-20261017-staging.json'
PRIOR_STAGE_SHA='41abf1eaf638739506763e12874b220dfbea2b85c39d3f896c5e868ba7eacb4d'
REVIEW_BUNDLE_SHA='518d4b69c5a61a2f75e45e193146ca8005f703c9aa0e299c6bcb377108700bf1'
EXPECTED_TEST_RESULTS={'sdk-library': [['233', '0', '0', '0', '0']], 'native-library': [['98', '0', '1', '0', '0']], 'runtime-library': [['230', '0', '0', '0', '0']], 'worker': [['119', '0', '0', '0', '0']], 'order_snapshot': [['6', '0', '0', '0', '0']], 'quote_priority': [['15', '0', '0', '0', '0']], 'maker_policy': [['33', '0', '0', '0', '0']], 'execution_distribution': [['25', '0', '0', '0', '0']], 'fundamental_acquisition': [['10', '0', '0', '0', '0']], 'fundamental_policy': [['114', '0', '0', '0', '0']], 'fundamental_quantity': [['2', '0', '0', '0', '0']], 'fundamental_source_bounds': [['7', '0', '0', '0', '0']], 'release': None, 'engine-probe-build': None, 'engine-probe-run': None, 'bridge-helper-build': None}
ENGINE='7a74f8c3a53119e158dae8338cd09d68140fc4299f09d4fb35ac060be5be7efa'
OLD_PID=584167
OLD_ENGINE='b5b28a476fc057de93f5784f7b3600116418aa7e775f25576cfd2906ecd77713'
BRIDGE_HELPER_SHA='bb020311eb780a3bac96b540ebc620fe0dbe701e78eb69867a3701ef74a41e5e'
STARTUP_GATE_SHA='d2ade49ed4747d1a50091d02fdf7d682823e88e000f80c28d91c05d9ae799d58'
PARITY_SHA='90ab493bd6f4850883065b8a3029bf329ca1a140f61e3321e99c080b001910d6'
STAGE_PATH='/tmp/moloch-capital-native-r16-20261017'
TARGET_PATH='/tmp/moloch-paper7-generation-live-20261016/target'
ACTIVE_OVERLAY_PATH='/home/archon/apps/moloch/shared/fundamental-paper7-stale-refresh-20261017/next-control-root-reviewed.json'
SETTINGS_PATH='/home/archon/apps/moloch/shared/fundamental-paper7-stale-refresh-20261017/settings-root-reviewed.json'
assert ENGINE and ENGINE!=OLD_ENGINE and BRIDGE_HELPER_SHA and STARTUP_GATE_SHA and PARITY_SHA and STAGE_PATH and TARGET_PATH and ACTIVE_OVERLAY_PATH and SETTINGS_PATH, 'OWNER_NATIVE_FREEZE_REQUIRED'
priorfile=pathlib.Path('/tmp/moloch-throughput-capital-r11-20261017-production-rollout.json')
assert SOURCE and VALIDATION_SHA and PRIOR_SHA and PRIOR_STAGE_PATH and PRIOR_STAGE_SHA and REVIEW_BUNDLE_SHA and EXPECTED_TEST_RESULTS, 'OWNER_FINAL_FREEZE_REQUIRED'
priorstage=pathlib.Path(PRIOR_STAGE_PATH)
assert sha(priorfile)==PRIOR_SHA and sha(priorstage)==PRIOR_STAGE_SHA
prior=load(priorfile);priors=load(priorstage)
assert prior['newPid']==OLD_PID and prior['engineIdentity']==OLD_ENGINE and prior['financialBridgeEntries']==13
OLD_BINARY=prior['binarySha256'];OLD_CONTROL=pathlib.Path(prior['control']);OLD_CONTROL_SHA=prior['controlSha256'];OLD_RELEASE=prior['release']
assert OLD_CONTROL.is_absolute()
OLD_UNIT_SHA=priors['pins'][priors['unitCandidate']]
STAGE=pathlib.Path(STAGE_PATH)
TARGET=pathlib.Path(TARGET_PATH)
meta=pathlib.Path('/tmp/moloch-throughput-capital-r13-20261017-staging.json')
boundfile=pathlib.Path('/tmp/moloch-throughput-capital-r13-20261017-boundary.json')
receiptfile=pathlib.Path('/tmp/moloch-throughput-capital-r13-20261017-production-rollout.json')
requestfile=root/'throughput-capital-r13-20261017-stop-request.json'
stopfile=root/'EXECUTION_STOP';globalstop=shared/'EXECUTION_STOP'
unit=pathlib.Path.home()/'.config/systemd/user/moloch-live.service'
dropin=unit.parent/'moloch-live.service.d/10-bounded-restart.conf'
controlfile=shared/'moloch-live-throughput-capital-r13-20261017.json'
settingsfile=pathlib.Path(SETTINGS_PATH)
reviewbundle=STAGE/'root-reviewed-settings-validation.json'
GENERATION_SHA='1bda14d8f71e80987792e36827f3e70c1338dc63f52ae8103d511238f088bee1'
ACTIVE_OVERLAY_SHA='49ad28ce52dd2c2cd0983710aa7a3b8f7212f5d2fec77165032a4ad832147325'
ORIGINAL_REQUEST_SHA='1bda14d8f71e80987792e36827f3e70c1338dc63f52ae8103d511238f088bee1'
assert GENERATION_SHA and ACTIVE_OVERLAY_SHA and ORIGINAL_REQUEST_SHA, 'OWNER_GENERATION_FREEZE_REQUIRED'
activefile=pathlib.Path(ACTIVE_OVERLAY_PATH)
proposalfile=pathlib.Path('/tmp/moloch-throughput-capital-r13-20261017-proposed-bridge.json')
PARITY=STAGE/'root-reviewed-financial-prefix-parity.json'
# All original generation requests are enumerated in the root-pinned review bundle.

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
 assert d['upgrades'][-1]['toEngineIdentity']==OLD_ENGINE
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
 assert expected and {'native-library','runtime-library','worker','sdk-library','release','engine-probe-build','engine-probe-run','bridge-helper-build'}<=expected
 assert set(v['checks'])==expected and v['sourceCommit']==SOURCE and v['engineIdentity']==ENGINE
 for name,row in v['checks'].items():
  log=STAGE/(name+'.log');assert row['exitCode']==0 and sha(log)==row['logSha256'] and log.stat().st_size==row['logBytes']
  assert row.get('testResults')==EXPECTED_TEST_RESULTS[name]
  if name in expected-{'release','engine-probe-build','engine-probe-run','bridge-helper-build'}:
   assert row.get('testResults') and all(int(x[0])>0 and x[1]=='0' and int(x[2])<=(1 if name=='native-library' else 0) for x in row['testResults'])
 assert (STAGE/'engine-probe-run.log').read_text().strip()==ENGINE
 assert sha(TARGET/'release/moloch-live-runtime')==v['binarySha256']
 return v
def startup_gate():
 p=STAGE/'root-reviewed-startup-parity-gate.json';assert sha(p)==STARTUP_GATE_SHA
 g=load(p);assert g['executionOwnerReviewed'] is True and g['finalPrefixParityVerified'] is False and g['actualStartupMustValidateBeforeFinancialWrites'] is True
 assert g['sourceCommit']==SOURCE and g['binarySha256']==load(STAGE/'validation.json')['binarySha256'] and g['nativeEngineIdentity']==ENGINE
 pins_ok(g['pins']);m=load(STAGE/'integrated-source-manifest.json')
 assert {'moloch/live-runtime/src/worker.rs','moloch/native/src/session.rs'}<=set(g['sourceHashes']), 'STARTUP_GATE_REQUIRED_SOURCES'
 for path,h in g['sourceHashes'].items():assert m['sourceHashes'][path]==h and sha(STAGE/path)==h
 return g

def validate_stage(s):
 startup_gate()
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
 assert sha(activefile)==ACTIVE_OVERLAY_SHA and sha(reviewbundle)==REVIEW_BUNDLE_SHA
 approved=load(reviewbundle);pins_ok(approved['pins'])
 assert approved['generationSha256']==GENERATION_SHA and approved['activeOverlaySha256']==ACTIVE_OVERLAY_SHA and approved['currentBaseSha256']==OLD_CONTROL_SHA
 expected=approved['generations'];assert 1<=len(expected)<=4096
 previous='0'*64;sequence=0;found=[]
 import re
 with (root/'fundamental.ndjson').open('rb') as f:
  for raw in f:
   assert len(raw)<=8*1024*1024
   row=json.loads(raw);unsigned,n=re.subn(rb'"hash":"'+row['hash'].encode()+rb'",',b'',raw.strip(),count=1)
   assert n==1 and hashlib.sha256(unsigned).hexdigest()==row['hash'] and row['sequence']==sequence+1 and row['previous']==previous
   previous=row['hash'];sequence=row['sequence']
   if row['event']['kind']=='review_generation':found.append((raw,row['event']))
 assert len(found)==len(expected), 'UNEXPECTED_GENERATION_CHAIN'
 prior_generation='0'*64
 for (raw,event),pin in zip(found,expected):
  meta=event['generation'];request=meta['request'];requestfile=cleanpath(pin['requestPath']);originalbase=cleanpath(pin['originalBasePath']);overlay=cleanpath(request['controlPath'])
  assert hashlib.sha256(raw).hexdigest()==pin['rowRawSha256'] and meta==pin['originalMetadata']
  assert sha(requestfile)==meta['generationSha256']==pin['generationSha256']
  assert requestfile.read_bytes()==meta['requestBodyText'].encode() and load(requestfile)==request
  assert request['previousGenerationSha256']==prior_generation
  assert sha(originalbase)==request['baseControlSha256']==pin['originalBaseSha256']
  assert sha(overlay)==request['controlSha256']==pin['activeOverlaySha256']
  old=load(originalbase);new=load(overlay);compare=copy.deepcopy(new);compare['fundamentalPolicy']=old['fundamentalPolicy'];assert compare==old
  fp=new['fundamentalPolicy'];assert sha(fp['authorityPath'])==fp['authoritySha256']
  for review in fp['reviews']:
   assert sha(review['path'])==review['fileSha256']
   for source in review['sourceFiles']:assert sha(source['path'])==source['sha256']
  assert request['runId']==load(OLD_CONTROL)['activation']['runId'] and request['bankManifestSha256']==seal(bank/'manifest.json')['hash']
  assert meta['controlIdentity']==pin['originalControlIdentity']
  st=overlay.stat();assert st.st_dev==pin['overlayDevice'] and st.st_ino==pin['overlayInode'] and st.st_size==pin['overlayBytes']
  assert {r['review']['exchangeId'] for r in event['reviews']}==set(pin['allExchangeIds'])
  prior_generation=meta['generationSha256']
 assert prior_generation==GENERATION_SHA and expected[-1]['activeOverlaySha256']==ACTIVE_OVERLAY_SHA
 assert expected[-1]['generationSha256']==ORIGINAL_REQUEST_SHA==GENERATION_SHA
 assert request['controlPath']==str(activefile)
 return load(activefile)

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
 # The root-pinned scopes are already managed by the committed generation chain.
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
 settings=load(settingsfile)
 assert settings==load(activefile)['fundamentalPolicy']
 assert settings['version']==1 and settings['allowInitialization'] is False and 1<=len(settings['reviews'])<=32
 assert len({row['path'] for row in settings['reviews']})==len(settings['reviews'])
 assert sha(settings['authorityPath'])==settings['authoritySha256'];pins_ok(v['pins']);actual=set()
 for row in settings['reviews']:
  assert sha(row['path'])==row['fileSha256'];review=load(row['path']);actual.add(review['exchangeId'])
  for source in row['sourceFiles']:assert sha(source['path'])==source['sha256']
 assert actual==set(v['allExchangeIds']) and len(actual)==len(settings['reviews'])
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
 assert v['sourceCommit']==SOURCE and v['engineIdentity']==ENGINE and m['nativeSourceBytesUnchanged'] is False
 assert v['sourceManifestSha256']==sha(STAGE/'integrated-source-manifest.json') and all(r['exitCode']==0 for r in v['checks'].values())
 for p,h in m['sourceHashes'].items():assert sha(STAGE/p)==h,p
 baseline=load('/tmp/moloch-throughput-entry-r10-20261017/integrated-source-manifest.json');changes={p:{'oldSha256':baseline['sourceHashes'].get(p),'newSha256':h} for p,h in m['sourceHashes'].items() if p.startswith('moloch/native/') and baseline['sourceHashes'].get(p)!=h};assert changes==load(reviewbundle)['nativeSourceChanges']
 assert sha(STAGE/'moloch-upgrade')==BRIDGE_HELPER_SHA
 require_parity();startup_gate()
 binary=v['binarySha256'];release=base/'releases'/('throughput-capital-'+binary[:12])/'moloch-live-runtime'
 immutable(release,(TARGET/'release/moloch-live-runtime').read_bytes(),0o500);assert sha(release)==binary
 before=load(OLD_CONTROL);after=copy.deepcopy(active_generation())
 assert after['fundamentalPolicy']==reviewed_settings(before)
 after['activation']['binarySha256']=binary
 immutable_json(controlfile,after)
 text=unit.read_text();oldline='ExecStart='+OLD_RELEASE+' run '+str(OLD_CONTROL);assert text.count(oldline)==1
 candidate=STAGE/'unit-candidate.service';immutable(candidate,text.replace(oldline,'ExecStart='+str(release)+' run '+str(controlfile)).encode())
 paths=[PARITY,STAGE/'root-reviewed-startup-parity-gate.json',STAGE/'moloch-upgrade',activefile,settingsfile,reviewbundle,controlfile,release,candidate,STAGE/'validation.json',STAGE/'integrated-source-manifest.json',OLD_CONTROL,priorfile,priorstage]
 s={'at':now(),'operator':OP,'sourceCommit':SOURCE,'engineIdentity':ENGINE,'binarySha256':binary,'release':str(release),'unitCandidate':str(candidate),
 'controlChanges':['activation.binarySha256 relative to ORIGINAL active overlay'],'generationSha256':GENERATION_SHA,'activeOverlaySha256':ACTIVE_OVERLAY_SHA,'oldBaseSha256':OLD_CONTROL_SHA,'newBaseSha256':sha(controlfile),'financialBridgeProposalRequired':True,'startupPrefixParityGateSha256':STARTUP_GATE_SHA,'finalStoppedPrefixParityVerified':False,'fromEngineIdentity':OLD_ENGINE,
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
  immutable_json(boundfile,{'at':now(),'operator':OP,'stagingSha256':expected,'snapshot':snap,'financialPrefixParity':require_parity(),'requestSha256':sha(requestfile),'stopSha256':sha(stopfile),'workerStatusSha256':sha(root/'status.json'),'workerStatus':load(root/'status.json'),'queueQuiet':{'globalUnknownCount':q['globalUnknownCount'],'active':q['state']['active'],'manualReview':q['state']['manual_review']}})
 print(json.dumps({'stopped':True,'boundarySha256':sha(boundfile),'sequence':snap['head']['sequence'],'fills':snap['fills']}))
def require_parity(path=PARITY,expected_sha=PARITY_SHA,boundary=None):
 assert expected_sha and sha(path)==expected_sha
 p=load(path)
 assert p['financialPrefixParityPassed'] is True and p['finalPublicStateExact'] is True and p['originalThirteenBridgesUnchanged'] is True
 assert p['sourceCommit']==SOURCE and p['nativeEngineIdentity']==ENGINE and p['fromEngineIdentity']==OLD_ENGINE
 assert p['productionAuthorization'] is False and not p.get('productionBridgeInstalled',False)
 assert p['originalPrefixSequence']>0 and p['originalPrefixBytes']>0
 pins_ok(p['pins'])
 if boundary is not None:
  snap=boundary['snapshot'];assert p['originalPrefixSequence']==snap['head']['sequence'] and p['originalPrefixBytes']==snap['prefixBytes'] and p['originalPrefixSha256']==snap['prefixSha256']
  assert p['originalReportEnvelopeSha256']==snap['reportSha256'] and p['originalBridgeSha256']==prior['bridgeSha256']
 return {'path':str(path),'sha256':expected_sha,'sequence':p['originalPrefixSequence'],'bytes':p['originalPrefixBytes'],'prefixSha256':p['originalPrefixSha256']}
def validate_proposal(boundary):
 future=seal(proposalfile);old=original_bridge();last=future['upgrades'][-1];snap=boundary['snapshot']
 assert len(future['upgrades'])==14 and future['upgrades'][:-1]==old['upgrades']
 assert last['oldHead']==snap['head'] and last['oldReportEnvelope']==seal(bank/'report.json')
 assert last['journalPrefix']=={'bytes':snap['prefixBytes'],'sha256':snap['prefixSha256']}
 assert last['fromEngineIdentity']==OLD_ENGINE and last['toEngineIdentity']==ENGINE
 assert last['approval']['role']=='human_authorized_execution_owner' and last['approval']['oldBinarySha256']==OLD_BINARY and last['approval']['previousProcessStopped'] is True
 return future
def mode_bridge(stage_sha,boundary_sha):
 assert sha(meta)==stage_sha and sha(boundfile)==boundary_sha and not proposalfile.exists()
 s=load(meta);b=load(boundfile);validate_stage(s);assert require_parity()==b['financialPrefixParity']
 with held():
  assert stopped_causally(load(requestfile))
  assert snapshot()==b['snapshot'] and sha(stopfile)==b['stopSha256'] and sha(requestfile)==b['requestSha256'] and sha(root/'status.json')==b['workerStatusSha256']
  quiet();assert sha(STAGE/'moloch-upgrade')==BRIDGE_HELPER_SHA
  # Checked helper acquires/releases bank writer.lock and ONLY proposes a
  # file; all financial history and original bridge bytes stay unchanged.
  result=subprocess.run([str(STAGE/'moloch-upgrade'),str(bank),OLD_BINARY,now(),str(proposalfile)],capture_output=True,timeout=3600,check=False)
  immutable(STAGE/'bridge-helper.stdout',result.stdout);immutable(STAGE/'bridge-helper.stderr',result.stderr);immutable_json(STAGE/'bridge-helper-exit.json',{'exitCode':result.returncode})
  assert result.returncode==0
  assert snapshot()==b['snapshot'];validate_proposal(b)
 print(json.dumps({'proposedOnly':True,'proposalSha256':sha(proposalfile),'upgrades':14,'boundaryUnchanged':True}))

def mode_promote(approvalpath,approvalsha):
 assert sha(approvalpath)==approvalsha and not receiptfile.exists()
 s=load(meta);b=load(boundfile);a=load(approvalpath)
 assert a=={'operator':OP,'executionOwner':'Rhaegal-Viserion-paper6-20261016','executionOwnerReviewed':True,'stagingSha256':sha(meta),'boundarySha256':sha(boundfile),'requestSha256':sha(requestfile),'stopSha256':sha(stopfile),'workerStatusSha256':sha(root/'status.json'),'proposalSha256':sha(proposalfile),'paritySha256':PARITY_SHA,'startupPrefixParityGateSha256':STARTUP_GATE_SHA,'fromEngineIdentity':OLD_ENGINE,'toEngineIdentity':ENGINE,'generationSha256':GENERATION_SHA,'activeOverlaySha256':ACTIVE_OVERLAY_SHA,'oldBaseSha256':OLD_CONTROL_SHA,'newBaseSha256':sha(controlfile),'reason':'root-reviewed-fourteenth-bridge-original-overlay-binary-rebase-native-startup-must-validate-exact-final-prefix-before-financial-writes'}
 assert sha(requestfile)==b['requestSha256']
 assert b['stagingSha256']==sha(meta) and load(requestfile)['operator']==OP and load(requestfile)['oldPid']==OLD_PID
 with held():
  assert stopped_causally(load(requestfile))
  validate_stage(s);assert snapshot()==b['snapshot'] and sha(stopfile)==b['stopSha256'] and sha(root/'status.json')==b['workerStatusSha256']
  quiet();assert snapshot()==b['snapshot'];original_bridge()
  # Checked prestage parity is actual evidence; final stopped-prefix parity is
  # NOT claimed here. The pinned unchanged native startup validates all14 exact
  # financial prefixes before the worker constructs financial-write authority.
  startup_gate()
  future=validate_proposal(b)
  assert require_parity()==b['financialPrefixParity']
  immutable(unit.with_name(unit.name+'.before-throughput-capital-r13-20261017'),unit.read_bytes())
  immutable((bank/'engine-upgrade-v1.json').with_name('engine-upgrade-v1.json.before-throughput-capital-r13-20261017'),(bank/'engine-upgrade-v1.json').read_bytes())
  atomic(bank/'engine-upgrade-v1.json',proposalfile.read_bytes())
  atomic(unit,pathlib.Path(s['unitCandidate']).read_bytes());subprocess.run(['systemctl','--user','daemon-reload'],check=True)
  assert sha(unit)==s['pins'][s['unitCandidate']] and sha(bank/'engine-upgrade-v1.json')==a['proposalSha256']
  installed=seal(bank/'engine-upgrade-v1.json');assert installed==future and len(installed['upgrades'])==14
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
 'bridgeSha256':sha(bank/'engine-upgrade-v1.json'),'financialBridgeEntries':14,'originalThirteenBridgeSha256':prior['bridgeSha256'],'proposalSha256':sha(proposalfile),'boundary':b,'controlChanges':s['controlChanges'],'financialHistoryChangedByOperator':False,'bridgeAppendedOnly':True,'freshTradingVerified':False,'finalStoppedPrefixParityVerified':False,'actualStartupMustValidateBeforeFinancialWrites':True,'startupPrefixParityGateSha256':STARTUP_GATE_SHA,'profitabilityEstablished':False}
 immutable_json(receiptfile,result);print(json.dumps({'startedRestoring':True,'newPid':newpid,'binarySha256':s['binarySha256'],'engineIdentity':ENGINE}))
if __name__=='__main__':
 mode=sys.argv[1]
 if mode=='stage' and len(sys.argv)==2:mode_stage()
 elif mode=='stop' and len(sys.argv)==3:mode_stop(sys.argv[2])
 elif mode=='bridge' and len(sys.argv)==4:mode_bridge(sys.argv[2],sys.argv[3])
 elif mode=='promote' and len(sys.argv)==4:mode_promote(sys.argv[2],sys.argv[3])
 else:raise ValueError('explicit stage|stop STAGE_SHA|bridge STAGE_SHA BOUNDARY_SHA|promote OWNER_APPROVAL RAW_SHA required')

