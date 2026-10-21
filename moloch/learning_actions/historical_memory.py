"""Persistent view of actually delivered history, never an unpaid price feed.

The cache mirrors the latest response per exact endpoint/source/market/side.
An older-page query or empty response replaces that endpoint's prior summary;
this does not merge contracts or infer price equivalence across venues.
"""
LATEST='paid_history_latest_response_record_v1'


def profile(value):
    if value is not None and(type(value)is not str or value!=LATEST):
        raise ValueError('EXPLICIT_HISTORICAL_MEMORY_PROFILE')
    return value


def bound_profile(value):
    mode=profile(getattr(value,'historical_memory_profile',None))
    if mode is not None:
        from .observation_protocol import HISTORICAL_INFORMATION
        if getattr(value,'observation_protocol',None)!=HISTORICAL_INFORMATION:
            raise ValueError('HISTORICAL_MEMORY_REQUIRES_HISTORY_OBSERVATIONS')
    return mode


def delivered(env,venue,key,receipt):
    if bound_profile(env)is None:return
    from .historical_neural import records,encode,OLD
    rows=records(key,receipt['payload'])
    ordinal,row=max(enumerate(rows),key=lambda pair:(
        pair[1]['simulated_available_at_ms'],pair[1]['source_at_ms'],pair[0]))if rows else(0,None)
    # Existing encoding checks request/receipt/publication causality and typed
    # prices. Store a value copy, not a pointer into provider/source payloads.
    vector=encode(venue,key,receipt,row,ordinal,env.now)[OLD:].copy()
    vector.setflags(write=False)
    clocks=(receipt['received_at_ms'],None if row is None else row['source_at_ms'],
            None if row is None else row['simulated_available_at_ms'])
    if not 0<=receipt['requested_at_ms']<=clocks[0]<=env.now:
        raise ValueError('PAID_HISTORY_RECEIPT_CLOCK_REQUIRED')
    env._paid_history_summaries[(venue,key)]=(vector,clocks)


def populate(env,values,entities,indices):
    if bound_profile(env)is None or not env._paid_history_summaries:return
    from .historical_neural import OLD,signed_log
    layout=getattr(env,'_paid_history_layout',None)
    if layout is None or layout[0]!=entities:
        layout=(entities,{entities[int(i)][1]:int(i)for i in indices})
        env._paid_history_layout=layout
    for address,(vector,(received,source,available))in env._paid_history_summaries.items():
        i=layout[1].get(address)
        if i is None:raise ValueError('PAID_HISTORY_ENDPOINT_DISAPPEARED')
        h=values[i,OLD:];h[:]=vector
        h[8]=signed_log((env.now-received)/1000)
        if source is not None:
            h[9]=signed_log((env.now-source)/1000)
            h[10]=signed_log((env.now-available)/1000)
