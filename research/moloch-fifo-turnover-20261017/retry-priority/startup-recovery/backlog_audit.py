"""Finite, original-only R31 first-window admission-backlog audit. No replay."""
import argparse
from collections import Counter, defaultdict
import hashlib
import importlib.util
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
READER_LF_SHA = '1fbc599e16417778d4aa48545eb2c7af9c3073875fd4ffb26f72c0333443a34b'
PACKET_SHA = '0906a063ba8f1cf5fc8362e6551ec079777a05aa296013ba3e4329ceff9641d4'


def audit(path, expected):
    reader = ROOT / 'research/moloch-source-latency-20261017/compare_book_clocks.py'
    raw = reader.read_bytes()
    assert hashlib.sha256(raw.replace(b'\r\n', b'\n')).hexdigest() == READER_LF_SHA
    spec = importlib.util.spec_from_file_location('_r31_backlog_clocks', reader)
    c = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(c)
    c.need(expected == PACKET_SHA, 'R31_FROZEN_PACKET')
    packet = c.Packet(path, expected)
    try:
        start, text = c.progress(packet, 'first-trading-progress.original.json', 690857)
        measured = c.measure(packet, start, text, 690857,
                             {'zipSha256': expected, **packet.original_pin('first-trading-progress.original.json')})
        end = start + c.WINDOW_NS
        events = []
        with packet.z.open(packet.name('native.suffix.jsonl')) as stream:
            for line in stream:
                row, _ = c.V.sealed(line)
                e = row['event']
                at = c.N.instant(e['processedAt'])
                if start <= at < end:
                    events.append((row['sequence'], at, e))
        stamp_groups = defaultdict(list)
        stale_groups = defaultdict(list)
        buckets = Counter({k: 0 for k in ('0_to_1000ms', '1000_to_10000ms', '10000_to_30000ms', 'over_30000ms')})
        predecessors = Counter()
        previous = None
        gaps = []
        stale = []
        for sequence, at, e in events:
            stamp_groups[at].append(e['type'])
            if previous is not None and at > previous[1]:
                gaps.append((at - previous[1], previous, (sequence, at, e)))
            if e['type'] == 'book' and e['book'].get('transport') == 'official_sig_realtime':
                received = c.N.instant(e['book']['receivedAt'])
                source = c.N.instant(e['book']['sourceAt'])
                delay = at - received
                c.need(delay >= 0, 'R31_RECEIPT_CLOCK')
                key = ('0_to_1000ms' if delay <= 1000*c.NS_PER_MS else
                       '1000_to_10000ms' if delay <= 10000*c.NS_PER_MS else
                       '10000_to_30000ms' if delay <= 30000*c.NS_PER_MS else 'over_30000ms')
                buckets[key] += 1
                if at - source > c.AGE_NS:
                    stale.append((sequence, at, received, source))
                    stale_groups[at].append(sequence)
                    predecessors[previous[2]['type'] if previous else 'window_start'] += 1
            previous = (sequence, at, e)
        # Adjacency is descriptive. A native processed stamp precedes its durable
        # work; it is NOT a completion timestamp or a CPU/fsync duration witness.
        long_intervals = []
        for duration, before, after in sorted(gaps, key=lambda x: (-x[0], x[1][0]))[:12]:
            bseq, bat, be = before
            aseq, aat, ae = after
            long_intervals.append({
                'previousSequence': bseq, 'nextSequence': aseq,
                'previousType': be['type'], 'nextType': ae['type'],
                'previousProcessedAt': be['processedAt'], 'nextProcessedAt': ae['processedAt'],
                'processedStampIntervalMs': c.ms(duration),
                'admittedWsBooksWhoseReceiptFallsInInterval': sum(
                    bat < c.N.instant(x['book']['receivedAt']) <= aat
                    for _, _, x in events if x['type'] == 'book' and x['book'].get('transport') == 'official_sig_realtime'),
                'staleWsBooksAdmittedAtNextStamp': len(stale_groups.get(aat, [])),
            })
        native_stamps = sorted(stamp_groups)
        by_previous_type = defaultdict(list)
        for duration, before, _ in gaps:
            by_previous_type[before[2]['type']].append(duration)
        packet.finish()
        return {
            'schemaVersion': 1, 'sourceCodeSha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
            'readerLfSha256': READER_LF_SHA, 'frozenClockMeasurement': measured,
            'wsAdmissionDelayBuckets': dict(buckets),
            'staleWsBooks': len(stale), 'staleWsDistinctProcessedStamps': len(stale_groups),
            'staleWsImmediateNativePredecessorTypes': dict(predecessors),
            'processedStampGroups': {
                'distinctStamps': len(stamp_groups),
                'eventCountPerStamp': dict(sorted(Counter(map(len, stamp_groups.values())).items())),
                'interStampInterval': c.distribution([b-a for a,b in zip(native_stamps,native_stamps[1:])]),
                'maximumEventsAtOneStamp': max(map(len, stamp_groups.values()), default=0),
            },
            'longestProcessedStampIntervals': long_intervals,
            'interStampIntervalsByLastPrecedingNativeType': {
                kind: {'interval': c.distribution(values),
                       'over10000ms': sum(v > c.AGE_NS for v in values)}
                for kind, values in sorted(by_previous_type.items())
            },
            'limitations': [
                'Original admitted-event receipt and processed clocks measure admission backlog, not socket callback, drain entry/exit or durable commit completion.',
                'Groups of equal processedAt are not independently proved Session batch boundaries.',
                'Native predecessor adjacency is not causal CPU/fsync attribution; unjournaled work may occur between stamps.',
                'Only books eventually admitted in this finite window are counted; superseded, queued beyond cutoff and dropped observations are unavailable.',
                'Raw receipt-body membership and complete market/source coverage are not independently authenticated by these native seals.',
                'No financial replay, runtime change, profitability or causal optimization benefit is established.',
            ],
            'executionAuthority': False, 'financialReplayPerformed': False,
            'causalProcessingCostAttributed': False,
        }
    finally:
        packet.z.close()


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--packet', required=True)
    p.add_argument('--sha256', required=True)
    p.add_argument('--output', required=True)
    a = p.parse_args()
    result = audit(a.packet, a.sha256)
    with Path(a.output).open('x', encoding='utf-8', newline='\n') as f:
        json.dump(result, f, indent=2, allow_nan=False)
        f.write('\n')


if __name__ == '__main__':
    main()
