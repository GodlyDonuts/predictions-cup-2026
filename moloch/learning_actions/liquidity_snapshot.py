"""Copy receipt state while sharing only proven recursively immutable depth.

No global deepcopy override: custom fixture objects and extended state keep
their original copy behavior. Each receipt owns its depletion/invalidation.
"""
from copy import deepcopy
from moloch.learning_v2.liquidity import Book,SharedLiquidity


def immutable_book(book):
    if type(book)is not Book or set(vars(book))!={
            'exchange','available_at_ms','source_at_ms','bids','asks','original_sha256'}:
        return False
    if(type(book.exchange)is not str or type(book.available_at_ms)is not int
       or book.source_at_ms is not None and type(book.source_at_ms)is not int
       or type(book.original_sha256)is not str):return False
    return all(type(levels)is tuple and all(
        type(level)is tuple and len(level)==2 and all(type(v)is int for v in level)
        for level in levels)for levels in(book.bids,book.asks))


def receipt_snapshot(value):
    if type(value)is not SharedLiquidity:return deepcopy(value)
    attrs=vars(value)
    if (not {'book','depleted','invalidated'}<=attrs.keys()
        or attrs.keys()-{'book','depleted','invalidated','source_state_at_ms'}
        or not immutable_book(value.book)
        or type(value.depleted)is not dict or type(value.invalidated)is not bool
        or 'source_state_at_ms'in attrs and type(attrs['source_state_at_ms'])is not int
        or any(type(k)is not tuple or len(k)!=2 or type(k[0])is not str
               or type(k[1])is not int or type(v)is not int for k,v in value.depleted.items())):
        return deepcopy(value)
    result=SharedLiquidity(value.book)
    result.depleted=value.depleted.copy();result.invalidated=value.invalidated
    if 'source_state_at_ms'in attrs:result.source_state_at_ms=attrs['source_state_at_ms']
    return result
