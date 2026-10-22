"""Finite, file-only cadence checks after the pinned worker becomes fresh.

Runs the existing original-receipt/FIFO audit. No API client, orders, controls,
process signals or claims based on acknowledgments/partial receipt counts.
"""
import argparse
import datetime
import json
from pathlib import Path
import subprocess
import time


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--runtime', type=Path, required=True)
    p.add_argument('--pid', type=int, required=True)
    p.add_argument('--start-ticks', type=int, required=True)
    p.add_argument('--auditor', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--seconds', type=int, default=900)
    a = p.parse_args()
    assert 60 <= a.seconds <= 3600
    a.output.mkdir(exist_ok=False)
    started = time.monotonic()
    measuring = None
    proc = Path('/proc')/str(a.pid)
    samples = []
    while time.monotonic()-started < 2700+a.seconds:
        assert int((proc/'stat').read_text().rsplit(') ',1)[1].split()[19]) == a.start_ticks
        assert not (a.runtime/'EXECUTION_STOP').exists()
        progress = json.loads((a.runtime/'progress.json').read_bytes())
        if progress.get('pid') != a.pid or progress.get('phase') != 'trading' or progress.get('realtimeHealthy') is not True:
            assert measuring is None, 'Previously fresh worker lost trading readiness'
            time.sleep(5)
            continue
        if measuring is None:
            measuring = time.monotonic()
        destination = a.output/f'audit-{len(samples):03d}.json'
        subprocess.run(['python3',str(a.auditor),'--output',str(destination)],
                       stdout=subprocess.DEVNULL,check=True,timeout=45)
        result = json.loads(destination.read_bytes())
        assert result['allNewFillsMatchOriginalOfficialReceipts'] is True
        assert result['remainingFifoExactlyMatches'] is True
        samples.append(dict(observedUtc=result['observedUtc'],cadence=result['cadence'],
                            financial=result['financial'],source=destination.name))
        status = dict(observedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),
                      pid=a.pid,elapsedSeconds=time.monotonic()-measuring,
                      samples=len(samples),latest=samples[-1],accountRequests=0)
        (a.output/'latest.json').write_text(json.dumps(status,indent=2)+'\n')
        if time.monotonic()-measuring >= a.seconds:
            status['observations'] = samples
            (a.output/'complete.json').write_text(json.dumps(status,indent=2)+'\n')
            return
        time.sleep(min(60,max(1,a.seconds-(time.monotonic()-measuring))))
    raise RuntimeError('No fresh measurement completed within bounded window')


if __name__ == '__main__':
    main()
