"""PPO minibatches from one sealed, originally on-policy TRAIN cohort.

This differs from stale/open streaming chunks: the whole behavior rollout is
closed and fixed before optimization, and its exact source replay is verified.
Training returns include every later reward and the existing terminal correction.
"""
import torch
from .episode_tape import EpisodeReader,EpisodeWriter
from .behavior_guard import identity,require_contract
from moloch.learning_v2.train_inner import fingerprint


def cpu_copy(node):
    import copy
    if isinstance(node,torch.Tensor):return node.detach().cpu().clone()
    if isinstance(node,dict):return {k:cpu_copy(v)for k,v in node.items()}
    if isinstance(node,list):return [cpu_copy(v)for v in node]
    if isinstance(node,tuple):return tuple(cpu_copy(v)for v in node)
    return copy.deepcopy(node)


def release_worlds(envs):
    # Closing an abandoned generator releases its own read-only SQL cursor; it
    # neither consumes future rows nor closes the shared source database.
    for env in envs:
        service=getattr(env,'managed_data_service',None)
        if service is not None:service.close()
        for event in env.clock._events:
            if event[3]=='api_source_cursor':
                close=getattr(event[-1].rows,'close',None)
                if close is not None:close()


class EpisodeSession:
    def __init__(self,reader,model):
        if type(reader)is not EpisodeReader:raise ValueError('SEALED_TRAIN_EPISODE_READER_REQUIRED')
        if (model.architecture!=reader.metadata['architecture']or
            model.observation_protocol!=reader.metadata['observation_protocol']):
            raise ValueError('BEHAVIOR_ARCHITECTURE_OR_PROTOCOL_CHANGED')
        require_contract(model,reader.metadata.get('behavior_contract'))
        if fingerprint(model)!=reader.metadata['actor_sha256']:raise ValueError('ORIGINAL_BEHAVIOR_WEIGHTS_REQUIRED')
        self.reader=reader;self.model=model;self.expected=identity(model,'tensor_versions_v1');self.next_batch=0

    def authorize(self,model,chunk):
        if model is not self.model or identity(model,'tensor_versions_v1')!=self.expected:
            raise ValueError('CLOSED_EPISODE_STUDENT_CHANGED_OUTSIDE_OPTIMIZER')
        if getattr(chunk,'episode_reader',None)is not self.reader or chunk.episode_batch!=self.next_batch:
            raise ValueError('EXACT_NEXT_CLOSED_EPISODE_MINIBATCH_REQUIRED')
        if chunk.behavior_sha256!=self.reader.metadata['actor_sha256']or chunk.role!='train':
            raise ValueError('CLOSED_EPISODE_ORIGINAL_BEHAVIOR_REQUIRED')
        targets=[]
        for step in range(len(chunk.frames)):
            values=dict(self.reader.db.execute('SELECT world,target FROM targets WHERE batch=? AND step=?',
                                               (chunk.episode_batch,step)))
            targets.append([values[i]for i in chunk.episode_worlds])
        return targets,(self.reader.metadata['advantage_mean'],self.reader.metadata['advantage_std'])

    def finished(self,model):
        self.expected=identity(model,'tensor_versions_v1');self.next_batch+=1


def optimize(reader,model,optimizer,world_factory,ledger_factory,*,epochs=1,checkpoint_encoder=False,entropy_coefficient=.01,progress=None,
             continue_after_kl_rejection=False):
    """Bounded replay chunks, original behavior probabilities and global advantages.

    Restore the whole learner on any replay/integrity error; never publish a
    partly invalid update sequence. By default KL rejection ends optimization.
    The explicit continuation profile rolls back the rejected step as before,
    then tries later, independently checked minibatches from the same rollout.
    """
    from .training import update
    if type(epochs)is not int or epochs<1:raise ValueError('POSITIVE_CLOSED_EPISODE_EPOCHS_REQUIRED')
    if type(continue_after_kl_rejection)is not bool:raise ValueError('EXPLICIT_CLOSED_EPISODE_REJECTION_PROFILE')
    # CPU backup bounds extra GPU allocation independently of rollout length.
    backup={k:v.detach().cpu().clone()for k,v in model.state_dict().items()}
    optimizer_backup=cpu_copy(optimizer.state_dict())
    session=EpisodeSession(reader,model);reports=[];original=reader.metadata['actor_sha256']
    try:
        # One source reconstruction pass is one minibatch epoch. Further epochs
        # use the same behavior rollout and targets, never recollected new data.
        for epoch in range(epochs):
            session.next_batch=0
            envs=[world_factory(a,b)for a,b in reader.metadata['world_bounds']]
            ledgers=[ledger_factory(e)for e in envs]
            try:
                for chunk,_ in reader.replay(envs,ledgers,next(model.parameters()).device):
                    row=update(model,optimizer,chunk,[f['economic_reward']for f in chunk.frames],role='train',
                        epochs=1,checkpoint_encoder=checkpoint_encoder,entropy_coefficient=entropy_coefficient,
                        episode_session=session)
                    reports.extend({**r,'episode_epoch':epoch,'episode_batch':chunk.episode_batch,
                                    'continue_after_kl_rejection':continue_after_kl_rejection}for r in row)
                    if progress is not None:progress(row,len(reports))
                    if not continue_after_kl_rejection and any(not r['accepted']for r in row):return reports
            finally:release_worlds(envs)
        return reports
    except BaseException:
        model.load_state_dict(backup);optimizer.load_state_dict(optimizer_backup)
        if fingerprint(model)!=original:raise ValueError('CLOSED_EPISODE_ROLLBACK_DID_NOT_RESTORE_ORIGINAL_POLICY')
        raise
