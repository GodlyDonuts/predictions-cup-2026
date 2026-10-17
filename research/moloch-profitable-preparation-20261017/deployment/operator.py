#!/usr/bin/env python3
"""Root-reviewed R21 binary-only genuine profit-exit preparation. Freeze exact source/build/four-generation pins before any explicit stage/stop/promote mode."""
import pathlib,json,hashlib,subprocess,datetime,os,sys,shutil,fcntl,time,uuid,copy
assert __debug__
helper=pathlib.Path('/tmp/moloch-throughput-rollout.py')
assert hashlib.sha256(helper.read_bytes()).hexdigest()=='1e0da4b462dd0627dc597b725fc592530d6ae3c91babe09d4ecb641d2ce87133'
prefix=helper.read_text().split('command=sys.argv[1]')[0]
assert 'def verified_dispatch(report):' in prefix and 'elif command' not in prefix
exec(compile(prefix,'pinned-helper-prefix','exec'),globals())
OP='moloch-profitable-preparation-20261017-r21'
# Pending actual owner-frozen source/build/review pins: no placeholders count as PASS.
SOURCE='b796e2c1ddb219ebd36cff97428db99a8de780ed'
VALIDATION_SHA='18cbf3217a650c5411fdc5c7b06319a6413f013a2125e58d228c81232d528f38'
PRIOR_SHA='cbf0daa5ccad7efd3a03ddbb3f094f28232bbf54424a9d95fff00f3c46fdc16f'
PRIOR_STAGE_PATH='/tmp/moloch-profitable-exit-r20-20261017-staging.json'
PRIOR_STAGE_SHA='e117829647e75b69e9b07b5520692c2e6ddf8fe5f93ce57f6de884175f1dc9b7'
REVIEW_BUNDLE_SHA='83c1b0d8c9d7942ea142b9fc971db59b4c481bc9fc1ae62fcde6d190ec4d9592'
EXPECTED_TEST_RESULTS={'sdk-library': [['233', '0', '0', '0', '0']], 'native-library': [['98', '0', '1', '0', '0']], 'runtime-library': [['238', '0', '1', '0', '0']], 'worker': [['126', '0', '0', '0', '0']], 'exit_refresh_lifecycle': [['9', '0', '0', '0', '0']], 'fundamental_policy': [['119', '0', '0', '0', '0']], 'fundamental_quantity': [['2', '0', '0', '0', '0']], 'fundamental_source_bounds': [['7', '0', '0', '0', '0']], 'alpha_proposal': [['18', '0', '0', '0', '0']], 'release': None, 'engine-probe-build': None, 'engine-probe-run': None}
ENGINE='7a74f8c3a53119e158dae8338cd09d68140fc4299f09d4fb35ac060be5be7efa'
OLD_PID=611948
priorfile=pathlib.Path('/tmp/moloch-profitable-exit-r20-20261017-production-rollout.json')
assert SOURCE and VALIDATION_SHA and PRIOR_SHA and PRIOR_STAGE_PATH and PRIOR_STAGE_SHA and REVIEW_BUNDLE_SHA and EXPECTED_TEST_RESULTS, 'OWNER_FINAL_FREEZE_REQUIRED'
priorstage=pathlib.Path(PRIOR_STAGE_PATH)
assert sha(priorfile)==PRIOR_SHA and sha(priorstage)==PRIOR_STAGE_SHA
prior=load(priorfile);priors=load(priorstage)
assert prior['newPid']==OLD_PID and prior['engineIdentity']==ENGINE and prior['financialBridgeEntries']==14
OLD_BINARY=prior['binarySha256'];OLD_CONTROL=pathlib.Path(prior['control']);OLD_CONTROL_SHA=prior['controlSha256'];OLD_RELEASE=prior['release']
assert OLD_CONTROL==shared/'moloch-live-profitable-exit-r20-20261017.json'
OLD_UNIT_SHA=priors['pins'][priors['unitCandidate']]
STAGE=pathlib.Path('/tmp/moloch-profit-preparation-r21-20261017')
TARGET=pathlib.Path('/tmp/moloch-paper7-generation-live-20261016/target')
meta=pathlib.Path('/tmp/moloch-profit-preparation-r21-20261017-staging.json')
boundfile=pathlib.Path('/tmp/moloch-profit-preparation-r21-20261017-boundary.json')
receiptfile=pathlib.Path('/tmp/moloch-profit-preparation-r21-20261017-production-rollout.json')
requestfile=root/'profit-preparation-r21-20261017-stop-request.json'
stopfile=root/'EXECUTION_STOP';globalstop=shared/'EXECUTION_STOP'
unit=pathlib.Path.home()/'.config/systemd/user/moloch-live.service'
dropin=unit.parent/'moloch-live.service.d/10-bounded-restart.conf'
controlfile=shared/'moloch-live-profit-preparation-r21-20261017.json'
settingsfile=shared/'fundamental-paper7-capital-expanded-20261017/settings-root-reviewed.json'
reviewbundle=STAGE/'root-reviewed-settings-validation.json'
EXPANDED_EXCHANGES={'872','885','993'}
GENERATION_SHA='b27c469978fbe3149dcbaa74a42a1d8b79867fe57be40ca50413d572eae84e17'
ACTIVE_OVERLAY_SHA='5478dab3b905ad1ea75cff6fa1e9c4a59ab646bc9e85be5d4a5872f96499036a'
ORIGINAL_REQUEST_SHA='b27c469978fbe3149dcbaa74a42a1d8b79867fe57be40ca50413d572eae84e17'
assert GENERATION_SHA and ACTIVE_OVERLAY_SHA and ORIGINAL_REQUEST_SHA, 'OWNER_GENERATION_FREEZE_REQUIRED'
activefile=shared/'fundamental-paper7-capital-expanded-20261017/next-control-root-reviewed.json'
generation_request=shared/'fundamental-paper7-capital-expanded-20261017/owner-request-001.json'

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
 assert sha(bank/'engine-upgrade-v1.json')==prior['bridgeSha256'] and len(d['upgrades'])==14
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
   assert row.get('testResults') and all(int(x[0])>0 and x[1]=='0' and int(x[2])<=(1 if name in ['native-library','runtime-library'] else 0) for x in row['testResults'])
 assert (STAGE/'engine-probe-run.log').read_text().strip()==ENGINE
 assert sha(STAGE/'moloch-live-runtime')==v['binarySha256']
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
 assert sha(STAGE/'moloch-live-runtime')==s['binarySha256']
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
 assert v['sourceCommit']==SOURCE and v['engineIdentity']==ENGINE and m['nativeSourceBytesUnchanged'] and m['sdkSourceBytesUnchanged']
 assert v['sourceManifestSha256']==sha(STAGE/'integrated-source-manifest.json') and all(r['exitCode']==0 for r in v['checks'].values())
 for p,h in m['sourceHashes'].items():assert sha(STAGE/p)==h,p
 binary=v['binarySha256'];release=base/'releases'/('throughput-capital-'+binary[:12])/'moloch-live-runtime'
 immutable(release,(STAGE/'moloch-live-runtime').read_bytes(),0o500);assert sha(release)==binary
 before=load(OLD_CONTROL);after=copy.deepcopy(active_generation())
 assert after['fundamentalPolicy']==reviewed_settings(before)
 after['activation']['binarySha256']=binary
 immutable_json(controlfile,after)
 text=unit.read_text();oldline='ExecStart='+OLD_RELEASE+' run '+str(OLD_CONTROL);assert text.count(oldline)==1
 candidate=STAGE/'unit-candidate.service';immutable(candidate,text.replace(oldline,'ExecStart='+str(release)+' run '+str(controlfile)).encode())
 paths=[activefile,generation_request,settingsfile,reviewbundle,controlfile,release,candidate,STAGE/'validation.json',STAGE/'integrated-source-manifest.json',OLD_CONTROL,priorfile,priorstage]
 s={'at':now(),'operator':OP,'sourceCommit':SOURCE,'engineIdentity':ENGINE,'binarySha256':binary,'release':str(release),'unitCandidate':str(candidate),
 'controlChanges':['activation.binarySha256 relative to ORIGINAL active overlay'],'generationSha256':GENERATION_SHA,'activeOverlaySha256':ACTIVE_OVERLAY_SHA,'oldBaseSha256':OLD_CONTROL_SHA,'newBaseSha256':sha(controlfile),'financialBridgeChanged':False,
 'pins':{str(p):sha(p) for p in paths},'priorBridgeSha256':prior['bridgeSha256'],'financialBridgeEntries':14}
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
  immutable(unit.with_name(unit.name+'.before-profit-preparation-r21-20261017'),unit.read_bytes())
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
 'bridgeSha256':sha(bank/'engine-upgrade-v1.json'),'financialBridgeEntries':14,'boundary':b,'controlChanges':s['controlChanges'],'financialHistoryChangedByOperator':False,'freshTradingVerified':False,'profitabilityEstablished':False}
 immutable_json(receiptfile,result);print(json.dumps({'startedRestoring':True,'newPid':newpid,'binarySha256':s['binarySha256'],'engineIdentity':ENGINE}))
if __name__=='__main__':
 mode=sys.argv[1]
 if mode=='stage' and len(sys.argv)==2:mode_stage()
 elif mode=='stop' and len(sys.argv)==3:mode_stop(sys.argv[2])
 elif mode=='promote' and len(sys.argv)==4:mode_promote(sys.argv[2],sys.argv[3])
 else:raise ValueError('explicit stage|stop STAGE_SHA|promote OWNER_APPROVAL RAW_SHA required')
