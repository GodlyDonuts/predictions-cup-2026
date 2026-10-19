"""Observed snapshot receipt as a hidden-world hypothesis, not exchange truth.

Original information-delivery clocks remain unchanged. Sparse observations do
not identify the continuous historical exchange book or counterfactual impact.
All hidden rows must belong to the current authorized INFORMATION calendar.
"""
from types import MappingProxyType
from moloch.learning_v2.liquidity import Book,SharedLiquidity
from moloch.learning_v2.reference_env import ReferenceEnv

MODE='original_observed_snapshot_world_separate_information_v1'


def source_clock(row):
    at=row['received_at_ms']if row['kind']=='book'else row['native_event_received_at_ms']
    processed=row['native_processed_at_ms'];received=row['native_event_received_at_ms'];available=row['available_at_ms']
    if any(type(v)is not int or v<0 for v in (at,processed,received,available))or not at<=received<=processed<=available:
        raise ValueError('SOURCE_SNAPSHOT_INFORMATION_CLOCK_ORDER')
    if row['kind']=='book'and row.get('source_at_ms')is not None and row['source_at_ms']>at:
        raise ValueError('SOURCE_CLOCK_AHEAD_OF_OBSERVED_SNAPSHOT')
    return at


def prepare(tape,information_bounds):
    lo,hi=information_bounds
    if lo>=hi:raise ValueError('AUTHORIZED_SNAPSHOT_INFORMATION_BOUNDS')
    rows=[];books={}
    for row in tape.rows:
        if row['venue']!='SIG'or row['kind']not in ('book','gap')or not lo<=row['available_at_ms']<hi:continue
        at=source_clock(row)
        if not lo<=at<hi:continue  # No cross-role world warm-up from later receipts.
        rows.append(row)
        if row['kind']=='book':
            books[id(row)]=Book(row['exchange_id'],at,row.get('source_at_ms'),
                tuple(map(tuple,row['bids'])),tuple(map(tuple,row['asks'])),row['original_sha256'])
    rows.sort(key=source_clock)
    tape.snapshot_world_mode=MODE;tape.snapshot_world_information_bounds=tuple(information_bounds)
    tape.snapshot_world_rows=tuple(rows);tape.snapshot_world_times=tuple(map(source_clock,rows))
    tape.prepared_world_books=MappingProxyType(books)


def older(payload,current):
    a=getattr(payload,'source_state_at_ms',None);b=getattr(current,'source_state_at_ms',None)
    return a is not None and b is not None and a<b


def world_event(env,at,row):
    before=len(env.api_fills)
    if row['kind']=='book':
        ex=row['exchange_id'];env.world_market_mapping[ex]=row['market_id']
        ReferenceEnv._event(env,at,'book',env.prepared_world_books[id(row)])
        env.books[ex].source_state_at_ms=at
        if env.passive is not None:env.passive.on_book(ex)
        for q,order in list(env.api_orders.items()):
            if order.spec.exchange==ex:
                env._match_api(order)
                if not order.remaining:env.api_orders.pop(q,None)
    else:
        ex=row.get('exchange_id');market=row.get('market_id')
        for exchange,world in env.books.items():
            if exchange==ex or(ex is None and(market is None or env.world_market_mapping.get(exchange)==market)):
                world.invalidate();world.source_state_at_ms=at
    if len(env.api_fills)>before:env.notify_fill()


def information_event(env,at,row):
    if row['kind']=='book':
        ex=row['exchange_id'];market=row['market_id']
        env.scope.setdefault(market,set()).add(ex);env.mapping[ex]=market;env._refresh_reference(market)
        env._remember('SIG',('book',ex),row)
        if env.realtime is not None:
            payload=SharedLiquidity(env.prepared_books[id(row)]);payload.source_state_at_ms=source_clock(row)
            env.realtime.source_book(at,ex,payload)
    elif env.realtime is not None:
        env.realtime.source_gap(row,source_state_at_ms=source_clock(row))
