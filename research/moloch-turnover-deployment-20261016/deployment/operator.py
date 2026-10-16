#!/usr/bin/env python3
"""Root-owned turnover deployment: immutable staging, original ledger, no HTTP."""
import pathlib,json,hashlib,subprocess,datetime,os,sys,re,shutil,fcntl,time,uuid,copy
assert __debug__
OP='moloch-turnover-20261016-v1'
SOURCE='20543435f9354ae5a599866545cbf1aae6a03cfa'
ENGINE='b5b28a476fc057de93f5784f7b3600116418aa7e775f25576cfd2906ecd77713'
BINARY='d9f5cbc628dd2e2d2d4f970106706029b8af56b723b5ac51f80ac1939ab48b3d'
OLD_ENGINE='8acc461bc02fb9236100058fe464b203d5da005375632af05c176223c7246754'
OLD_BINARY='1180882d0a47f1af948258c37b333fb51552c31ef9823a51e1c058cf8558e95f'
OLD_PID=545165
OLD_CONTROL='/home/archon/apps/moloch/shared/moloch-live-paper6-cold-six-forecast-20261016.json'
OLD_CONTROL_SHA='d27b027d911799b94c7880f7031354b109b781414fda7712a44c8e279a3e5ed5'
OLD_RELEASE='/home/archon/apps/moloch/releases/paper6-buy-recovery-1180882d0a47/moloch-live-runtime'
OLD_UNIT_SHA='b798f1e918094a44614c3c6c3a37b28d22c2468e1c9890edef234744c2101c42'
OLD_BRIDGE_SHA='76fab91acccfe26cfb1a80010a7b1fb00df7f92f02dc20d5127192258bcf0723'
HELPER_SHA='1e0da4b462dd0627dc597b725fc592530d6ae3c91babe09d4ecb641d2ce87133'
STAGE=pathlib.Path('/tmp/moloch-turnover-20543435-20261016')
TARGET=pathlib.Path('/tmp/moloch-turnover-730c0962-20261016/moloch/live-runtime/target')
PARITY=pathlib.Path('/tmp/moloch-native-flow-benchmark-turnover-b5-20261016/result.json')
helper=pathlib.Path('/tmp/moloch-throughput-rollout.py')
assert hashlib.sha256(helper.read_bytes()).hexdigest()==HELPER_SHA
prefix=helper.read_text().split('command=sys.argv[1]')[0]
assert 'def verified_dispatch(report):' in prefix and 'elif command' not in prefix
exec(compile(prefix,'pinned-helper-prefix','exec'),globals())
# prefix helpers above initialize paths; replace only their old metadata names.
meta=pathlib.Path('/tmp/moloch-turnover-20261016-staging.json')
boundfile=pathlib.Path('/tmp/moloch-turnover-20261016-boundary.json')
proposalfile=pathlib.Path('/tmp/moloch-turnover-20261016-proposed-bridge.json')
receiptfile=pathlib.Path('/tmp/moloch-turnover-20261016-production-rollout.json')
requestfile=root/'turnover-20261016-stop-request.json'
stopfile=root/'EXECUTION_STOP'
globalstop=shared/'EXECUTION_STOP'
unit=pathlib.Path.home()/'.config/systemd/user/moloch-live.service'
dropin=unit.parent/'moloch-live.service.d/10-bounded-restart.conf'
generation=shared/'fundamental-paper6-turnover-20261016'
controlfile=shared/'moloch-live-paper6-turnover-20261016.json'
release=base/'releases'/('paper6-turnover-'+BINARY[:12])/'moloch-live-runtime'
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
def raw_pins(paths):return {str(p):sha(cleanpath(p)) for p in paths}
def pins_ok(pins):
 for p,h in pins.items():assert sha(cleanpath(p))==h,p
