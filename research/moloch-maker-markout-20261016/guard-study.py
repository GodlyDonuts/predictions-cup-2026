#!/usr/bin/env python3
"""Fixed-policy retrospective microprice-guard falsification, never authority."""
import argparse
from collections import Counter, defaultdict
from decimal import Decimal, localcontext
import hashlib
import importlib.util
import json
from pathlib import Path
import sys

HERE = Path(__file__).resolve().parent
ANALYZER_SHA = 'c8d693b45c24543122500d0ca4cedf699d9ac3efaafec5b37d02b6dd9ec30cb2'
POLICY_SHA = 'a2664a76823975c43575b5e88c8ba07fce7241e28c1d68d6f88de26c09182597'
spec = importlib.util.spec_from_file_location('original_maker_analyzer', HERE/'analyze.py')
A = importlib.util.module_from_spec(spec); spec.loader.exec_module(A)
need = A.need


def ratio(n, d):
    with localcontext() as c:
        c.prec = 40
        return format(Decimal(n) / Decimal(d), '.12f')


def guard(side, limit, bid, ask, bid_n, ask_n):
    """Exactly native positive-integer microprice truncation, fixed original fee0."""
    need(side in ('YES', 'NO') and 0 < bid < ask < A.RAW.SCALE and bid_n > 0 and ask_n > 0, 'MICRO_INPUT')
    numerator, denominator = ask * bid_n + bid * ask_n, bid_n + ask_n
    native = numerator // denominator
    exact_side = numerator if side == 'YES' else A.RAW.SCALE * denominator - numerator
    value = native if side == 'YES' else A.RAW.SCALE - native
    edge = value - limit
    return {'passesFixedOriginalMinimumEdge': edge >= 1000, 'nativeSideEdgeUnits': edge,
            'exactSideEdgeNumerator': str(exact_side - limit * denominator),
            'exactSideEdgeDenominator': denominator,
            'exactSideEdgePerShare': ratio(exact_side - limit * denominator, denominator * A.RAW.SCALE)}


def recover_labels(snapshot, study, report_sha):
    fills, excluded, duplicates = A.fill_inputs(snapshot)
    need(duplicates == 0 and study['sourceCodeSha256'] == ANALYZER_SHA and
         study['source']['report']['sha256'] == report_sha, 'ORIGINAL_STUDY_BINDING')
    order_map = {hashlib.sha256((report_sha+':'+str(q['id'])).encode()).hexdigest(): str(q['id']) for q in snapshot['orders']}
    result = []
    for cohort in study['intentCohorts']:
        qid = order_map[cohort['intentCohortSha256']]
        need(qid not in excluded, 'NONORDINARY_STUDY_COHORT')
        for e in cohort['labelEvidence']:
            matches = [f for f in fills if f['qid'] == qid and f['action'] == 'buy' and
                       str(Decimal(f['executed']) / A.MS) == e['actualExecutionMs'] and
                       f['n'] == e['actualFillShares'] and f['price'] == A.units(e['actualPrice']) and
                       f['fee'] == A.units(e['actualEntryFee'])]
            need(len(matches) == 1, 'AMBIGUOUS_OR_CHANGED_ACTUAL_FILL')
            f = dict(matches[0]); f.update(labels=e['horizons'], contexts={})
            result.append(f)
    need(len(result) == study['aggregate']['actualFillObservations'], 'ORIGINAL_COHORT_COUNT')
    return result


