"""Bounded, protocol-pinned trade-trigger research. No financial authority.

CLI: --manifest PATH --manifest-sha256 SHA --output NEW_PATH
Manifest schema1: sealed:true, asOfUtc, analysisProtocolSha256, cohorts:[{
 cohortId, producer:{path,bytes,sha256}, samplingPolicySha256, configSha256,
 mappingSha256, files:[{path,bytes,sha256}]}]. Paths are manifest-relative.
The operator must freeze this manifest/hash before outcomes are opened. Original
producer bytes are hashed, never executed. No network, arbitrary model options,
legacy ID rewriting, missing-value imputation or live-file collection exists.
Source hashes embedded in emitted rows are REFERENCES, not independent raw-source
verification. Proving first-later-book/tape completeness needs original journals.
"""
import argparse
from collections import Counter, defaultdict
from datetime import datetime
from decimal import Decimal
import gzip
import hashlib
import json
import math
from pathlib import Path
import re
import stat

PROTOCOL_SHA = 'dbf827a8cab904595b6173873949cd8c5f9666769754993b5c7636344bddc291'
POLICY_SHA = '031edcb2d4d6ca144e89d86b467e6b879113f1e172c3ef19457f2b577996717b'
HORIZONS = (1000, 3000, 10000, 30000, 120000)
MODEL_COLUMNS = {'unchanged_sig_bbo': (), 'sig_local_ridge': (4, 5),
                 'price_seven_ridge': tuple(range(7)), 'price_plus_trigger_ridge': tuple(range(10))}
FEATURE_NAMES = ('externalLogitDelta1s', 'externalLogitDelta5s', 'externalLogitDelta30s', 'externalImbalance',
                 'sigImbalance', 'sigSpread', 'externalSigLogitGap', 'triggerDirection',
                 'triggerSignedLogQuantity', 'triggerExecutionToObserverAgeSeconds')
ECONOMIC = ('ticker', 'tradeId', 'sourceTsMs', 'quantity', 'yesPrice', 'noPrice', 'direction', 'isBlockTrade')
FLOW_FIELDS = ('tradeId', 'ticker', 'bodySha256', 'receivedMs', 'availableMs', 'sourceTsMs',
               'sid', 'sequence', 'streamEpoch', 'quantity', 'yesPrice', 'noPrice', 'direction', 'isBlockTrade')
HEX = re.compile(r'^[0-9a-f]{64}$')
MAX_BYTES, MAX_ROWS, MAX_LINE = 512*1024*1024, 500000, 1024*1024


def need(ok, code):
    if not ok:
        raise ValueError('TRADE_ANALYSIS_' + code)


def compact(x):
    # Exact original observer compact(): sorted keys, UTF-8, no spaces.
    return json.dumps(x, sort_keys=True, separators=(',', ':'), ensure_ascii=False, allow_nan=False)


def sha(b):
    return hashlib.sha256(b).hexdigest()


def digest(x):
    return sha(json.dumps(x, sort_keys=True, separators=(',', ':'), ensure_ascii=False, allow_nan=False).encode())


def unique(pairs):
    out = {}
    for k, v in pairs:
        need(k not in out, 'DUPLICATE_JSON_KEY')
        out[k] = v
    return out


def decode(b):
    return json.loads(b, object_pairs_hook=unique,
                      parse_constant=lambda _: need(False, 'NONFINITE_JSON'))


def hash_field(v):
    need(isinstance(v, str) and HEX.fullmatch(v), 'HASH')
    return v


def integer(v):
    need(type(v) is int and 0 <= v <= 2**53-1, 'CLOCK_OR_INTEGER')
    return v


def clock(s):
    need(isinstance(s, str), 'CLOCK')
    t = datetime.fromisoformat(s.replace('Z', '+00:00'))
    need(t.tzinfo is not None, 'CLOCK')
    return integer(round(t.timestamp()*1000))


def decimal(v, signed=False):
    need(type(v) in (int, str) and re.fullmatch(r'-?\d+(?:\.\d{1,6})?' if signed else r'\d+(?:\.\d{1,6})?', str(v)), 'DECIMAL')
    x = Decimal(v)
    need(x.is_finite() and abs(x) <= Decimal(10)**15, 'DECIMAL_RANGE')
    return x


def numeric(v):
    need(type(v) in (float, int) and math.isfinite(v), 'FINITE_FEATURE')
    return float(v)


