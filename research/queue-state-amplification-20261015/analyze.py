"""Offline size/CPU projections and optional lossless archive measurements.

Only reads a synthetic benchmark JSON/fixture and creates NEW result/archive
files. It accepts no production queue path, performs no account call and never
deletes source material. Python standard library plus optional installed zstd.
"""
from __future__ import annotations
import argparse
import hashlib
import json
import lzma
import math
import platform
import shutil
import subprocess
import sys
import time
from pathlib import Path

MINUTES = 20 * 24 * 60
CAP = 64 * 1024 * 1024


def journal_bytes(reads, writes, minutes, initial, base, increment):
    """Exact fixed-width synthetic model: each minute reads first, then writes.

    Three records per HTTP operation. All maps retained; ignores status/wait,
    active-flight/rolling-array overhead and sequence digit growth.
    """
    records_per_minute = 3 * (reads + writes)
    bindings_sum = records_per_minute * (minutes * initial + writes * minutes * (minutes - 1) // 2)
    bindings_sum += 3 * minutes * writes * (writes + 1) // 2
    return records_per_minute * minutes * base + increment * bindings_sum


def budget_minute(reads, writes, initial, base, increment, budget):
    lo, hi = 0, MINUTES
    if journal_bytes(reads, writes, hi, initial, base, increment) < budget:
        return None
    while lo + 1 < hi:
        mid = (lo + hi) // 2
        if journal_bytes(reads, writes, mid, initial, base, increment) >= budget:
            hi = mid
        else:
            lo = mid
    return hi


def projection(benchmark, free_bytes):
    model = benchmark['sizeModel']
    base, increment = model['sparseRecordBaseBytes'], model['perRetainedBindingBytes']
    initial = benchmark.get('observedProduction', {}).get('intents', 98)
    points = sorted(benchmark['measurements'], key=lambda p: p['bindings'])
    left, right = points[0], points[-1]
    count_delta = right['bindings'] - left['bindings']
    cpu_base = left['cloneHashSerialize']['p50Micros']
    cpu_slope = max(0, right['cloneHashSerialize']['p50Micros'] - cpu_base) / count_delta if count_delta else None
    scenarios = []
    for writes in [1, 5, 24]:
        offered_reads = 100
        feasible_reads = max(0, (60_000 - 2500 * writes) // 600)
        final_bindings = initial + MINUTES * writes
        first_oversize = (CAP - base) // increment + 1
        row = {'contestWritesPerMinute': writes, 'retainedBindingsAfter20Days': final_bindings,
               'minimumRetainedMapsStringPayloadBytes': final_bindings * (3 * (model['intentBytes'] + model['keyBytes']) + 128),
               'singleRecordBytesAt20Days': base + increment * final_bindings,
               'firstOversizedRecordBindingCount': first_oversize,
               'singleRecordLimitDayAtOfferedWriteRate': max(0, first_oversize - initial) / writes / 1440,
               'offeredPacingSecondsPerMinute': offered_reads * .6 + writes * 2.5,
               'zeroLatencyPacingUpperBoundReadsPerMinute': feasible_reads,
               'pacingNote': '100 reads plus any write rate exceeds shared completion pacing; zero-latency bound also ignores inclusive rolling-window and real network costs'}
        for label, reads in [('offered', offered_reads), ('pacingUpperBound', feasible_reads)]:
            raw = journal_bytes(reads, writes, MINUTES, initial, base, increment)
            exhausted = budget_minute(reads, writes, initial, base, increment, free_bytes)
            item = {'readsPerMinute': reads, 'minimumQueueRecords': 3 * (reads + writes) * MINUTES,
                    'journalBytesFixedModel20Days': raw, 'terabytesDecimal': raw / 1e12,
                    'freeDiskBudgetReachedDay': None if exhausted is None else exhausted / 1440,
                    'compressionRatioNeededForJournalAlone': raw / free_bytes,
                    'checkpointArchiveBytesIncluded': False,
                    'statusWaitActiveRollingSequenceOverheadIncluded': False,
                    'counterfactualAfterSingleRecordCap': row['singleRecordBytesAt20Days'] > CAP}
            if cpu_slope is not None:
                final_us = cpu_base + cpu_slope * (final_bindings - left['bindings'])
                average_bindings = initial + writes * MINUTES / 2
                average_us = cpu_base + cpu_slope * (average_bindings - left['bindings'])
                item['linearComputeWallExtrapolation'] = {
                    'method': 'p50 clone+hash+serialization work measured with Instant under nice10/live host; WALL time, not OS process CPU; uncertain outside measured range; excludes IO/status/apply',
                    'lastMinuteComputeWallSecondsPerWallMinute': final_us * 3 * (reads + writes) / 1e6,
                    'averageComputeWallSecondsPerWallMinute': average_us * 3 * (reads + writes) / 1e6,
                    'serializedWorkerTimeBudgetExceededAtEnd': final_us * 3 * (reads + writes) > 60e6}
            row[label] = item
        scenarios.append(row)
    return {'assumptions': {'days': 20, 'initialRetainedBindings': initial, 'recordByteBase': base,
                           'bytesPerAdditionalRetainedBinding': increment, 'freeDiskBudgetBytes': free_bytes,
                           'identityShape': 'fixed synthetic lengths, not a claim that actual future identities stay fixed',
                           'archives': 'old generations/checkpoints retained; journal model excludes checkpoint additions, so total storage is larger',
                           'stateCpu': 'full State clone/hash/serialization still scales with retained bindings even if archives are compressed'},
            'scenarios': scenarios}


def digest_stream(file):
    h = hashlib.sha256()
    n = 0
    while chunk := file.read(1024 * 1024):
        h.update(chunk)
        n += len(chunk)
    return n, h.hexdigest()


def compress_fixture(fixture, work):
    source = Path(fixture['path']).resolve(strict=True)
    if source.name != 'queue-compression-fixture.jsonl' or source.stat().st_size > 64 * 1024 * 1024:
        raise ValueError('requires bounded synthetic benchmark fixture')
    with source.open('rb') as file:
        size, digest = digest_stream(file)
    if size != fixture['bytes'] or digest != fixture['sha256']:
        raise ValueError('synthetic fixture source hash/size changed')
    results = []
    # Whole independent files, not a persistent cross-archive dictionary. Keep
    # this distinction explicit when extrapolating archive compression ratios.
    for mib in [8, 64]:
        out = work / f'synthetic-xz-dict{mib}MiB.xz'
        if out.exists():
            raise FileExistsError(out)
        at = time.perf_counter()
        filters = [{'id': lzma.FILTER_LZMA2, 'dict_size': mib * 1024 * 1024, 'preset': 3}]
        with source.open('rb') as src, out.open('xb') as dst:
            codec = lzma.LZMACompressor(format=lzma.FORMAT_XZ, check=lzma.CHECK_CRC64, filters=filters)
            while chunk := src.read(1024 * 1024):
                dst.write(codec.compress(chunk))
            dst.write(codec.flush())
        seconds = time.perf_counter() - at
        at = time.perf_counter()
        with lzma.open(out, 'rb') as recovered:
            recovered_size, recovered_hash = digest_stream(recovered)
        decode_seconds = time.perf_counter() - at
        if recovered_size != size or recovered_hash != digest:
            raise ValueError('xz round-trip mismatch')
        results.append({'codec': 'xz', 'dictionaryMiB': mib, 'preset': 3, 'inputBytes': size,
                        'outputBytes': out.stat().st_size, 'ratio': size / out.stat().st_size,
                        'compressSeconds': seconds, 'decompressAndHashSeconds': decode_seconds,
                        'roundTripSha256': recovered_hash, 'roundTripVerified': True})
    zstd = shutil.which('zstd')
    if zstd:
        version = subprocess.check_output([zstd, '--version'], text=True).strip()
        for log in [23, 27]:
            out = work / f'synthetic-zstd-windowLog{log}.zst'
            if out.exists():
                raise FileExistsError(out)
            at = time.perf_counter()
            with out.open('xb') as dst:
                subprocess.run([zstd, '-q', '-3', '-T1', f'--long={log}', '--stdout', str(source)], stdout=dst, check=True)
            seconds = time.perf_counter() - at
            at = time.perf_counter()
            with subprocess.Popen([zstd, '-q', '-d', '--stdout', str(out)], stdout=subprocess.PIPE) as proc:
                recovered_size, recovered_hash = digest_stream(proc.stdout)
                if proc.wait() != 0:
                    raise ValueError('zstd decoder failed')
            decode_seconds = time.perf_counter() - at
            if recovered_size != size or recovered_hash != digest:
                raise ValueError('zstd round-trip mismatch')
            results.append({'codec': 'zstd', 'version': version, 'windowLog': log, 'level': 3, 'threads': 1,
                            'inputBytes': size, 'outputBytes': out.stat().st_size, 'ratio': size / out.stat().st_size,
                            'compressSeconds': seconds, 'decompressAndHashSeconds': decode_seconds,
                            'roundTripSha256': recovered_hash, 'roundTripVerified': True})
    return {'sourceSha256': digest, 'inputBytes': size, 'records': fixture['records'],
            'initialBindings': fixture['initialBindings'], 'finalBindings': fixture['finalBindings'],
            'independentFileCompression': True, 'zstdAvailable': bool(zstd), 'measurements': results,
            'limitation': 'current synthetic cohort/size only; repeated-state ratios do not establish late-stage or cross-archive ratios; large dictionaries need measured RAM'}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('benchmark', type=Path)
    ap.add_argument('new_output', type=Path)
    ap.add_argument('--free-bytes', type=int, default=211_514_302_464)
    ap.add_argument('--compress', action='store_true')
    args = ap.parse_args()
    if args.new_output.exists() or not args.new_output.parent.is_dir():
        raise ValueError('output must be new in an existing benchmark directory')
    benchmark = json.loads(args.benchmark.read_text(encoding='utf-8'))
    if benchmark.get('accountCalls') != 0 or benchmark.get('productionModified') is not False:
        raise ValueError('requires offline synthetic benchmark provenance')
    result = {'schemaVersion': 1, 'platform': platform.platform(), 'sourceBenchmarkSha256': hashlib.sha256(args.benchmark.read_bytes()).hexdigest(),
              'projection': projection(benchmark, args.free_bytes), 'productionModified': False, 'accountCalls': 0}
    if args.compress:
        result['compression'] = compress_fixture(benchmark['compressionFixture'], args.new_output.parent)
    with args.new_output.open('x', encoding='utf-8') as file:
        json.dump(result, file, indent=2, sort_keys=True)
        file.write('\n')
    print(json.dumps({'output': str(args.new_output), 'accountCalls': 0, 'productionModified': False}))


if __name__ == '__main__':
    main()
