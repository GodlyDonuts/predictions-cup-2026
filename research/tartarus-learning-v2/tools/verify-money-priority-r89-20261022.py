"""Verify the installed single worker and retained financial prefix; no API."""
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path

WORK = Path('/tmp/moloch-money-priority-r89-20261022')
ROOT = Path('/home/archon/apps/moloch/shared/live-production-v1')


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def main():
    deployment = json.loads((WORK / 'deployment.public.json').read_bytes())
    pid = deployment['pid']
    assert digest(Path('/proc', str(pid), 'exe')) == deployment['binarySha256']
    assert not (ROOT / 'EXECUTION_STOP').exists()
    assert not (ROOT.parent / 'EXECUTION_STOP').exists()
    progress = json.loads((ROOT / 'progress.json').read_bytes())
    now = datetime.now(timezone.utc)
    assert progress['pid'] == pid and progress['phase'] == 'trading' and progress['realtimeHealthy']
    assert 0 <= (now-datetime.fromisoformat(progress['at'].replace('Z', '+00:00'))).total_seconds() < 120
    bank = ROOT / 'bank-all-markets-v1'
    original = json.loads((Path(deployment['incident']) / 'report.json').read_bytes())
    current = json.loads((bank / 'report.json').read_bytes())
    assert current['fills'][:len(original['fills'])] == original['fills']
    prefix = deployment['originalFinancialPrefix']
    h, remaining = hashlib.sha256(), prefix['bytes']
    with (bank / 'events.jsonl').open('rb') as stream:
        while remaining:
            block = stream.read(min(remaining, 4*1024*1024))
            assert block
            h.update(block)
            remaining -= len(block)
    assert h.hexdigest() == prefix['sha256']
    assert digest(bank / 'engine-upgrade-v1.json') == deployment['bridgeSha256']
    view = json.loads((ROOT / 'tartarus-current-native-state.json').read_bytes())
    result = dict(observedUtc=now.isoformat(), pid=pid,
        binarySha256=deployment['binarySha256'], sourceCommit=deployment['sourceCommit'],
        progress=progress, fullOriginalPrefixVerified=True, originalFinancialPrefix=prefix,
        originalFillCount=len(original['fills']), originalFillsUnchanged=True,
        currentFillCount=len(current['fills']), nativeEngineUnchanged=True,
        engineIdentity=deployment['engineIdentity'], allPriorEngineBridgesPreserved=True,
        unchangedBridgeSha256=deployment['bridgeSha256'],
        reportSha256=digest(bank/'report.json'),
        financial={k: current['report'][k] for k in ['cash', 'reservedCash', 'ownedCost', 'realized']},
        knownPhysicalOrders=view['knownUnclosedAcknowledgments'],
        accountRequestsByObserver=0, filledMinuteTargetVerified=False,
        wholeAccountProfitEstablished=False)
    with (WORK/'fresh-live.public.json').open('x') as stream:
        stream.write(json.dumps(result, indent=2)+'\n')
    print(json.dumps(result))


if __name__ == '__main__':
    main()