def logit(p):
    p = min(.999999, max(.000001, p))
    return math.log(p/(1-p))


def book(f):
    b, a, bq, aq = (decimal(f[k]) for k in ('bid', 'ask', 'bidQuantity', 'askQuantity'))
    need(0 <= b <= a <= 1 and bq > 0 and aq > 0, 'BOOK')
    derived = {'midpoint': float((b+a)/2), 'spread': float(a-b),
               'imbalance': float((bq-aq)/(bq+aq)), 'microprice': float((b*aq+a*bq)/(bq+aq))}
    need(all(numeric(f[k]) == v for k, v in derived.items()), 'BOOK_FEATURE_CONFLICT')
    return b, a, bq, aq


def flow_evidence(row):
    """Verify included original snapshot/hash and exact trigger membership."""
    t, d, r = row['trigger'], row['decisionMs'], row['trigger']['record']
    f = row['externalTradeFlow']
    need(isinstance(f, dict) and f['decisionMs'] == d and f['availableMs'] == t['observerFirstAvailableMs'], 'FLOW_SOURCE')
    s = f['sourceSnapshot']
    need(sha(compact(s).encode()) == f['sourceSnapshotSha256'] == t['sourceSnapshotSha256'], 'FLOW_HASH')
    need(s['asOfMs'] == t['sourceExportMs'] and f['snapshotAgeMs'] == d-s['asOfMs'] and
         s['source'] == 'kalshi_official_ws_v2_public_trade' and s['ticker'] == r['ticker'] and
         s['streamEpoch'] == r['streamEpoch'] and s['sid'] == r['sid'] and s['observing'] is True, 'FLOW_SOURCE')
    need(all(s[k] is False for k in ('bookFreshnessRenewed', 'pricingAuthority', 'executionAuthority')) and
         f['pricingAuthority'] is False and f['executionAuthority'] is False and set(s['windows']) == {'1000', '5000', '30000'}, 'FLOW_AUTHORITY')
    found = False
    identities = {}
    for h, w in s['windows'].items():
        evidence, n = w['evidence'], integer(w['observedTradeCount'])
        need(isinstance(evidence, list) and len(evidence) <= 8 and n <= 4096 and len(evidence) <= n and
             w['evidenceComplete'] is (len(evidence) == n) and w['completeTradeTape'] is False, 'FLOW_COMPLETENESS')
        hash_field(w['evidenceSha256'])
        ordered, ids = [], set()
        for item in evidence:
            need(set(item) == set(FLOW_FIELDS) and item['ticker'] == s['ticker'] and
                 item['sid'] == s['sid'] and item['streamEpoch'] == s['streamEpoch'], 'FLOW_SCOPE')
            need(item['tradeId'] not in identities or identities[item['tradeId']] == item, 'FLOW_CONFLICT')
            need(item['tradeId'] not in ids, 'FLOW_DUPLICATE_ID'); ids.add(item['tradeId'])
            identities[item['tradeId']] = item
            need(integer(item['receivedMs']) <= integer(item['availableMs']) <= s['asOfMs'] and
                 s['asOfMs']-int(h) < integer(item['sourceTsMs']) <= s['asOfMs'] and item['sourceTsMs']-item['receivedMs'] <= 1000, 'FLOW_CLOCK')
            hash_field(item['bodySha256'])
            need(item['direction'] is None or type(item['direction']) is int and item['direction'] in (-1, 1), 'FLOW_DIRECTION')
            need(decimal(item['quantity']) > 0 and decimal(item['yesPrice'])+decimal(item['noPrice']) == 1, 'FLOW_PRICE')
            ordered.append({k: item[k] for k in FLOW_FIELDS})
            found |= item == r and w['evidenceComplete'] and w['missingExecutionClockCount'] == 0
        if w['evidenceComplete']:
            # Exporter evidence hashes use FLOW_FIELDS order, unlike observer compact.
            need(sha(json.dumps(ordered, separators=(',', ':'), ensure_ascii=False, allow_nan=False).encode()) == w['evidenceSha256'], 'FLOW_EVIDENCE_HASH')
            qty = sum((decimal(x['quantity']) for x in evidence), Decimal(0))
            signed = sum((decimal(x['quantity'])*x['direction'] for x in evidence if x['direction'] is not None), Decimal(0))
            unknown = sum((decimal(x['quantity']) for x in evidence if x['direction'] is None), Decimal(0))
            need(decimal(w['observedQuantity']) == qty and decimal(w['knownSignedQuantity'], True) == signed and decimal(w['unknownDirectionQuantity']) == unknown, 'FLOW_TOTALS')
    need(found, 'TRIGGER_NOT_IN_COMPLETE_ORIGINAL_EVIDENCE')


