"""Start only the checked finite read-only V7 observer; never restart SIG."""
from pathlib import Path
import datetime as dt
import hashlib,json,os,re,shutil,subprocess,sys
sha=lambda p:hashlib.sha256(Path(p).read_bytes()).hexdigest()
assert len(sys.argv)==2 and sha(__file__)==sys.argv[1] and __debug__
root=Path('/home/archon/apps/moloch/shared/live-production-v1');shared=root.parent
roll=json.loads(Path('/tmp/moloch-cold-six-forecast-20261016-production-rollout.json').read_bytes())
pid=roll['newPid'];assert pid==545165 and roll['binarySha256']=='1180882d0a47f1af948258c37b333fb51552c31ef9823a51e1c058cf8558e95f'
assert int(subprocess.check_output(['systemctl','--user','show','moloch-live.service','--property=MainPID','--value'],text=True))==pid
assert sha(Path('/proc')/str(pid)/'exe')==roll['binarySha256'] and sha(roll['control'])==roll['controlSha256']
assert not (root/'EXECUTION_STOP').exists() and not (shared/'EXECUTION_STOP').exists()
progress=json.loads((root/'progress.json').read_bytes());now=dt.datetime.now(dt.timezone.utc)
assert progress['pid']==pid and progress['phase']=='trading' and progress['realtimeHealthy'] is True
at=dt.datetime.fromisoformat(progress['at'].replace('Z','+00:00'));assert 0<=(now-at).total_seconds()<=120
assert int(subprocess.check_output(['systemctl','--user','show','moloch-crossvenue-observer.service','--property=MainPID','--value'],text=True))==0
assert shutil.disk_usage(shared).free>10*1024**3
checked=Path('/tmp/moloch-pretrigger-v2-b387861c-20261016-r2')
validation=checked/'validation-r4.json'
assert sha(validation)=='ebce7d387b59202212567f476806cf208e9028fc2f9776873816eecd1f6f50b4'
assert json.loads(validation.read_bytes())['passed']==106
producer_sha='3d3b5d43a6c79293d28b424fc4ab31626267acdd1c1b71918d9cdefd94824cd7'
source=checked/'moloch/external-data/crossvenue_observer.py';assert sha(source)==producer_sha
producer=Path('/home/archon/apps/moloch/sidecars/crossvenue-observer-3d3b5d43a6c79293.py')
assert not producer.exists()
fd=os.open(producer,os.O_CREAT|os.O_EXCL|os.O_WRONLY,0o400)
with os.fdopen(fd,'wb') as f:f.write(source.read_bytes());f.flush();os.fsync(f.fileno())
assert sha(producer)==producer_sha
mapping=Path('/home/archon/apps/moloch/sidecars/kalshi-universe-3f8dd6fa3ef6e5e2.json')
assert sha(mapping)=='3f8dd6fa3ef6e5e20a6f19030b347f47fa72e3103609c70f6c7e0ab3293b21b0'
previous=shared/'crossvenue-observer-trade-trace-v6';seal=previous/'input-trace-seal.json'
seal_sha='569a8e0c1ca0b8e326797cb5e8d83de68f3b80be98f5964303cb161e1216dde9'
assert sha(seal)==seal_sha and json.loads(seal.read_bytes())['complete'] is True
journals=[p for p in root.glob(f'breadth-{pid}-*.jsonl') if re.fullmatch(r'breadth-\d+-\d+\.jsonl',p.name)]
assert len(journals)==1;journal=journals[0]
assert journal.is_file() and not journal.is_symlink() and journal.stat().st_nlink==1 and journal.stat().st_size>0
assert journal.stat().st_mtime>=dt.datetime.fromisoformat(roll['at']).timestamp()
output=shared/'crossvenue-observer-trade-trace-v7';assert not output.exists()
unit=Path.home()/'.config/systemd/user/moloch-crossvenue-observer.service';old=unit.read_bytes();text=old.decode()
assert sha(unit)=='3cefd4d73a0e8d5febfa0ccfe74615b53cbabd45cb35807b04ef6829d277192f'
oldjournal='/home/archon/apps/moloch/shared/live-production-v1/breadth-540259-1792180336619.jsonl'
oldproducer='/home/archon/apps/moloch/sidecars/crossvenue-observer-2fa9b142b672f4a9.py'
assert text.count(oldjournal)==text.count(str(previous))==text.count(oldproducer)==1
assert '--duration-seconds 3600' in text and '--sampling-mode trade --input-trace' in text
new=text.replace(oldjournal,str(journal)).replace(str(previous),str(output)).replace(oldproducer,str(producer)).encode()
backup=unit.with_name(unit.name+'.before-v7-20261016');assert not backup.exists()
with backup.open('xb') as f:f.write(old);f.flush();os.fsync(f.fileno())
candidate=Path('/tmp/moloch-v7-observer-unit-20261016.service')
with candidate.open('xb') as f:f.write(new);f.flush();os.fsync(f.fileno())
subprocess.run(['systemd-analyze','--user','verify',str(candidate)],check=True)
assert unit.read_bytes()==old
temporary=unit.with_name(unit.name+'.v7-new');assert not temporary.exists()
with temporary.open('xb') as f:f.write(new);f.flush();os.fsync(f.fileno())
os.replace(temporary,unit)
subprocess.run(['systemctl','--user','daemon-reload'],check=True)
subprocess.run(['systemctl','--user','reset-failed','moloch-crossvenue-observer.service'],check=True)
subprocess.run(['systemctl','--user','start','moloch-crossvenue-observer.service'],check=True)
observer=int(subprocess.check_output(['systemctl','--user','show','moloch-crossvenue-observer.service','--property=MainPID','--value'],text=True));assert observer>0
cmd=(Path('/proc')/str(observer)/'cmdline').read_bytes()
assert str(producer).encode() in cmd and str(output).encode() in cmd and str(journal).encode() in cmd
assert sha(seal)==seal_sha and sha(producer)==producer_sha
r={'at':dt.datetime.now(dt.timezone.utc).isoformat(),'sigPid':pid,'observerPid':observer,'journal':str(journal),'output':str(output),'producerSha256':sha(producer),'mappingSha256':sha(mapping),'unitSha256':sha(unit),'previousUnitSha256':hashlib.sha256(old).hexdigest(),'v6SealSha256':sha(seal),'durationSeconds':3600,'defaultCombinedOutputHardCapBytes':2147483648,'minimumFreeDiskBytes':10*1024**3,'sigRestarted':False,'newAccountClient':False,'executionAuthority':False,'modelQualified':False,'orderingPolicyVersion':2,'originalSamplingPolicyUnchanged':True}
p=Path('/tmp/moloch-v7-observer-rollout-20261016.json')
with p.open('x') as f:f.write(json.dumps(r,indent=2)+'\n');f.flush();os.fsync(f.fileno())
print(json.dumps(r))
