"""Explicit owned-order read semantics, without changing action head shapes."""
from .joint_memory_policy import JointMemoryPolicy
from .observation_protocol import ORDER_PAGES


class OrderPagePolicy(JointMemoryPolicy):
    architecture='tartarus-raw-api-full-depth-owned-order-pages-gru-v6'
    observation_protocol=ORDER_PAGES


def migrate_owned_orders(parent):
    if type(parent)is not JointMemoryPolicy:raise ValueError('EXACT_JOINT_MEMORY_PARENT_REQUIRED_FOR_ORDER_PAGE_MIGRATION')
    child=OrderPagePolicy(parent.hidden_size).to(next(parent.parameters()).device)
    child.load_state_dict(parent.state_dict());child.global_state_encoding=getattr(parent,'global_state_encoding','raw');child.train(parent.training)
    return child,{'operator':'explicit_paid_owned_order_pagination_and_status_history',
        'old_architecture':parent.architecture,'new_architecture':child.architecture,
        'all_weights_preserved_exactly':True,'optimizer_reset_required':True,
        'old_trajectories_must_not_be_reused':True,'old_scores_must_not_be_reused':True,
        'independent_test_qualification':False}