def decision(row, cutoff):
    """Identity/source checks precede representative selection, not completeness."""
    d = integer(row['decisionMs'])
    need(d == clock(row['decisionAt']) and d <= cutoff, 'DECISION_CLOCK')
    need(row['samplingPolicySha256'] == POLICY_SHA and sha(compact(row['samplingPolicy']).encode()) == POLICY_SHA, 'SAMPLING_POLICY')
    need(row['horizonsMs'] == list(HORIZONS) and row['pricingAuthority'] is False and row['executionAuthority'] is False, 'DECISION_AUTHORITY')
    for k in ('marketId', 'exchangeId'):
        need(isinstance(row[k], str) and re.fullmatch(r'[1-9]\d*', row[k]), 'SCOPE')
    t, s, e = row['trigger'], row['sig'], row['external']
    need(t['kind'] == 'first_observed_public_trade_v1' and t['resolutionEquivalenceVerified'] is False, 'TRIGGER_KIND')
    r = t['record']
    need(all(isinstance(r[k], str) and 0 < len(r[k]) <= 256 for k in ('tradeId', 'ticker')), 'TRIGGER_ID')
    need(type(r['direction']) is int and r['direction'] in (-1, 1) and type(r['isBlockTrade']) in (bool, type(None)), 'TRIGGER_DIRECTION')
    need(decimal(r['quantity']) > 0 and decimal(r['yesPrice'])+decimal(r['noPrice']) == 1 and 0 <= decimal(r['yesPrice']) <= 1, 'TRIGGER_ECONOMICS')
    source, received, available = (integer(r[k]) for k in ('sourceTsMs', 'receivedMs', 'availableMs'))
    export, first, observed = (integer(t[k]) for k in ('sourceExportMs', 'observerFirstAvailableMs', 'firstObservedAtMs'))
    need(source <= export <= first <= observed == d and received <= available <= export and source-received <= 1000 and source-available <= 1000, 'TRIGGER_CLOCK')
    need(d-source <= 2000 and d-received <= 2000 and d-export <= 2000, 'TRIGGER_STALE')
    need(integer(r['sid']) > 0 and (r['sequence'] is None or integer(r['sequence']) >= 0) and integer(r['streamEpoch']) > 0, 'TRIGGER_CURSOR')
    for x in (r['bodySha256'], t['sourceSnapshotSha256'], s['journalHash'], e['bodySha256']):
        hash_field(x)
    ss, sr = clock(s['sourceAt']), clock(s['receivedAt'])
    need(ss <= sr <= integer(t['sigBookObservedMs']) <= d and d-ss <= 3000 and d-sr <= 3000, 'SIG_CLOCK')
    integer(s['journalSequence'])
    er, ea = integer(e['receivedMs']), integer(e['availableMs'])
    need(er <= ea <= d and d-er <= 20000 and e['ticker'] == r['ticker'], 'EXTERNAL_CLOCK_SCOPE')
    need(e['source'] in ('kalshi_official_ws_v2', 'kalshi_official_v2') and e['pricingAuthority'] is False, 'EXTERNAL_SOURCE')
    if e['source'] == 'kalshi_official_ws_v2':
        need(integer(e['sid']) > 0, 'EXTERNAL_CURSOR')
        integer(e['sequence'])
    es = e.get('sourceTsMs')
    need(es is None or 0 <= d-integer(es) <= 20000 and es-er <= 1000 and es <= ea, 'EXTERNAL_SOURCE_CLOCK')
    book(s['features']); book(e['features'])
    flow_evidence(row)
    base = sha(f"{row['marketId']}:{row['exchangeId']}:{s['journalHash']}:{e['bodySha256']}:{d}".encode())
    need(row['sampleId'] == sha(compact({'policy': POLICY_SHA, 'trigger': t, 'bookSampleId': base}).encode()), 'SAMPLE_BINDING')
    economic = tuple(decimal(r[k]) if k in ('quantity', 'yesPrice', 'noPrice') else r[k] for k in ECONOMIC)
    return (r['ticker'], r['tradeId']), economic


