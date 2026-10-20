"""Past public information is queryable across scoring roles, never future data.

This changes the server-side reporting tape only. It does not inherit actor
observations, orders, wallets, hidden books or counterparty execution events.
"""
MODE = 'causal_paid_public_trade_history_prefix_v1'


def enabled(config):
    value = config.get('public_history_prefix', config.get('mode') == 'full_history')
    if type(value) is not bool:
        raise ValueError('EXPLICIT_PUBLIC_HISTORY_PREFIX_BOOLEAN')
    return value


def query_bounds(train, scoring):
    if (len(train) != 2 or len(scoring) != 2 or
            any(type(x) is not int or x < 0 for x in (*train, *scoring)) or
            not train[0] < train[1] or not scoring[0] < scoring[1] or
            train[0] > scoring[0] or
            not (tuple(train) == tuple(scoring) or train[1] <= scoring[0])):
        raise ValueError('CAUSAL_PUBLIC_HISTORY_AND_SCORING_BOUNDS')
    return train[0], scoring[1]


def configure(tape, train, scoring):
    bounds = query_bounds(train, scoring)
    # Score/hidden-world bounds remain authoritative and unchanged.
    if tuple(tape.counterparty_information_bounds) != tuple(scoring):
        raise ValueError('PUBLIC_HISTORY_CANNOT_REWRITE_EXECUTION_ROLE')
    tape.reporting_history_information_bounds = bounds
    return bounds


def information_bounds(tape, scoring):
    history = getattr(tape, 'reporting_history_information_bounds', scoring)
    if (len(history) != 2 or any(type(x) is not int or x < 0 for x in history) or
            history[0] > scoring[0] or history[1] != scoring[1]):
        raise ValueError('PUBLIC_HISTORY_MUST_KEEP_CURRENT_ROLE_FUTURE_BOUNDARY')
    return tuple(history)
