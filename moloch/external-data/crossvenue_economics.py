"""Bounded offline economic evaluation of original crossvenue_observer records.

No execution authority or model approval. Ridge scores are expected-return
research estimates, never probabilities. Missing features remain missing.
"""
import argparse
from collections import Counter
from datetime import datetime
from decimal import Decimal
import gzip
import hashlib
import json
import math
from pathlib import Path
import re
import stat

HORIZONS = (1000, 3000, 10000, 30000, 120000)
FEATURES = ('externalLogitDelta1s', 'externalLogitDelta5s', 'externalLogitDelta30s',
            'externalImbalance', 'sigImbalance', 'sigSpread', 'externalSigLogitGap')
MAX_LINE = 1024 * 1024
MAX_ROWS = 500000
MAX_BYTES = 512 * 1024 * 1024
HEX = re.compile(r'^[0-9a-f]{64}$')

def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'), allow_nan=False)

def digest(value):
    return hashlib.sha256(canonical(value).encode()).hexdigest()

def require(ok, code):
    if not ok:
        raise ValueError('ECONOMICS_' + code)

def clock(value):
    require(isinstance(value, str), 'CLOCK')
    d = datetime.fromisoformat(value.replace('Z', '+00:00'))
    require(d.tzinfo is not None, 'CLOCK')
    return int(d.timestamp() * 1000)

def number(value):
    require(type(value) in (int, float) and math.isfinite(value), 'NUMBER')
    return float(value)

def money(value):
    require(type(value) in (str, int) and bool(re.fullmatch(r'\d+(?:\.\d{1,6})?', str(value))), 'DECIMAL')
    result = Decimal(value)
    require(result.is_finite(), 'DECIMAL')
    return result

def signed_money(value):
    require(isinstance(value, str) and bool(re.fullmatch(r'-?\d+(?:\.\d{1,6})?', value)), 'DECIMAL')
    return Decimal(value)

def unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, 'DUPLICATE_JSON_KEY')
        result[key] = value
    return result

def load(paths, as_of, max_rows=MAX_ROWS, max_bytes=MAX_BYTES):
    require(0 < len(paths) <= 128 and 1 <= max_rows <= MAX_ROWS and 1 <= max_bytes <= MAX_BYTES, 'INPUT_BOUNDS')
    paths = [Path(p) for p in paths]
    require(len({str(p.resolve()) for p in paths}) == len(paths), 'DUPLICATE_INPUT')
    decisions, labels, files, counts = {}, {}, [], Counter()
    total = 0
    compressed = 0
    for path in sorted(paths):
        before = path.lstat()
        require(stat.S_ISREG(before.st_mode) and not path.is_symlink(), 'INPUT_FILE')
        compressed += before.st_size
        require(compressed <= max_bytes, 'INPUT_BOUNDS')
        sha = hashlib.sha256()
        with path.open('rb') as f:
            while chunk := f.read(1024 * 1024):
                sha.update(chunk)
        with gzip.open(path, 'rb') if path.suffix == '.gz' else path.open('rb') as stream:
            while line := stream.readline(MAX_LINE + 1):
                total += len(line)
                counts['rows'] += 1
                require(len(line) <= MAX_LINE and line.endswith(b'\n') and total <= max_bytes and counts['rows'] <= max_rows, 'INPUT_BOUNDS')
                row = json.loads(line, object_pairs_hook=unique_object, parse_constant=lambda _: (_ for _ in ()).throw(ValueError('ECONOMICS_NONFINITE_JSON')))
                require(isinstance(row, dict), 'RECORD')
                kind = row.get('kind')
                if kind not in ('decision_features', 'horizon_label'):
                    counts['ignoredOtherRecords'] += 1
                    continue
                sid = row.get('sampleId')
                require(isinstance(sid, str) and bool(HEX.fullmatch(sid)), 'SAMPLE_ID')
                if kind == 'decision_features':
                    at = row.get('decisionMs')
                    require(type(at) is int and at == clock(row['decisionAt']), 'DECISION_CLOCK')
                    key, store = sid, decisions
                else:
                    h = row.get('horizonMs')
                    require(type(h) is int and h in HORIZONS, 'HORIZON')
                    at = clock(row['observedAt'])
                    key, store = (sid, h), labels
                if at > as_of:
                    counts['excludedAfterCutoff'] += 1
                    continue
                if key in store:
                    require(store[key] == row, 'CONFLICTING_DUPLICATE')
                    counts['identicalDuplicates'] += 1
                else:
                    store[key] = row
        after = path.lstat()
        require((before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns) ==
                (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns), 'INPUT_CHANGED')
        files.append({'name': path.name, 'bytes': before.st_size, 'sha256': sha.hexdigest()})
    counts['unmatchedLabels'] = sum(sid not in decisions for sid, _ in labels)
    return decisions, labels, files, counts