def features(row):
    """Exact causal anchor validation; None stays None for common-case filtering."""
    at, ext, changes, anchors = row['decisionMs'], row['external'], row['externalLogitChanges'], row['externalLagAnchors']
    need(set(changes) == set(anchors) == {'1000', '5000', '30000'}, 'ANCHOR_SHAPE')
    values = []
    for lag in (1000, 5000, 30000):
        a, v = anchors[str(lag)], changes[str(lag)]
        if a is None:
            need(v is None, 'ANCHOR_MISSING')
            values.append(None)
            continue
        need(v is not None, 'ANCHOR_MISSING')
        hash_field(a['bodySha256'])
        received, available = integer(a['receivedMs']), integer(a['availableMs'])
        need(received <= available <= at and received <= ext['receivedMs'] and received <= at-lag and at-lag-received <= 20000, 'ANCHOR_CLOCK')
        need(all(a.get(k) == ext.get(k) for k in ('source', 'ticker', 'mappingKind', 'sid', 'wsEpoch', 'streamBoundary')), 'ANCHOR_SOURCE')
        if ext['source'] == 'kalshi_official_ws_v2':
            need(integer(a['sequence']) <= ext['sequence'], 'ANCHOR_CURSOR')
        source = a.get('sourceTsMs')
        need(source is None or 0 <= integer(source) <= available and source-received <= 1000, 'ANCHOR_SOURCE_CLOCK')
        midpoint = numeric(a['midpoint'])
        need(0 <= midpoint <= 1 and numeric(v) == logit(ext['features']['midpoint'])-logit(midpoint), 'ANCHOR_DELTA')
        values.append(numeric(v))
    sig = row['sig']['features']
    sm, em = numeric(sig['midpoint']), numeric(ext['features']['midpoint'])
    need(0 < sm < 1 and 0 < em < 1, 'FEATURE_MIDPOINT')
    r = row['trigger']['record']
    return values + [numeric(ext['features']['imbalance']), numeric(sig['imbalance']), numeric(sig['spread']),
                     math.log(em/(1-em))-math.log(sm/(1-sm)), r['direction'],
                     r['direction']*math.log1p(float(decimal(r['quantity']))), (at-r['sourceTsMs'])/1000]


def label(sample, row, horizon, cutoff):
    need(row['sampleId'] == sample['sampleId'] and row['horizonMs'] == horizon and row['samplingPolicySha256'] == POLICY_SHA and row['samplingMode'] == 'first_observed_public_trade_v1', 'LABEL_BINDING')
    start, end = sample['decisionMs']+horizon, sample['decisionMs']+horizon+3000
    source, received, observed = (clock(row[k]) for k in ('sourceAt', 'receivedAt', 'observedAt'))
    need(start <= source <= received <= observed <= cutoff and received <= end, 'LABEL_CLOCK')
    need(row['counterfactual'] is True and row['actualFill'] is False and decimal(row['feesAssumed']) == 0, 'LABEL_AUTHORITY')
    hash_field(row['sigJournalHash'])
    b, a, bq, aq = book(sample['sig']['features'])
    fb, fa, fbq, faq = book(row['features'])
    need(numeric(row['logitMidpointChange']) == logit(row['features']['midpoint'])-logit(sample['sig']['features']['midpoint']), 'LABEL_MIDPOINT_CONFLICT')
    yes, no = fb-a, b-fa
    # A null payoff with insufficient original capacity is missing, never zero.
    for key, value, capacity in (('yesOneShareTakerRoundTrip', yes, min(aq, fbq)), ('noOneShareTakerRoundTrip', no, min(bq, faq))):
        need(row[key] is None if capacity < 1 else row[key] is not None and decimal(row[key], True) == value, 'LABEL_RETURN_CONFLICT')
    if min(bq, aq, fbq, faq) < 1:
        return None
    return {'yes': yes-Decimal('.005'), 'no': no-Decimal('.005'), 'available': observed}


def signature(path):
    s = path.lstat()
    need(stat.S_ISREG(s.st_mode) and not path.is_symlink(), 'INPUT_FILE')
    return s.st_dev, s.st_ino, s.st_size, s.st_mtime_ns


