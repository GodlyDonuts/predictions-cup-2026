"""Frozen prospective metadata audit. Offline, no prices/results/model authority.

The fixed original verifier independently replays sealed outputs before this
metadata-only pass. Its equality check necessarily parses those outputs; this
module never selects, reports or evaluates their economic values.
"""
import argparse
import base64
from collections import Counter, defaultdict
import datetime as dt
import hashlib
import json
from pathlib import Path
import time
import types

HERE = Path(__file__).resolve().parent
PROTOCOL_SHA = '153f93fa21ac935d3aea2aa65c6733326f1d47e678594e30d4b63cd2fd2dce8a'
REPLAY_LF_SHA = 'a51f89856d4848d4bc576d8aa15e0e51db942513db21fd8dac8a09d53702d57b'
PRODUCER_SHA = '2fa9b142b672f4a9076e185ac91be0b288945b0e1bba6afb0797594ed6678529'


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def need(ok, code):
    if not ok:
        raise ValueError('COVERAGE_' + code)


def checked_replay():
    path = HERE.parent / 'moloch-trade-trigger-20261016/witness/trace_replay.py'
    raw = path.read_bytes()
    # Fixed repository source only. No manifest-selected code, import cache or
    # general source normalization: admit exactly reviewed LF or full CRLF.
    lf = raw.replace(b'\r\n', b'\n')
    need(len(raw) < 1024**2 and sha(lf) == REPLAY_LF_SHA and
         raw in (lf, lf.replace(b'\n', b'\r\n')), 'FIXED_VERIFIER_SOURCE')
    module = types.ModuleType('_fixed_coverage_input_replay')
    module.__file__ = str(path)
    exec(compile(raw, str(path), 'exec'), module.__dict__)
    return module


def clock(value):
    parsed = dt.datetime.fromisoformat(value.replace('Z', '+00:00'))
    need(parsed.tzinfo is not None, 'CLOCK')
    return int(parsed.timestamp() * 1000)


def _now_ms():
    return time.time_ns() // 1_000_000


def age_band(age):
    for upper in (3000, 10000, 60000, 300000):
        if age <= upper:
            return str(upper)
    return 'over300000'


