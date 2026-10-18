"""Decode a sealed historical observation trace once, with the fixed verifier.

Taps all accepted source observations, not old strategy triggers or labels.
The older inspected V5 capture is DEVELOPMENT ONLY. Its input verification
does not establish calibrated fills, settlement equivalence or a locked test.
"""
import argparse
import gzip
import hashlib
import importlib.util
import json
from collections import Counter
from decimal import Decimal
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
VERIFIER = ROOT/'research/moloch-trade-trigger-20261016/witness/trace_replay.py'
VERIFIER_SHA = '3b7895720a06ee52104c9a23cbaf06fd0987e90405fac71673afaa0bdd602bc1'
V5_MANIFEST_SHA = '1db5b9472ac54a041f75120659084f8e23bc070ee24d979bcffc09d7130399cc'


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def money(raw):
    n = Decimal(str(raw))*1_000_000
    if not n.is_finite() or n != n.to_integral_value():
        raise ValueError('INEXACT_SOURCE_PRICE')
    return int(n)


def millis(raw):
    from datetime import datetime
    return int(datetime.fromisoformat(raw.replace('Z','+00:00')).timestamp()*1000)


def remap(originals: Path, out: Path):
    raw = (originals/'manifest.json').read_bytes()
    if digest(raw) != V5_MANIFEST_SHA:
        raise ValueError('ORIGINAL_V5_MANIFEST_PIN')
    manifest = json.loads(raw)
    (out/'original-manifest.json').write_bytes(raw)
    # Materialize exact pinned prefixes; never alter the downloaded originals.
    for ref in [manifest[k] for k in ['mapping','producer','samples','seal','trace']] + manifest['sigSources']:
        source = originals/Path(ref['path']).name
        destination = out/('source-'+ref['sha256']+''.join(source.suffixes))
        if source.stat().st_size < ref['bytes']:
            raise ValueError('SOURCE_TRANSFER_INCOMPLETE:'+source.name)
        h = hashlib.sha256()
        with source.open('rb') as src, destination.open('xb') as dst:
            left = ref['bytes']
            while left:
                chunk = src.read(min(left, 1024**2))
                if not chunk: raise ValueError('TRUNCATED_PINNED_PREFIX')
                h.update(chunk); dst.write(chunk); left -= len(chunk)
        if h.hexdigest() != ref['sha256']:
            raise ValueError('SOURCE_PREFIX_HASH:'+source.name)
        ref['path'] = str(destination.resolve())
    mapped = out/'relocated-manifest.json'
    mapped.write_text(json.dumps(manifest,indent=2)+'\n',encoding='utf-8')
    return mapped