def verify_file(base, spec, limit):
    need(set(spec) == {'path', 'bytes', 'sha256'}, 'FILE_SPEC')
    p = (base / spec['path']).resolve()
    need(isinstance(spec['path'], str) and integer(spec['bytes']) <= limit, 'FILE_SPEC')
    before = signature(p)
    need(before[2] == spec['bytes'], 'FILE_SIZE')
    h = hashlib.sha256()
    with p.open('rb') as f:
        while chunk := f.read(1024*1024):
            h.update(chunk)
    need(h.hexdigest() == hash_field(spec['sha256']) and signature(p) == before, 'FILE_HASH_OR_CHANGED')
    return p, before


def load(manifest_path, expected_sha):
    p = Path(manifest_path).resolve()
    need(signature(p)[2] <= MAX_LINE, 'MANIFEST_BOUNDS')
    original = p.read_bytes()
    need(sha(original) == hash_field(expected_sha), 'MANIFEST_HASH')
    m = decode(original)
    need(m['schemaVersion'] == 1 and m['sealed'] is True and m['analysisProtocolSha256'] == PROTOCOL_SHA, 'MANIFEST_POLICY')
    cutoff = clock(m['asOfUtc'])
    need(isinstance(m['cohorts'], list) and 0 < len(m['cohorts']) <= 128, 'COHORT_BOUNDS')
    files, seen, cohort_ids = [], set(), set()
    for c in m['cohorts']:
        need(isinstance(c['cohortId'], str) and c['cohortId'] and c['cohortId'] not in cohort_ids, 'COHORT_ID')
        cohort_ids.add(c['cohortId'])
        need(c['samplingPolicySha256'] == POLICY_SHA, 'MANIFEST_SAMPLING_POLICY')
        hash_field(c['configSha256']); hash_field(c['mappingSha256'])
        producer, _ = verify_file(p.parent, c['producer'], MAX_LINE)
        need(producer.is_file() and isinstance(c['files'], list) and 0 < len(c['files']) <= 128, 'COHORT_FILES')
        for f in c['files']:
            path, sig = verify_file(p.parent, f, MAX_BYTES)
            need(path not in seen and len(files) < 128, 'DUPLICATE_OR_TOO_MANY_FILES')
            seen.add(path); files.append((path, sig, c, f))
    need(sum(f[1][2] for f in files) <= MAX_BYTES, 'INPUT_BOUNDS')
    # All original bytes/producers are verified BEFORE any outcomes are decoded.
    groups, fingerprints, conflicts, labels, provenance = defaultdict(list), {}, set(), {}, []
    original_samples = {}
    counts, total = Counter(), 0
    for path, sig, c, spec in files:
        file_line = 0
        with gzip.open(path, 'rb') if path.suffix == '.gz' else path.open('rb') as stream:
            while line := stream.readline(MAX_LINE+1):
                counts['rows'] += 1; total += len(line); file_line += 1
                need(len(line) <= MAX_LINE and line.endswith(b'\n') and total <= MAX_BYTES and counts['rows'] <= MAX_ROWS, 'INPUT_BOUNDS')
                r = decode(line)
                need(isinstance(r, dict), 'ROW')
                kind = r.get('kind')
                if kind == 'decision_features':
                    key, fingerprint = decision(r, cutoff)
                    original_key = c['cohortId'], r['sampleId']
                    need(original_key not in original_samples or original_samples[original_key] == r, 'CONFLICTING_SAMPLE_ID')
                    original_samples[original_key] = r
                    if key in fingerprints and fingerprint != fingerprints[key]:
                        conflicts.add(key)
                    fingerprints[key] = fingerprint
                    meta = {'cohort': c['cohortId'], 'producerSha256': c['producer']['sha256'], 'fileSha256': spec['sha256'], 'line': file_line, 'sampleId': r['sampleId']}
                    order = (r['decisionMs'], int(r['marketId']), int(r['exchangeId']), POLICY_SHA,
                             meta['producerSha256'], meta['cohort'], meta['fileSha256'], meta['line'], r['sampleId'])
                    groups[key].append({'sample': r, 'meta': meta, 'order': order, 'key': key})
                elif kind == 'horizon_label':
                    hash_field(r['sampleId']); need(type(r['horizonMs']) is int and r['horizonMs'] in HORIZONS, 'HORIZON')
                    need(clock(r['observedAt']) <= cutoff, 'LABEL_AFTER_CUTOFF')
                    key = c['cohortId'], r['sampleId'], r['horizonMs']
                    need(key not in labels or labels[key] == r, 'CONFLICTING_LABEL')
                    labels[key] = r
                else:
                    counts['ignoredOtherRecords'] += 1
        need(signature(path) == sig, 'INPUT_CHANGED')
    representatives = []
    for key, aliases in groups.items():
        provenance.append({'ticker': key[0], 'tradeId': key[1], 'conflict': key in conflicts,
                           'occurrences': [a['meta'] for a in sorted(aliases, key=lambda x: x['order'])]})
        if key not in conflicts:
            representatives.append(min(aliases, key=lambda x: x['order']))
    representatives.sort(key=lambda x: x['order'])
    # Validate every matched original label, even if its representative later has
    # missing features/overlap. Invalid evidence must not disappear via filtering.
    for (cohort, sid, h), row in labels.items():
        original_sample = original_samples.get((cohort, sid))
        if original_sample is not None:
            label(original_sample, row, h, cutoff)
    counts.update(globalEconomicEvents=len(groups), economicConflicts=len(conflicts), canonicalRepresentatives=len(representatives),
                  duplicateAliases=sum(len(v)-1 for v in groups.values()))
    emitted_ids = {(a['meta']['cohort'], a['sample']['sampleId']) for aliases in groups.values() for a in aliases}
    counts['unmatchedLabels'] = sum((cohort, sid) not in emitted_ids for cohort, sid, _ in labels)
    return m, cutoff, representatives, labels, counts, provenance


