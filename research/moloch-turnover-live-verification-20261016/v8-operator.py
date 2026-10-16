from pathlib import Path
import os,json,hashlib,subprocess,shlex,re,shutil,datetime
sha=lambda p:hashlib.sha256(Path(p).read_bytes()).hexdigest()
root=Path('/home/archon/apps/moloch/shared/live-production-v1');shared=root.parent
health=json.loads(Path('/tmp/moloch-turnover-live-verification-20261016.json').read_bytes())
roll=json.loads(Path('/tmp/moloch-turnover-20261016-production-rollout.json').read_bytes())
assert health['pid']==roll['newPid']==565108 and health['running'] and health['binarySha256Matches']
assert not health['executionStopPresent']
at=datetime.datetime.now(datetime.timezone.utc)
assert 0<=(at-datetime.datetime.fromisoformat(health['checkedAt'])).total_seconds()<30
assert shutil.disk_usage('/tmp').free>=14*1024**3
assert sha(Path('/proc/565108/exe'))==roll['binarySha256']
unit=Path.home()/'.config/systemd/user/moloch-crossvenue-observer.service'
assert subprocess.check_output(['systemctl','--user','show','moloch-crossvenue-observer.service','--property=MainPID','--value'],text=True).strip()=='0'
old=unit.read_text();execs=[s for s in old.splitlines()if s.startswith('ExecStart=')];assert len(execs)==1
args=shlex.split(execs[0][len('ExecStart='):])
producer='/home/archon/apps/moloch/sidecars/crossvenue-observer-3d3b5d43a6c79293.py'
mapping='/home/archon/apps/moloch/sidecars/kalshi-universe-3f8dd6fa3ef6e5e2.json'
assert args.count(producer)==1 and sha(producer)=='3d3b5d43a6c79293d28b424fc4ab31626267acdd1c1b71918d9cdefd94824cd7'
assert args[args.index('--trace-mapping')+1]==mapping and sha(mapping)=='3f8dd6fa3ef6e5e20a6f19030b347f47fa72e3103609c70f6c7e0ab3293b21b0'
assert args[args.index('--sampling-mode')+1]=='trade' and '--input-trace' in args
assert args[args.index('--duration-seconds')+1]=='3600'
oldcohort=shared/'crossvenue-observer-trade-trace-v7'
seal=oldcohort/'input-trace-seal.json';sv=json.loads(seal.read_bytes())
assert sv['complete'] is False
sealsha=sha(seal)
oldout=args[args.index('--output')+1];assert oldout==str(oldcohort)
oldjournal=args[args.index('--journal')+1];assert Path(oldjournal).name=='breadth-545165-1792184096595.jsonl'
files=[p for p in root.glob('breadth-565108-*.jsonl')if re.fullmatch(r'breadth-565108-[0-9]+\.jsonl',p.name)]
assert len(files)==1 and files[0].stat().st_size>0
journal=files[0]
fds=[]
for f in Path('/proc/565108/fd').iterdir():
 try:fds.append(os.readlink(f))
 except OSError:pass
assert any(x==str(journal) or x.startswith(str(journal)+'.')for x in fds)
out=shared/'crossvenue-observer-trade-trace-v8';assert not out.exists()
assert old.count(oldjournal)==1 and old.count(oldout)==1 and old.count('Restart=on-failure')==1
new=old.replace(oldjournal,str(journal),1).replace(oldout,str(out),1).replace('Restart=on-failure','Restart=no',1)
check=new.replace(str(journal),oldjournal,1).replace(str(out),oldout,1).replace('Restart=no','Restart=on-failure',1)
assert check==old
candidate=Path('/tmp/moloch-crossvenue-observer-v8.service');assert not candidate.exists();candidate.write_text(new)
subprocess.run(['systemd-analyze','--user','verify',str(candidate)],check=True,capture_output=True)
backup=unit.with_name(unit.name+'.before-trace-v8-20261016');assert not backup.exists();backup.write_bytes(unit.read_bytes())
oldsha=sha(unit);tmp=unit.with_name(unit.name+'.v8-pending');assert not tmp.exists()
with tmp.open('xb')as f:f.write(new.encode());f.flush();os.fsync(f.fileno())
os.replace(tmp,unit)
subprocess.run(['systemctl','--user','daemon-reload'],check=True)
assert sha(seal)==sealsha and sha('/proc/565108/exe')==roll['binarySha256']
assert subprocess.check_output(['systemctl','--user','show','moloch-live.service','--property=MainPID','--value'],text=True).strip()=='565108'
subprocess.run(['systemctl','--user','reset-failed','moloch-crossvenue-observer.service'],check=True)
subprocess.run(['systemctl','--user','start','moloch-crossvenue-observer.service'],check=True)
pid=int(subprocess.check_output(['systemctl','--user','show','moloch-crossvenue-observer.service','--property=MainPID','--value'],text=True).strip());assert pid>0
receipt={'at':at.isoformat(),'pid':pid,'producerSha256':sha(producer),'mappingSha256':sha(mapping),'journal':str(journal),
 'output':str(out),'durationSeconds':3600,'restart':'no','traceBudgetBytes':2*1024**3,'traceMinimumFreeBytes':10*1024**3,
 'priorV7SealSha256':sealsha,'priorV7Complete':False,'priorUnitSha256':oldsha,'newUnitSha256':sha(unit),
 'sigPidUnchanged':565108,'sourceAgeCriteriaChanged':False,'sigRequestsAdded':0,'newKalshiAccountClient':False,'modelQualified':False}
p=Path('/tmp/moloch-v8-trace-rollout-20261016.json');assert not p.exists();p.write_text(json.dumps(receipt,indent=2)+'\n')
print(json.dumps(receipt))

