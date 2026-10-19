"""Full joint score for samples that become the actor's next local memory.

V4 and V5 have identical parameters and sampling laws. Density semantics are
different: an unused API argument is still a stochastic memory action when
the next observation exposes it. Old rollouts/optimizer state are not reused.
"""
from .query_policy import QueryWindowPolicy
from .observation_protocol import MEMORY_JOINT


class JointMemoryPolicy(QueryWindowPolicy):
    architecture='tartarus-raw-api-full-depth-joint-memory-gru-v5'
    observation_protocol=MEMORY_JOINT
    local_memory_joint_density=True


def migrate_query_memory(parent):
    if type(parent)is not QueryWindowPolicy:raise ValueError('EXACT_QUERY_PARENT_REQUIRED_FOR_JOINT_MEMORY_MIGRATION')
    child=JointMemoryPolicy(parent.hidden_size).to(next(parent.parameters()).device)
    child.load_state_dict(parent.state_dict())
    child.global_state_encoding=getattr(parent,'global_state_encoding','raw')
    child.train(parent.training)
    return child,{'operator':'explicit_full_local_memory_joint_density',
        'old_architecture':parent.architecture,'new_architecture':child.architecture,
        'all_existing_weights_preserved_exactly':True,'sampling_law_unchanged':True,
        'optimizer_reset_required':True,'old_trajectories_must_not_be_reused':True,
        'old_scores_must_not_be_reused':True,'independent_test_qualification':False}