def compile_capture(originals: Path, output: Path):
    output.mkdir(parents=True, exist_ok=False)
    manifest = remap(originals, output)
    raw = VERIFIER.read_bytes().replace(b'\r\n',b'\n')
    if digest(raw) != VERIFIER_SHA:
        raise ValueError('FIXED_VERIFIER_SOURCE_CHANGED')
    spec = importlib.util.spec_from_file_location('_learning_source_verifier', VERIFIER)
    verifier = importlib.util.module_from_spec(spec); spec.loader.exec_module(verifier)
    original_sampler = verifier.known_sampler
    counts = Counter(); externals = {}; last_time = None
    events_path = output/'observations.jsonl.gz'
    with events_path.open('xb') as outer, gzip.GzipFile(fileobj=outer, mode='wb', mtime=0, compresslevel=1) as events:
        def emit(value):
            nonlocal last_time
            at = value['available_at_ms']
            if last_time is not None and at < last_time:
                raise ValueError('EXTRACTED_INFORMATION_CLOCK_REVERSED')
            last_time = at; counts[value['kind']] += 1
            events.write((json.dumps(value,separators=(',',':'),allow_nan=False)+'\n').encode())

        def tapped_sampler(producer_sha):
            module = original_sampler(producer_sha)
            Original = module.Study
            class Tapped(Original):
                def accept(self, row, now):
                    before = self.counts['sigBooks']
                    super().accept(row, now)
                    event = row['event']
                    if event.get('type') == 'gap':
                        emit(dict(kind='gap', venue='SIG', available_at_ms=now,
                                  market_id=event.get('marketId'), exchange_id=event.get('exchangeId')))
                    elif event.get('type') == 'book' and self.counts['sigBooks'] > before:
                        b = event['book']
                        def levels(name):
                            result = []
                            for x in b[name]:
                                q = Decimal(str(x['quantity']))
                                if not q.is_finite() or q != q.to_integral_value() or q <= 0:
                                    raise ValueError('NONINTEGER_SIG_DEPTH')
                                result.append([money(x['price']),int(q)])
                            return result
                        emit(dict(kind='book', venue='SIG', available_at_ms=now,
                                  received_at_ms=millis(b['receivedAt']), source_at_ms=millis(b['sourceAt']),
                                  source_clock_semantics='original_asOf_at', publisher_at_ms=None,
                                  market_id=str(b['marketId']), exchange_id=str(b['exchangeId']),
                                  sequence=b['sequence'], original_sha256=row['hash'],
                                  bids=levels('bids'), asks=levels('asks')))

                def external_update(self, envelope, now):
                    super().external_update(envelope, now)
                    source = envelope.get('source','kalshi_official_v2')
                    for market, item in self.external_cache[source].items():
                        key = (source,market)
                        if (source,market) in self.external_invalid:
                            if externals.get(key) is not None:
                                externals[key] = None
                                emit(dict(kind='reference_gap',venue='Kalshi',available_at_ms=now,market_id=market,source=source))
                            continue
                        identity = (item['ticker'],item['receivedMs'],item['bodySha256'])
                        if item['availableMs'] != now or externals.get(key) == identity: continue
                        externals[key] = identity
                        emit(dict(kind='reference',venue='Kalshi',available_at_ms=now,
                                  market_id=market,source=source,ticker=item['ticker'],
                                  received_at_ms=item['receivedMs'],publisher_at_ms=None,
                                  source_at_ms=item.get('sourceTsMs'),original_sha256=item['bodySha256'],
                                  mapping_kind=item['mappingKind'],settlement_equivalence_verified=False,
                                  price_yes_micros=money(item['features']['midpoint'])))
            module.Study = Tapped
            return module
        verifier.known_sampler = tapped_sampler
        OriginalSources = verifier.Sources
        class TappedSources(OriginalSources):
            def rotate(self, event):
                super().rotate(event)
                # The original verifier expressly cannot certify a processing
                # epoch across file segments. Preserve that as a data barrier;
                # matching sequence numbers alone do not repair this absence.
                emit(dict(kind='gap',venue='SIG',available_at_ms=event['observedAtMs'],
                          market_id=None,exchange_id=None,reason='unverified_source_segment_transition'))
        verifier.Sources = TappedSources
        verification = verifier.verify(manifest, digest(manifest.read_bytes()))
    report = {'schema_version':1,'originalManifestSha256':V5_MANIFEST_SHA,
              'verifierLfSha256':VERIFIER_SHA,'compilerSha256':digest(Path(__file__).read_bytes()),
              'sourceVerification':verification,'observationCounts':dict(counts),
              'observationsSha256':digest(events_path.read_bytes()),'venuesPresent':['SIG','Kalshi'],
              'polymarketPresent':False,'classification':'inspected_history_development_only',
              'lockedTestEligible':False,'executionCalibrationAvailable':False,
              'sourcePublisherTimesInferred':False,'strategyTriggersUsedAsObservations':False,
              'productionChanged':False,'executionAuthority':False,'trainedCheckpoint':None}
    (output/'dataset-manifest.json').write_text(json.dumps(report,indent=2)+'\n',encoding='utf-8')
    (output/'COMPLETE.json').write_text(json.dumps({'manifestSha256':digest((output/'dataset-manifest.json').read_bytes())})+'\n')
    return report


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--originals',required=True,type=Path)
    parser.add_argument('--output',required=True,type=Path)
    args = parser.parse_args()
    print(json.dumps(compile_capture(args.originals,args.output)),flush=True)