def examples(representatives, labels, horizon, cutoff):
    rows, counts, next_time = [], Counter(), {}
    for rep in representatives:
        r, ticker, at = rep['sample'], rep['key'][0], rep['sample']['decisionMs']
        if at < next_time.get(ticker, -1):
            counts['overlappingTickerWindow'] += 1
            continue
        next_time[ticker] = at+max(1000, horizon+3000)
        counts['canonicalTimeClusters'] += 1
        if r['external']['mappingKind'] != 'direct_party_reference':
            counts['excludedOtherMapping'] += 1
            continue
        x = features(r)
        if any(v is None for v in x):
            counts['missingTenFeatures'] += 1
            continue
        l = labels.get((rep['meta']['cohort'], r['sampleId'], horizon))
        if l is None:
            counts['missingLabel'] += 1
            continue
        y = label(r, l, horizon, cutoff)
        if y is None:
            counts['insufficientOneShareDepth'] += 1
            continue
        b, a, _, _ = book(r['sig']['features'])
        rows.append({'id': r['sampleId'], 'ticker': ticker, 'market': r['marketId'], 'exchange': r['exchangeId'], 'at': at,
                     'end': at+horizon+3000, 'available': y['available'], 'x': x, 'yes': y['yes'], 'no': y['no'],
                     'bbo': -(a-b)-Decimal('.005')})
    counts['commonCompleteClusters'] = len(rows)
    return rows, dict(counts)


def solve(a, b):
    a = [list(r)+[v] for r, v in zip(a, b)]
    for i in range(len(b)):
        pivot = max(range(i, len(b)), key=lambda k: abs(a[k][i]))
        need(abs(a[pivot][i]) > 1e-12, 'SINGULAR_MODEL')
        a[i], a[pivot] = a[pivot], a[i]
        scale = a[i][i]; a[i] = [x/scale for x in a[i]]
        for k in range(len(b)):
            if k != i:
                factor = a[k][i]; a[k] = [x-factor*y for x, y in zip(a[k], a[i])]
    answer = [r[-1] for r in a]
    need(all(math.isfinite(x) for x in answer), 'NONFINITE_FIT')
    return answer


def fit(train, columns, side):
    means = [sum(r['x'][j] for r in train)/len(train) for j in columns]
    scales = [max(1e-9, math.sqrt(sum((r['x'][j]-m)**2 for r in train)/len(train))) for j, m in zip(columns, means)]
    x = [[1.]+[(r['x'][j]-m)/s for j, m, s in zip(columns, means, scales)] for r in train]
    n = len(columns)+1
    matrix = [[sum(r[i]*r[j] for r in x)+(1. if i == j and i else 0.) for j in range(n)] for i in range(n)]
    rhs = [sum(r[i]*float(t[side]) for r, t in zip(x, train)) for i in range(n)]
    return {'columns': columns, 'means': means, 'scales': scales, 'coefficients': solve(matrix, rhs)}


def predict(model, x):
    z = [1.]+[(x[j]-m)/s for j, m, s in zip(model['columns'], model['means'], model['scales'])]
    value = sum(a*b for a, b in zip(z, model['coefficients']))
    need(math.isfinite(value), 'NONFINITE_PREDICTION')
    return value


