"""New experiments explicitly choose the raw-action policy and event stream.

No fallback to the legacy fixed-step/target-fraction actor is allowed here.
"""
from pathlib import Path
import hashlib
from moloch.learning_realism.fast_replay import open_prepared_replay
from .engine import ApiTape
from .policy import ApiPolicy
from .stream import Stream


def open_experiment(specification,dataset,mechanics_path,role,*,start_ms,end_ms,
                    initial_cash_micros,target_wealth_micros,hidden_size=4096,device='cuda',
                    frozen_finalist_sha256=None,policy_checkpoint=None,checkpoint_sha256=None):
    if role in('evolution','test')and(policy_checkpoint is None or checkpoint_sha256 is None):
        raise ValueError('EVALUATION_REQUIRES_ACTUAL_CANDIDATE_CHECKPOINT')
    if role=='test'and checkpoint_sha256!=frozen_finalist_sha256:
        raise ValueError('TEST_MODEL_MUST_BE_EXACT_FROZEN_FINALIST')
    original,mechanics=open_prepared_replay(specification,dataset,mechanics_path,role,
                                         frozen_finalist_sha256=frozen_finalist_sha256)
    tape=ApiTape.from_loaded(original,split=original.split,role=role,delivery=original.delivery,
                            frozen_finalist_sha256=original.finalist)
    env=tape.episode(mechanics,start_ms,end_ms,initial_cash_micros=initial_cash_micros,
                     target_wealth_micros=target_wealth_micros,burnin_ms=0)
    if policy_checkpoint is None:policy=ApiPolicy(hidden_size=hidden_size).to(device)
    else:
        from .training import load_checkpoint
        policy,_=load_checkpoint(policy_checkpoint,checkpoint_sha256,device)
    stream=Stream(env,policy)
    stream.source_sha256={p.name:hashlib.sha256(p.read_bytes()).hexdigest()
                          for p in Path(__file__).parent.glob('*.py')if not p.name.startswith('test_')}
    return stream
