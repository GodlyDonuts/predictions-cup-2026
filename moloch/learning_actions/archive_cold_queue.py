"""Finite, verified archival of cold SDK generations while the owner runs.

No HTTP, account client, trading command, manifest update, or active-file write.
The current manifest, its source journal and every file open by the pinned owner
are excluded. Selection may only advance. All archived bytes are decoded and
hashed before any original is unlinked. A separate OS lease serializes archives.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import time
from .preserve_superseded_queue import PATTERN, ROOT, GIB, selected, require, save, digest, archive_batch


def identity(path):
    s = path.stat()
    return s.st_dev, s.st_ino


class ColdQueue:
    def __init__(self, queue, *, owner_check, open_files, minimum_age_seconds=86400):
        self.queue = Path(queue).resolve()
        require(type(minimum_age_seconds) is int and minimum_age_seconds >= 86400,
                'AT_LEAST_ONE_DAY_COLD_RETENTION')
        self.cutoff_time = time.time() - minimum_age_seconds
        self.owner_check = owner_check
        self.open_files = open_files
        raw, _, self.anchor = selected(self.queue)
        self.scope = json.loads(raw).get('scope')
        require(type(self.scope) is str and bool(self.scope), 'ORIGINAL_QUEUE_SCOPE_REQUIRED')
        self.last_anchor = self.anchor
        self.selected_raw = raw
        self.owner_check()

    def protected(self):
        self.owner_check()
        raw, names, anchor = selected(self.queue)
        value = json.loads(raw)
        require(value.get('schema_version') == 1 and value.get('scope') == self.scope,
                'ORIGINAL_QUEUE_SCHEMA_AND_SCOPE_REQUIRED')
        require(anchor >= self.last_anchor, 'QUEUE_SELECTION_REGRESSED')
        if anchor == self.last_anchor:
            require(raw == self.selected_raw, 'SAME_ANCHOR_SELECTION_CHANGED')
        checkpoint_path = self.queue / value['checkpoint']
        require(not checkpoint_path.is_symlink() and checkpoint_path.stat().st_size <= 32*1024**2,
                'BOUNDED_SELECTED_CHECKPOINT')
        checkpoint = json.loads(checkpoint_path.read_bytes())
        source = checkpoint['anchor']['source_journal']
        require(Path(source).name == source and '/' not in source and '\\' not in source,
                'LOCAL_SOURCE_JOURNAL_REQUIRED')
        self.last_anchor, self.selected_raw = anchor, raw
        return names | {source}, self.open_files()

    def choose(self, maximum_bytes):
        protected, opened = self.protected()
        choices = []
        for p in self.queue.iterdir():
            m = PATTERN.fullmatch(p.name)
            if not m or int(m[2]) >= self.anchor or p.name in protected:
                continue
            if (m[1] == 'queue') != (m[4] == 'jsonl'):
                continue
            require(p.is_file() and not p.is_symlink(), 'REGULAR_COLD_FILE_REQUIRED')
            s = p.stat()
            if s.st_mtime > self.cutoff_time or (s.st_dev, s.st_ino) in opened:
                continue
            choices.append((int(m[2]), p))
        result, size = [], 0
        for _, p in sorted(choices, key=lambda row: (row[0], row[1].name)):
            n = p.stat().st_size
            if size + n > maximum_bytes:
                break
            result.append(p); size += n
        return result

    def check(self, paths):
        protected, opened = self.protected()
        for p in paths:
            m = PATTERN.fullmatch(p.name)
            require(p.parent == self.queue and p.resolve() == p and not p.is_symlink()
                    and m is not None and int(m[2]) < self.anchor and p.name not in protected,
                    'COLD_FILE_MUST_REMAIN_SUPERSEDED')
            require(p.stat().st_mtime <= self.cutoff_time and identity(p) not in opened,
                    'COLD_FILE_CHANGED_OR_OPEN')


class LiveOwner:
    def __init__(self, pid, binary_sha256):
        require(os.name == 'posix' and type(pid) is int and pid > 0, 'EXACT_LINUX_OWNER')
        self.pid = pid
        self.proc = Path('/proc') / str(pid)
        self.start = self.start_ticks()
        self.executable = (self.proc / 'exe').resolve()
        require(digest(self.proc / 'exe') == binary_sha256, 'LIVE_OWNER_BINARY_CHANGED')
        self.lease = ROOT / 'account-queue/offline-account-queue.lock'
        self.lease_bytes = self.lease.read_bytes()
        self.lease_id = identity(self.lease)
        require(json.loads(self.lease_bytes)['pid'] == pid, 'SDK_OWNER_MISMATCH')
        self.capture = ROOT.parent / 'capture.lock'
        self.capture_id = identity(self.capture)
        self.check()

    def start_ticks(self):
        return int((self.proc / 'stat').read_text().rsplit(')', 1)[1].split()[19])

    def check(self):
        require(self.start_ticks() == self.start and (self.proc / 'exe').resolve() == self.executable,
                'ORIGINAL_OWNER_PROCESS_CHANGED')
        require(not self.lease.is_symlink() and identity(self.lease) == self.lease_id
                and self.lease.read_bytes() == self.lease_bytes, 'SDK_OWNER_LEASE_CHANGED')
        require(not self.capture.is_symlink() and identity(self.capture) == self.capture_id,
                'CAPTURE_LEASE_CHANGED')
        require(self.capture_id in self.open_files(), 'OWNER_CAPTURE_LEASE_NOT_OPEN')

    def open_files(self):
        opened = set()
        for fd in (self.proc / 'fd').iterdir():
            try:
                opened.add(identity(fd))
            except FileNotFoundError:
                pass
        return opened


def run(output, pid, binary_sha256, *, target_free_bytes=20*GIB, maximum_batches=32):
    import fcntl
    output = Path(output)
    require(ROOT.resolve() == ROOT and output.is_absolute() and output.resolve() == output
            and ROOT not in output.parents, 'ORIGINAL_ROOT_AND_EXTERNAL_ARCHIVE')
    require(type(target_free_bytes) is int and 0 < target_free_bytes <= 64*GIB
            and type(maximum_batches) is int and 1 <= maximum_batches <= 128, 'FINITE_ARCHIVE_BUDGET')
    owner = LiveOwner(pid, binary_sha256)
    queue = ROOT / 'account-queue'
    with (ROOT.parent / 'cold-queue-archive.lock').open('a+b') as lease:
        fcntl.flock(lease, fcntl.LOCK_EX | fcntl.LOCK_NB)
        output.mkdir(exist_ok=False)
        guard = ColdQueue(queue, owner_check=owner.check, open_files=owner.open_files)
        save(output / 'plan.json', dict(pid=pid, binarySha256=binary_sha256,
             initialGeneration=guard.anchor, minimumColdAgeSeconds=86400,
             targetFreeBytes=target_free_bytes, maximumBatches=maximum_batches,
             currentSourceJournalAndOpenFilesExcluded=True, accountRequests=0))
        receipts = []
        for i in range(maximum_batches):
            if shutil.disk_usage(queue).free >= target_free_bytes:
                break
            receipt = archive_batch(queue, output / f'batch-{i:03d}', GIB, cold_guard=guard)
            if receipt is None:
                break
            receipts.append(receipt)
            print(json.dumps(dict(batch=i, originalBytes=receipt['originalBytes'],
                  archiveBytes=receipt['archiveBytes'], freeBytes=shutil.disk_usage(queue).free)), flush=True)
        result = dict(completedBatches=len(receipts), originalBytes=sum(r['originalBytes'] for r in receipts),
                      archiveBytes=sum(r['archiveBytes'] for r in receipts),
                      freeBytes=shutil.disk_usage(queue).free, financialStateChanged=False,
                      accountRequests=0, allOriginalBytesRecoverable=True,
                      observedUtc=time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()))
        save(output / 'RESULT.json', result)
        return result


def main(argv=None):
    # `python -m` loads this file as __main__. Use the canonical module's run
    # so its guard has the exact class identity required by the archive core.
    from .archive_cold_queue import run as canonical_run
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--pid', type=int, required=True)
    parser.add_argument('--binary-sha256', required=True)
    args = parser.parse_args(argv)
    print(json.dumps(canonical_run(args.output, args.pid, args.binary_sha256)), flush=True)


if __name__ == '__main__':
    main()
