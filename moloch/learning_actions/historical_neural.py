"""Delivered historical records as recurrent actor inputs, never fill depth.

Every row in a paid response is encoded. A receipt remains pending through
repeated observation/replay calls until the actor uses that observation. Its
information then lives in the actor's recurrent state; a paid re-query delivers
it again. This avoids repeating every archived page on every inference tick.
"""
import math
from decimal import Decimal
import numpy as np
from torch import nn
from .api_affordances import ApiAffordanceEpisode, ApiAffordancePolicy, FEATURES as OLD
from .api_affordances import CONTRACT, MARKET, SIGNATURE
from .feature_cache import identity
from .historical_delivery import HistoricalInformationEpisode, HistoricalInformationTape, EVENT
from .observation_protocol import HISTORICAL_INFORMATION

WIDTH = 48
FEATURES = OLD + WIDTH
KINDS = ('historical_candles', 'historical_trades', 'historical_sample')
VENUES = ('SIG', 'Kalshi', 'Polymarket')


def numeric(value):
    number = float(Decimal(str(value)))
    if not math.isfinite(number):
        raise ValueError('FINITE_HISTORICAL_OBSERVATION')
    return number


def signed_log(value):
    number = numeric(value)
    return math.copysign(math.log1p(abs(number)), number)


def records(key, payload):
    if key[0] == 'historical_candles':
        return payload['candles']
    if key[0] == 'historical_trades':
        return payload
    return [] if payload is None else [payload]


def encode(venue, key, receipt, row, ordinal, now):
    out = np.zeros(FEATURES, np.float32)
    h = out[OLD:]
    h[KINDS.index(key[0])] = 1
    h[3 + VENUES.index(venue)] = 1
    h[6:8] = [key[3] == 'YES', key[3] == 'NO']
    h[8] = signed_log((now - receipt['received_at_ms']) / 1000)
    h[11] = math.log1p(ordinal)
    h[32:39] = identity('historical_source', (venue, key[1]))
    out[MARKET] = identity('market', key[2])
    out[SIGNATURE] = identity('read', (venue, key))
    if venue == 'SIG':
        out[CONTRACT] = identity('contract', key[1])
    if row is None:  # A delivered empty response is information, not a zero price.
        h[47] = 1
        return out
    source, available = row['source_at_ms'], row['simulated_available_at_ms']
    if (type(source) is not int or type(available) is not int
            or source > available or available > receipt['requested_at_ms']
            or not receipt['requested_at_ms'] <= receipt['received_at_ms'] <= now):
        raise ValueError('HISTORICAL_ROW_MUST_PRECEDE_REQUEST')
    h[9] = signed_log((now - source) / 1000)
    h[10] = signed_log((now - available) / 1000)
    prices = [None] * 7
    volume = count = None
    event = source
    if key[0] == 'historical_candles':
        if row['bar_end_ms'] > available:
            raise ValueError('CANDLE_CLOSE_MUST_PRECEDE_PUBLICATION')
        prices[:5] = [row.get(k) for k in ('open', 'high', 'low', 'close', 'vwap')]
        volume, count = row['volume'], row['tradeCount']
    elif key[0] == 'historical_trades':
        trade = row['trade']
        prices[5:] = [trade.get('yes_price_dollars'), trade.get('no_price_dollars')]
        volume = trade.get('count_fp', trade.get('count'))
        event = trade['trade_id']
        h[39:43] = [trade.get('taker_side') == 'yes', trade.get('taker_side') == 'no',
                    trade.get('taker_book_side') == 'bid', trade.get('taker_book_side') == 'ask']
        h[43:46] = [trade.get('taker_outcome_side') == 'yes',
                    trade.get('taker_outcome_side') == 'no', trade.get('is_block_trade') is True]
    else:
        # Sample outcome price is distinct from YES/NO bid/ask.
        prices[3] = row['price']
    for i, price in enumerate(prices):
        if price is not None:
            value = numeric(price)
            if not 0 <= value <= 1:
                raise ValueError('HISTORICAL_PRICE_OUTSIDE_UNIT_INTERVAL')
            h[12 + i], h[21 + i] = value, 1
    if volume is not None:
        h[19], h[28] = signed_log(volume), 1
    if count is not None:
        h[20], h[29] = signed_log(count), 1
    h[30] = 1
    h[31] = 1  # Historical observation; never executable quote depth.
    h[46] = math.log1p(ordinal + 1)
    out[33:40] = identity('historical_record', (venue, key[1], event))
    if not np.isfinite(out).all():
        raise ValueError('FINITE_HISTORICAL_FEATURES_REQUIRED')
    return out


