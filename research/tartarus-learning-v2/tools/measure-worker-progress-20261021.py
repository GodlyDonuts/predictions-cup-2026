"""Finite file-only measurement of completed worker scopes, not fill frequency."""
from datetime import datetime, timezone
from pathlib import Path
import argparse
import json
import statistics
import time


def main():
    p = argparse.ArgumentParser()
    p.add_argument('--runtime', type=Path, required=True)
    p.add_argument('--pid', type=int, required=True)
    p.add_argument('--start-ticks', type=int, required=True)
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--seconds', type=int, default=600)
    a = p.parse_args()
    a.output.mkdir()
    started, begin, previous = time.monotonic(), None, None
    intervals, samples = [], 0
    with (a.output / 'progress.jsonl').open('x') as log:
        while time.monotonic() - started < 2700 + a.seconds:
            proc = Path('/proc') / str(a.pid)
            assert proc.exists(), 'Observed worker ended'
            assert int((proc / 'stat').read_text().rsplit(') ', 1)[1].split()[19]) == a.start_ticks
            assert not (a.runtime / 'EXECUTION_STOP').exists(), 'Worker STOP present'
            progress = json.loads((a.runtime / 'progress.json').read_bytes())
            if progress.get('pid') == a.pid and progress.get('phase') == 'trading' and progress.get('realtimeHealthy') is True:
                if begin is None:
                    begin = time.monotonic()
                if previous is None or progress['at'] != previous['at']:
                    if previous is not None:
                        gap = (datetime.fromisoformat(progress['at'].replace('Z', '+00:00')) -
                               datetime.fromisoformat(previous['at'].replace('Z', '+00:00'))).total_seconds()
                        assert gap > 0
                        intervals.append(gap)
                    previous = progress
                    log.write(json.dumps(progress, separators=(',', ':')) + '\n')
                    log.flush()
                    samples += 1
            status = dict(observedUtc=datetime.now(timezone.utc).isoformat(), pid=a.pid,
                          phase='measuring' if begin else 'waiting_for_fresh_worker',
                          elapsedMeasurementSeconds=0 if begin is None else round(time.monotonic() - begin, 3),
                          completedProgressSamples=samples, completionIntervalsSeconds=intervals,
                          medianCompletionIntervalSeconds=statistics.median(intervals) if intervals else None,
                          accountRequests=0, filledMinuteTargetVerified=False,
                          interpretation='Scope completion gaps only; counters include ACKs and partial fill receipts, not distinct filled orders')
            temp = a.output / 'latest.next.json'
            temp.write_text(json.dumps(status, indent=2) + '\n')
            temp.replace(a.output / 'latest.json')
            if begin is not None and time.monotonic() - begin >= a.seconds:
                (a.output / 'complete.json').write_text(json.dumps(status, indent=2) + '\n')
                return
            time.sleep(2)
    raise RuntimeError('No completed measurement in the finite startup window')


if __name__ == '__main__':
    main()
