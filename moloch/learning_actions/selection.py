"""Durable finalist freeze and one-shot independent test access.

An exclusive create is the local concurrency boundary. A failed test consumes
the reservation too: it cannot be repaired by inspecting its prices and retrying
another candidate. This is not a production execution lock.
"""
import hashlib,json
from pathlib import Path
from datetime import datetime,timezone
from moloch.learning_realism.protocol import valid_sha,digest


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
