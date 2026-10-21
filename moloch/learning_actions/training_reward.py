"""Explicit training horizons and one reward law for collection and replay.

Short inspected episodes remain useful diagnostics, but cannot stand in for
the human's multi-day reward. Complete-day development keeps qualification
false even when it computes exactly that reward.
"""
from moloch.learning_realism.protocol import DAY
from .economics import GrowthLedger


def horizon(config, bounds, *, development):
    a,b=bounds
    daily=config.get('development_training_daily_growth',False)
    if type(daily)is not bool or daily and not development:
        raise ValueError('EXPLICIT_DEVELOPMENT_TRAINING_DAILY_GROWTH_PROFILE')
    if development and not daily:
        length=config.get('development_episode_ms',120000)
        if type(length)is not int or length<=0:
            raise ValueError('POSITIVE_DEVELOPMENT_TRAINING_HORIZON_REQUIRED')
        return min(length,b-a)
    days=config.get('training_days',2)
    if type(days)is not int or days<2:
        raise ValueError('AT_LEAST_TWO_INTEGER_TRAINING_DAYS_REQUIRED')
    length=days*DAY
    if length>b-a:
        raise ValueError('COMPLETE_DAILY_TRAINING_HISTORY_REQUIRED')
    if development and 'development_episode_ms'in config and config['development_episode_ms']!=length:
        raise ValueError('DAILY_TRAINING_CANNOT_USE_SHORT_DEVELOPMENT_EPISODE')
    return length


def ledger(env, config, *, development):
    daily=config.get('development_training_daily_growth',False)
    # Validate the exact reconstructed world too, not just its parent's bounds.
    length=horizon(config,(env.now,env.end),development=development)
    if (not development or daily)and length!=env.end-env.now:
        raise ValueError('EXACT_COMPLETE_TRAINING_WORLD_REQUIRED')
    return GrowthLedger(env,development=development,
        development_daily_growth=daily,
        historical_last_mark=config.get('development_last_observed_marks',False),
        maximum_mark_age_ms=config.get('maximum_mark_age_ms',120000))
