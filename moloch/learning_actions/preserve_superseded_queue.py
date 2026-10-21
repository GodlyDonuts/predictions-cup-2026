"""Bounded byte-preserving archival of strictly superseded SDK generations.

This never removes the selected checkpoint/journal or non-generation proofs.
Archives must be explicitly extracted for later historical audits.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tarfile
from datetime import datetime, timezone

PATTERN = re.compile(r'^(queue|checkpoint)-([0-9]+)-([0-9]+)\.(jsonl|json)$')
ROOT = Path('/home/archon/apps/moloch/shared/live-production-v1')
GIB = 1024 ** 3


def digest(path):
    with Path(path).open('rb') as source:
        return hashlib.file_digest(source, 'sha256').hexdigest()


def require(ok, message):
    if not ok:
        raise ValueError(message)


def save(path, value):
    with Path(path).open('x', encoding='utf-8') as out:
        json.dump(value, out, indent=2)
        out.flush()
        os.fsync(out.fileno())


def selected(queue):
    path = queue / 'account-queue-manifest.json'
    raw = path.read_bytes()
    value = json.loads(raw)
    names = [value['journal'], value['checkpoint']]
    matches = [PATTERN.fullmatch(x) for x in names]
    require(all(matches) and names[0].startswith('queue-') and names[1].startswith('checkpoint-'),
            'ORDINARY_SELECTED_GENERATION_REQUIRED')
    require(matches[0].group(2, 3) == matches[1].group(2, 3), 'SELECTED_GENERATION_PAIR_REQUIRED')
    return raw, set(names), int(matches[0][2])


def choose(queue, maximum_bytes):
    _, excluded, anchor = selected(queue)
    rows = []
    for path in queue.iterdir():
        match = PATTERN.fullmatch(path.name)
        if (not match or path.name in excluded or int(match[2]) >= anchor
                or (match[1] == 'queue') != (match[4] == 'jsonl')):
            continue
        require(path.is_file() and not path.is_symlink(), 'REGULAR_SUPERSEDED_GENERATION_REQUIRED')
        rows.append((int(match[2]), path))
    result = []
    size = 0
    for _, path in sorted(rows, key=lambda row: (row[0], row[1].name)):
        n = path.stat().st_size
        if size + n > maximum_bytes:
            break
        result.append(path)
        size += n
    return result


def archive_batch(queue, destination, maximum_bytes, *, recheck=lambda: None,
                  cold_guard=None):
    """Offline callers hold owner/SDK locks; online callers supply ColdQueue.

    The online guard independently pins the live owner and admits only immutable
    generations older than a full day, below the initial selected generation.
    """
    queue, destination = Path(queue), Path(destination)
    manifest, excluded, _ = selected(queue)
    if cold_guard is None:
        paths = choose(queue, maximum_bytes)
    else:
        from .archive_cold_queue import ColdQueue
        require(type(cold_guard) is ColdQueue and cold_guard.queue == queue,
                'EXACT_COLD_QUEUE_GUARD_REQUIRED')
        paths = cold_guard.choose(maximum_bytes)
    def check_selection(remaining=None):
        if cold_guard is None:
            require(selected(queue)[0] == manifest, 'SELECTED_GENERATION_CHANGED')
        else:
            cold_guard.check(paths if remaining is None else remaining)
    if not paths:
        return None
    require(shutil.disk_usage(destination.parent).free > maximum_bytes + 128 * 1024 ** 2,
            'ARCHIVE_WORST_CASE_HEADROOM_REQUIRED')
    destination.mkdir(exist_ok=False)
    inventory = {p.name: {'bytes': p.stat().st_size, 'sha256': digest(p),
                         'inode': p.stat().st_ino, 'mtime_ns': p.stat().st_mtime_ns}
                 for p in paths}
    save(destination / 'inventory.json', inventory)
    archive = destination / 'originals.tar.zst'
    with archive.open('xb') as out:
        process = subprocess.Popen(['zstd', '-q', '-3', '-T1', '-c'], stdin=subprocess.PIPE,
                                   stdout=out, stderr=subprocess.PIPE)
        try:
            with tarfile.open(fileobj=process.stdin, mode='w|') as tar:
                for path in paths:
                    tar.add(path, arcname=path.name, recursive=False)
            process.stdin.close()
            error = process.stderr.read()
            require(process.wait() == 0, 'ARCHIVE_COMPRESSION_FAILED:' + error.decode(errors='replace')[:200])
        finally:
            if process.poll() is None:
                process.kill()
                process.wait()
            process.stdin.close()
            process.stderr.close()
        out.flush()
        os.fsync(out.fileno())
    decoded = set()
    process = subprocess.Popen(['zstd', '-q', '-d', '-c', str(archive)],
                               stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    try:
        with tarfile.open(fileobj=process.stdout, mode='r|') as tar:
            for member in tar:
                require(member.isfile() and member.name in inventory and member.name not in decoded,
                        'EXACT_ARCHIVE_MEMBER_INVENTORY')
                expected = inventory[member.name]
                require(member.size == expected['bytes'], 'ARCHIVE_MEMBER_SIZE')
                stream = tar.extractfile(member)
                h = hashlib.sha256()
                for block in iter(lambda: stream.read(1024 * 1024), b''):
                    h.update(block)
                require(h.hexdigest() == expected['sha256'], 'ARCHIVE_DECODED_HASH')
                decoded.add(member.name)
        # Drain any trailing tar padding and confirm decoder exit.
        while process.stdout.read(1024 * 1024):
            pass
        error = process.stderr.read()
        require(process.wait() == 0 and decoded == set(inventory), 'ARCHIVE_DECODE_FAILED')
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()
        process.stdout.close()
        process.stderr.close()
    receipt = {'archive': str(archive), 'archiveSha256': digest(archive),
               'archiveBytes': archive.stat().st_size, 'originalBytes': sum(x['bytes'] for x in inventory.values()),
               'fileCount': len(paths), 'fullDecompressionHashesVerified': True,
               'selectedManifestSha256': hashlib.sha256(manifest).hexdigest(),
               'historicalAuditRequiresExplicitExtraction': True}
    save(destination / 'verified-before-unlink.json', receipt)
    for folder in (destination, destination.parent, destination.parent.parent):
        directory = os.open(folder, os.O_RDONLY)
        try: os.fsync(directory)
        finally: os.close(directory)
    check_selection()
    recheck()
    for path in paths:
        expected = inventory[path.name]
        stat = path.stat()
        require(not path.is_symlink() and stat.st_ino == expected['inode']
                and stat.st_mtime_ns == expected['mtime_ns'] and stat.st_size == expected['bytes']
                and digest(path) == expected['sha256'], 'ORIGINAL_CHANGED_BEFORE_UNLINK')
    # All original bytes are now in a durable, fully decoded verified archive.
    for path in paths:
        recheck()
        check_selection([path])
        path.unlink()
    directory = os.open(queue, os.O_RDONLY)
    try: os.fsync(directory)
    finally: os.close(directory)
    receipt.update(complete=True, originalsRehashedBeforeUnlink=True,
                   selectedGenerationUntouched=True, financialStateChanged=False,
                   observedUtc=datetime.now(timezone.utc).isoformat())
    save(destination / 'complete.json', receipt)
    return receipt


def run(output):
    import fcntl
    output = Path(output)
    require(os.name == 'posix' and ROOT.resolve() == ROOT, 'ORIGINAL_LINUX_ROOT')
    require(output.is_absolute() and output.resolve() == output
            and ROOT not in output.parents, 'ARCHIVE_OUTSIDE_PRODUCTION_ROOT')
    output.mkdir(exist_ok=False)
    queue = ROOT / 'account-queue'
    control = ROOT.parent / 'moloch-live-tartarus-r57-g12-20261019.json'
    stop = ROOT / 'EXECUTION_STOP'
    network = ROOT.parent / 'NETWORK_DISABLED'
    require(digest(control) == 'bed178634e699f969a344bb3c1aa3a595c37ef40db1198ee9935a87cce00467a',
            'ORIGINAL_CONTROL')
    require(digest(stop) == '1b1b24f1379d9e3a11c13991afd588d085dc16d99c392eda2a3920769c0b2043'
            and network.read_bytes() == b'', 'ORIGINAL_EXECUTION_AND_NETWORK_STOPS')
    pinned = {p: (p.stat().st_ino, p.read_bytes()) for p in (control, stop, network)}
    manifest, names, _ = selected(queue)
    pinned.update({queue / name: ((queue / name).stat().st_ino, (queue / name).read_bytes())
                   for name in names})
    owner = Path(json.loads(control.read_text())['ownerLock'])
    require(owner == ROOT.parent / 'capture.lock' and not owner.is_symlink(), 'ORIGINAL_OWNER_LOCK')
    lock = queue / 'offline-account-queue.lock'
    identity = json.dumps({'scope': 'sig-midterm-shared-account',
                          'token': 'preserve-superseded-' + str(os.getpid()), 'pid': os.getpid()}).encode()
    def signature(path):
        stat = path.stat()
        return stat.st_dev, stat.st_ino, stat.st_size, stat.st_mtime_ns
    pinned_stats = {p: signature(p) for p in pinned}
    def recheck(full=False):
        require(lock.read_bytes() == identity and selected(queue)[0] == manifest, 'EXCLUSIVE_QUEUE_UNCHANGED')
        require(all(not p.is_symlink() and signature(p) == pinned_stats[p]
                    and (not full or p.read_bytes() == value)
                    for p, (inode, value) in pinned.items()), 'ORIGINAL_CONTROL_OR_GENERATION_CHANGED')
    receipts = []
    with owner.open('rb') as lease:
        fcntl.flock(lease, fcntl.LOCK_EX | fcntl.LOCK_NB)
        with lock.open('xb') as handle:
            handle.write(identity); handle.flush(); os.fsync(handle.fileno())
        try:
            while shutil.disk_usage(ROOT).free < 20 * GIB and len(receipts) < 24:
                recheck(full=True)
                receipt = archive_batch(queue, output / ('batch-%03d' % len(receipts)), GIB, recheck=recheck)
                if receipt is None:
                    break
                recheck(full=True)
                receipts.append(receipt)
                print(json.dumps({'batch': len(receipts), 'originalBytes': receipt['originalBytes'],
                                  'archiveBytes': receipt['archiveBytes'],
                                  'freeBytes': shutil.disk_usage(ROOT).free}), flush=True)
            recheck(full=True)
        finally:
            if lock.exists() and lock.read_bytes() == identity:
                lock.unlink()
    summary = {'complete': True, 'batches': receipts, 'freeBytes': shutil.disk_usage(ROOT).free,
               'accountRequests': 0, 'financialStateChanged': False,
               'activeGenerationUnchanged': True, 'maximumSourceBytes': 24 * GIB}
    save(output / 'complete.json', summary)
    return summary


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', required=True)
    args = parser.parse_args()
    run(args.output)
