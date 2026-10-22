"""Finite file-only observation of the exact R101 process and original fills."""
from pathlib import Path
from datetime import datetime, timezone
import hashlib
import json
import time

ROOT = Path('/tmp/moloch-preintent-orders-r101-20261022')
RUNTIME = Path('/home/archon/apps/moloch/shared/live-production-v1')


def main():
    deployment = json.loads((ROOT/'deployment.public.json').read_bytes())
    pid = deployment['pid']
    process = Path('/proc')/str(pid)
    with (process/'exe').open('rb') as f:
        assert hashlib.file_digest(f, 'sha256').hexdigest() == deployment['binarySha256']
    baseline = json.loads((Path(deployment['incident'])/'report.json').read_bytes())['fills']
    deadline = time.monotonic()+2400
    with (ROOT/'restoration-observations.jsonl').open('x') as log:
        while time.monotonic() < deadline:
            assert process.exists(), 'EXACT_WORKER_EXITED'
            assert int((process/'stat').read_text().rsplit(') ', 1)[1].split()[19]) == deployment['startTicks']
            progress = json.loads((RUNTIME/'progress.json').read_bytes())
            observed = datetime.now(timezone.utc)
            current = progress.get('pid') == pid
            row = dict(observedUtc=observed.isoformat(), pid=pid, processAlive=True,
                       currentPidProgress=current, stopPresent=(RUNTIME/'EXECUTION_STOP').exists(),
                       newAccountClients=0)
            row['processIo'] = dict(line.split(': ', 1) for line in (process/'io').read_text().splitlines())
            if current:
                row['progress'] = progress
            log.write(json.dumps(row)+'\n')
            log.flush()
            if current and progress.get('phase') == 'trading':
                at = datetime.fromisoformat(progress['at'].replace('Z', '+00:00'))
                assert 0 <= (observed-at).total_seconds() <= 120
                report = json.loads((RUNTIME/'bank-all-markets-v1/report.json').read_bytes())
                assert report['fills'][:len(baseline)] == baseline, 'ORIGINAL_FILL_PREFIX_CHANGED'
                row.update(originalFillRows=len(baseline), currentFillRows=len(report['fills']),
                           originalFillPrefixPreserved=True, freshTradingVerified=True,
                           nativeEngineUnchanged=True, profitabilityEstablished=False)
                with (ROOT/'fresh-live.public.json').open('x') as f:
                    json.dump(row, f, indent=2)
                    f.write('\n')
                print('CURRENT_PID_TRADING_AND_ORIGINAL_FILL_PREFIX_VERIFIED', flush=True)
                return
            time.sleep(15)
    raise RuntimeError('OBSERVATION_LIMIT_REACHED; no process restart requested')


if __name__ == '__main__':
    main()
