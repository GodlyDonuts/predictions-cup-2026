"""Finite low-priority observation of the existing worker; no API or orders."""
import argparse
import datetime
import json
from pathlib import Path
import time
from turnover_exit_census import sample


def main():
    p=argparse.ArgumentParser();p.add_argument('--runtime',type=Path,required=True)
    p.add_argument('--reference',type=Path,required=True);p.add_argument('--pid',type=int,required=True)
    p.add_argument('--output',type=Path,required=True);p.add_argument('--seconds',type=int,default=3600)
    a=p.parse_args();a.output.mkdir(exist_ok=True)
    started=time.monotonic(); begin=None; observations=0; failures=0; nominated=set(); missed=set()
    with (a.output/'observations.jsonl').open('x',encoding='utf-8') as f:
        while time.monotonic()-started < 2700+a.seconds:
            if not (Path('/proc')/str(a.pid)).exists():
                raise RuntimeError('Observed worker ended; no continued-live claim')
            if (a.runtime/'EXECUTION_STOP').exists():
                raise RuntimeError('Execution STOP present; observation suspended')
            progress=json.loads((a.runtime/'progress.json').read_bytes())
            fresh=progress.get('pid')==a.pid and progress.get('phase')=='trading' and progress.get('realtimeHealthy') is True
            if fresh:
                if begin is None: begin=time.monotonic()
                try:
                    value=sample(a.runtime,a.reference,a.pid)
                    observations+=1
                    nominated.update(r['exchangeId'] for r in value['rows'] if r.get('currentResearchNomination'))
                    missed.update(r['exchangeId'] for r in value['rows'] if r.get('currentResearchNomination') and r.get('askRelativePolicyMissesProfitableBid'))
                except (AssertionError,ValueError,KeyError,OSError) as e:
                    failures+=1;value=dict(observationFailure=str(e),executionAuthority=False)
                value['observedByFileOnlyMonitorUtc']=datetime.datetime.now(datetime.timezone.utc).isoformat()
                f.write(json.dumps(value,separators=(',',':'))+'\n');f.flush()
            summary=dict(observedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),
                pid=a.pid,phase='observing' if begin is not None else 'waiting_for_fresh_worker',
                elapsedObservationSeconds=0 if begin is None else round(time.monotonic()-begin,3),
                targetSeconds=a.seconds,samples=observations,observationFailures=failures,
                marketsWithCurrentProfitableBids=sorted(nominated),marketsWithAskRelativeMiss=sorted(missed),
                accountRequests=0,executionAuthority=False,
                repeatedDepthIsNotSummedAsTurnover=True,filledOrdersClaimed=0)
            tmp=a.output/'latest.pending.json';tmp.write_text(json.dumps(summary,indent=2)+'\n');tmp.replace(a.output/'latest.json')
            if begin is not None and time.monotonic()-begin>=a.seconds:
                (a.output/'complete.json').write_text(json.dumps(summary,indent=2)+'\n');return
            time.sleep(15)
    raise RuntimeError('Fresh live evidence not available in finite startup window')


if __name__=='__main__':main()