class HistoricalNeuralEpisode(HistoricalInformationEpisode):
    observation_protocol = HISTORICAL_INFORMATION

    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.history_receipt_sequence = 0
        self.history_consumed_sequence = 0
        self.pending_history_receipts = []

    def _event(self, at, kind, value):
        super()._event(at, kind, value)
        if kind == EVENT:
            venue, key, *_ = value
            self.history_receipt_sequence += 1
            self.pending_history_receipts.append((self.history_receipt_sequence, venue, key,
                self.known_historical_information[(venue, key)]))

    def observe(self):
        # Do not copy all previously delivered raw archives into each frame.
        base = ApiAffordanceEpisode.observe(self)
        extra = []
        entities = list(base['entities'])
        for sequence, venue, key, receipt in self.pending_history_receipts:
            rows = records(key, receipt['payload'])
            for ordinal, row in enumerate(rows or [None]):
                extra.append(encode(venue, key, receipt, row, ordinal, self.now))
                entities.append(('historical_record', (sequence, ordinal)))
        count = len(extra)
        values = np.zeros((len(entities), FEATURES), np.float32)
        values[:len(base['entities']), :OLD] = base['contracts']
        for i, (kind, address) in enumerate(base['entities']):
            if kind == 'read' and address in self.historical_providers:
                venue, key = address
                h = values[i, OLD:]
                h[KINDS.index(key[0])] = 1
                h[3 + VENUES.index(venue)] = 1
                h[6:8] = [key[3] == 'YES', key[3] == 'NO']
                h[32:39] = identity('historical_source', (venue, key[1]))
                values[i, MARKET] = identity('market', key[2])
                if venue == 'SIG':
                    values[i, CONTRACT] = identity('contract', key[1])
        if count:
            values[-count:] = extra
        masks = {k: np.concatenate((v, np.zeros((count, *v.shape[1:]), dtype=v.dtype)))
                 for k, v in base['masks'].items()}
        return {**base, 'contracts': values, 'entities': entities, 'masks': masks,
                'present': np.ones(len(entities), bool),
                'history_receipt_sequence': self.history_receipt_sequence,
                'observation_feature_schema': 'historical_receipt_records_517_v1'}

    def act_raw(self, action, observation):
        sequence = observation['history_receipt_sequence']
        if (type(sequence) is not int or not self.history_consumed_sequence <= sequence
                <= self.history_receipt_sequence):
            raise ValueError('EXACT_OBSERVED_HISTORY_RECEIPT_SEQUENCE')
        result = super().act_raw(action, observation)
        # Receipts arriving during inference are newer than the observation;
        # preserve them for the next decision, even if they share a target.
        self.pending_history_receipts = [r for r in self.pending_history_receipts if r[0] > sequence]
        self.history_consumed_sequence = sequence
        return result


class HistoricalNeuralTape(HistoricalInformationTape):
    episode_type = HistoricalNeuralEpisode
    observation_protocol = HISTORICAL_INFORMATION


class HistoricalNeuralPolicy(ApiAffordancePolicy):
    architecture = 'tartarus-historical-information-gru-v10'
    observation_protocol = HISTORICAL_INFORMATION

    def __init__(self, hidden_size=4096):
        super().__init__(hidden_size)
        self.contract_features = FEATURES
        self.encoder[0] = nn.Linear(FEATURES, 128)
