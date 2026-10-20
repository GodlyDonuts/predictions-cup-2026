"""Paid historical information on the existing simulated account clocks.

Providers are read-only, caller-owned audited readers. Publication clocks stay
distinct from request receipts. These payloads never create executable books.
The raw payload extension is not yet a neural tensor encoding.
"""
from copy import deepcopy
from .api_affordances import ApiAffordanceEpisode, ApiAffordanceTape

EVENT = 'historical_information_receipt'


class HistoricalInformationEpisode(ApiAffordanceEpisode):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.historical_providers = {}
        self.historical_query_providers = {}
        self.known_historical_information = {}

    def register_history(self, venue, key, provider, *, query_provider=None):
        expected = {'SIG': ('historical_candles','historical_trades'), 'Kalshi': ('historical_trades',),
                    'Polymarket': ('historical_sample',)}
        if (venue not in expected or not isinstance(key, tuple) or len(key) != 4
                or key[0] not in expected[venue] or not all(isinstance(x, str) and x for x in key)):
            raise ValueError('EXPLICIT_HISTORICAL_INFORMATION_TARGET')
        if self.feed_sequence or self.known_historical_information:
            raise ValueError('HISTORY_CATALOG_MUST_PRECEDE_FIRST_REQUEST')
        if not callable(provider):
            raise ValueError('READ_ONLY_HISTORICAL_PROVIDER_REQUIRED')
        if query_provider is not None and not callable(query_provider):
            raise ValueError('CALLABLE_HISTORY_QUERY_PROVIDER_REQUIRED')
        address = (venue, key)
        if address in self.historical_providers or key in self.keys[venue]:
            raise ValueError('DUPLICATE_HISTORY_TARGET')
        self.historical_providers[address] = provider
        if query_provider is not None:self.historical_query_providers[address]=query_provider
        self.keys[venue].append(key)

    def request_observation(self, venue, key, **kwargs):
        provider = self.historical_providers.get((venue, key))
        if provider is None:
            return super().request_observation(venue, key, **kwargs)
        now = self.now
        if venue == 'SIG':
            if self.budget.active is not None:
                return {'status': 'BUSY', 'ready_ms': self.busy_until}
            ready = self.budget.ready_at(now, False)
            latency = self.mechanics.read_latency_ms
        else:
            admissions = self.external_admissions[venue]
            while admissions and now - admissions[0] >= 1000:
                admissions.popleft()
            cap = (self.delivery.kalshi_reads_per_second if venue == 'Kalshi'
                   else self.delivery.polymarket_reads_per_second)
            ready = max(now, self.external_ready[venue], self.external_next[venue],
                        admissions[0] + 1000 if len(admissions) >= cap else now)
            latency = self.delivery.external_latency_ms
        if ready > now:
            return {'status': 'RATE_WAIT', 'ready_ms': ready}
        # Snapshot at request time, never at its later receipt time. Validation
        # errors leave the existing queue unchanged and fail the experiment.
        query_provider=self.historical_query_providers.get((venue,key))
        payload = deepcopy(provider(now)if query_provider is None else query_provider(now,**kwargs))
        self.feed_sequence += 1
        rid = f'history:{venue}:{self.feed_sequence}'
        if venue == 'SIG':
            if not self.budget.admit(rid, now, False):
                raise ValueError('HISTORICAL_READ_ADMISSION_CHANGED')
            self.busy_until = now + latency
        else:
            admissions.append(now)
            self.external_next[venue] = now + (1000 + cap - 1) // cap
            self.external_ready[venue] = self.external_next[venue]
        self.feed_requests.append({'venue': venue, 'key': key, 'at_ms': now,
                                   'request_id': rid})
        self.feed_request_counts[venue] += 1
        self.clock.schedule(now + latency, 30, EVENT,
                            (venue, key, rid, now, payload))
        return {'status': 'IN_FLIGHT'}

    def _event(self, at, kind, value):
        if kind != EVENT:
            return super()._event(at, kind, value)
        venue, key, rid, requested, payload = value
        if venue == 'SIG':
            self.budget.complete(rid, at)
        self.known_historical_information[(venue, key)] = {
            'requested_at_ms': requested, 'received_at_ms': at,
            'payload': payload, 'executable_liquidity': False}
        self.feed_receipts.append({'venue': venue, 'key': key, 'at_ms': at})
        self.feed_receipt_counts[venue] += 1
        self.observation_revision += 1

    def observe(self):
        base = super().observe()
        return {**base, 'historical_information':
                deepcopy(self.known_historical_information)}


class HistoricalInformationTape(ApiAffordanceTape):
    episode_type = HistoricalInformationEpisode


def register_candles(env, reader):
    for exchange, market in reader.exchanges.items():
        env.register_history('SIG', ('historical_candles', exchange, market, 'YES'),
                             lambda now, ex=exchange: reader.page(ex, now))


def register_sig_trades(env, reader):
    from .historical_sig_trades import SigTradePager
    for exchange, market in reader.exchanges.items():
        pager=SigTradePager(reader,exchange)
        env.register_history('SIG', ('historical_trades', exchange, market, 'YES'),
                             pager,query_provider=pager)


def register_kalshi(env, reader, mapping):
    """Mapping supplies exact ticker -> (SIG market, side), without equivalence."""
    for ticker, (market, side) in sorted(mapping.items()):
        if ticker not in reader.tickers or side not in ('YES', 'NO'):
            raise ValueError('PINNED_KALSHI_INFORMATION_MAPPING')
        env.register_history('Kalshi', ('historical_trades', ticker, market, side),
                             lambda now, t=ticker: reader.page(t, now))


def register_polymarket(env, reader, mapping):
    for asset, (market, side) in sorted(mapping.items()):
        if side not in ('YES', 'NO'):
            raise ValueError('PINNED_POLYMARKET_INFORMATION_MAPPING')
        env.register_history('Polymarket', ('historical_sample', asset, market, side),
                             lambda now, a=asset: reader.at(a, now))
