"""Use the existing verified cold-generation archiver without pausing trading.

Only generations older than a day are compressed. Every byte is decoded and
hashed before raw duplicates are unlinked. Current/open/source files are excluded.
"""
from pathlib import Path
import fcntl
import json
import shutil
import time
from moloch.learning_actions.archive_cold_queue import ColdQueue, LiveOwner
from moloch.learning_actions.preserve_superseded_queue import ROOT, GIB, archive_batch, save

PID = 1057647
BINARY = '94fc09256728e09aed80f78775220446222641fdcdd820f55858fbe3f0572721'
OUT = Path('/home/archon/apps/moloch/shared/account-queue-preserved-v1/r80-online-20261022')
WORK = Path('/tmp/moloch-managed-age-r80-20261022')


def main():
    owner = LiveOwner(PID, BINARY)
    queue = ROOT / 'account-queue'
    started = time.monotonic()
    receipts = []
    with (ROOT.parent / 'cold-queue-archive.lock').open('a+b') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        OUT.mkdir(exist_ok=False)
        guard = ColdQueue(queue, owner_check=owner.check, open_files=owner.open_files)
        save(OUT/'plan.json', dict(pid=PID, binarySha256=BINARY,
            initialGeneration=guard.anchor, minimumColdAgeSeconds=86400,
            targetFreeBytes=16*GIB, maximumBatches=80, maximumElapsedSeconds=900,
            batchBytes=GIB//4, accountRequests=0, workerPaused=False))
        for i in range(80):
            if shutil.disk_usage(queue).free >= 16*GIB or time.monotonic()-started > 900:
                break
            receipt = archive_batch(queue, OUT/f'batch-{i:03d}', GIB//4, cold_guard=guard)
            if receipt is None:
                break
            receipts.append(receipt)
            progress = dict(batches=len(receipts), originalBytes=sum(r['originalBytes'] for r in receipts),
                archiveBytes=sum(r['archiveBytes'] for r in receipts),
                availableBytes=shutil.disk_usage(queue).free,
                elapsedSeconds=time.monotonic()-started)
            (WORK/'archive-progress.json').write_text(json.dumps(progress))
            print(json.dumps(progress), flush=True)
        owner.check()
        result = dict(observedUtc=time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),
            pid=PID, batches=len(receipts), rawBytesRetainedLosslessly=sum(r['originalBytes'] for r in receipts),
            compressedBytes=sum(r['archiveBytes'] for r in receipts),
            availableBytes=shutil.disk_usage(queue).free,
            originalOwnerUnchanged=True, workerPaused=False, accountRequests=0,
            financialStateChanged=False, allOriginalBytesRecoverable=True,
            elapsedSeconds=time.monotonic()-started, archiveDirectory=str(OUT))
        save(OUT/'RESULT.json', result)
        save(WORK/'archive-result.public.json', result)
        print(json.dumps(result), flush=True)


if __name__ == '__main__':
    main()