def study_class(module):
    class CoverageStudy(module.Study):
        """Reuse genuine validation, never invoke _sample or economic labels."""
        def __init__(self, protocol, mapping, start, end):
            super().__init__(lambda _: need(False, 'UNEXPECTED_ECONOMIC_OUTPUT'), sampling_mode='trade')
            self.p = protocol
            self.start, self.end = start, end
            self.cutoff = clock(protocol['enrollment']['maximumMetadataObservationUtc'])
            self.unit_cap = protocol['enrollment']['maximumValidMappedTradeUnits']
            self.unit_rows, self.units_by_key = [], defaultdict(list)
            self.seen, self.bindings, self.meta = {}, defaultdict(set), {}
            self.market_counts, self.exclusions, self.diagnostics = Counter(), Counter(), Counter()
            self.last_nomination = {}
            self.first_cap_at = None
            # The original published schema normally contains market mappings
            # only. Recognize an EXPLICIT sigExchangeId when actually present;
            # neither title nor future observed books can invent this field.
            for row in mapping.get('markets', []):
                for item in row.get('sigMappings', []):
                    if 'sigExchangeId' in item:
                        market, exchange = item.get('sigMarketId'), item['sigExchangeId']
                        need(isinstance(market, str) and market and isinstance(exchange, str) and exchange,
                             'EXPLICIT_EXCHANGE_BINDING')
                        self._bind(market, exchange, 'original_pinned_mapping')

        def _bind(self, market, exchange, witness):
            key = (market, exchange)
            if exchange not in self.bindings[market]:
                need(sum(map(len, self.bindings.values())) < self.p['enrollment']['maximumSigScopes'], 'SCOPE_BOUND')
                self.bindings[market].add(exchange)
                self.meta[key] = {'state': 'missing', 'bindingWitness': witness}

        def _enrolling(self, now):
            return self.start <= now < self.end and self.first_cap_at is None

        def _expire(self, key, now):
            keep = []
            for unit in self.units_by_key[key]:
                for horizon, outcome in unit['availability'].items():
                    if outcome['status'] == 'pending' and now > unit['decisionMs'] + int(horizon) + 3000:
                        outcome.update(status='missing', reason='no_timely_target_book')
                if unit['nextObservation']['status'] == 'pending' and now > unit['decisionMs'] + 123000:
                    unit['nextObservation'].update(status='missing', reason='no_fresh_next_observation')
                if any(v['status'] == 'pending' for v in unit['availability'].values()) or unit['nextObservation']['status'] == 'pending':
                    keep.append(unit)
            self.units_by_key[key] = keep

        def boundary(self, event):
            for key in list(self.meta):
                if event.get('marketId') and key[0] != str(event['marketId']):
                    continue
                if event.get('exchangeId') and key[1] != str(event['exchangeId']):
                    continue
                self.meta[key]['state'] = 'gapped'
                for unit in self.units_by_key[key]:
                    for outcome in unit['availability'].values():
                        if outcome['status'] == 'pending':
                            outcome.update(status='missing', reason='gap_crossing')
                    unit['nextObservation']['gapObserved'] = True

        def accept(self, row, now):
            event = row['event']
            if event.get('type') == 'gap' or event.get('type') == 'coverage' and event.get('complete') is not True:
                self.boundary(event)
                return super().accept(row, now)
            if event.get('type') != 'book':
                return super().accept(row, now)
            book = event['book']; key = (str(book['marketId']), str(book['exchangeId']))
            before = self.counts['sigBooks']
            super().accept(row, now)
            if self.counts['sigBooks'] == before:
                if key in self.meta and key not in self.sig_current:
                    self.meta[key]['state'] = 'invalid'
                return
            self._bind(*key, witness='earlier_original_sig_callback')
            need(not any(self.pending.values()), 'ECONOMIC_PENDING_FORBIDDEN')
            source, received = module.millis(book['sourceAt']), module.millis(book['receivedAt'])
            depth = self.last_source[key][2]
            depth_hash = sha(module.compact([[(format(p, '.6f'), format(q, '.6f')) for p, q in side] for side in depth]).encode())
            previous = self.meta[key]
            version = (int(book['sequence']), module.book_version_time(book['sourceAt']))
            if 'originalVersion' in previous:
                old = previous['originalVersion']
                old_version = (int(old['sequence']), module.book_version_time(old['at']))
                if previous['state'] == 'ordering_unknown':
                    return  # No original epoch witness can repair this here.
                if version < old_version:
                    self.boundary({'marketId': key[0], 'exchangeId': key[1]})
                    self.meta[key]['state'] = 'ordering_unknown'
                    for unit in self.units_by_key[key]:
                        if unit['nextObservation']['status'] == 'pending':
                            unit['nextObservation'].update(status='missing', reason='unprovable_book_ordering')
                    return  # A gap cannot make an old engine read new.
                if version == old_version:
                    if depth_hash != previous['depthSha256']:
                        self.boundary({'marketId': key[0], 'exchangeId': key[1]})
                        self.meta[key]['state'] = 'conflicted'
                    return  # Preserve earliest receipt/availability across gaps.
            self.meta[key] = dict(state='valid', bindingWitness=self.meta[key]['bindingWitness'],
                                  sourceMs=source, receivedMs=received, sourceAt=book['sourceAt'], receivedAt=book['receivedAt'],
                                  firstAvailableMs=now, journalHash=row['hash'], journalSequence=row['sequence'],
                                  depthSha256=depth_hash, originalVersion={'sequence': book['sequence'], 'at': book['sourceAt']})
            if 'requestStartedAt' in book:
                request = module.millis(book['requestStartedAt'])
                need(0 <= request <= received, 'OPTIONAL_ORIGINAL_REQUEST_CLOCK')
                self.meta[key]['requestStartedAt'] = book['requestStartedAt']
            self._expire(key, now)
            if not (0 <= now-source <= 3000 and 0 <= now-received <= 3000):
                return
            observation = {'sourceMs': source, 'receivedMs': received, 'firstAvailableMs': now,
                           'sourceAt': book['sourceAt'], 'receivedAt': book['receivedAt'],
                           'journalHash': row['hash'], 'journalSequence': row['sequence']}
            for unit in self.units_by_key[key]:
                if source > unit['decisionMs'] and received > unit['decisionMs'] and now > unit['decisionMs'] and unit['nextObservation']['status'] == 'pending':
                    unit['nextObservation'].update(status='available', delayMs=now-unit['decisionMs'], observation=observation)
                for horizon, outcome in unit['availability'].items():
                    target = unit['decisionMs'] + int(horizon)
                    if outcome['status'] == 'pending' and all(target <= t <= target+3000 for t in (source, received, now)):
                        outcome.update(status='available', observation=observation)

        def _trigger_trades(self, envelope, now):
            # Follow the original external validator/selection lifecycle. Only
            # target-state, spacing and pending filters move AFTER denominator.
            for row in envelope['markets']:
                for market in map(str, row['sigMarketIds']):
                    cached = self.external_flow.get(market)
                    if not cached:
                        if self.start <= now < self.end:
                            self.diagnostics['poll_market_no_valid_flow'] += 1
                        continue
                    snapshot = cached['sourceSnapshot']
                    if snapshot['ticker'] != row['ticker']:
                        continue
                    records = {}
                    for window in snapshot['windows'].values():
                        if window['evidenceComplete']:
                            for record in window['evidence']:
                                records[record['tradeId']] = record
                        elif self.start <= now < self.end:
                            self.diagnostics['partial_evidence_windows'] += 1
                    for record in sorted(records.values(), key=lambda r: (r['availableMs'], r['sourceTsMs'], r['tradeId'])):
                        identity = (market, record['ticker'], record['tradeId'])
                        fingerprint = sha(module.compact(record).encode())
                        if identity in self.seen:
                            need(self.seen[identity] == fingerprint, 'TRADE_IDENTITY_CONFLICT')
                            if self.start <= now < self.end:
                                self.diagnostics['duplicate_reobservations'] += 1
                            continue
                        need(len(self.seen) < self.p['enrollment']['maximumSeenTradeIdentities'], 'IDENTITY_BOUND')
                        self.seen[identity] = fingerprint
                        if not self._enrolling(now):
                            continue
                        reason = None
                        if row['mappingKind'] != 'direct_party_reference':
                            reason = 'mapping_not_direct_party_reference'
                        elif not (record['sourceTsMs'] <= now and record['receivedMs'] <= record['availableMs'] <= snapshot['asOfMs'] <= cached['availableMs'] <= now):
                            reason = 'future_or_reversed_trigger_clock'
                        elif any(now-t > 2000 for t in (record['sourceTsMs'], record['receivedMs'], snapshot['asOfMs'])):
                            reason = 'old_trigger_or_export'
                        elif record['direction'] is None:
                            reason = 'unknown_original_direction'
                        else:
                            ext = self._select_external(market, now)
                            if not ext or ext['ticker'] != record['ticker'] or ext['mappingKind'] != 'direct_party_reference':
                                reason = 'no_current_external_book'
                        if reason:
                            self.exclusions[reason] += 1
                            continue
                        self.market_counts['valid_market_trigger_observations'] += 1
                        exchanges = sorted(self.bindings.get(market, ()))
                        if not exchanges:
                            self.market_counts['target_exchange_witness_missing'] += 1
                            continue
                        for exchange in exchanges:
                            if len(self.unit_rows) >= self.unit_cap:
                                break
                            self._unit(market, exchange, record, fingerprint, cached, now)
                            if len(self.unit_rows) == self.unit_cap:
                                self.first_cap_at = now
                                self.cutoff = min(self.cutoff, now+123000)

        def _unit(self, market, exchange, record, fingerprint, cached, now):
            key = (market, exchange); self._expire(key, now)
            meta = self.meta[key].copy()
            valid = meta['state'] in ('valid', 'gapped') and 'sourceMs' in meta
            ages = ({'sourceAgeMs': now-meta['sourceMs'], 'receiptAgeMs': now-meta['receivedMs'],
                     'availabilityAgeMs': now-meta['firstAvailableMs'], 'sourceToReceiptMs': meta['receivedMs']-meta['sourceMs']} if valid else None)
            unit = {'unitHash': sha(module.compact([market, exchange, fingerprint]).encode()),
                    'marketId': market, 'exchangeId': exchange, 'decisionMs': now,
                    'timeBlockMs': now//60000*60000, 'tradeEvidenceSha256': fingerprint,
                    'tradeSourceMs': record['sourceTsMs'], 'tradeReceivedMs': record['receivedMs'],
                    'tradeCollectorAvailableMs': record['availableMs'],
                    'tradeSourceAgeMs': now-record['sourceTsMs'], 'tradeReceiptAgeMs': now-record['receivedMs'],
                    'tradeCollectorAvailabilityAgeMs': now-record['availableMs'],
                    'originalSourceSnapshotSha256': cached['sourceSnapshotSha256'],
                    'originalSourceExportMs': cached['sourceSnapshot']['asOfMs'],
                    'originalExportObserverAvailableMs': cached['availableMs'],
                    'state': meta, 'ages': ages,
                    'sourceAgeBand': age_band(ages['sourceAgeMs']) if valid else 'missing_or_invalid',
                    'oldThreeSecondBookEligible': meta['state'] == 'valid' and valid and all(0 <= ages[k] <= 3000 for k in ('sourceAgeMs', 'receiptAgeMs')),
                    'availability': {str(h): {'status': 'pending'} for h in self.horizons},
                    'nextObservation': {'status': 'pending', 'gapObserved': meta['state'] == 'gapped'}}
            if hasattr(self, 'cache_input_context'):
                unit['originalCacheInput'] = self.cache_input_context.copy()
            self.unit_rows.append(unit)
            if meta['state'] == 'gapped':
                for outcome in unit['availability'].values():
                    outcome.update(status='missing', reason='gap_crossing')
            elif meta['state'] == 'ordering_unknown':
                for outcome in [*unit['availability'].values(), unit['nextObservation']]:
                    outcome.update(status='missing', reason='unprovable_book_ordering')
            reason = None
            if now-self.last_nomination.get(key, -10000) < 1000:
                reason = 'spacing_exclusion'
            elif len(self.units_by_key[key]) >= self.p['enrollment']['maximumPendingNominatedUnitsPerScope']:
                reason = 'pending_capacity_exclusion'
            if reason:
                unit['labelNomination'] = reason
                for outcome in unit['availability'].values():
                    outcome.update(status='missing', reason=reason)
                unit['nextObservation'].update(status='missing', reason=reason)
            else:
                unit['labelNomination'] = 'nominated'
                self.last_nomination[key] = now
                self.units_by_key[key].append(unit)

        def finish(self, seal_at):
            cutoff = min(seal_at, self.cutoff)
            for key in list(self.units_by_key):
                # Every callback through this inclusive metadata bound has
                # already been considered; a deadline equal to it is complete.
                self._expire(key, cutoff+1)
                for unit in self.units_by_key[key]:
                    for outcome in [*unit['availability'].values(), unit['nextObservation']]:
                        if outcome['status'] == 'pending':
                            outcome.update(status='censored', reason='terminal_right_censoring')
            availability = {str(h): Counter() for h in self.horizons}
            by_age, by_scope, by_block = defaultdict(Counter), defaultdict(Counter), defaultdict(Counter)
            for unit in self.unit_rows:
                for h, outcome in unit['availability'].items():
                    status = outcome.get('reason', outcome['status'])
                    availability[h][status] += 1
                    token = h+':'+status
                    by_age[unit['sourceAgeBand']][token] += 1
                    by_scope[unit['marketId']+':'+unit['exchangeId']][token] += 1
                    by_block[str(unit['timeBlockMs'])][token] += 1
            return {'marketTriggerCounts': dict(self.market_counts), 'uniqueUnitExclusions': dict(self.exclusions),
                    'pathDiagnosticsNotDenominators': dict(self.diagnostics),
                    'witnessedExchangeUnitDenominator': len(self.unit_rows),
                    'exchangeDenominatorKnownComplete': False,
                    'enrollmentCapAtMs': self.first_cap_at, 'metadataCutoffMs': cutoff,
                    'sourceAgeBands': dict(Counter(u['sourceAgeBand'] for u in self.unit_rows)),
                    'oldThreeSecondBookEligibility': dict(Counter(str(u['oldThreeSecondBookEligible']).lower() for u in self.unit_rows)),
                    'horizonAvailability': {h: dict(c) for h, c in availability.items()},
                    'availabilityByAgeBand': {k: dict(v) for k, v in by_age.items()},
                    'availabilityByScope': {k: dict(v) for k, v in by_scope.items()},
                    'availabilityByMinute': {k: dict(v) for k, v in by_block.items()},
                    'units': self.unit_rows}
    return CoverageStudy


