"""Opt-in historical liquidity confidence, never an actor information clock.

A fixed source lifetime is frozen before rollout. Expiry does not reveal a
future disconnect, emit a receipt, forbid an order or change original prices.
"""


def validate_lifetime(value):
    if value is not None and(type(value)is not int or value<=0):
        raise ValueError('EXPLICIT_POSITIVE_WORLD_BOOK_LIFETIME')
    return value


def schedule(env,exchange,source_at):
    lifetime=getattr(env,'world_book_lifetime_ms',None)
    if lifetime is None:return
    deadline=source_at+lifetime
    if deadline<=env.now:expire(env,env.now,(exchange,source_at))
    else:env.clock.schedule(deadline,15,'api_world_book_expiry',(exchange,source_at))


def expire(env,at,value):
    exchange,source_at=value;world=env.books.get(exchange)
    if world is None or world.invalidated or getattr(world,'source_state_at_ms',None)!=source_at:return
    lifetime=getattr(env,'world_book_lifetime_ms',None)
    if lifetime is None or at<source_at+lifetime:raise ValueError('WORLD_BOOK_EXPIRY_NOT_DUE')
    world.invalidate();env.world_confidence_expiries+=1


def known(env,exchange):
    if getattr(env,'world_book_lifetime_ms',None)is None:return True
    world=env.books.get(exchange)
    return world is not None and not world.invalidated
