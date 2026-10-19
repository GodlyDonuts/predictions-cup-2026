"""Actor-controlled reporting filters; source/role clocks remain authoritative."""
import math

MAX_TIMESTAMP_MS=253402300799999  # Last millisecond of year9999 UTC.


def timestamp(fraction):
    if type(fraction)not in(int,float)or not math.isfinite(fraction)or not 0<=fraction<=1:
        raise ValueError('QUERY_TIME_LATENT_RANGE')
    return min(MAX_TIMESTAMP_MS,int(fraction*(MAX_TIMESTAMP_MS+1)))


def parameters(action):
    if action['query_filter']not in(0,1):raise ValueError('BINARY_QUERY_FILTER_CHOICE')
    # Continuations use the original opaque cursor's bounds and source watermark.
    if int(action['side'])or not int(action['query_filter']):return {}
    a=timestamp(float(action['query_from_fraction']));b=timestamp(float(action['query_to_fraction']))
    return {'trade_from_ms':min(a,b),'trade_to_ms':max(a,b)}
