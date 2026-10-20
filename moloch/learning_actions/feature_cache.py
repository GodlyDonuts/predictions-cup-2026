"""Pure feature memoization. No stale ages, masks, positions or source facts.

LRU limits bound a performance cache only: misses compute the same full value.
Returned arrays are immutable; each observation still owns its output matrix.
"""
from functools import lru_cache
import hashlib
import numpy as np
from .contract import UNIT


@lru_cache(maxsize=8192)
def identity(kind,key):
    value=np.frombuffer(hashlib.sha256(repr((kind,key)).encode()).digest()[:7],dtype=np.uint8)/255.
    value.setflags(write=False)
    return value


def entity_identities(env,entities):
    """Immutable default identity columns for this exact ordered layout.

    Receipt rows may override these identities with delivered contract IDs;
    those overrides remain dynamic in the observation builder.
    """
    context=tuple(entities);cached=getattr(env,'_entity_identity_matrix_cache',None)
    if cached is not None and cached[0]==context:return cached[1]
    values=np.asarray([identity(kind,key)for kind,key in context],dtype=np.float32).reshape(len(context),7)
    values.setflags(write=False);env._entity_identity_matrix_cache=(context,values)
    return values


@lru_cache(maxsize=8192)
def log_quantity(quantity):
    return np.log1p(quantity)


def book_statistics(env,exchange,book):
    cache=getattr(env,'_book_statistics_cache',None)
    if cache is None:cache=env._book_statistics_cache={}
    cached=cache.get(exchange)
    if cached is None or cached[0]is not book:
        bid=book.bids[0][0]if book.bids else 0
        ask=book.asks[0][0]if book.asks else UNIT
        values=np.array([bid/UNIT,ask/UNIT,(ask-bid)/UNIT,0.,
            log_quantity(sum(n for p,n in book.bids)),log_quantity(sum(n for p,n in book.asks)),1.,0.],np.float32)
        values.setflags(write=False);cache[exchange]=(book,values)
    return cache[exchange][1]
