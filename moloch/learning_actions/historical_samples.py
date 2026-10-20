"""Pinned historical samples with declared replay delivery, never quote depth.

The clock profile is a simulation assumption. Actual retrieval remains evidence;
it is never relabeled as a historical receive time. Venue-wide request admission
belongs to the environment's shared limiter, not this per-series lookup.
"""
import hashlib
import json
import math
import mmap
import struct
from dataclasses import dataclass
from pathlib import Path

RECORD = struct.Struct('<qdII')


def integer(value):
    return type(value) is int and value >= 0


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


@dataclass(frozen=True)
class Delivery:
    delay_ms: int
    poll_ms: int
    phase_ms: int
    maximum_age_ms: int

    def __post_init__(self):
        if (not all(integer(v) for v in (self.delay_ms, self.poll_ms,
                self.phase_ms, self.maximum_age_ms)) or self.poll_ms == 0
                or self.phase_ms >= self.poll_ms):
            raise ValueError('EXPLICIT_CAUSAL_DELIVERY_PROFILE')

    def source_frontier(self, now):
        if not integer(now):
            raise ValueError('EXACT_REPLAY_CLOCK')
        return ((now - self.phase_ms) // self.poll_ms) * self.poll_ms + self.phase_ms - self.delay_ms

    def available(self, source):
        target = source + self.delay_ms - self.phase_ms
        return -(-target // self.poll_ms) * self.poll_ms + self.phase_ms


class Samples:
    def __init__(self, directory, manifest_sha256, *, scoring_bounds,
                 information_start_ms, delivery):
        self.root = Path(directory).resolve()
        raw = (self.root / 'complete.json').read_bytes()
        if hashlib.sha256(raw).hexdigest() != manifest_sha256:
            raise ValueError('HISTORICAL_SAMPLE_MANIFEST_CHANGED')
        self.manifest = m = json.loads(raw)
        if (m.get('classification') != 'inert_source_time_price_samples_not_replay_dataset'
                or m.get('trainingAdmitted') is not False
                or m.get('originalHistoricalDeliveryVerified') is not False
                or m.get('recordBytes') != RECORD.size):
            raise ValueError('ORIGINAL_INERT_SAMPLE_INDEX_REQUIRED')
        lo, hi = scoring_bounds
        if (not all(integer(v) for v in (lo, hi, information_start_ms))
                or not information_start_ms <= lo < hi
                or not isinstance(delivery, Delivery)):
            raise ValueError('FIXED_SCORING_AND_INFORMATION_BOUNDS')
        self.bounds = (lo, hi)
        self.start = information_start_ms
        self.delivery = delivery
        receipts = self.root / 'receipts.json'
        if digest(receipts) != m['receiptsSha256']:
            raise ValueError('ORIGINAL_SAMPLE_RECEIPTS_CHANGED')
        self.receipts = json.loads(receipts.read_bytes())
        self.assets = {}
        self.opened = {}
        self.closed = False
        for asset in m['assets']:
            name = asset['file']
            path = (self.root / name).resolve()
            if (path.parent != self.root or type(asset['records']) is not int
                    or asset['records'] < 0 or asset['assetId'] in self.assets
                    or path.stat().st_size != asset['records'] * RECORD.size
                    or digest(path) != asset['sha256']):
                raise ValueError('ORIGINAL_SAMPLE_ASSET_CHANGED')
            self.assets[asset['assetId']] = (asset, path)
        if sum(a['records'] for a, _ in self.assets.values()) != m['records']:
            raise ValueError('SAMPLE_INVENTORY_COUNT')

    def _records(self, asset_id):
        if self.closed:
            raise ValueError('HISTORICAL_SAMPLE_READER_CLOSED')
        asset, path = self.assets[asset_id]
        if not asset['records']:
            return None
        if asset_id not in self.opened:
            stream = path.open('rb')
            try:
                mapped = mmap.mmap(stream.fileno(), 0, access=mmap.ACCESS_READ)
                if (len(mapped) != asset['records'] * RECORD.size
                        or hashlib.sha256(mapped).hexdigest() != asset['sha256']):
                    raise ValueError('ORIGINAL_SAMPLE_ASSET_CHANGED')
                previous = -1
                for offset in range(0, len(mapped), RECORD.size):
                    at, price, receipt, flag = RECORD.unpack_from(mapped, offset)
                    if (at <= previous or not math.isfinite(price) or not 0 <= price <= 1
                            or flag != 1 or receipt >= len(self.receipts)):
                        raise ValueError('SCOPED_SORTED_SAMPLE_REQUIRED')
                    r = self.receipts[receipt]
                    if (r['assetId'] != asset_id
                            or not r['startTs'] * 1000 <= at < r['endExclusiveTs'] * 1000
                            or not self.manifest['startTs'] * 1000 <= at < self.manifest['endExclusiveTs'] * 1000):
                        raise ValueError('ORIGINAL_SAMPLE_RECEIPT_SCOPE')
                    previous = at
            except BaseException:
                if 'mapped' in locals():
                    mapped.close()
                stream.close()
                raise
            self.opened[asset_id] = (stream, mapped)
        return self.opened[asset_id][1]

    def at(self, asset_id, now):
        if not integer(now) or not self.bounds[0] <= now < self.bounds[1]:
            raise ValueError('QUERY_OUTSIDE_SCORING_ROLE')
        data = self._records(asset_id)
        if data is None:
            return None
        frontier = self.delivery.source_frontier(now)
        lo, hi = 0, len(data) // RECORD.size
        while lo < hi:
            mid = (lo + hi) // 2
            if struct.unpack_from('<q', data, mid * RECORD.size)[0] <= frontier:
                lo = mid + 1
            else:
                hi = mid
        if not lo:
            return None
        source, price, receipt, _ = RECORD.unpack_from(data, (lo - 1) * RECORD.size)
        if source < self.start or now - source > self.delivery.maximum_age_ms:
            return None
        r = self.receipts[receipt]
        return {'kind': 'historical_price_sample', 'asset_id': asset_id,
                'source_at_ms': source, 'price': price,
                'simulated_available_at_ms': self.delivery.available(source),
                'actual_retrieved_at': r['actualReceivedAt'],
                'original_receipt_sha256': r['sha256'],
                'original_body_sha256': r['bodySha256'],
                'original_historical_delivery_verified': False,
                'executable_quote': False}

    def close(self):
        for stream, mapped in self.opened.values():
            mapped.close()
            stream.close()
        self.opened.clear()
        self.closed = True

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.close()