def original_process():
 p=pathlib.Path('/proc')/str(OLD_PID)
 assert pid()==OLD_PID and sha(p/'exe')==OLD_BINARY and os.readlink(p/'exe')==OLD_RELEASE
 assert (p/'cmdline').read_bytes().split(b'\0')==[OLD_RELEASE.encode(),b'run',OLD_CONTROL.encode(),b'']
def service_pins():
 assert sha(unit)==OLD_UNIT_SHA
 assert sha(dropin)=='ff4237fb5e0b5f2a68159e4fa43d737eefc68ebdd422f56eea068c4ffba0e795'
def require_parity():
 p=load(PARITY)
 assert p['financialPrefixParityPassed'] is True
 assert not p.get('productionBridgeInstalled',False)
 # exact source/binary/helper are additionally frozen by original input pins.
 assert p['nativeEngineIdentity']==ENGINE and p['sourceCommit']==SOURCE
 assert p['finalPublicStateExact'] is True and p['originalTwelveBridgesUnchanged'] is True
 assert p['originalPrefixSequence']==308166 and p['originalPrefixBytes']==952440605
 assert p['productionAuthorization'] is False
 return {'path':str(PARITY),'sha256':sha(PARITY)}
def original_bridge():
 d=seal(bank/'engine-upgrade-v1.json')
 assert sha(bank/'engine-upgrade-v1.json')==OLD_BRIDGE_SHA and len(d['upgrades'])==12
 assert d['upgrades'][-1]['toEngineIdentity']==OLD_ENGINE
 return d
def snapshot():
 h,r,m=(seal(bank/n) for n in ['head.json','report.json','manifest.json'])
 assert h['sequence']==r['sequence'] and h['previous']==r['previous'] and h['manifestHash']==r['manifestHash']==m['hash']
 assert m['hash']=='78acaef952ed95b9bff24a3349ece669ccbe88ef7294b27dd9c2aa7d09cae325'
 original_bridge();d=verified_dispatch(r)
 return {'head':h,'reportSha256':sha(bank/'report.json'),'prefixBytes':(bank/'events.jsonl').stat().st_size,
 'prefixSha256':sha(bank/'events.jsonl'),'dispatch':d,
 'pins':raw_pins([bank/'manifest.json',bank/'engine-upgrade-v1.json',root/'dispatch.ndjson',
 root/'fundamental.ndjson',root/'fundamental.initialized.json',root/'package-dispatch/package-dispatch.ndjson']),
 'fills':len(r['fills']),'reportRealized':r['report']['realized']}
def held():
 assert pid()==0 and not pathlib.Path('/proc',str(OLD_PID)).exists() and not globalstop.exists()
 f=cleanpath(shared/'capture.lock').open('r+')
 try:fcntl.flock(f,fcntl.LOCK_EX|fcntl.LOCK_NB)
 except BaseException:f.close();raise
 return f
def validate_stage(s):
 assert s['sourceCommit']==SOURCE and s['engineIdentity']==ENGINE and s['binarySha256']==BINARY
 pins_ok(s['pins']);service_pins()
 manifest=load(STAGE/'source-manifest.json')
 for n,h in manifest['sourceHashes'].items():assert sha(STAGE/n)==h,n
 before=load(OLD_CONTROL);after=load(controlfile);check=copy.deepcopy(after)
 check['activation']['binarySha256']=before['activation']['binarySha256']
 check['fundamentalPolicy']=before['fundamentalPolicy'];check.pop('profitableDepthMode')
 assert check==before and after['profitableDepthMode'] is True
 assert after['activation']['binarySha256']==BINARY
 assert after['fundamentalPolicy']==load(generation/'settings.json')
 typed=load(generation/'typed-settings.json')
 assert typed['settingsLoadPassed'] and typed['sourcesVerified'] and typed['reviewAccepted']
 assert typed['settingsCanonicalSha256']==s['settingsCanonicalSha256']
 # Re-execute pure source/typed checks, retaining immutable initial outputs.
 result=subprocess.run([str(STAGE/'validate-turnover-forecasts'),'settings',str(generation/'settings.json')],check=True,capture_output=True,text=True)
 assert json.loads(result.stdout)==typed