def metrics(scored):
    chosen = [r for r in scored if max(r['predictions']) > 0]
    baseline = [r for r in scored if max(r['intercept']) > 0]
    side = lambda x: 0 if x[0] >= x[1] else 1
    net = sum((r['outcomes'][side(r['predictions'])] for r in chosen), Decimal(0))
    base_net = sum((r['outcomes'][side(r['intercept'])] for r in baseline), Decimal(0))
    mse = lambda key, i: sum((float(r['outcomes'][i])-r[key][i])**2 for r in scored)/len(scored) if scored else None
    paired = lambda key: sum((float(r['outcomes'][i])-r[key][i])**2 for r in scored for i in (0, 1))/(2*len(scored)) if scored else None
    model_mse, base_mse = paired('predictions'), paired('intercept')
    coverage = len(scored) >= 100 and len(chosen) >= 100
    positive = net > 0
    better = model_mse is not None and model_mse < base_mse and net > base_net
    return {'outOfSampleClusters': len(scored), 'selectedClusters': len(chosen), 'selectedTickers': len({r['ticker'] for r in chosen}),
            'selectedNetCrossingSumExact': str(net), 'selectedNetCrossingMeanExact': str(net/len(chosen)) if chosen else None,
            'interceptSelectedClusters': len(baseline), 'interceptSelectedNetCrossingSumExact': str(base_net),
            'pairedMse': model_mse, 'interceptPairedMse': base_mse,
            'perSideMse': {s: mse('predictions', i) for i, s in enumerate(('yes', 'no'))},
            'perSideInterceptMse': {s: mse('intercept', i) for i, s in enumerate(('yes', 'no'))},
            'outOfSampleIdsSha256': digest([r['id'] for r in scored]), 'selectedIdsSha256': digest([r['id'] for r in chosen]),
            'selectedWindows': [{'sampleId': r['id'], 'ticker': r['ticker'], 'marketId': r['market'], 'startMs': r['at'], 'endExclusiveMs': r['end'], 'side': 'YES' if side(r['predictions']) == 0 else 'NO'} for r in chosen],
            'unselectedClusters': len(scored)-len(chosen), 'coverageThresholdsPass': coverage,
            'positiveSelectedEconomics': positive, 'beatsInterceptMseAndSelectedEconomics': better,
            'abstain': not (coverage and positive and better), 'status': 'UNQUALIFIED_COUNTERFACTUAL_RESEARCH',
            'abstentionReasons': [v for ok, v in ((len(scored) >= 100, 'FEWER_THAN_100_OOS'), (len(chosen) >= 100, 'FEWER_THAN_100_SELECTED'), (positive, 'NONPOSITIVE_NET'), (better, 'NO_INTERCEPT_IMPROVEMENT')) if not ok]}


def compare(rows, boundaries, cutoff):
    scored = {m: [] for m in MODEL_COLUMNS}; folds = []
    for i, start in enumerate(boundaries):
        stop = boundaries[i+1] if i < 2 else cutoff
        train = [r for r in rows if r['end'] < start and r['available'] <= start]
        test = [r for r in rows if start <= r['at'] < stop]
        f = {'startMs': start, 'endExclusiveMs': stop, 'trainingClusters': len(train), 'testClusters': len(test),
             'trainingIdsSha256': digest([r['id'] for r in train]), 'testIdsSha256': digest([r['id'] for r in test]), 'trained': False}
        folds.append(f)
        if len(train) < 16:
            f['abstentionReason'] = 'FEWER_THAN_16_TRAINING_CLUSTERS'; continue
        try:
            intercept = [sum(float(r[s]) for r in train)/len(train) for s in ('yes', 'no')]
            models = {m: [fit(train, cols, s) for s in ('yes', 'no')] for m, cols in MODEL_COLUMNS.items() if cols}
            batch = {m: [] for m in MODEL_COLUMNS}
            for r in test:
                for m in MODEL_COLUMNS:
                    pred = [predict(v, r['x']) for v in models[m]] if m in models else [float(r['bbo'])]*2
                    batch[m].append({**{k: r[k] for k in ('id', 'ticker', 'market', 'at', 'end')}, 'outcomes': [r['yes'], r['no']], 'predictions': pred, 'intercept': intercept})
        except (ValueError, OverflowError, ZeroDivisionError) as e:
            # All models lose this fold together; numerical failure is explicit.
            f['abstentionReason'] = 'NUMERICAL_FIT_FAILURE'; f['errorCode'] = str(e) if str(e).startswith('TRADE_ANALYSIS_') else type(e).__name__; continue
        for m in MODEL_COLUMNS:
            scored[m].extend(batch[m])
        f.update(trained=True, latestTrainingLabelAvailableMs=max(r['available'] for r in train), modelsResearchOnly=models)
    result = {m: metrics(v) for m, v in scored.items()}
    need(len({r['outOfSampleIdsSha256'] for r in result.values()}) == 1, 'COMMON_COHORT')
    full, price, local = (result[k] for k in ('price_plus_trigger_ridge', 'price_seven_ridge', 'sig_local_ridge'))
    difference = lambda a, b: a-b if a is not None and b is not None else None
    return {'folds': folds, 'models': result, 'triggerMinusPricePairedMse': difference(full['pairedMse'], price['pairedMse']),
            'triggerMinusLocalPairedMse': difference(full['pairedMse'], local['pairedMse']),
            'triggerMinusPriceSelectedNetExact': str(Decimal(full['selectedNetCrossingSumExact'])-Decimal(price['selectedNetCrossingSumExact']))}


