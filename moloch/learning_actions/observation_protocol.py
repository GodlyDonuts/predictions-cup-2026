"""Replay semantics are checkpoint identity, not just tensor dimensions."""
LEGACY='legacy_full_finance_unpaged_flow_v1'
CURRENT='sig_scoped_finance_paged_tape_v1'
QUERY_WINDOW='sig_scoped_finance_query_window_v2'
SCOPED_EXECUTION='sig_scoped_execution_receipts_v3'
MEMORY_JOINT='sig_scoped_local_memory_joint_v4'
ORDER_PAGES='sig_scoped_owned_order_pages_v5'
COMPONENT_DEBT='sig_scoped_component_debt_v6'


def validate(value):
    if type(value)is not str or value not in(LEGACY,CURRENT,QUERY_WINDOW,SCOPED_EXECUTION,MEMORY_JOINT,ORDER_PAGES,COMPONENT_DEBT):
        raise ValueError('EXPLICIT_OBSERVATION_PROTOCOL_REQUIRED')
    return value


def migrate_development(model,config):
    old=validate(getattr(model,'observation_protocol',LEGACY))
    target=validate(config.get('observation_protocol',CURRENT))
    if target not in(CURRENT,QUERY_WINDOW,SCOPED_EXECUTION,MEMORY_JOINT,ORDER_PAGES,COMPONENT_DEBT):raise ValueError('CURRENT_SIMULATOR_PROTOCOL_REQUIRED')
    if old==target:return None
    if config.get('mode')!='inspected_development'or config.get('migrate_observation_protocol')is not True:
        raise ValueError('EXPLICIT_DEVELOPMENT_OBSERVATION_PROTOCOL_MIGRATION_REQUIRED')
    model.observation_protocol=target
    return {'old':old,'new':target,'all_weights_retained':True,'optimizer_moments_reset':True,
            'old_trajectories_discarded':True,'old_scores_discarded':True,
            'new_training_and_scores_required':True,'independent_test_qualification':False}


def validate_architecture(model):
    from .joint_memory_policy import JointMemoryPolicy
    from .order_policy import OrderPagePolicy
    from .component_execution import ComponentPolicy
    expected={JointMemoryPolicy:MEMORY_JOINT,OrderPagePolicy:ORDER_PAGES,ComponentPolicy:COMPONENT_DEBT}
    if type(model)in expected:
        if model.observation_protocol!=expected[type(model)]:raise ValueError('JOINT_MEMORY_ARCHITECTURE_PROTOCOL_MISMATCH')
    elif model.observation_protocol in(MEMORY_JOINT,ORDER_PAGES,COMPONENT_DEBT):raise ValueError('JOINT_MEMORY_ARCHITECTURE_PROTOCOL_MISMATCH')
