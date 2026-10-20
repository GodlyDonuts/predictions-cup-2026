"""Fixed development screens, distinct from full-window confirmation/test."""


def calendars(start,end,blocks,*,development,episode_ms=None,confirmation=False):
    if type(blocks)is not int or blocks<1 or blocks>end-start:
        raise ValueError('VALID_DEVELOPMENT_SELECTION_BLOCKS')
    if not development and blocks!=1:
        raise ValueError('FULL_DAILY_GROWTH_EVALUATION_MUST_REMAIN_CONTINUOUS')
    if episode_ms is not None:
        if not development or type(episode_ms)is not int or episode_ms<=0:
            raise ValueError('SHORT_SCREENS_REQUIRE_POSITIVE_EXPLICIT_DEVELOPMENT_HORIZON')
        if episode_ms>(end-start)//blocks:
            raise ValueError('SCREEN_CANNOT_EXCEED_ITS_CHRONOLOGICAL_STRATUM')
    out=[]
    for i in range(blocks):
        a=start+(end-start)*i//blocks;b=start+(end-start)*(i+1)//blocks
        if episode_ms is not None and not confirmation:
            a+=(b-a-episode_ms)//2;b=a+episode_ms
        out.append((a,b))
    return out