def context_books(fills, breadth, starts):
    by_scope = defaultdict(list)
    for f in fills:
        need(f['qid'] in starts, 'MISSING_ORIGINAL_POST_START')
        f['context_at'] = {'submission': starts[f['qid']], 'prefill': f['executed']}
        by_scope[f['market'], f['exchange']].append(f)
    revisions, cursors, last = {}, {}, None
    counts = Counter()
    for row in breadth:
        e = row['event']; need(e['source'] == 'official_sig_realtime', 'BREADTH_SOURCE')
        at = A.clock(e['receivedAt']); need(last is None or at >= last, 'BREADTH_CLOCK_ORDER'); last = at
        if e['event'] == 'stream_reset':
            revisions.clear(); cursors.clear(); counts['resets'] += 1
            for f in fills:
                for name, target in f['context_at'].items():
                    if at <= target: f['contexts'].pop(name, None)
            continue
        if e['event'] != 'markets_batch': continue
        for item in e['payload']['markets']:
            market, b = str(item['marketId']), item['batch']; d = b['delivery']
            rev, prev = d['revision'], d['previousRevision']
            need(type(rev) is int and type(prev) is int and rev > prev >= 0 and
                 d['model'] == 'best-effort-authoritative-resync', 'DELIVERY')
            prior = revisions.get(market)
            if prior is not None and rev <= prior: continue
            revisions[market] = rev
            if b.get('resyncRequired') or b.get('marketSettled') or (prior is not None and prev != prior):
                for key in list(cursors):
                    if key[0] == market: del cursors[key]
                for key, fs in by_scope.items():
                    if key[0] == market:
                        for f in fs:
                            for name, target in f['context_at'].items():
                                if at <= target: f['contexts'].pop(name, None)
                continue
            for book in b.get('books', []):
                key = market, str(book['exchangeId'])
                if key not in by_scope: continue
                source, seq = A.clock(book['asOf']['at']), A.RAW.integer(book['asOf']['sequence'])
                signature = json.dumps(book, sort_keys=True, separators=(',', ':'), default=str)
                prior = cursors.get(key)
                if prior and seq == prior[0]:
                    need(signature == prior[1], 'BOOK_CURSOR_CONFLICT'); continue
                need(not prior or seq > prior[0], 'BOOK_CURSOR_ROLLBACK'); cursors[key] = seq, signature
                try:
                    bids, asks = A.levels(book['bids'], True), A.levels(book['asks'], False)
                    need(bids[0][0] < asks[0][0] and not book.get('invalid'), 'INVALID_OR_CROSSED')
                except (ValueError, KeyError, TypeError): bids, asks = None, None
                for f in by_scope[key]:
                    for name, target in f['context_at'].items():
                        if at > target: continue
                        f['contexts'].pop(name, None)
                        if bids and source <= at and target - source <= A.AGE:
                            f['contexts'][name] = dict(bid=bids[0][0], bid_n=bids[0][1], ask=asks[0][0], ask_n=asks[0][1],
                                                       journalHash=row['hash'], source=source, received=at)
    return dict(counts)


def summarize(fills, study):
    results = {}
    for name in ('submission', 'prefill'):
        contexts = {}
        for f in fills:
            b = f['contexts'].get(name)
            if b:
                contexts[id(f)] = guard(f['side'], A.units(f['order']['price']), b['bid'], b['ask'], b['bid_n'], b['ask_n'])
        horizons = {}
        for h in A.HORIZONS:
            buckets = {x: {'fills': 0, 'shares': 0, 'markUnits': 0, 'negative': 0, 'zero': 0, 'positive': 0,
                           'intents': set(), 'markets': set(), 'edges': []} for x in ('pass', 'veto', 'unavailable')}
            all_n, all_value = 0, 0
            for f in fills:
                label = f['labels'].get(str(h))
                if not label or label['status'] != 'full_depth': continue
                mark = A.units(label['hypotheticalProceeds']) - f['price'] * f['n'] - f['fee']
                g = contexts.get(id(f)); key = 'unavailable' if g is None else ('pass' if g['passesFixedOriginalMinimumEdge'] else 'veto')
                b = buckets[key]; b['fills'] += 1; b['shares'] += f['n']; b['markUnits'] += mark
                b['negative'] += mark < 0; b['zero'] += mark == 0; b['positive'] += mark > 0
                b['intents'].add(f['qid']); b['markets'].add((f['market'], f['exchange']))
                if g: b['edges'].append(g['nativeSideEdgeUnits'])
                all_n += f['n']; all_value += mark
            baseline = study['aggregate']['horizonsMs'][str(h)]
            need(all_n == baseline['fullDepthShares'] and (A.money(all_value) if all_n else None) ==
                 baseline['hypotheticalLiquidationMinusEntryAndEntryFees'], 'ORIGINAL_HORIZON_PARITY')
            for b in buckets.values():
                b['intentCohorts'] = len(b.pop('intents')); b['marketScopes'] = len(b.pop('markets'))
                mark = b.pop('markUnits')
                b['displayedMarkMinusEntryAndEntryFees'] = A.money(mark) if b['shares'] else None
                edges = b.pop('edges'); b['nativeEdgeUnitsRange'] = [min(edges), max(edges)] if edges else None
            horizons[str(h)] = buckets
        results[name] = {'contextAvailableFills': len(contexts), 'contextMissingFills': len(fills)-len(contexts), 'horizonsMs': horizons}
    return results


