from pathlib import Path
import json,os,time,datetime,hashlib,subprocess
D=Path('/tmp/moloch-r45-rank-expiry-20261018');R=Path('/home/archon/apps/moloch/shared/live-production-v1');B=R/'bank-all-markets-v1'
start=json.loads((D/'runtime-first-start.json').read_text());pid=start['pid'];P=Path('/proc')/str(pid);deadline=time.monotonic()+5400;os.umask(0o077)
sha=lambda b:hashlib.sha256(b).hexdigest()
while time.monotonic()<deadline:
 if not P.exists() or P.joinpath('stat').read_text().split(') ',1)[1].split()[19]!=start['startTicks']:raise RuntimeError('Original worker handle missing or replaced; no restart')
 if (R/'EXECUTION_STOP').exists():raise RuntimeError('Actual execution STOP exists; no capture')
 progress=json.loads((R/'progress.json').read_text())
 if progress.get('pid')!=pid or progress.get('phase')!='trading' or progress.get('realtimeHealthy') is not True:time.sleep(5);continue
 try:
  rawv=(R/'tartarus-current-native-state.json').read_bytes();rawr=(B/'report.json').read_bytes();v=json.loads(rawv);r=json.loads(rawr)
  assert rawv==(R/'tartarus-current-native-state.json').read_bytes()
  assert v['nativeHead']['sequence']==r['sequence'] and v['nativeHead']['previous']==r['previous'] and v['nativeHead']['manifestHash']==r['manifestHash']
  assert v['account']['positions']==r['report']['positions'] and v['account']['cash']==r['report']['cash'] and v['account']['fillCount']==len(r['fills'])
  now=datetime.datetime.now(datetime.timezone.utc);age=(now-datetime.datetime.fromisoformat(v['writtenAt'].replace('Z','+00:00'))).total_seconds();assert 0<=age<=90
  assert v['generationSha256']==start['modelGeneration']
  assert sha(P.joinpath('exe').read_bytes())==start['binarySha256']
  assert subprocess.check_output(['systemctl','--user','show','moloch-live.service','-p','MainPID','--value'],text=True).strip()==str(pid)
 except (AssertionError,FileNotFoundError,json.JSONDecodeError):time.sleep(.1);continue
 out=D/'matched-current-portfolio';out.mkdir(mode=0o700,exist_ok=False)
 for name,raw in [('report.original.json',rawr),('native-view.original.json',rawv)]:
  with (out/name).open('xb') as f:f.write(raw);f.flush();os.fsync(f.fileno())
  (out/name).chmod(0o400)
 metadata={'capturedAt':now.isoformat(),'workerPid':pid,'workerStartTicks':start['startTicks'],'binarySha256':start['binarySha256'],'controlSha256':start['controlSha256'],'generationSha256':v['generationSha256'],'nativeHead':v['nativeHead'],'originalViewWrittenAt':v['writtenAt'],'originalReportAt':r['report']['at'],'reportSha256':sha(rawr),'viewSha256':sha(rawv),'fillCount':len(r['fills']),'positionParity':True,'readOnlyFilesystemCapture':True,'networkRequests':0,'executionAuthority':False}
 (out/'capture.json').write_text(json.dumps(metadata,indent=2));(out/'capture.json').chmod(0o400);print(json.dumps(metadata));break
else:raise TimeoutError('Finite capture timed out; worker status not inferred')