def mode_stage():
 assert not meta.exists() and not boundfile.exists() and not receiptfile.exists()
 original_process();service_pins();original_bridge()
 assert sha(OLD_CONTROL)==OLD_CONTROL_SHA and not stopfile.exists() and not globalstop.exists()
 component=load(STAGE/'release-component-validation.json')
 assert component['binarySha256']==BINARY and component['engineIdentity']==ENGINE
 assert sha(TARGET/'release/moloch-live-runtime')==BINARY
 assert sha(STAGE/'release-component-validation.json')=='496fc474ed9b1278755e16d46e636fd931d9ac6d1d83058f3bd56258f9b6a400'
 for v in component['priorRuntimeChecks'].values():assert v['exitCode']==0
 for v in component['checks'].values():assert v['exitCode']==0
 assert sha('/tmp/moloch-fundamental-fixture-a40888d7-20261016/validation.json')=='41f2463515b4d8a2b60e7de26776c229bf29603469d3f3645657224b0d40bb62'
 for n,h in component['helperPins'].items():assert sha(STAGE/n)==h
 before=load(OLD_CONTROL);settings=copy.deepcopy(before['fundamentalPolicy'])
 authority=load(settings['authorityPath']);assert sha(settings['authorityPath'])==settings['authoritySha256']
 original_authority=copy.deepcopy(authority)
 generation.mkdir(mode=0o700);jobs=[];deltas=[];refs=[]
 moment=time.time_ns()//1_000_000
 caps={'977':'5000.000000','1067':'3000.000000','865':'3000.000000'}
 modelpins=raw_pins([settings['authorityPath']])
 for ref in settings['reviews']:
  assert sha(ref['path'])==ref['fileSha256'];old=load(ref['path'])
  modelpins.update(raw_pins([ref['path']]))
  for src in ref['sourceFiles']:
   assert sha(src['path'])==src['sha256'];modelpins.update(raw_pins([src['path']]))
  ex=old['exchangeId']
  if ex not in caps:refs.append(copy.deepcopy(ref));continue
  assert old['version']==2 and 'existingOrigin' not in old and 'sameSideTopUps' not in old
  assert old['maximumPrincipal']=='2000.000000' and old['maximumQuantity']==10000
  new=copy.deepcopy(old);new['maximumPrincipal']=caps[ex]
  new['sameSideTopUps']={'version':1,'maximumEntries':2};new['reviewedAtMs']=moment
  new['assumptionLabel']+='. Root turnover review: aggregate held-plus-new principal ceiling '+caps[ex]+', maximum10000 total cohort shares, at most2 independently closed/attributed same-side physical entries. Original model point, exceptional assumptions, source bytes/clocks, contract and settlement objective unchanged. Risk caps are operator limits, not Kelly or calibrated confidence bounds. Prior actual ACK closure, FIFO ownership, fresh execution facts, exact limits and final authority remain mandatory; no retry or recredit. Authorized same-cohort top-up does not renew an expired source.'
  check=copy.deepcopy(new)
  for key in ['maximumPrincipal','reviewedAtMs','assumptionLabel']:check[key]=old[key]
  check.pop('sameSideTopUps');assert check==old
  path=generation/(ex+'-review.json');immutable_json(path,new)
  nr=copy.deepcopy(ref);nr['path']=str(path);nr['fileSha256']=sha(path);refs.append(nr)
  jobs.append({'key':ex,'path':str(path),'sourceFiles':ref['sourceFiles']})
  deltas.append({'exchangeId':ex,'priorReviewPath':ref['path'],'priorReviewRawSha256':ref['fileSha256'],
   'priorPrincipal':old['maximumPrincipal'],'newPrincipal':new['maximumPrincipal'],'maximumEntries':2,
   'reviewedAtMs':moment,'originalSourcesAndLifetimesUnchanged':True,'valuationObjectiveUnchanged':True})
 immutable_json(generation/'jobs.json',{'reviews':jobs})
 rr=subprocess.run([str(STAGE/'validate-turnover-forecasts'),'hash',str(generation/'jobs.json')],check=True,capture_output=True)
 immutable(generation/'typed-reviews.json',rr.stdout)
 tr=json.loads(rr.stdout);assert tr['typedCurrentReviewChecksPassed'] and len(tr['reviews'])==3
 newhashes=[r['canonicalSha256'] for r in tr['reviews']]
 authority['trustedReviewSha256']+=newhashes
 assert authority['trustedReviewSha256'][:-3]==original_authority['trustedReviewSha256']
 immutable_json(generation/'authority.json',authority)
 settings['authorityPath']=str(generation/'authority.json');settings['authoritySha256']=sha(generation/'authority.json');settings['reviews']=refs
 immutable_json(generation/'settings.json',settings)
 ss=subprocess.run([str(STAGE/'validate-turnover-forecasts'),'settings',str(generation/'settings.json')],check=True,capture_output=True)
 immutable(generation/'typed-settings.json',ss.stdout);ts=json.loads(ss.stdout)
 assert ts['settingsLoadPassed'] and ts['sourcesVerified'] and ts['reviewAccepted']
 immutable(release,(TARGET/'release/moloch-live-runtime').read_bytes(),0o500);assert sha(release)==BINARY
 control=copy.deepcopy(before);control['activation']['binarySha256']=BINARY;control['fundamentalPolicy']=settings;control['profitableDepthMode']=True
 immutable_json(controlfile,control)
 oldexec=('ExecStart='+OLD_RELEASE+' run '+OLD_CONTROL).encode()
 newexec=('ExecStart='+str(release)+' run '+str(controlfile)).encode()
 raw=unit.read_bytes();assert raw.count(oldexec)==1
 candidate=generation/'moloch-turnover-20261016.service';immutable(candidate,raw.replace(oldexec,newexec,1))
 subprocess.run(['systemd-analyze','--user','verify',str(candidate)],check=True,capture_output=True)
 proofpaths=[STAGE/'source-manifest.json',STAGE/'release-component-validation.json',
 pathlib.Path('/tmp/moloch-fundamental-fixture-a40888d7-20261016/validation.json'),pathlib.Path(OLD_CONTROL),
 release,controlfile,candidate,*generation.glob('*.json'),*[STAGE/n for n in component['helperPins']]]
 pins=raw_pins(proofpaths);pins.update(modelpins)
 s={'at':now(),'operator':OP,'oldPid':OLD_PID,'priorBinarySha256':OLD_BINARY,'priorRelease':OLD_RELEASE,
 'priorControl':OLD_CONTROL,'sourceCommit':SOURCE,'engineIdentity':ENGINE,'binarySha256':BINARY,
 'release':str(release),'control':str(controlfile),'unitCandidate':str(candidate),
 'settingsCanonicalSha256':ts['settingsCanonicalSha256'],'newReviewCanonicalSha256':newhashes,
 'reviewDeltas':deltas,'controlChanges':['activation.binarySha256','fundamentalPolicy','profitableDepthMode'],
 'pins':pins,'deployed':False,'profitabilityEstablished':False}
 immutable_json(meta,s);print(json.dumps({'staged':True,'stagingSha256':sha(meta),'binarySha256':BINARY,'reviewDeltas':deltas}))