def run(directory, manifest_sha, study_path, study_sha):
    directory = Path(directory); manifest_path = directory/'manifest.json'
    A.pinned(manifest_path, manifest_sha)
    manifest = json.loads(manifest_path.read_text()); items = manifest['files']
    policy_path = directory/'policy-extract.json'; A.pinned(policy_path, POLICY_SHA)
    policy_extract = json.loads(policy_path.read_text()); policy = policy_extract['policy']
    need(policy['minimumEdge'] == '0.001000' and policy['feeBps'] == 0 and policy['feePerShare'] == '0.000000'
         and policy['quoteMode'] == 'pairedSpread' and policy_extract['originalPolicyCurrentPricingFieldsUnchanged'] is True, 'FIXED_ORIGINAL_POLICY')
    need(hashlib.sha256((HERE/'analyze.py').read_bytes()).hexdigest() == ANALYZER_SHA, 'ORIGINAL_ANALYZER_CHANGED')
    A.pinned(study_path, study_sha); study = json.loads(Path(study_path).read_text())
    for item in items: A.pinned(directory/Path(item['path']).name, item['sha256'], item['bytes'])
    rp = next(x for x in items if x['path'].endswith('/report.json'))
    dp = next(x for x in items if x['path'].endswith('/dispatch.ndjson'))
    snapshot = A.RAW.native_sealed_object((directory/Path(rp['path']).name).read_bytes())
    fills = recover_labels(snapshot, study, rp['sha256'])
    starts = {}
    for row in A.rows(directory/Path(dp['path']).name, '0'*64):
        e = row['event']
        if e.get('kind') == 'ack' and 'receipt' in e and e['receipt'].get('endpoint') == '/orders':
            qid = str(e['qid']); need(qid not in starts, 'DUPLICATE_ORIGINAL_POST_ACK')
            starts[qid] = A.clock(e['receipt']['requestStartedAt'])
    bs = sorted([x for x in items if x['path'].endswith('.gz')], key=lambda x: ('-part-' in x['path'], x['path']))
    need(study['source']['dispatch']['sha256'] == dp['sha256'] and
         [(x['sha256'], x['bytes']) for x in study['source']['breadth']] ==
         [(x['sha256'], x['bytes']) for x in bs], 'ORIGINAL_LABEL_COHORT_SOURCE_MISMATCH')
    diagnostics = context_books(fills, (row for x in bs for row in A.rows(directory/Path(x['path']).name, '')), starts)
    result = {'schemaVersion': 1, 'analysis': 'fixed_existing_microprice_guard_retrospective_falsification',
       'threshold': '0.001000', 'thresholdTuned': False, 'feesPerShare': '0.000000',
       'nativeIntegerMicroprice': 'floor((YESask*bidQty+YESbid*askQty)/(bidQty+askQty)); NO=1-YESmicro',
       'originalQuotedLimitUsed': True, 'sourceCodeSha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
       'originalAnalyzerSha256': ANALYZER_SHA, 'originalStudySha256': study_sha, 'inputCaptureManifestSha256': manifest_sha,
       'originalReportSha256': rp['sha256'], 'policyExtractSha256': POLICY_SHA,
       'policyProvenance': {k: policy_extract[k] for k in ('originalBankManifestRawSha256', 'originalBankManifestEnvelopeHash', 'controlRawSha256')},
       'reportCutoff': study['reportCutoff'], 'ordinaryActualBuyFillObservations': len(fills),
       'contexts': summarize(fills, study), 'diagnostics': diagnostics,
       'profitabilityEstablished': False, 'runtimeGuardImplemented': False,
       'limitations': ['Retrospective same finite fill-conditioned cohort; no threshold or feature fitting, no prospective qualification.',
        'Submission context uses original breadth received before POST requestStartedAt, not proof that runtime consumed it.',
        'Prefill context may change long after order submission; a submission-only veto cannot prevent later adverse resting fills.',
        'No alternative order, cancellation, replacement fill or avoided PnL is credited.',
        'Own resting liquidity and STP can distort microprice/depth; markouts remain optimistic displayed-depth diagnostics.',
        'Missing contexts/outcomes remain missing; partial fills share intent and market risk.']}
    for item in items: A.pinned(directory/Path(item['path']).name, item['sha256'], item['bytes'])
    return result


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--input-directory', required=True); p.add_argument('--manifest-sha256', required=True)
    p.add_argument('--original-study', required=True); p.add_argument('--original-study-sha256', required=True)
    p.add_argument('--output', required=True); a = p.parse_args()
    need(Path(a.output).resolve().parent != Path(a.input_directory).resolve(), 'OUTPUT_MUST_BE_SEPARATE')
    result = run(a.input_directory, a.manifest_sha256, a.original_study, a.original_study_sha256)
    Path(a.output).write_text(json.dumps(result, sort_keys=True, indent=2)+'\n', encoding='utf-8')


if __name__ == '__main__': main()
