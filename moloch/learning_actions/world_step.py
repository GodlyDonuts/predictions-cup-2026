"""One original economic transition, shared by local and spawned replay worlds."""


def transition(env,ledger,action,observation,latency):
    before=env.now
    env.advance(min(env.end,before+latency))
    if env.now<env.end:
        result=env.act_raw(action,observation)
        if result['status']=='ALARM':
            from .managed_data_service import advance_alarm
            advance_alarm(env,before,result)
    reward=ledger.transition();final=None
    if env.now>=env.end:
        final=ledger.finish();reward+=final['terminal_reward_correction']
    return {'reward':reward,'terminal':env.now>=env.end,
            'elapsed_ms':env.now-before,'final':final}


def details(env):
    return {'fill_count':len(env.api_fills),
            'passive_execution':None if env.passive is None else {
                'mode':env.passive.mode,'source_volume':env.passive.source_volume,
                'intercepted_volume':env.passive.intercepted_volume,'calibrated':False},
            'derived_stream':None if env.realtime is None else {
                'mints':env.realtime.mints,'book_deliveries':env.realtime.deliveries,
                'public_trades_enabled':env.realtime.public_trades,
                'public_trade_deliveries':env.realtime.public_deliveries,
                'public_trade_duplicates':env.realtime.public_duplicates,
                'public_trade_suppressed':env.realtime.public_suppressed,
                'gap_signals':env.realtime.gaps,'connected_at_end':env.realtime.enabled}}