def mode_stop(stage_sha):
 assert sha(meta)==stage_sha and not boundfile.exists() and not requestfile.exists() and not receiptfile.exists()
 s=load(meta);validate_stage(s);parity=require_parity()
 original_process();assert not stopfile.exists() and not globalstop.exists()
 request={'operator':OP,'token':uuid.uuid4().hex,'at':now(),'oldPid':OLD_PID,
 'reason':'CONTROLLED_TURNOVER_UPGRADE','stagingSha256':stage_sha,
 'action':'finish current operation and stop; no new account requests or writes'}
 immutable_json(requestfile,request);immutable_json(stopfile,request)
 for _ in range(240):
  if pid()==0:break
  time.sleep(.5)
 with held():
  snap=snapshot();q=quiet();assert snapshot()==snap
  boundary={'at':now(),'operator':OP,'stagingSha256':stage_sha,'snapshot':snap,'financialPrefixParity':parity,
   'requestSha256':sha(requestfile),'stopSha256':sha(stopfile),'workerStatusSha256':sha(root/'status.json'),
   'workerStatus':load(root/'status.json'),'queueQuiet':{'globalUnknownCount':q['globalUnknownCount'],'active':q['state']['active'],'manualReview':q['state']['manual_review']}}
  immutable_json(boundfile,boundary)
 print(json.dumps({'stopped':True,'boundarySha256':sha(boundfile),'sequence':snap['head']['sequence'],'fills':snap['fills'],'stopReason':load(stopfile).get('reason')}))
