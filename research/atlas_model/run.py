"""Offline fitting, actual held-cycle validation, joint artifact export. No client."""
import os
os.environ.setdefault('OPENBLAS_NUM_THREADS','1');os.environ.setdefault('OMP_NUM_THREADS','1')
import argparse,datetime,hashlib,json
from pathlib import Path
from model import fit,validate,artifact,canonical

def main():
 p=argparse.ArgumentParser();p.add_argument('--training',type=Path,required=True);p.add_argument('--current',type=Path,required=True);p.add_argument('--out',type=Path,required=True);p.add_argument('--draws',type=int,default=65536);p.add_argument('--seed',type=int,default=1701);a=p.parse_args()
 if not all(x.is_absolute() for x in [a.training,a.current,a.out]):raise ValueError('absolute private input/output paths required')
 if a.out.exists():raise ValueError('output must not exist; immutable runs')
 training_bytes=a.training.read_bytes();training=json.loads(training_bytes);current_bytes=a.current.read_bytes();current=json.loads(current_bytes)
 model=fit(training['rows']);calibration=validate(training['rows']);provenance=dict(training['provenance']);provenance['sources']=provenance['sources']+current['sources']+[{'sha256':hashlib.sha256(training_bytes).hexdigest(),'role':'normalized training rows'},{'sha256':hashlib.sha256(current_bytes).hexdigest(),'role':'exact current inputs'},{'sha256':hashlib.sha256(Path(__file__).with_name('model.py').read_bytes().replace(b'\r\n',b'\n')).hexdigest(),'role':'model source normalized Git LF'}]
 surface=artifact(model,current['rows'],provenance,calibration,current['observedAtMs'],current['expiresAtMs'],a.draws,a.seed)
 a.out.mkdir(mode=0o700)
 outputs={'model.json':model,'validation.json':calibration,'atlas.json':surface,'run.json':{'schemaVersion':1,'observedAtUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'trainingSha256':hashlib.sha256(training_bytes).hexdigest(),'currentSha256':hashlib.sha256(current_bytes).hexdigest(),'modelCodeSha256':hashlib.sha256(Path(__file__).with_name('model.py').read_bytes().replace(b'\r\n',b'\n')).hexdigest(),'modelCodeHashNormalization':'git_lf','seed':a.seed,'draws':a.draws,'accountRequests':0,'executionAuthority':False}}
 for name,data in outputs.items():
  fd=os.open(a.out/name,os.O_CREAT|os.O_EXCL|os.O_WRONLY,0o600)
  with os.fdopen(fd,'wb') as f:f.write(canonical(data)+b'\n');f.flush();os.fsync(f.fileno())
 print(json.dumps({'races':model['races'],'observations':model['pollsterRaceObservations'],'cycles':len(model['cycles']),'variance':model['variance'],'loco':calibration['leaveElectionCycleOut'],'baseline':calibration['sameFoldPooledErrorBaseline'],'chronological':calibration['chronologicalOutcomeDiagnostic'],'scenarios':len(surface['scenarios']),'qualification':surface['qualification'],'modelSha256':surface['modelSha256']},indent=2))
if __name__=='__main__':main()
