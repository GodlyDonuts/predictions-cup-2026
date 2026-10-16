"""Offline, fixed-rule passive-quote markouts. Never a maker-fill/PnL estimate."""
import argparse
from collections import Counter
from decimal import Decimal
import hashlib
from pathlib import Path
from crossvenue_economics import (load, clock, canonical, digest, require, money,
                                 signed_money, HEX)

HORIZONS = (1000, 3000, 10000, 30000)
POLICY = {
    'version': 1, 'innovationThresholds': {'1000': '0.002', '5000': '0.005', '30000': '0.010'},
    'maximumSigSourceAgeMs': 10000, 'maximumExternalAvailabilityAgeMs': 30000,
    'maximumExternalOriginalAgeMs': 20000,
    'maximumSpread': '0.050000', 'minimumClusters': 100,
    'rule': 'all three signed innovations meet thresholds: favorable; opposite: toxic; otherwise quiet',
    'quote': 'YES BUY at original bid; NO BUY at one minus original ask',
    'fillAssumption': 'hypothetical one unit at resting price; no fill or queue probability inferred',
}


def feature_state(row, sample_id_format):
    """Causal decision only; future labels never enter classification."""
    require(sample_id_format in ('scoped_v2', 'legacy_unscoped_v1'), 'PRODUCER_FORMAT')
    at, sig, ext = row['decisionMs'], row['sig'], row['external']
    source, received = clock(sig['sourceAt']), clock(sig['receivedAt'])
    require(source <= received <= at, 'FEATURE_LOOKAHEAD')
    require(type(ext.get('receivedMs')) is int and type(ext.get('availableMs')) is int and
            0 <= ext['receivedMs'] <= ext['availableMs'] <= at, 'FEATURE_LOOKAHEAD')
    # Existing producer emits receipt/availability only. If a later original
    # producer retains sourceTs, it must also remain causal and independently old.
    source_ts = ext.get('sourceTs')
    if source_ts is not None:
        require(type(source_ts) is int and 0 <= source_ts <= ext['receivedMs'], 'FEATURE_LOOKAHEAD')
    require(all(isinstance(h, str) and HEX.fullmatch(h) for h in (sig['journalHash'], ext['bodySha256'])), 'FEATURE_SOURCE_HASH')
    prefix = f"{row['marketId']}:{row['exchangeId']}:" if sample_id_format == 'scoped_v2' else ''
    require(row['sampleId'] == hashlib.sha256(f"{prefix}{sig['journalHash']}:{ext['bodySha256']}:{at}".encode()).hexdigest(), 'SAMPLE_BINDING')
    before = sig['features']
    bid, ask = money(before['bid']), money(before['ask'])
    require(0 <= bid <= ask <= 1, 'BOOK_PRICES')
    if (at-source > POLICY['maximumSigSourceAgeMs'] or
            at-ext['receivedMs'] > POLICY['maximumExternalOriginalAgeMs'] or
            (source_ts is not None and at-source_ts > POLICY['maximumExternalOriginalAgeMs']) or
            at-ext['availableMs'] > POLICY['maximumExternalAvailabilityAgeMs']):
        return None, 'staleSource'
    if not Decimal(0) < ask-bid <= Decimal(POLICY['maximumSpread']):
        return None, 'spreadIneligible'
    innovations = row['externalLogitChanges']
    if any(innovations.get(h) is None for h in POLICY['innovationThresholds']):
        return None, 'missingInnovation'
    values = []
    for h, threshold in POLICY['innovationThresholds'].items():
        value = innovations[h]
        require(type(value) in (int, float), 'INNOVATION')
        value = Decimal(str(value))
        require(value.is_finite(), 'INNOVATION')
        values.append((value, Decimal(threshold)))
    direction = 1 if all(v >= t for v, t in values) else -1 if all(v <= -t for v, t in values) else 0
    return {'YES': 'FAVORABLE' if direction == 1 else 'TOXIC' if direction == -1 else 'QUIET',
            'NO': 'FAVORABLE' if direction == -1 else 'TOXIC' if direction == 1 else 'QUIET'}, None


