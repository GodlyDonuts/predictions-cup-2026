"""Join existing public quote exports to pinned, independently checked identities.

No account client, relay mutation, venue request, or source-clock renewal.
Metadata expires on its original clock; unavailable supplements do not suppress
the ordinary capture's unrelated scopes.
"""
import base64
import gzip
import hashlib
import importlib.util
import json
from pathlib import Path
import urllib.request


def pinned(path, expected):
    raw = Path(path).read_bytes()
    if len(raw) > 4_000_000 or hashlib.sha256(raw).hexdigest() != expected:
        raise ValueError('SUPPLEMENT_PIN_CHANGED')
    return raw


def append(config, capture, now, cache=None):
    settings = config['supplementalReference']
    seed = json.loads(pinned(settings['seedPath'], settings['seedSha256']))
    source = pinned(settings['partyReferencePath'], settings['partyReferenceSha256'])
    spec = importlib.util.spec_from_file_location('pinned_party_reference', settings['partyReferencePath'])
    reference = importlib.util.module_from_spec(spec)
    # Execute the bytes whose digest was checked, rather than reopening a path.
    exec(compile(source, settings['partyReferencePath'], 'exec'), reference.__dict__)
    wanted = set(settings['exchangeIds'])
    if (seed['version'] != 1 or len(wanted) != len(settings['exchangeIds'])
            or {x['registry']['exchangeId'] for x in seed['rows']} != wanted
            or len(seed['rows']) != len(wanted)
            or wanted & {x['exchangeId'] for x in capture['rows']}):
        raise ValueError('SUPPLEMENT_EXACT_SCOPE')
    result = dict(capture, rows=list(capture['rows']), supplementalUnavailable=[])
    if cache is None:
        try:
            with urllib.request.urlopen(settings['rankCacheUrl'], timeout=20) as response:
                raw = response.read(8_000_001)
                if len(raw) > 8_000_000:
                    raise ValueError('PUBLIC_RANK_CACHE_BOUND')
                if response.headers.get('Content-Encoding') == 'gzip':
                    raw = gzip.decompress(raw)
                if len(raw) > 32_000_000:
                    raise ValueError('PUBLIC_RANK_CACHE_DECOMPRESSED_BOUND')
            cache = json.loads(raw)
        except Exception as error:
            result['supplementalUnavailable'] = [dict(exchangeId=x, reason=type(error).__name__)
                                                  for x in sorted(wanted)]
            return result
    for row in seed['rows']:
        registry = row['registry']
        ex, ticker = registry['exchangeId'], row['mappingClaim']['ticker']
        try:
            matches = [q for q in cache['markets'] if q.get('ticker') == ticker]
            if len(matches) != 1:
                raise ValueError('SUPPLEMENT_QUOTE_MISSING_OR_DUPLICATE')
            qraw = json.dumps(matches[0], separators=(',', ':'), ensure_ascii=False).encode()
            metadata = base64.b64decode(row['metadataRecordBase64'], validate=True)
            original = base64.b64decode(row['officialRecordBase64'], validate=True)
            observed = reference.observe(registry, {registry['semanticSourceSha256']: original},
                [row['mappingClaim']], {ticker: qraw}, {ticker: metadata}, decision_ms=now,
                maximum_receipt_age_ms=30_000, maximum_metadata_age_ms=86_400_000)
            bid, ask = reference.Decimal(observed['yesBid']), reference.Decimal(observed['yesAsk'])
            if not 0 < bid <= ask < 1:
                raise ValueError('SUPPLEMENT_TWO_SIDED_REFERENCE')
            result['rows'].append(dict(exchangeId=ex, marketId=registry['marketId'],
                contractId=registry['contractId'], officialContractRootSha256=registry['officialContractRootSha256'],
                kalshiTicker=ticker, yesBid=str(bid), yesAsk=str(ask), yesMidpoint=str((bid+ask)/2),
                quoteObservedAtMs=reference.clock(matches[0]['receivedAt']), mapping=observed,
                quoteRecordBase64=base64.b64encode(qraw).decode(),
                metadataRecordBase64=row['metadataRecordBase64'], polymarketContext=[],
                provenance='existing_public_rank_cache_and_pinned_original_identity',
                sourceClocksRenewed=False))
        except (ValueError, KeyError, TypeError) as error:
            result['supplementalUnavailable'].append(dict(exchangeId=ex, reason=str(error)[:200]))
    return result
