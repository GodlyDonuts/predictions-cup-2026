"""Finite, file-only observation of completed ordinary candidate sweeps."""
import argparse
import datetime
import json
from pathlib import Path
import time


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--runtime', type=Path, required=True)
    parser.add_argument('--pid', type=int, required=True)
    parser.add_argument('--seconds', type=int, default=900)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    assert args.seconds > 0
    args.output.mkdir(parents=True, exist_ok=False)
    proc = Path('/proc') / str(args.pid)
    start_ticks = (proc / 'stat').read_text().rsplit(') ', 1)[1].split()[19]
    started = time.monotonic()
    seen, samples = set(), []
    reason = 'duration_complete'
    with (args.output / 'observations.jsonl').open('x') as history:
        while time.monotonic() - started < args.seconds:
            if not proc.exists() or (proc / 'stat').read_text().rsplit(') ', 1)[1].split()[19] != start_ticks:
                reason = 'original_worker_exited'
                break
            try:
                sweep = json.loads((args.runtime / 'ordinary-candidate-sweep.json').read_bytes())
            except (FileNotFoundError, json.JSONDecodeError):
                time.sleep(2)
                continue
            if sweep.get('pid') == args.pid and sweep['at'] not in seen:
                seen.add(sweep['at'])
                samples.append(sweep)
                history.write(json.dumps(sweep) + '\n')
                history.flush()
                summary = dict(observedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),
                    pid=args.pid, samples=len(samples),
                    multiplePassSweeps=sum(s['passes'] > 1 for s in samples),
                    maximumPasses=max(s['passes'] for s in samples),
                    journalChangedSweeps=sum(s['dispatchJournalChanged'] for s in samples),
                    latest=sweep, accountRequests=0, fillsNotInferredFromSweeps=True)
                (args.output / 'latest.json').write_text(json.dumps(summary, indent=2) + '\n')
            time.sleep(2)
    result = dict(observedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),
        pid=args.pid, elapsedSeconds=time.monotonic()-started, completionReason=reason,
        completedWindow=reason == 'duration_complete', samples=samples,
        accountRequests=0, fillsNotInferredFromSweeps=True)
    (args.output / 'complete.json').write_text(json.dumps(result, indent=2) + '\n')


if __name__ == '__main__':
    main()
