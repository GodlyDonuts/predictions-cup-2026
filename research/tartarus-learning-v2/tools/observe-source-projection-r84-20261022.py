"""Finite file-only startup verification; no account API client."""
from pathlib import Path
import datetime, hashlib, json, time

L=Path('/tmp/moloch-source-projection-r84-20261022')
R=Path('/home/archon/apps/moloch/shared/live-production-v1')
B=R/'bank-all-markets-v1'


def sha(path):
    with Path(path).open('rb') as f: return hashlib.file_digest(f,'sha256').hexdigest()


def main():
    d=json.loads((L/'deployment.public.json').read_bytes()); pid=d['pid']; proc=Path('/proc')/str(pid)
    baseline=json.loads((Path(d['incident'])/'report.json').read_bytes())['fills']
    started=time.monotonic()
    while time.monotonic()-started<2700:
        progress=json.loads((R/'progress.json').read_bytes())
        current=progress if progress.get('pid')==pid else None
        status=dict(observedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),pid=pid,alive=proc.exists(),stop=(R/'EXECUTION_STOP').exists(),elapsedSeconds=round(time.monotonic()-started),progress=current)
        (L/'observer.status.json').write_text(json.dumps(status))
        if not status['alive'] or status['stop']:
            status['stopEvidence']=json.loads((R/'EXECUTION_STOP').read_bytes()) if status['stop'] else None
            (L/'fresh-live.failed.json').write_text(json.dumps(status,indent=2))
            raise RuntimeError('Replacement process stopped; no live claim')
        if current and current.get('phase')=='trading' and current.get('realtimeHealthy') is True:
            prefix=d['originalFinancialPrefix']; h=hashlib.sha256(); left=prefix['bytes']
            with (B/'events.jsonl').open('rb') as f:
                while left:
                    chunk=f.read(min(left,4*1024*1024)); assert chunk; h.update(chunk); left-=len(chunk)
            assert h.hexdigest()==prefix['sha256']
            raw=(B/'report.json').read_bytes(); report=json.loads(raw)
            assert report['fills'][:len(baseline)]==baseline
            assert int((proc/'stat').read_text().rsplit(') ',1)[1].split()[19])==d['startTicks']
            assert sha(proc/'exe')==d['binarySha256'] and sha(B/'engine-upgrade-v1.json')==d['newBridgeSha256'] and sha(Path(d['incident'])/'engine-upgrade-v1.json')==d['bridgeSha256']
            assert not (R/'EXECUTION_STOP').exists()
            result=dict(observedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),pid=pid,binarySha256=d['binarySha256'],progress=current,
                fullOriginalPrefixVerified=True,originalFinancialPrefix=prefix,originalFillCount=len(baseline),originalFillsUnchanged=True,
                currentFillCount=len(report['fills']),approvedNativeUpgradeChainVerified=True,allPriorEngineBridgesPreserved=d['allPriorEngineBridgesPreserved'],engineIdentity=d['engineIdentity'],reportSha256=hashlib.sha256(raw).hexdigest(),
                financial={k:report['report'][k] for k in ['cash','ownedCost','realized','newTradingRealized','inheritedRealizedGross']},
                accountRequestsByObserver=0,filledMinuteTargetVerified=False,wholeAccountProfitEstablished=False)
            (L/'fresh-live.public.json').write_text(json.dumps(result,indent=2)+'\n')
            print(json.dumps(result),flush=True); return
        time.sleep(10)
    raise RuntimeError('Fresh trading not observed in finite45-minute window')


if __name__=='__main__': main()
