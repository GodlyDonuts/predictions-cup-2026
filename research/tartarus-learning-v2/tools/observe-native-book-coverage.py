"""Compare retained raw and native books at the SAME native-view clock.

File-only diagnostic: raw availability is not proof of complete trade coverage,
execution eligibility or a missed fill. Never edits the worker or makes requests.
"""
import argparse
import json
import time
from datetime import datetime, timezone
from pathlib import Path


def clock(value):
    return int(datetime.fromisoformat(value.replace('Z', '+00:00')).timestamp()*1000)


def retained_books(runtime, pid, cutoff):
    books = {}
    paths = sorted(runtime.glob(f'breadth-{pid}-*.jsonl'), key=lambda p:p.stat().st_mtime)[-2:]
    for path in paths:
        with path.open('rb') as stream:
            offset = max(0, path.stat().st_size-8*1024*1024)
            stream.seek(offset)
            if offset:
                stream.readline()
            for line in stream:
                if not line.endswith(b'\n'):
                    continue
                row = json.loads(line)['event']
                if clock(row['receivedAt']) > cutoff:
                    continue
                if row['event'] == 'stream_reset':
                    books.clear()
                if row['event'] != 'markets_batch':
                    continue
                for market in row['payload']['markets']:
                    mid = str(market['marketId'])
                    batch = market['batch']
                    if batch.get('resyncRequired'):
                        books = {k:v for k,v in books.items() if v['marketId'] != mid}
                        continue
                    for book in batch.get('books', []):
                        books[str(book['exchangeId'])] = dict(marketId=mid,
                            sourceAt=book['asOf']['at'], receivedAt=row['receivedAt'],
                            sequence=str(book['asOf']['sequence']),
                            twoSided=bool(book['bids'] and book['asks']))
    return books


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--runtime',type=Path,required=True)
    p.add_argument('--pid',type=int,required=True)
    p.add_argument('--output',type=Path,required=True)
    p.add_argument('--seconds',type=int,default=900)
    a=p.parse_args();a.output.mkdir(exist_ok=False)
    proc=Path('/proc')/str(a.pid)
    ticks=(proc/'stat').read_text().rsplit(') ',1)[1].split()[19]
    started=time.monotonic();begin=None;seen=set();samples=[]
    with (a.output/'observations.jsonl').open('x') as history:
        while time.monotonic()-started<2700+a.seconds:
            assert (proc/'stat').read_text().rsplit(') ',1)[1].split()[19]==ticks
            assert not (a.runtime/'EXECUTION_STOP').exists()
            progress=json.loads((a.runtime/'progress.json').read_bytes())
            if progress.get('pid')==a.pid and progress.get('realtimeHealthy') is True:
                if begin is None:begin=time.monotonic()
                view=json.loads((a.runtime/'tartarus-current-native-state.json').read_bytes())
                stamp=view['writtenAt'];at=clock(stamp)
                if stamp not in seen and at>=clock(progress['at'])-10000:
                    seen.add(stamp)
                    raw=retained_books(a.runtime,a.pid,at)
                    native={b['exchangeId']:b for b in view['books']}
                    fresh=lambda b:all(0<=at-clock(b[k])<=10000 for k in ['receivedAt','sourceAt'])
                    raw_fresh={ex for ex,b in raw.items() if b['twoSided'] and fresh(b)}
                    native_fresh={ex for ex,b in native.items() if fresh(b)}
                    managed=set(view['managedTargetAdmissionProjections']) if isinstance(view['managedTargetAdmissionProjections'],dict) else set()
                    sample=dict(observedUtc=datetime.now(timezone.utc).isoformat(),workerPid=a.pid,
                        viewWrittenAt=stamp,rawBooksBeforeView=len(raw),nativeBooks=len(native),
                        rawFreshBooks=len(raw_fresh),nativeFreshBooks=len(native_fresh),
                        freshRawAbsentNative=sorted(raw_fresh-set(native)),
                        freshRawButStaleNative=sorted(raw_fresh & (set(native)-native_fresh)),
                        managedScopes=sorted(managed),accountRequests=0,
                        rawBooksProveCompleteTape=False,missedFillsClaimed=0)
                    samples.append(sample);history.write(json.dumps(sample)+'\n');history.flush()
                    (a.output/'latest.json').write_text(json.dumps(sample,indent=2)+'\n')
                if time.monotonic()-begin>=a.seconds:
                    (a.output/'complete.json').write_text(json.dumps(dict(samples=samples,
                        elapsedSeconds=time.monotonic()-begin,accountRequests=0),indent=2)+'\n');return
            time.sleep(15)
    raise RuntimeError('No fresh native-view window completed')


if __name__=='__main__':main()