def validate_lag_anchors(row):
    """Optional new producer evidence. Old explicitly pinned producers stay readable."""
    if 'externalLagAnchors' not in row:
        return
    anchors, changes, ext, at = row['externalLagAnchors'], row['externalLogitChanges'], row['external'], row['decisionMs']
    require(isinstance(anchors, dict) and set(anchors) == {'1000', '5000', '30000'}, 'LAG_ANCHORS')
    for lag in (1000, 5000, 30000):
        anchor, change = anchors[str(lag)], changes.get(str(lag))
        if anchor is None:
            require(change is None, 'LAG_ANCHOR_MISSING')
            continue
        require(isinstance(anchor, dict) and change is not None, 'LAG_ANCHOR_MISSING')
        require(isinstance(anchor.get('bodySha256'), str) and HEX.fullmatch(anchor['bodySha256']), 'LAG_ANCHOR_HASH')
        received, available = anchor.get('receivedMs'), anchor.get('availableMs')
        require(type(received) is int and type(available) is int and 0 <= received <= available <= at and
                received <= ext['receivedMs'] and received <= at-lag and at-lag-received <= 20000, 'LAG_ANCHOR_CLOCK')
        require(all(anchor.get(k) == ext.get(k) for k in ('source', 'ticker', 'mappingKind', 'sid', 'wsEpoch', 'streamBoundary')), 'LAG_ANCHOR_IDENTITY')
        if ext.get('source') == 'kalshi_official_ws_v2':
            require(type(anchor.get('sequence')) is int and type(ext.get('sequence')) is int and
                    0 <= anchor['sequence'] <= ext['sequence'], 'LAG_ANCHOR_CURSOR')
        source = anchor.get('sourceTsMs')
        require(source is None or type(source) is int and 0 <= source <= available and source-received <= 1000, 'LAG_ANCHOR_CLOCK')
        before, current = number(anchor.get('midpoint')), number(ext['features'].get('midpoint'))
        require(0 <= before <= 1 and 0 <= current <= 1, 'LAG_ANCHOR_MIDPOINT')
        def logit(value):
            value = min(.999999, max(.000001, value))
            return math.log(value/(1-value))
        require(number(change) == logit(current)-logit(before), 'LAG_ANCHOR_DELTA')


def feature_vector(row, sample_id_format="scoped_v2"):
    d, sig, ext = row['decisionMs'], row['sig'], row['external']
    require(clock(sig['sourceAt']) <= clock(sig['receivedAt']) <= d, 'FEATURE_LOOKAHEAD')
    require(type(ext['receivedMs']) is int and type(ext['availableMs']) is int and ext['receivedMs'] <= ext['availableMs'] <= d, 'FEATURE_LOOKAHEAD')
    require(isinstance(sig.get('journalHash'), str) and HEX.fullmatch(sig['journalHash']) and isinstance(ext.get('bodySha256'), str) and HEX.fullmatch(ext['bodySha256']), 'FEATURE_SOURCE_HASH')
    require(sample_id_format in ('scoped_v2', 'legacy_unscoped_v1'), 'PRODUCER_FORMAT')
    prefix = f"{row['marketId']}:{row['exchangeId']}:" if sample_id_format == 'scoped_v2' else ''
    expected = hashlib.sha256(f"{prefix}{sig['journalHash']}:{ext['bodySha256']}:{d}".encode()).hexdigest()
    require(row['sampleId'] == expected, 'SAMPLE_BINDING')
    validate_lag_anchors(row)
    changes = row['externalLogitChanges']
    values = [changes.get(str(h)) for h in (1000, 5000, 30000)]
    values += [ext['features'].get('imbalance'), sig['features'].get('imbalance'), sig['features'].get('spread')]
    if any(v is None for v in values):
        return None
    sm, em = number(sig['features']['midpoint']), number(ext['features']['midpoint'])
    require(0 < sm < 1 and 0 < em < 1, 'MIDPOINT')
    values = [number(v) for v in values]
    require(all(-1 <= v <= 1 for v in values[3:5]) and 0 <= values[5] <= 1, 'FEATURE_RANGE')
    return values + [math.log(em / (1-em)) - math.log(sm / (1-sm))]

