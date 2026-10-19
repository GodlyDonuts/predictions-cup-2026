"""Bounded source cursors and equivalent same-role initial market state."""
import bisect
from .disk_tape import Books


class Cursor:
    def __init__(self,env,rows,kind,priority,clock='available_at_ms',predicate=None):
        self.rows=iter(rows);self.kind=kind;self.priority=priority
        self.clock=clock;self.predicate=predicate;self.advance(env)
    def advance(self,env):
        for row in self.rows:
            if self.predicate is not None and not self.predicate(row):continue
            self.current=row
            env.clock.schedule(row[self.clock],self.priority,'api_source_cursor',self)
            return
        self.current=None
    def consume(self,env,at):
        row=self.current
        kind=self.kind(row)if callable(self.kind)else self.kind
        env._event(at,kind,row)
        self.advance(env)


def initialize_prefix(tape,env,before):
    from .snapshot_clock import world_event,information_event,source_clock
    store=tape.disk_store;bounds=tape.snapshot_world_information_bounds
    # Register in original first-discovery order, without actor prices or fills.
    for row in store.prefix(bounds,before,first=True):
        if row['venue']=='SIG':env._remember('SIG',('book',row['exchange_id']),row)
        else:env._remember(row['venue'],('reference',row['venue'],row['market_id'],row.get('source',row['venue'])),row)
    for row in store.prefix(bounds,before,world=True):world_event(env,source_clock(row),row)
    for row in store.prefix(bounds,before):
        if row['venue']=='SIG':information_event(env,row['available_at_ms'],row)
        else:env._remember(row['venue'],('reference',row['venue'],row['market_id'],row.get('source',row['venue'])),row)
    name,_,_=store.world_view(bounds)
    _,_,times=store.world_view(bounds);count=bisect.bisect_left(times,before)
    if env.world_book_lifetime_ms is not None:
        expired=bisect.bisect_right(times,before-env.world_book_lifetime_ms)
        env.world_confidence_expiries=store.db.execute(f'SELECT books FROM {name} WHERE i=?',(expired-1,)).fetchone()[0]if expired else 0
    env.initial_market_state={'mode':'same_authorized_role_prefix_v1','before_ms':before,
        'world_rows':count,'known_books':len(env.books),'actor_books':len(env.actor_books)}


def schedule(tape,env,start,end,separate_world):
    if separate_world:
        # Existing eager source families are all priority10 with world-before-
        # information-before-flow-before-counterparty insertion ties. These
        # subpriorities preserve that order before expiry15/arrival20/receipt30.
        a=bisect.bisect_left(tape.snapshot_world_times,start);b=bisect.bisect_left(tape.snapshot_world_times,end)
        # Native-derived snapshot world clocks can precede information clocks.
        class WorldCursor(Cursor):
            def advance(self,env):
                from .snapshot_clock import source_clock
                for row in self.rows:
                    self.current=row;env.clock.schedule(source_clock(row),10,'api_source_cursor',self);return
                self.current=None
        WorldCursor(env,tape.snapshot_world_rows[a:b],'api_snapshot_world',10)
    a=bisect.bisect_left(tape.times,start);b=bisect.bisect_left(tape.times,end)
    kind=lambda r:'api_snapshot_information'if separate_world and r['venue']=='SIG'and r['kind']in('book','gap')else'tape'
    Cursor(env,tape.rows[a:b],kind,11)
    a=bisect.bisect_left(tape.flow_times,start);b=bisect.bisect_left(tape.flow_times,end)
    Cursor(env,tape.flow_rows[a:b],'trade_flow',12)
    if getattr(tape,'counterparty_replay_enabled',False):
        store=tape.disk_store;a=bisect.bisect_left(store.execution_times,start);b=bisect.bisect_left(store.execution_times,end)
        lo,hi=getattr(tape,'counterparty_information_bounds',tape.split.bounds(tape.role))
        Cursor(env,store.execution_rows[a:b],'api_counterparty_trade',13,clock='executed_at_ms',
               predicate=lambda row:lo<=row['available_at_ms']<hi)