def mode_bridge(stage_sha,boundary_sha):
 assert sha(meta)==stage_sha and sha(boundfile)==boundary_sha and not proposalfile.exists()
 s=load(meta);b=load(boundfile);validate_stage(s);assert require_parity()==b['financialPrefixParity']
 with held():
  assert snapshot()==b['snapshot'] and sha(stopfile)==b['stopSha256'] and sha(requestfile)==b['requestSha256']
  assert sha(root/'status.json')==b['workerStatusSha256'];quiet()
  result=subprocess.run([str(STAGE/'moloch-upgrade'),str(bank),OLD_BINARY,now(),str(proposalfile)],capture_output=True,check=True)
  immutable(pathlib.Path('/tmp/moloch-turnover-20261016-bridge-helper.stdout'),result.stdout)
  assert snapshot()==b['snapshot']
  future=seal(proposalfile);prior=original_bridge();last=future['upgrades'][-1]
  assert len(future['upgrades'])==13 and future['upgrades'][:-1]==prior['upgrades']
  assert last['oldHead']==b['snapshot']['head'] and last['oldReportEnvelope']==seal(bank/'report.json')
  assert last['journalPrefix']=={'bytes':b['snapshot']['prefixBytes'],'sha256':b['snapshot']['prefixSha256']}
  assert last['approval']['oldBinarySha256']==OLD_BINARY and last['approval']['previousProcessStopped'] is True
  assert last['fromEngineIdentity']==OLD_ENGINE and last['toEngineIdentity']==ENGINE
 print(json.dumps({'proposedOnly':True,'proposalSha256':sha(proposalfile),'upgrades':13,'sequence':last['oldHead']['sequence']}))
