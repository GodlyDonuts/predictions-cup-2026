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
        self.historical_latest_providers = {}
        self.owned_history_payloads = set()
        self.known_historical_information = {}

    def register_history(self, venue, key, provider, *, query_provider=None, latest_provider=None,
                         payload_is_owned=False):
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
        if latest_provider is not None and (venue=='SIG' or not callable(latest_provider)):
            raise ValueError('CALLABLE_EXTERNAL_LATEST_HISTORY_PROVIDER_REQUIRED')
        if type(payload_is_owned) is not bool:
            raise ValueError('EXPLICIT_HISTORY_PAYLOAD_OWNERSHIP')
        address = (venue, key)
        if address in self.historical_providers or key in self.keys[venue]:
            raise ValueError('DUPLICATE_HISTORY_TARGET')
        self.historical_providers[address] = provider
        if query_provider is not None:self.historical_query_providers[address]=query_provider
        if latest_provider is not None:self.historical_latest_providers[address]=latest_provider
        if payload_is_owned:self.owned_history_payloads.add(address)
        self.keys[venue].append(key)

    def request_observation(self, venue, key, **kwargs):
        provider = self.historical_providers.get((venue, key))
        latest=kwargs.pop('history_latest_only',False)
        if type(latest)is not bool:raise ValueError('EXPLICIT_LATEST_HISTORY_QUERY')
        if latest:
            service=getattr(self,'managed_data_service',None)
            from .managed_data_service import history_targets
            if (service is None or not service.in_background or not history_targets(self.managed_data_profile)
                    or (venue,key)not in self.historical_latest_providers):
                raise ValueError('REGISTERED_BACKGROUND_HISTORY_QUERY_REQUIRED')
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
        payload = (self.historical_latest_providers[(venue,key)](now)if latest
                   else provider(now)if query_provider is None else query_provider(now,**kwargs))
        # Audited external readers return a new value-owned response on every
        # call. Transfer that response to its delayed receipt; generic providers
        # retain the original defensive snapshot because they may reuse data.
        if (venue,key) not in self.owned_history_payloads:payload=deepcopy(payload)
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
        if latest:self.feed_requests[-1]['history_query']='latest_record'
        self.feed_request_counts[venue] += 1
        event = (venue, key, rid, now, payload)
        # Mark the request while its owner is known. The service's background
        # flag has already reset when this delayed receipt arrives. Old V3 and
        # all explicit actor queries retain their original event representation.
        from .managed_data_service import SNAPSHOT_PROFILE
        if latest and self.managed_data_profile == SNAPSHOT_PROFILE:
            event += (True,)
        self.clock.schedule(now + latency, 30, EVENT, event)
        return {'status': 'IN_FLIGHT'}

    def _event(self, at, kind, value):
        if kind != EVENT:
            return super()._event(at, kind, value)
        venue, key, rid, requested, payload = value[:5]
        if len(value) not in (5, 6) or len(value) == 6 and value[5] is not True:
            raise ValueError('EXACT_MANAGED_HISTORY_SNAPSHOT_RECEIPT')
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
                             lambda now, t=ticker: reader.page(t, now),
                             latest_provider=lambda now, t=ticker: reader.page(t, now,limit=1),
                             payload_is_owned=owns_external_payload(reader,'Kalshi'))


def register_polymarket(env, reader, mapping):
    for asset, (market, side) in sorted(mapping.items()):
        if side not in ('YES', 'NO'):
            raise ValueError('PINNED_POLYMARKET_INFORMATION_MAPPING')
        env.register_history('Polymarket', ('historical_sample', asset, market, side),
                             lambda now, a=asset: reader.at(a, now),
                             latest_provider=lambda now, a=asset: reader.at(a, now),
                             payload_is_owned=owns_external_payload(reader,'Polymarket'))


def owns_external_payload(reader,venue):
    """Only exact built-in readers (and unions entirely made of them) transfer.

    KalshiHistory parses fresh JSON and Samples constructs a fresh flat value
    object. Their union readers retain none of these responses. Subclasses and
    arbitrary test/plugin readers continue using defensive snapshots.
    """
    from .historical_kalshi import KalshiHistory
    from .historical_samples import Samples
    from .historical_union import KalshiUnion,SampleUnion
    direct,union={'Kalshi':(KalshiHistory,KalshiUnion),'Polymarket':(Samples,SampleUnion)}[venue]
    return type(reader)is direct or (type(reader)is union and bool(reader.readers)
        and all(owns_external_payload(child,venue)for child in reader.readers))
