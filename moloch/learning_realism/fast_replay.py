"""Decode once; share only original immutable books, never wallet/liquidity state."""
from types import MappingProxyType
from moloch.learning_flow.episodes import FlowTape
from moloch.learning_v2.liquidity import Book
from moloch.learning_v2.reference_env import ReferenceEnv
from .protocol import Delivery
from .replay import PollingEpisode, ReplayTape


class PreparedEpisode(PollingEpisode):
    def _event(self, at, kind, value):
        if kind == 'tape' and value['kind'] == 'book':
            market, ex = value['market_id'], value['exchange_id']
            self.scope.setdefault(market, set()).add(ex); self.mapping[ex] = market
            from moloch.learning_actions.disk_tape import record_key
            ReferenceEnv._event(self, at, 'book', self.prepared_books[record_key(value)])
            self._refresh_reference(market)
            self._remember('SIG', ('book', ex), value)
            return
        super()._event(at, kind, value)

class PreparedTape(ReplayTape):
    episode_type = PreparedEpisode

    def __init__(self,directory,*,split,role,delivery=None,frozen_finalist_sha256=None):
        original=FlowTape(directory)
        self.__dict__.update(self.from_loaded(original,split=split,role=role,delivery=delivery,
            frozen_finalist_sha256=frozen_finalist_sha256).__dict__)

    @classmethod
    def from_loaded(cls, original, *, split, role, delivery=None, frozen_finalist_sha256=None):
        if not isinstance(original, FlowTape): raise TypeError('VERIFIED_FLOW_TAPE_REQUIRED')
        split.bounds(role)
        tape = cls.__new__(cls)
        # Original rows/levels are treated as read-only, as in the existing Tape.
        # Every episode still allocates fresh world/actor caches and accounting.
        for name in ('rows', 'times', 'exchanges', 'index', 'flow_rows', 'flow_times', 'manifest', 'manifest_sha256'):
            setattr(tape, name, getattr(original, name))
        tape.split, tape.role = split, role
        tape.delivery = delivery or Delivery(); tape.finalist = frozen_finalist_sha256
        if hasattr(original,'disk_store'):
            from moloch.learning_actions.disk_tape import Books
            tape.disk_store=original.disk_store;tape.prepared_books=Books(tape.disk_store)
            return tape
        tape.prepared_books = MappingProxyType({id(row): Book(row['exchange_id'], row['available_at_ms'],
            row.get('source_at_ms'), tuple(map(tuple, row['bids'])), tuple(map(tuple, row['asks'])),
            row['original_sha256']) for row in original.rows if row['kind'] == 'book'})
        return tape

    def episode(self, *args, advance_to_start=True, **kwargs):
        env = super().episode(*args, advance_to_start=False, **kwargs)
        env.prepared_books = self.prepared_books
        if advance_to_start: env.advance(env.start)
        return env


def open_prepared_replay(specification,dataset,mechanics_path,role,*,frozen_finalist_sha256=None,original=None):
    from .replay import open_replay
    tape,mechanics=open_replay(specification,dataset,mechanics_path,role,
                              frozen_finalist_sha256=frozen_finalist_sha256,original=original)
    return PreparedTape.from_loaded(tape,split=tape.split,role=tape.role,delivery=tape.delivery,
                                   frozen_finalist_sha256=tape.finalist),mechanics