def mode_promote(approvalpath,approvalsha):
 assert sha(approvalpath)==approvalsha and not receiptfile.exists()
 a=load(approvalpath);s=load(meta);b=load(boundfile)
 assert a=={'operator':OP,'executionOwner':'Rhaegal-Viserion-paper6-20261016','executionOwnerReviewed':True,
 'stagingSha256':sha(meta),'boundarySha256':sha(boundfile),'proposalSha256':sha(proposalfile),
 'requestSha256':sha(requestfile),'stopSha256':sha(stopfile),'workerStatusSha256':sha(root/'status.json'),
 'reason':'root-reviewed-controlled-turnover-stop-only-not-human-stop'}
 assert b['stagingSha256']==sha(meta) and load(requestfile)['operator']==OP and load(requestfile)['oldPid']==OLD_PID
 assert load(requestfile)['stagingSha256']==sha(meta)
 with held():
  validate_stage(s);assert require_parity()==b['financialPrefixParity']
  assert snapshot()==b['snapshot'] and sha(stopfile)==b['stopSha256'] and sha(root/'status.json')==b['workerStatusSha256']
  quiet();assert snapshot()==b['snapshot']
  # Recheck exact original bridge and exact current boundary before sole bridge installation.
  prior=original_bridge();future=seal(proposalfile);last=future['upgrades'][-1]
  assert len(future['upgrades'])==13 and future['upgrades'][:-1]==prior['upgrades']
  assert last['oldHead']==b['snapshot']['head'] and last['oldReportEnvelope']==seal(bank/'report.json')
  assert last['toEngineIdentity']==ENGINE and last['fromEngineIdentity']==OLD_ENGINE
  assert last['approval']['oldBinarySha256']==OLD_BINARY
  assert last['journalPrefix']=={'bytes':b['snapshot']['prefixBytes'],'sha256':b['snapshot']['prefixSha256']}
  for p in [unit,bank/'engine-upgrade-v1.json']:
   immutable(p.with_name(p.name+'.before-turnover-20261016'),p.read_bytes())
  atomic(bank/'engine-upgrade-v1.json',proposalfile.read_bytes())
  atomic(unit,pathlib.Path(s['unitCandidate']).read_bytes())
  subprocess.run(['systemctl','--user','daemon-reload'],check=True)
  assert sha(unit)==s['pins'][s['unitCandidate']] and sha(bank/'engine-upgrade-v1.json')==a['proposalSha256']
  assert not globalstop.exists() and sha(stopfile)==b['stopSha256']
  archive=root/('EXECUTION_STOP.turnover-'+load(requestfile)['token']+'.json')
  assert not archive.exists();os.rename(stopfile,archive);assert sha(archive)==b['stopSha256']
 try:
  assert pid()==0 and not pathlib.Path('/proc',str(OLD_PID)).exists() and not stopfile.exists() and not globalstop.exists()
  subprocess.run(['systemctl','--user','reset-failed','moloch-live.service'],check=True)
  subprocess.run(['systemctl','--user','start','moloch-live.service'],check=True)
  newpid=pid();p=pathlib.Path('/proc')/str(newpid)
  assert newpid>0 and newpid!=OLD_PID and sha(p/'exe')==BINARY and os.readlink(p/'exe')==str(release)
  assert (p/'cmdline').read_bytes().split(b'\0')==[str(release).encode(),b'run',str(controlfile).encode(),b'']
 except BaseException:
  if not stopfile.exists():save(stopfile,{'operator':OP,'reason':'TURNOVER_START_FAILED_NO_RETRY','at':now()})
  raise
 result={'at':now(),'operator':OP,'oldPid':OLD_PID,'newPid':newpid,'sourceCommit':SOURCE,'binarySha256':BINARY,
 'engineIdentity':ENGINE,'release':str(release),'control':str(controlfile),'controlSha256':sha(controlfile),
 'stagingSha256':sha(meta),'approvalSha256':approvalsha,'bridgeSha256':sha(bank/'engine-upgrade-v1.json'),
 'financialBridgeEntries':13,'boundary':b,'controlChanges':s['controlChanges'],
 'reviewDeltas':s['reviewDeltas'],'financialHistoryChangedByOperator':False,
 'freshTradingVerified':False,'profitabilityEstablished':False}
 immutable_json(receiptfile,result);print(json.dumps({'startedRestoring':True,'newPid':newpid,'binarySha256':BINARY,'engineIdentity':ENGINE}))
if __name__=='__main__':
 mode=sys.argv[1]
 if mode=='stage' and len(sys.argv)==2:mode_stage()
 elif mode=='stop' and len(sys.argv)==3:mode_stop(sys.argv[2])
 elif mode=='bridge' and len(sys.argv)==4:mode_bridge(sys.argv[2],sys.argv[3])
 elif mode=='promote' and len(sys.argv)==4:mode_promote(sys.argv[2],sys.argv[3])
 else:raise ValueError('explicit stage|stop STAGE_SHA|bridge STAGE_SHA BOUNDARY_SHA|promote OWNER_APPROVAL RAW_SHA required')