def matched(decisions, labels, horizon, as_of, slack, fee, buffer, sample_id_format):
    rows, counts = [], Counter()
    for sid, row in sorted(decisions.items(), key=lambda item: (item[1]['decisionMs'], item[0])):
        counts['decisions'] += 1
        if row['external'].get('mappingKind') != 'direct_party_reference':
            counts['otherMapping'] += 1
            continue
        states, reason = feature_state(row, sample_id_format)
        if reason:
            counts[reason] += 1
            continue
        counts['causalAvailableDecisions'] += 1
        label = labels.get((sid, horizon))
        if label is None:
            counts['missingLabel'] += 1
            for state in states.values():
                counts['missingLabel_' + state] += 1
            continue
        target = row['decisionMs'] + horizon
        source, received, observed = map(clock, (label['sourceAt'], label['receivedAt'], label['observedAt']))
        require(target <= source <= received <= observed <= as_of and source <= target+slack and received <= target+slack, 'LABEL_LOOKAHEAD_OR_WINDOW')
        require(label.get('counterfactual') is True and label.get('actualFill') is False and
                isinstance(label.get('sigJournalHash'), str) and HEX.fullmatch(label['sigJournalHash']), 'LABEL_PROVENANCE')
        require(signed_money(label['feesAssumed']) == 0, 'LABEL_FEE_ASSUMPTION')
        before, after = row['sig']['features'], label['features']
        bid, ask, future_bid, future_ask = map(money, (before['bid'], before['ask'], after['bid'], after['ask']))
        require(0 <= bid <= ask <= 1 and 0 <= future_bid <= future_ask <= 1, 'BOOK_PRICES')
        # Future BBO supports the counterfactual one-unit liquidation only.
        # Original bid/ask quantity is NOT evidence the hypothetical quote filled.
        if money(after['bidQuantity']) < 1 or money(after['askQuantity']) < 1:
            counts['missingOneShareLiquidationCapacity'] += 1
            continue
        for field, expected in [('yesOneShareTakerRoundTrip', future_bid-ask), ('noOneShareTakerRoundTrip', bid-future_ask)]:
            require(label.get(field) is not None and signed_money(label[field]) == expected, 'LABEL_RETURN_CONFLICT')
        rows.append({'id': sid, 'market': str(row['marketId']), 'at': row['decisionMs'],
                     'end': target+slack, 'states': states,
                     'YES': future_bid-bid-fee-buffer, 'NO': ask-future_ask-fee-buffer,
                     'featureSha256': digest(row)})
    counts['matchedOpportunities'] = len(rows)
    selected, ends = [], {}
    for row in rows:
        if row['at'] < ends.get(row['market'], -1):
            counts['overlappingMarketTimeRows'] += 1
            continue
        selected.append(row)
        ends[row['market']] = row['end']
    counts['nonoverlappingMarketTimeClusters'] = len(selected)
    return selected, dict(counts), rows


def summary(rows, side):
    def group(items):
        values = [r[side] for r in items]
        return {'count': len(values), 'negativeCount': sum(v < 0 for v in values),
                'netExecutableMarkoutSum': str(sum(values, Decimal(0))),
                'adverseMagnitudeSum': str(sum((-v for v in values if v < 0), Decimal(0)))}
    baseline = group(rows)
    states = {state: group([r for r in rows if r['states'][side] == state]) for state in ('QUIET', 'TOXIC', 'FAVORABLE')}
    skipped = [r for r in rows if r['states'][side] == 'TOXIC']
    return {'allMatchedControl': baseline, 'states': states,
            'defensiveRetained': group([r for r in rows if r['states'][side] != 'TOXIC']),
            'avoidedAdverseCounterfactualMagnitude': str(sum((-r[side] for r in skipped if r[side] < 0), Decimal(0))),
            'missedPositiveCounterfactualMagnitude': str(sum((r[side] for r in skipped if r[side] > 0), Decimal(0)))}


