"""Scoped simulated ACKs/private events, never free account reconciliation.

Internal revisions order receipts but are not observations. Cash, positions and
available advance require their paid baselines; raw visible fills remain usable
as information. This does not claim calibrated account-topic delivery.
"""
from copy import deepcopy
from .observation_protocol import SCOPED_EXECUTION
from .contract import UNIT


def enabled(env):return env.observation_protocol==SCOPED_EXECUTION


def orders(env,values,revision,*,complete=False):
    revisions=env.known_order_revisions
    keys=set(values)
    if complete:keys|=set(env.known_orders)
    for key in keys:
        if revision<max(revisions.get(key,-1),env.known_resource_revisions['orders']):continue
        value=values.get(key)
        if value is None:env.known_orders.pop(key,None)
        else:env.known_orders[key]=deepcopy(value)
        revisions[key]=revision


def ack(env,operation,payload,record,fill_start):
    """Only the operation's documented order results enter its receipt."""
    revision=env.finance_revision;targets={};rows=[]
    if record['status']=='EXECUTED':
        result=record['result']
        if operation in('ORDER','BATCH','MULTI','ALL_OR_NONE'):
            for value in result:
                if value is None or isinstance(value,str):continue
                targets[value.id]=env.api_orders.get(value.id)
                fills=[f for f in env.api_fills[fill_start:]if f['order']==value.id]
                rows.append({'order':value.id,'exchange':value.spec.exchange,'side':value.spec.side,
                    'action':value.spec.action,'quantity':value.spec.quantity,'remaining':value.remaining,
                    'open':value.id in env.api_orders,
                    'quantity_traded':sum(f['quantity']for f in fills),
                    'total_cost_micros':sum(f['quantity']*f['price']for f in fills),
                    'last_price_micros':fills[-1]['price']if fills else None,'at_ms':env.now})
        elif operation=='CANCEL':targets[payload]=None
        else:
            # Cancel-all does not reveal previously unknown order metadata.
            for q,o in env.known_orders.items():
                if payload is None or(isinstance(payload,str)and o.spec.exchange==payload)or(
                    isinstance(payload,dict)and env.mapping.get(o.spec.exchange)==payload.get('market')):
                    targets[q]=None
    return deepcopy({'revision':revision,'orders':targets,'acknowledgments':rows})


def publish_ack(env,snapshot):
    orders(env,snapshot['orders'],snapshot['revision'])
    env.known_acknowledgments=deepcopy(snapshot['acknowledgments'])
    for row in env.known_acknowledgments:row['at_ms']=env.now  # Local HTTP receipt, not hidden server arrival.
    env.observation_revision+=1


def fill(row,revision):
    # Realtime prices are YES-normalized, unlike REST outcome-side prices.
    return {'order':row['order'],'exchange':row['exchange'],'quantity':row['quantity'],
        'yes_price_micros':row['price']if row['side']=='YES'else UNIT-row['price'],
        'executed_at_ms':row['filled_at_ms'],'revision':revision}


def publish_private(env,snapshot):
    fills=snapshot['fills'];env.known_private_fills=deepcopy(fills)
    # Apply all same-revision fills together, so a multi-price fill is not lost.
    groups={}
    for row in fills:groups.setdefault((row['revision'],row['order']),[]).append(row)
    for(revision,key),rows in groups.items():
        if revision<=max(env.known_order_revisions.get(key,-1),env.known_resource_revisions['orders']):continue
        known=env.known_orders.get(key)
        if known is not None:
            known=deepcopy(known);known.remaining=max(0,known.remaining-sum(r['quantity']for r in rows))
            orders(env,{key:known if known.remaining else None},revision)
        else:env.known_order_revisions[key]=revision  # Do not resurrect pre-fill ACK state; REST can reconcile.
    for revision,values in snapshot['orders']:orders(env,values,revision)
    env.observation_revision+=1