def analyze(manifest_path, manifest_sha):
    protocol = Path(__file__).with_name('analysis-protocol.json').read_bytes()
    need(sha(protocol.replace(b'\r\n', b'\n')) == PROTOCOL_SHA, 'ANALYSIS_PROTOCOL_CHANGED')
    m, cutoff, reps, labels, counts, provenance = load(manifest_path, manifest_sha)
    times = sorted({r['sample']['decisionMs'] for r in reps})
    boundaries = [times[len(times)*i//4] for i in (1, 2, 3)] if len(times) >= 4 else []
    horizons = {}
    for h in HORIZONS:
        rows, data = examples(reps, labels, h, cutoff)
        horizons[str(h)] = {**compare(rows, boundaries, cutoff), 'coverage': data, 'commonCompleteIdsSha256': digest([r['id'] for r in rows])}
    variation = {name: {'available': 0, 'missing': 0, 'nonzero': 0, 'sumAbsolute': 0., 'maxAbsolute': None} for name in FEATURE_NAMES}
    for r in reps:
        x = features(r['sample'])
        for i, v in enumerate(x):
            d = variation[FEATURE_NAMES[i]]
            if v is None: d['missing'] += 1
            else:
                d['available'] += 1; d['nonzero'] += v != 0; d['sumAbsolute'] += abs(v)
                d['maxAbsolute'] = max(d['maxAbsolute'] or 0., abs(v))
    return {'schemaVersion': 1, 'status': 'UNQUALIFIED_COUNTERFACTUAL_RESEARCH', 'analysisProtocolSha256': PROTOCOL_SHA,
            'samplingPolicySha256': POLICY_SHA, 'sourceCodeSha256': sha(Path(__file__).read_bytes()), 'manifestSha256': manifest_sha,
            'sourceManifest': m, 'asOfMs': cutoff, 'input': dict(counts), 'calendarBoundariesMs': boundaries,
            'calendarAbstention': 'FEWER_THAN_FOUR_UNIQUE_DECISION_TIMES' if not boundaries else None,
            'aliasProvenance': provenance, 'featureVariationCanonicalRepresentatives': variation,
            'blockTradeRepresentatives': dict(Counter(str(r['sample']['trigger']['record']['isBlockTrade']) for r in reps)),
            'horizons': horizons, 'rawSourceWitnessesIndependentlyVerified': False,
            'sourceCaveat': 'Original producer/data bytes and emitted references/causality verified; underlying SIG/trade bodies and first-later-book completeness require separately retained original journals.',
            'selectionCaveat': 'Known-direction poll-visible complete cases are selective; ticker/time clusters remain correlated across elections. No observed fill, queue probability or actual PnL is inferred.',
            'modelApproval': False, 'actualProfitEstablished': False, 'executionAuthority': False}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--manifest', required=True, type=Path)
    p.add_argument('--manifest-sha256', required=True)
    p.add_argument('--output', required=True, type=Path)
    a = p.parse_args()
    result = analyze(a.manifest, a.manifest_sha256)
    with a.output.open('x', encoding='utf-8', newline='\n') as f:
        f.write(json.dumps(result, sort_keys=True, separators=(',', ':'), allow_nan=False)+'\n')


if __name__ == '__main__':
    main()