def examples(decisions, labels, horizon, as_of, slack, fee, buffer, mapping_kind, sample_id_format="scoped_v2"):
    rows, counts = [], Counter()
    for sid, sample in sorted(decisions.items(), key=lambda v: (v[1]['decisionMs'], v[0])):
        counts['decisions'] += 1
        if sample['external'].get('mappingKind') != mapping_kind:
            counts['otherMapping'] += 1
            continue
        x = feature_vector(sample, sample_id_format)
        if x is None:
            counts['missingFeature'] += 1
            continue
        label = labels.get((sid, horizon))
        if label is None:
            counts['missingLabel'] += 1
            continue
        target = sample['decisionMs'] + horizon
        source, received, observed = clock(label['sourceAt']), clock(label['receivedAt']), clock(label['observedAt'])
        require(target <= source <= received <= observed <= as_of and source <= target+slack and received <= target+slack, 'LABEL_LOOKAHEAD_OR_WINDOW')
        require(label.get('counterfactual') is True and label.get('actualFill') is False and isinstance(label.get('sigJournalHash'), str) and HEX.fullmatch(label['sigJournalHash']), 'LABEL_PROVENANCE')
        require(signed_money(label.get('feesAssumed')) == 0, 'LABEL_FEE_ASSUMPTION')
        before, after = sample['sig']['features'], label['features']
        bid, ask, future_bid, future_ask = map(money, [before['bid'], before['ask'], after['bid'], after['ask']])
        require(0 <= bid <= ask <= 1 and 0 <= future_bid <= future_ask <= 1, 'BOOK_PRICES')
        if any(money(v) < 1 for v in [before['askQuantity'], before['bidQuantity'], after['bidQuantity'], after['askQuantity']]):
            counts['missingOneShareCapacity'] += 1
            continue
        yes, no = future_bid-ask, bid-future_ask
        require(label.get('yesOneShareTakerRoundTrip') is not None and label.get('noOneShareTakerRoundTrip') is not None, 'LABEL_CAPACITY_CONFLICT')
        require(signed_money(label['yesOneShareTakerRoundTrip']) == yes and signed_money(label['noOneShareTakerRoundTrip']) == no, 'LABEL_RETURN_CONFLICT')
        rows.append({'id': sid, 'market': str(sample['marketId']), 'exchange': str(sample['exchangeId']), 'at': sample['decisionMs'], 'available': observed,
                     'end': target+slack, 'x': x, 'yes': float(yes-fee-buffer), 'no': float(no-fee-buffer),
                     'yesExact': str(yes-fee-buffer), 'noExact': str(no-fee-buffer), 'yesGross': str(yes), 'noGross': str(no), 'entryAsk': str(ask), 'futureBid': str(future_bid)})
    # Market clusters, not exchanges/sides/prints: greedily nonoverlapping full
    # decision-to-label-slack windows. This is deterministic, never outcome-based.
    selected, ends = [], {}
    for row in rows:
        if row['at'] < ends.get(row['market'], -1):
            counts['overlappingClusterRows'] += 1
            continue
        selected.append(row)
        ends[row['market']] = row['end']
    counts['independentClusters'] = len(selected)
    return selected, dict(counts)

def solve(a, b):
    n = len(b)
    rows = [list(a[i])+[b[i]] for i in range(n)]
    for i in range(n):
        pivot = max(range(i, n), key=lambda j: abs(rows[j][i]))
        require(abs(rows[pivot][i]) > 1e-12, 'SINGULAR_MODEL')
        rows[i], rows[pivot] = rows[pivot], rows[i]
        scale = rows[i][i]
        rows[i] = [v/scale for v in rows[i]]
        for j in range(n):
            if j != i:
                factor = rows[j][i]
                rows[j] = [x-factor*y for x, y in zip(rows[j], rows[i])]
    return [rows[i][-1] for i in range(n)]

def fit(train, side):
    require(len(train) >= 16, 'TRAINING_COUNT')
    means = [sum(r['x'][j] for r in train)/len(train) for j in range(len(FEATURES))]
    scales = [max(1e-9, math.sqrt(sum((r['x'][j]-means[j])**2 for r in train)/len(train))) for j in range(len(FEATURES))]
    rows = [[1.0]+[(v-m)/s for v, m, s in zip(r['x'], means, scales)] for r in train]
    a = [[sum(x[i]*x[j] for x in rows)+(1.0 if i == j and i else 0.0) for j in range(8)] for i in range(8)]
    b = [sum(x[i]*r[side] for x, r in zip(rows, train)) for i in range(8)]
    return {'means': means, 'scales': scales, 'coefficients': solve(a, b), 'baseline': sum(r[side] for r in train)/len(train)}

