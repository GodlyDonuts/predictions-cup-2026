"""File-only coverage/price survey; never a SIG client or trading authority.

Capture locally from the existing original Kalshi cache, then compare on the
worker host with its existing SIG recorder. Price differences are conditional
research nominations, not calibrated expected profit or settlement equivalence.
"""
import argparse
from collections import Counter
from datetime import datetime, timezone
from decimal import Decimal as D
import hashlib
import importlib.util
import json
from pathlib import Path
import sys
import time


def load(path):
    return json.loads(Path(path).read_bytes())


def clock(value):
    return int(datetime.fromisoformat(value.replace('Z', '+00:00')).timestamp()*1000)


def capture(config_path, repo, now):
    sys.path.insert(0, str(repo))
    from moloch.tartarus import party_reference as reference
    config = load(config_path)
    root, cache = Path(config['registryRoot']), Path(config['kalshiRoot'])
    old = load(root/'full-universe-expanded/census.private.json')['rows']
    registry = [{k: r[k] for k in ('exchangeId', 'marketId', 'contractId',
        'semanticSourceSha256', 'officialContractRootSha256', 'originalFile')} for r in old]
    originals = {r['semanticSourceSha256']: (root/'roots'/r['originalFile']).read_bytes() for r in old}
    quotes = {r['kalshiTicker']: (cache/'latest'/(r['kalshiTicker']+'.json')).read_bytes() for r in old}
    metadata = {r['kalshiTicker']: (cache/'metadata'/(r['kalshiTicker']+'.json')).read_bytes() for r in old}
    checked = reference.census(registry, originals, load(cache/'universe.json')['markets'],
        quotes, metadata, decision_ms=now, maximum_receipt_age_ms=30000,
        maximum_metadata_age_ms=86400000)
    rows = []
    for r in checked['rows']:
        q = json.loads(quotes[r['kalshiTicker']])
        rows.append({**{k: r[k] for k in ('exchangeId', 'marketId', 'contractId',
            'kalshiTicker', 'yesBid', 'yesAsk', 'referenceRecordSha256',
            'metadataRecordSha256', 'officialContractRootSha256')},
            'receivedAt': q['receivedAt'], 'activeReferenceTarget': r['exchangeId'] in config['exchanges']})
    return {'capturedAtMs': now, 'registryMarkets': len(old), 'rows': rows,
        'unavailable': checked['unavailable'], 'officialSigRequests': 0,
        'executionAuthority': False, 'settlementEquivalenceClaimed': False}


def compare(reference, books, now):
    rows = []
    for r in reference['rows']:
        b = books.get(r['exchangeId'])
        out = {**r, 'reasons': [], 'current': False}
        out['referenceAgeMs'] = now-clock(r['receivedAt'])
        if not 0 <= out['referenceAgeMs'] <= 30000:
            out['reasons'].append('reference_not_current')
        if b is None:
            out['reasons'].append('missing_sig_book'); rows.append(out); continue
        out['bookAgeMs'], out['sourceAgeMs'] = now-clock(b['receivedAt']), now-clock(b['sourceAt'])
        out['bookSequence'] = b['sequence']
        out['bookSha256'] = hashlib.sha256(json.dumps(b, sort_keys=True, separators=(',', ':')).encode()).hexdigest()
        if b['exchangeId'] != r['exchangeId'] or b['marketId'] != r['marketId']:
            out['reasons'].append('scope_mismatch')
        if not all(0 <= out[k] <= 10000 for k in ('bookAgeMs', 'sourceAgeMs')):
            out['reasons'].append('sig_book_not_current')
        bids = sorted([(D(str(x['price'])), int(x['quantity'])) for x in b['bids']], reverse=True)
        asks = sorted((D(str(x['price'])), int(x['quantity'])) for x in b['asks'])
        if not bids or not asks or not 0 < bids[0][0] < asks[0][0] < 1:
            out['reasons'].append('invalid_or_crossed_book'); rows.append(out); continue
        lo, hi = D(r['yesBid']), D(r['yesAsk'])
        if not 0 < lo <= hi < 1:
            out['reasons'].append('invalid_reference'); rows.append(out); continue
        mid = (lo+hi)/2
        out.update(sigYesBid=str(bids[0][0]), sigYesAsk=str(asks[0][0]),
            sigYesBidQuantity=bids[0][1], sigYesAskQuantity=asks[0][1],
            conditionalMidpoint=str(mid), yesBuyDifference=str(mid-asks[0][0]),
            noBuyDifference=str(bids[0][0]-mid), current=not out['reasons'])
        # Illustrative existing 2% exceptional payout allowance plus 1 cent
        # margin, not a new review or authority to change account allocations.
        for side, p, levels in [('YES', mid, asks), ('NO', 1-mid, [(1-p, q) for p,q in bids])]:
            ceiling = D('.98')*p-D('.01')
            eligible = [(p,q) for p,q in levels if 0 < p <= ceiling and q > 0]
            out[side.lower()+'ConditionalDepthQuantity'] = sum(q for p,q in eligible)
            out[side.lower()+'ConditionalDepthCost'] = str(sum((p*q for p,q in eligible), D(0)))
        rows.append(out)
    current = [r for r in rows if r['current']]
    nominations = [r for r in current if r['yesConditionalDepthQuantity'] or r['noConditionalDepthQuantity']]
    return {'observedUtc': datetime.fromtimestamp(now/1000,timezone.utc).isoformat(),
        'referenceCapturedAtMs': reference['capturedAtMs'], 'mappedReferences': len(rows),
        'currentPairedBooks': len(current), 'conditionalNominations': len(nominations),
        'additionalNominations': sum(not r['activeReferenceTarget'] for r in nominations),
        'reasonCounts': dict(Counter(s for r in rows for s in r['reasons'])), 'rows': rows,
        'officialSigRequests': 0, 'executionAuthority': False, 'settlementEquivalenceClaimed': False,
        'accountEligibilityVerified': False, 'calibratedProbabilityClaimed': False,
        'quantityOrCapitalReserved': False, 'crossVenueExecutableArbitrageClaimed': False}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    sub = p.add_subparsers(dest='mode', required=True)
    a = sub.add_parser('capture'); a.add_argument('--config', type=Path, required=True)
    a.add_argument('--repo', type=Path, required=True)
    a = sub.add_parser('compare'); a.add_argument('--reference', type=Path, required=True)
    a.add_argument('--runtime', type=Path, required=True); a.add_argument('--pid', type=int, required=True)
    a.add_argument('--book-reader', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    a = p.parse_args(); now = time.time_ns()//1000000
    if a.output.exists(): raise ValueError('IMMUTABLE_OUTPUT_EXISTS')
    if a.mode == 'capture': value = capture(a.config, a.repo, now)
    else:
        spec = importlib.util.spec_from_file_location('book_reader', a.book_reader)
        module = importlib.util.module_from_spec(spec); spec.loader.exec_module(module)
        value = compare(load(a.reference), module.recent_books(a.runtime, a.pid), now)
    a.output.parent.mkdir(parents=True, exist_ok=True)
    a.output.write_text(json.dumps(value, indent=2), encoding='utf8')
    print(json.dumps({k:v for k,v in value.items() if k not in ('rows', 'unavailable')}))


if __name__ == '__main__': main()
