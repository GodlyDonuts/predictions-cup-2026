"""Keep PPO sample count useful as independent worlds finish.

This sizes one optimizer batch. It does not cap or skip policy decisions, advance
market time, end a world, or change any economic reward. Legacy runs keep their
original sequence length unless the transition target is explicitly configured.
"""


def validate(config):
    value = config.get('training_chunk_transitions')
    if value is not None and (type(value) is not int or value <= 0):
        raise ValueError('POSITIVE_EXPLICIT_TRAINING_CHUNK_TRANSITIONS')
    if value is not None and config.get('closed_episode_return_replay', False):
        raise ValueError('TRANSITION_TARGET_REQUIRES_ONLINE_PPO')
    return value


def steps(config, active_worlds):
    target = validate(config)
    base = config.get('compute_chunk_decisions', 32)
    if type(base) is not int or base <= 0:
        raise ValueError('POSITIVE_COMPUTE_CHUNK')
    if type(active_worlds) is not int or active_worlds <= 0:
        raise ValueError('POSITIVE_ACTIVE_TRAINING_WORLD_COUNT')
    return base if target is None else max(base, (target + active_worlds - 1) // active_worlds)
