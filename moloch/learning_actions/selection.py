"""Durable finalist freeze and one-shot independent test access.

An exclusive create is the local concurrency boundary. A failed test consumes
the reservation too: it cannot be repaired by inspecting its prices and retrying
another candidate. This is not a production execution lock.
"""
import hashlib,json
import os
from pathlib import Path
from datetime import datetime,timezone
from moloch.learning_realism.protocol import valid_sha,digest


class DataUse:
    """Conservative local record of calendars already exposed to optimization.

    This is not proof about unrecorded/manual/other-host use. Records cross
    dataset compilations and experiment directories; moving a test boundary
    slightly cannot make already inspected outcomes independent again.
    """
    def __init__(self,directory=None):
        self.root=(Path(directory)if directory is not None else
            Path(os.environ.get('LOCALAPPDATA',Path.home()/'.local/share'))/
                'Moloch/learning-v2/data-use-registry-v1')
        self.root.mkdir(parents=True,exist_ok=True)

    def overlaps(self,start,end,*,ignore_test=False):
        found=[]
        for p in self.root.glob('*.json'):
            row=json.loads(p.read_text())
            if ignore_test and row['kind']=='test':continue
            if start<row['end_ms']and row['start_ms']<end:found.append(row)
        return found

    def require_unseen(self,start,end):
        if self.overlaps(start,end):raise ValueError('TEST_CALENDAR_PREVIOUSLY_USED_NOT_INDEPENDENT')

    def record(self,kind,start,end,source_sha256):
        if kind not in('train','selection','inspected_development','test')or not valid_sha(source_sha256):
            raise ValueError('EXPLICIT_DATA_USE_AND_SOURCE_REQUIRED')
        if any(type(v)is not int or v<0 for v in(start,end))or end<=start:
            raise ValueError('DATA_USE_CALENDAR_REQUIRED')
        if kind=='test':self.require_unseen(start,end)
        value={'kind':kind,'start_ms':start,'end_ms':end,'source_sha256':source_sha256}
        path=self.root/(digest(value)+'.json')
        if path.exists():
            if json.loads(path.read_text())!=value:raise ValueError('DATA_USE_RECORD_CHANGED')
        else:frozen(path,value)
        return path


def frozen(path,value):
    path=Path(path);path.parent.mkdir(parents=True,exist_ok=True)
    raw=json.dumps(value,sort_keys=True,indent=2,allow_nan=False).encode()
    with path.open('xb')as f:f.write(raw)
    return hashlib.sha256(raw).hexdigest()


class IndependentTest:
    def __init__(self,directory,experiment_sha256):
        if not valid_sha(experiment_sha256):raise ValueError('PINNED_EXPERIMENT_REQUIRED')
        self.root=Path(directory)/experiment_sha256
        self.specification=experiment_sha256

    def freeze(self,checkpoint,checkpoint_sha256,evolution_result):
        if not valid_sha(checkpoint_sha256):raise ValueError('REAL_CHECKPOINT_SHA_REQUIRED')
        if hashlib.sha256(Path(checkpoint).read_bytes()).hexdigest()!=checkpoint_sha256:
            raise ValueError('FINALIST_CHECKPOINT_CHANGED')
        if evolution_result.get('role')!='evolution' or evolution_result.get('eligible')is not True:
            raise ValueError('QUALIFIED_EVOLUTION_REPORT_REQUIRED')
        if evolution_result.get('checkpoint_sha256')!=checkpoint_sha256:
            raise ValueError('EVOLUTION_MUST_EVALUATE_THIS_CHECKPOINT')
        if evolution_result.get('reward',0)<=0:raise ValueError('POSITIVE_EVOLUTION_REWARD_REQUIRED')
        return frozen(self.root/'finalist.json',{'experiment_sha256':self.specification,
            'checkpoint_sha256':checkpoint_sha256,'evolution_report_sha256':digest(evolution_result),
            'frozen_at_utc':datetime.now(timezone.utc).isoformat()})

    def reserve(self,checkpoint_sha256):
        value=json.loads((self.root/'finalist.json').read_text())
        if value['checkpoint_sha256']!=checkpoint_sha256:raise ValueError('ONLY_FROZEN_FINALIST_MAY_TEST')
        return frozen(self.root/'test-access-consumed.json',{
            'experiment_sha256':self.specification,'checkpoint_sha256':checkpoint_sha256,
            'reserved_at_utc':datetime.now(timezone.utc).isoformat(),
            'retry_or_adaptive_candidate_replacement_allowed':False})

    def finish(self,report):
        value=json.loads((self.root/'test-access-consumed.json').read_text())
        if report.get('role')!='test' or report.get('checkpoint_sha256')!=value['checkpoint_sha256']:
            raise ValueError('EXACT_RESERVED_TEST_REPORT_REQUIRED')
        return frozen(self.root/'independent-result.json',report)
