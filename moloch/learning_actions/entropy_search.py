"""Reach configured exploration settings without resetting learned actors."""
import math


def probability(value):
    if type(value)not in(int,float)or not math.isfinite(value)or not 0<=value<=1:
        raise ValueError('FINITE_ENTROPY_RESAMPLE_PROBABILITY_REQUIRED')
    return value


def coefficients(values):
    if not isinstance(values,(list,tuple))or not values or any(
        type(v)not in(int,float)or not math.isfinite(v)or v<0 for v in values):
        raise ValueError('FINITE_CONFIGURED_ENTROPY_CHOICES_REQUIRED')
    return values


def offspring(parent,operator,scales,rng,choices,*,resample_probability=0):
    p=probability(resample_probability);coefficients(choices)
    if type(parent)not in(int,float)or not math.isfinite(parent)or parent<0:
        raise ValueError('FINITE_PARENT_ENTROPY_REQUIRED')
    if operator=='fresh':return rng.choice(choices),'fresh_configuration_choice'
    if operator not in scales:raise ValueError('KNOWN_ENTROPY_MUTATION_OPERATOR_REQUIRED')
    # Do not consume another random number in the legacy profile.
    if p and rng.random()<p:return rng.choice(choices),'configured_grid_resample'
    return min(.1,max(0.,parent*rng.choice(scales[operator]))),'multiplicative_perturbation'