def audit(manifest_path, manifest_sha256):
    replay = checked_replay()
    path = Path(manifest_path).absolute()
    outer = replay.Pinned({'path': str(path), 'sha256': manifest_sha256, 'bytes': path.stat().st_size})
    spec = replay.loads(outer.small(1024**2))
    need(set(spec) == {'schemaVersion', 'protocol', 'inputReplay'} and type(spec['schemaVersion']) is int and spec['schemaVersion'] == 1, 'MANIFEST_FIELDS')
    def pin(ref, maximum):
        need(isinstance(ref, dict) and set(ref) == {'path', 'sha256', 'bytes'} and isinstance(ref['path'], str), 'PIN_FIELDS')
        target = Path(ref['path']); target = target if target.is_absolute() else path.parent/target
        result = replay.Pinned({**ref, 'path': str(target.absolute())})
        need(result.bytes <= maximum, 'PIN_BOUND')
        return result
    protocol_pin = pin(spec['protocol'], 65536)
    need(protocol_pin.sha == PROTOCOL_SHA, 'FROZEN_PROTOCOL_BYTES')
    protocol = replay.loads(protocol_pin.small(65536))
    start, end = (clock(protocol['enrollment'][k]) for k in ('startInclusiveUtc', 'endExclusiveUtc'))
    need(_now_ms() >= clock(protocol['enrollment']['maximumMetadataObservationUtc']), 'PROSPECTIVE_INTERVAL_NOT_FINISHED')
    need(set(spec['inputReplay']) == {'manifest', 'result'}, 'ORIGINAL_REPLAY_PROOF_REQUIRED')
    original = pin(spec['inputReplay']['manifest'], 1024**2)
    retained = pin(spec['inputReplay']['result'], 4*1024**2)
    verified = replay.verify(original.path, original.sha)
    need(replay.compact(verified) == replay.compact(replay.loads(retained.small(4*1024**2))), 'RETAINED_REPLAY_RESULT_MISMATCH')
    need(verified['producerSha256'] == PRODUCER_SHA and verified['samplingMode'] == 'trade', 'V5_PRODUCER_OR_MODE')
    inputs = replay.loads(original.small(1024**2))
    trace, seal_pin, mapping_pin = (replay.Pinned(inputs[k]) for k in ('trace', 'seal', 'mapping'))
    other_original_pins = [replay.Pinned(inputs[k]) for k in ('samples', 'producer')]
    seal = replay.loads(seal_pin.small(65536))
    mapping = replay.loads(mapping_pin.small(1024**2))
    module = replay.known_sampler(PRODUCER_SHA)
    study = study_class(module)(protocol, mapping, start, end)
    sources = replay.Sources(inputs['sigSources'])
    body = None; body_sha = None; callbacks = 0; run = None
    for record in replay.lines(trace):
        event = record['event']; kind = event['kind']
        if kind == 'run_start':
            run = event
            need(run['traceStartedAtMs'] < start and run['samplingPolicySha256'] == protocol['existingSamplingPolicySha256'], 'RUN_START_OR_POLICY')
            sources.start(run['initialBookmark'])
        elif kind == 'poll_body':
            body = base64.b64decode(event['bodyBytesBase64'], validate=True)
            body_sha = event['bodySha256']
        elif kind == 'apply_begin':
            now = event['samplerNowMs']
            if now > study.cutoff:
                break
            callbacks += 1
            if event['inputKind'] == 'cache_body':
                need(body is not None, 'ORIGINAL_CACHE_BODY_MISSING')
                study.cache_input_context = {'bodySha256': body_sha, 'applyTraceSequence': record['sequence'],
                                             'applyTraceHash': record['hash'], 'samplerNowMs': now}
                study.external_update(replay.loads(body), now); body = None
            else:
                row = sources.consume(event['input'])
                (study.breadth if run['effectiveSamplerConfig']['format'] == 'breadth' else study.accept)(row, now)
        elif kind == 'poll_failed':
            body = None
        elif kind == 'sig_rotation':
            if event['observedAtMs'] > study.cutoff:
                break
            study.boundary({})
            sources.rotate(event)
    need(run is not None, 'RUN_START_MISSING')
    result = study.finish(seal['sealedAtMs'])
    for source in [outer, protocol_pin, original, retained, trace, seal_pin, mapping_pin, *other_original_pins, *sources.pins]:
        source.recheck()
    result.update(schemaVersion=1, studyId=protocol['studyId'], protocolSha256=PROTOCOL_SHA,
                  auditManifestSha256=outer.sha, inputReplayManifestSha256=original.sha,
                  retainedReplayResultSha256=retained.sha, producerSha256=PRODUCER_SHA,
                  originalSealSha256=seal_pin.sha, traceSha256=trace.sha,
                  mappingSha256=mapping_pin.sha,
                  fixedVerifierLfSha256=REPLAY_LF_SHA, processedCallbacks=callbacks,
                  exactInputReplayVerified=True, fullRunStartPrefixUsed=True,
                  runTraceStartedAtMs=run['traceStartedAtMs'],
                  originalSigCallbackMembershipVerified=verified['originalSigMembershipVerified'],
                  historyPrefixVerified=False, wholeWitnessVerified=False, providerTradeTapeComplete=False,
                  economicOutcomesEvaluated=False, modelFit=False, executionAuthority=False,
                  pricesOrReturnsIncluded=False, sourceCodeSha256=sha(Path(__file__).read_bytes()))
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--manifest', type=Path, required=True)
    parser.add_argument('--manifest-sha256', required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    result = audit(args.manifest, args.manifest_sha256)
    with args.output.open('x', encoding='utf-8', newline='\n') as target:
        target.write(json.dumps(result, sort_keys=True, separators=(',', ':'), allow_nan=False)+'\n')


if __name__ == '__main__':
    main()
