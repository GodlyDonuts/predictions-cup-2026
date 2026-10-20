"""Role-bounded, source-pinned history readers owned by one offline experiment."""
from .historical_samples import Samples, Delivery
from .historical_candles import CandleHistory
from .historical_kalshi import KalshiHistory
from .historical_delivery import register_candles, register_kalshi, register_polymarket
from moloch.learning_realism.protocol import valid_sha


def validate(config):
    if not isinstance(config, dict) or config.get('format') != 'three_venue_history_information_v1':
        raise ValueError('EXPLICIT_THREE_VENUE_HISTORY_BINDING')
    if type(config.get('historical_mapping_availability_verified')) is not bool:
        raise ValueError('EXPLICIT_HISTORICAL_MAPPING_EVIDENCE_STATUS')
    roles = config.get('roles')
    if not isinstance(roles, dict) or not {'train', 'evolution'} <= roles.keys():
        raise ValueError('TRAIN_AND_EVOLUTION_HISTORY_BINDINGS_REQUIRED')
    for role, sources in roles.items():
        if role not in ('train', 'evolution', 'test') or set(sources) != {'SIG', 'Kalshi', 'Polymarket'}:
            raise ValueError('THREE_VENUES_PER_REGISTERED_ROLE')
        for venue, spec in sources.items():
            if not isinstance(spec.get('path'), str) or not valid_sha(spec.get('manifest_sha256')):
                raise ValueError('PINNED_HISTORY_READER_SOURCE')
            if type(spec.get('information_start_ms')) is not int or spec['information_start_ms'] < 0:
                raise ValueError('EXPLICIT_PUBLIC_HISTORY_LOWER_BOUND')
            Delivery(**spec['delivery'])
            if venue != 'SIG':
                mapping = spec.get('mapping')
                if not isinstance(mapping, dict) or not mapping:
                    raise ValueError('EXPLICIT_REFERENCE_INFORMATION_MAPPING')
                for asset, pair in mapping.items():
                    if (not isinstance(asset, str) or not asset or not isinstance(pair, list)
                            or len(pair) != 2 or not isinstance(pair[0], str) or not pair[0]
                            or pair[1] not in ('YES', 'NO')):
                        raise ValueError('REFERENCE_MARKET_AND_SIDE_MAPPING')


class HistoryBindings:
    def __init__(self, config):
        validate(config)
        self.config = config
        self.readers = {}

    def attach(self, env, role, bounds):
        if role not in self.config['roles']:
            raise ValueError('NO_HISTORY_SOURCE_FOR_THIS_ROLE')
        if not bounds[0] <= env.now < env.end <= bounds[1]:
            raise ValueError('HISTORY_EPISODE_OUTSIDE_REGISTERED_ROLE')
        key = (role, tuple(bounds))
        if key not in self.readers:
            opened = {}
            try:
                for venue, cls in (('SIG', CandleHistory), ('Kalshi', KalshiHistory), ('Polymarket', Samples)):
                    spec = self.config['roles'][role][venue]
                    opened[venue] = cls(spec['path'], spec['manifest_sha256'],
                        scoring_bounds=bounds, information_start_ms=spec['information_start_ms'],
                        delivery=Delivery(**spec['delivery']))
            except BaseException:
                for reader in opened.values():
                    reader.close()
                raise
            self.readers[key] = opened
        readers = self.readers[key]
        sources = self.config['roles'][role]
        register_candles(env, readers['SIG'])
        register_kalshi(env, readers['Kalshi'], sources['Kalshi']['mapping'])
        register_polymarket(env, readers['Polymarket'], sources['Polymarket']['mapping'])

    def close(self):
        for group in self.readers.values():
            for reader in group.values():
                reader.close()
        self.readers.clear()
