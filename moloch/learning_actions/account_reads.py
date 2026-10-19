"""Endpoint-scoped simulated finance reads, with internal receipt ordering.

GET/account exposes cash/available advance, positions exposes lots, orders
exposes known orders, and collateral exposes available advance. Complete order
pagination, collateral components and official position valuation are separate
unfinished adapters. Internal version guards are never actor observations.
"""
from copy import deepcopy

FIELDS = ('cash','lots','orders','advance')
RESOURCES = {'account':('cash','advance'), 'positions':('lots',),
             'owned_orders':('orders',), 'collateral':('advance',)}


def capture(env, resource):
    if resource not in RESOURCES:
        raise ValueError('KNOWN_FINANCIAL_READ_RESOURCE_REQUIRED')
    getters={'cash':lambda:env.cash, 'lots':lambda:env.lots,
             'orders':lambda:env.api_orders, 'advance':env.advance_capacity}
    return {'revision':env.finance_revision,
            'fields':deepcopy({k:getters[k]()for k in RESOURCES[resource]})}


def publish(env, snapshot):
    revision, fields = snapshot['revision'], snapshot['fields']
    if type(revision)is not int or revision<0 or not fields or any(k not in FIELDS for k in fields):
        raise ValueError('INTERNAL_FINANCE_SNAPSHOT_REQUIRED')
    applied=False
    for field,value in deepcopy(fields).items():
        if revision<env.known_resource_revisions[field]:
            continue
        if field=='cash':env.known_cash=value
        elif field=='lots':env.known_lots=value
        elif field=='advance':env.known_advance=value
        else:
            from .execution_receipts import enabled,orders
            if enabled(env):orders(env,value,revision,complete=True)
            else:env.known_orders={q:o for q,o in value.items()
                  if o.spec.expiration_ms is None or o.spec.expiration_ms>env.now}
        env.known_resource_revisions[field]=revision
        applied=True
    if applied:
        env.known_finance_revision=max(env.known_resource_revisions.values())
        env.finance_observation_count+=1
        env.observation_revision+=1
    return applied