def predict(model, x):
    values = [1.0]+[(v-m)/s for v, m, s in zip(x, model['means'], model['scales'])]
    return sum(a*b for a, b in zip(values, model['coefficients']))

def walk_forward(rows, minimum_clusters=100):
    require(type(minimum_clusters) is int and minimum_clusters >= 100, 'MINIMUM_CLUSTERS')
    times = sorted({r['at'] for r in rows})
    folds, scored = [], []
    if len(times) >= 4:
        boundaries = [times[len(times)*i//4] for i in (1, 2, 3)]
        for k, start in enumerate(boundaries):
            stop = boundaries[k+1] if k < 2 else math.inf
            # Full horizon+slack purge AND actual first available label fence.
            train = [r for r in rows if r['end'] < start and r['available'] <= start]
            test = [r for r in rows if start <= r['at'] < stop]
            fold = {'testStartMs': start, 'testEndExclusiveMs': None if math.isinf(stop) else stop, 'trainingClusters': len(train), 'testClusters': len(test), 'trained': len(train) >= 16}
            folds.append(fold)
            if len(train) < 16:
                continue
            models = {side: fit(train, side) for side in ('yes', 'no')}
            fold['trainingSampleIdsSha256'] = digest([r['id'] for r in train])
            fold['latestTrainingLabelAvailableMs'] = max(r['available'] for r in train)
            fold['modelsResearchOnly'] = models
            for r in test:
                estimates = {side: predict(models[side], r['x']) for side in ('yes', 'no')}
                side = max(('yes', 'no'), key=lambda s: estimates[s])
                scored.append({'sampleId': r['id'], 'market': r['market'], 'at': r['at'], 'side': side, 'prediction': estimates[side], 'actual': r[side], 'baseline': models[side]['baseline'],
                               'yes': r['yes'], 'no': r['no'], 'yesExact': r['yesExact'], 'noExact': r['noExact'], 'yesPrediction': estimates['yes'], 'noPrediction': estimates['no'],
                               'yesBaseline': models['yes']['baseline'], 'noBaseline': models['no']['baseline']})
    chosen = [r for r in scored if r['prediction'] > 0]
    baseline_chosen = [r for r in scored if max(r['yesBaseline'], r['noBaseline']) > 0]
    mse = lambda key: sum((r[s]-r[s+key])**2 for r in scored for s in ('yes', 'no'))/(2*len(scored)) if scored else None
    baseline_exact = sum((Decimal(r['yesExact'] if r['yesBaseline'] >= r['noBaseline'] else r['noExact']) for r in baseline_chosen), Decimal(0))
    baseline_return = float(baseline_exact)
    count = len(scored)
    net_exact = sum((Decimal(r[r['side']+'Exact']) for r in chosen), Decimal(0))
    net = float(net_exact)
    per_side = {s: {'allCrossingNetReturnSum': sum(r[s] for r in scored), 'allCrossingNetReturnSumExact': str(sum((Decimal(r[s+'Exact']) for r in scored),Decimal(0))), 'allCrossingNetReturnMean': sum(r[s] for r in scored)/count if count else None} for s in ('yes', 'no')}
    insufficient = count < minimum_clusters or len(chosen) < minimum_clusters
    model_mse, baseline_mse = mse('Prediction'), mse('Baseline')
    beats_baseline = net_exact > baseline_exact and model_mse is not None and model_mse < baseline_mse
    return {'status': 'UNQUALIFIED_RESEARCH', 'abstain': insufficient or net <= 0 or not beats_baseline,
            'reason': 'INSUFFICIENT_INDEPENDENT_CLUSTERS' if insufficient else 'NONPOSITIVE_OUT_OF_SAMPLE_ECONOMICS' if net <= 0 else 'NO_IMPROVEMENT_OVER_INTERCEPT' if not beats_baseline else 'POSITIVE_COUNTERFACTUAL_RESEARCH_ONLY',
            'outOfSampleClusters': count, 'selectedClusters': len(chosen), 'minimumClusters': minimum_clusters, 'folds': folds,
            'modelMseNetReturn': model_mse, 'interceptBaselineMseNetReturn': baseline_mse, 'beatsInterceptOnMseAndSelectedEconomics': beats_baseline,
            'selectedNetCrossingReturnSum': net, 'selectedNetCrossingReturnSumExact': str(net_exact), 'selectedNetCrossingReturnMean': net/len(chosen) if chosen else None,
            'interceptBaselineSelectedClusters': len(baseline_chosen), 'interceptBaselineNetReturnSum': baseline_return, 'interceptBaselineNetReturnSumExact': str(baseline_exact),
            'allCrossingBySide': per_side, 'outOfSampleSampleIdsSha256': digest([r['sampleId'] for r in scored]),
            'selectedSampleIdsSha256': digest([r['sampleId'] for r in chosen]), 'actualFill': False, 'executionAuthority': False,
            'clusterCaveat': 'Nonoverlapping market/time windows do not prove cross-market statistical independence.'}

def report(paths, as_of, *, slack=3000, fee=Decimal('0'), buffer=Decimal('0.005'), minimum_clusters=100, mapping_kind='direct_party_reference', max_rows=MAX_ROWS, max_bytes=MAX_BYTES, sample_id_format="scoped_v2", producer_source=None, producer_sha256=None):
    require(sample_id_format in ("scoped_v2", "legacy_unscoped_v1"), "PRODUCER_FORMAT")
    require((producer_source is None) == (producer_sha256 is None), "PRODUCER_PROVENANCE")
    producer = None
    if producer_source is not None:
        source = Path(producer_source)
        require(source.is_file() and not source.is_symlink() and source.stat().st_size <= 1024*1024 and isinstance(producer_sha256, str) and HEX.fullmatch(producer_sha256), "PRODUCER_SOURCE")
        actual = hashlib.sha256(source.read_bytes()).hexdigest()
        require(actual == producer_sha256, "PRODUCER_SOURCE_HASH")
        producer = {"name": source.name, "sha256": actual, "sampleIdFormat": sample_id_format}
    require(type(slack) is int and 0 <= slack <= 30000 and fee.is_finite() and buffer.is_finite() and fee >= 0 and buffer >= 0, 'POLICY')
    d, labels, files, counts = load(paths, as_of, max_rows, max_bytes)
    policy = {'features': FEATURES, 'sampleIdFormat': sample_id_format, 'algorithm': 'two-fixed-ridge-return-regressions', 'ridgePenalty': 1.0, 'folds': 3, 'labelSlackMs': slack, 'feePerShareRoundTrip': str(fee), 'bufferPerShare': str(buffer), 'mappingKind': mapping_kind, 'minimumClusters': minimum_clusters, 'counterfactualQuantityShares': 1, 'predictionThresholdNet': 0, 'yesCrossingConvention': 'future SIG bid minus original SIG ask', 'noCrossingConvention': 'original SIG bid minus future SIG ask'}
    horizons = {}
    for h in HORIZONS:
        rows, diagnostics = examples(d, labels, h, as_of, slack, fee, buffer, mapping_kind, sample_id_format)
        horizons[str(h)] = {**walk_forward(rows, minimum_clusters), 'data': diagnostics}
    return {'schemaVersion': 1, 'asOfMs': as_of, 'producerSource': producer, 'producerSourceVerified': producer is not None, 'sourceBundleSha256': digest(files), 'sources': files,
            'policy': policy, 'cutoffPolicySha256': digest({'asOfMs': as_of, 'policy': policy, 'producerSource': producer}), 'input': dict(counts),
            'sourceCodeSha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(), 'horizons': horizons,
            'status': 'UNQUALIFIED_RESEARCH', 'modelApproval': False, 'profitEstablished': False, 'executionAuthority': False}

def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--input', nargs='+', required=True, type=Path)
    p.add_argument('--as-of', required=True)
    p.add_argument('--output', required=True, type=Path)
    p.add_argument('--producer-source', required=True, type=Path)
    p.add_argument('--producer-sha256', required=True)
    p.add_argument('--sample-id-format', required=True, choices=['scoped_v2', 'legacy_unscoped_v1'])
    p.add_argument('--fee-per-share', default='0')
    p.add_argument('--buffer-per-share', default='0.005')
    p.add_argument('--label-slack-ms', type=int, default=3000)
    p.add_argument('--minimum-clusters', type=int, default=100)
    a = p.parse_args()
    result = report(a.input, clock(a.as_of), slack=a.label_slack_ms, fee=signed_money(a.fee_per_share), buffer=signed_money(a.buffer_per_share), minimum_clusters=a.minimum_clusters, sample_id_format=a.sample_id_format, producer_source=a.producer_source, producer_sha256=a.producer_sha256)
    with a.output.open('x', encoding='utf-8') as f:
        f.write(canonical(result)+'\n')

if __name__ == '__main__':
    main()
