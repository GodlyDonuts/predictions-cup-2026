"""Capture existing public feeds for an execution-owner review; no API calls.

This consumes live data for production valuation, never ML fit or selection.
Venue price agreement is not proof of equivalent settlement or calibrated odds.
"""
import argparse
import base64
import hashlib
import json
import time
from datetime import datetime, timezone
from decimal import Decimal
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from moloch.tartarus import party_reference as reference


def capture(registry_root, kalshi_root, polymarket_universe, polymarket_cache, exchanges, now):
    census = json.loads((registry_root / 'full-universe-expanded/census.private.json').read_bytes())
    rows = [r for r in census['rows'] if r['exchangeId'] in exchanges]
    if {r['exchangeId'] for r in rows} != set(exchanges):
        raise ValueError('EXACT_REQUESTED_REGISTRY_COVERAGE')
    registry = [{k: r[k] for k in ('exchangeId', 'marketId', 'contractId', 'semanticSourceSha256', 'officialContractRootSha256', 'originalFile')} for r in rows]
    originals = {r['semanticSourceSha256']: (registry_root / 'roots' / r['originalFile']).read_bytes() for r in rows}
    quotes = {r['kalshiTicker']: (kalshi_root / 'latest' / (r['kalshiTicker'] + '.json')).read_bytes() for r in rows}
    metadata = {r['kalshiTicker']: (kalshi_root / 'metadata' / (r['kalshiTicker'] + '.json')).read_bytes() for r in rows}
    universe = json.loads((kalshi_root / 'universe.json').read_bytes())
    checked = reference.census(registry, originals, universe['markets'], quotes, metadata,
        decision_ms=now, maximum_receipt_age_ms=30_000, maximum_metadata_age_ms=86_400_000)
    if checked['unavailable'] or {r['exchangeId'] for r in checked['rows']} != set(exchanges):
        raise ValueError('CURRENT_REFERENCE_COVERAGE:' + json.dumps(checked['unavailable']))
    pm = json.loads(polymarket_universe.read_bytes())['outcomes']
    result = []
    for r in checked['rows']:
        ticker = r['kalshiTicker']
        quote = json.loads(quotes[ticker])
        meta = json.loads(metadata[ticker])
        for receipt in (quote, meta):
            if (receipt['status'] != 200
                    or hashlib.sha256(receipt['bodyText'].encode()).hexdigest() != receipt['bodySha256']
                    or json.loads(receipt['bodyText']) != receipt['body']):
                raise ValueError('ORIGINAL_PUBLIC_BODY_HASH')
        lo, hi = Decimal(r['yesBid']), Decimal(r['yesAsk'])
        if not Decimal(0) < lo <= hi < Decimal(1):
            raise ValueError('TWO_SIDED_REFERENCE_REQUIRED')
        corroboration = []
        for outcome in pm:
            refs = [x for x in outcome['sigReferences'] if x['sigMarketId'] == r['marketId'] and x['relationship'] == 'same_party_conditional' and x['sigSide'] == 'YES']
            if not refs:
                continue
            path = polymarket_cache / 'latest' / (outcome['assetId'] + '.json')
            if not path.exists():
                continue
            raw = path.read_bytes()
            q = json.loads(raw)
            observed = reference.clock(q['receipt']['receivedAt'])
            if not 0 <= now - observed <= 30_000:
                continue
            book = q['book']
            if not book.get('bestBid') or not book.get('bestAsk'):
                continue
            corroboration.append({'role': 'conditional_context_only_not_model_input',
                'assetId': outcome['assetId'], 'observedAtMs': observed,
                'bestBid': book['bestBid'], 'bestAsk': book['bestAsk'],
                'recordSha256': hashlib.sha256(raw).hexdigest(), 'recordBase64': base64.b64encode(raw).decode()})
        result.append({'exchangeId': r['exchangeId'], 'marketId': r['marketId'], 'contractId': r['contractId'],
            'officialContractRootSha256': r['officialContractRootSha256'], 'kalshiTicker': ticker,
            'yesBid': str(lo), 'yesAsk': str(hi), 'yesMidpoint': str((lo + hi) / 2),
            'quoteObservedAtMs': reference.clock(quote['receivedAt']),
            'mapping': r, 'quoteRecordBase64': base64.b64encode(quotes[ticker]).decode(),
            'metadataRecordBase64': base64.b64encode(metadata[ticker]).decode(), 'polymarketContext': corroboration})
    return {'version': 1, 'capturedAtMs': now, 'capturedAt': datetime.fromtimestamp(now / 1000, timezone.utc).isoformat(),
        'model': 'conditional_Kalshi_party_midpoint_v1', 'calibratedProbabilityClaimed': False,
        'settlementEquivalenceClaimed': False, 'officialSigRequests': 0,
        'productionReviewInputOnly': True, 'mlFitOrSelectionUse': False, 'rows': result}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    for name in ('registry-root', 'kalshi-root', 'polymarket-universe', 'polymarket-cache', 'output'):
        p.add_argument('--' + name, type=Path, required=True)
    p.add_argument('--exchanges', required=True)
    a = p.parse_args()
    if a.output.exists():
        raise ValueError('IMMUTABLE_CAPTURE_EXISTS')
    result = capture(a.registry_root, a.kalshi_root, a.polymarket_universe, a.polymarket_cache, a.exchanges.split(','), time.time_ns() // 1_000_000)
    a.output.parent.mkdir(parents=True, exist_ok=True)
    a.output.write_text(json.dumps(result, separators=(',', ':')), encoding='utf-8')
    print(json.dumps({'capture': str(a.output), 'sha256': hashlib.sha256(a.output.read_bytes()).hexdigest(),
        'rows': len(result['rows']), 'polymarketContextRows': sum(bool(r['polymarketContext']) for r in result['rows']),
        'officialSigRequests': 0, 'mlFitOrSelectionUse': False}))


if __name__ == '__main__':
    main()