def report(paths, as_of, *, sample_id_format='scoped_v2', producer_source=None, producer_sha256=None,
           fee=Decimal(0), buffer=Decimal('0.005'), slack=3000):
    require(isinstance(fee, Decimal) and isinstance(buffer, Decimal) and fee.is_finite() and buffer.is_finite() and fee >= 0 and buffer >= 0, 'COSTS')
    require(type(slack) is int and 0 <= slack <= 3000, 'LABEL_SLACK')
    producer = None
    if producer_source is not None:
        path = Path(producer_source)
        require(path.is_file() and not path.is_symlink() and path.stat().st_size <= 4*1024*1024, 'PRODUCER_SOURCE')
        raw = path.read_bytes()
        require(isinstance(producer_sha256, str) and HEX.fullmatch(producer_sha256) and hashlib.sha256(raw).hexdigest() == producer_sha256, 'PRODUCER_SOURCE_HASH')
        producer = {'name': path.name, 'sha256': producer_sha256, 'bytes': len(raw)}
    else:
        require(producer_sha256 is None, 'PRODUCER_SOURCE')
    decisions, labels, files, counts = load(paths, as_of)
    policy = {**POLICY, 'sampleIdFormat': sample_id_format, 'feePerShare': str(fee), 'bufferPerShare': str(buffer), 'labelSlackMs': slack}
    horizons = {}
    for h in HORIZONS:
        rows, data, raw_rows = matched(decisions, labels, h, as_of, slack, fee, buffer, sample_id_format)
        enough = len(rows) >= POLICY['minimumClusters']
        horizons[str(h)] = {'data': data, 'YES': summary(rows, 'YES'), 'NO': summary(rows, 'NO'),
                            'allMatchedBeforeClustering': {'YES': summary(raw_rows, 'YES'), 'NO': summary(raw_rows, 'NO')},
                            'cohortSha256': digest([[r['id'], r['featureSha256']] for r in rows]),
                            'abstain': True, 'researchCoverageSufficient': enough,
                            'reason': 'MAKER_FILL_QUALIFICATION_UNAVAILABLE' if enough else 'INSUFFICIENT_MARKET_TIME_CLUSTERS'}
    return {'schemaVersion': 1, 'status': 'UNQUALIFIED_SHADOW_RESEARCH', 'asOfMs': as_of,
            'sourceCodeSha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
            'makerFillQualificationAvailable': False, 'modelApproval': False, 'profitEstablished': False,
            'producerSourceVerified': producer is not None, 'producerSource': producer,
            'input': dict(counts), 'sources': files, 'sourceBundleSha256': digest(files),
            'policy': policy, 'cutoffPolicySha256': digest({'asOfMs': as_of, 'policy': policy, 'producer': producer}),
            'horizons': horizons, 'limitations': [
                'Hypothetical original resting prices are not observed fills; no queue position, fill probability or realized PnL.',
                'Only passive YES/NO acquisitions are evaluated; covered passive SELL strategies require a separate inventory-conditioned study.',
                'Future executable liquidation marks are conditional on hypothetical acquisition, not expected maker returns.',
                'Market/time nonoverlap does not remove shared-election, temporal or cross-market correlation.',
                'Missing labels and source availability can be selective; matched control and missing-state counts are reported.',
                'Fixed rules were declared before this analysis, not fitted or calibrated; no causal treatment effect is established.']}


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
    a = p.parse_args()
    result = report(a.input, clock(a.as_of), sample_id_format=a.sample_id_format, producer_source=a.producer_source,
                    producer_sha256=a.producer_sha256, fee=signed_money(a.fee_per_share), buffer=signed_money(a.buffer_per_share))
    with a.output.open('x', encoding='utf-8') as f:
        f.write(canonical(result)+'\n')


if __name__ == '__main__':
    main()
