"""Recover only a byte-proved, uncommitted READ admission after storage failure.

No API client, credentials, dispatch, state reset or selected-generation change.
The original journal is archived and decoded before the incomplete suffix is
removed. Completed records and the selected checkpoint remain byte-identical.
"""
import argparse
import copy
import gzip
import hashlib
import json
import os
from pathlib import Path
from datetime import datetime, timezone

ROOT = Path('/home/archon/apps/moloch/shared/live-production-v1')


def raw(value):
    return json.dumps(value, separators=(',', ':'), ensure_ascii=False).encode()


def sha(value):
    return hashlib.sha256(value).hexdigest()


def require(ok, reason):
    if not ok:
        raise ValueError(reason)


def analyze(manifest_raw, checkpoint_raw, journal_raw):
    manifest, cp = json.loads(manifest_raw), json.loads(checkpoint_raw)
    require(cp['hash'] == manifest['checkpoint_hash'] ==
            sha(raw([1, cp['anchor'], cp['state']])), 'CHECKPOINT_HASH')
    scope = manifest['scope']
    require(cp['schema_version'] == cp['state']['schema_version'] == 1
            and cp['state']['scope'] == scope, 'CHECKPOINT_SCOPE')
    sequence, previous, state = cp['anchor']['sequence'], cp['anchor']['previous'], cp['state']
    prefix = 0
    count = 0
    for line in journal_raw.splitlines(keepends=True):
        if not line.endswith(b'\n'):
            break
        row = json.loads(line)
        require(row['sequence'] == sequence + 1 and row['previous'] == previous
                and row['hash'] == sha(raw([row['sequence'], row['previous'], row['state']]))
                and row['state']['scope'] == scope and row['state']['schema_version'] == 1,
                'COMPLETE_JOURNAL_INTEGRITY')
        sequence, previous, state = row['sequence'], row['hash'], row['state']
        prefix += len(line)
        count += 1
    tail = journal_raw[prefix:]
    require(tail and b'\n' not in tail, 'ONE_INCOMPLETE_SUFFIX_REQUIRED')
    require(state['active'] is None and not state['unknown'] and state['manual_review'] is False,
            'LAST_COMPLETE_QUEUE_NOT_QUIET')
    require(state['read_profile'] == {'cap': 100, 'gap_ms': 600}, 'ORIGINAL_READ_PROFILE')
    text = tail.decode()
    decoder = json.JSONDecoder()
    clock = decoder.raw_decode(text.split('"last_clock":', 1)[1])[0]
    flight = decoder.raw_decode(text.split('"active":', 1)[1])[0]
    require(type(clock) is int and state['last_clock'] <= clock <= 8640000000000000,
            'MONOTONIC_READ_CLOCK')
    require(isinstance(flight, dict) and set(flight) == {'request', 'write', 'binding', 'network_started'}
            and isinstance(flight['request'], str) and flight['request']
            and flight['write'] is False and flight['binding'] is None
            and flight['network_started'] is False, 'ONLY_UNSTARTED_READ_ADMISSION')
    expected = copy.deepcopy(state)
    expected['last_clock'] = clock
    for name in ('reads', 'writes'):
        expected[name] = [v for v in expected[name] if max(0, clock - v) <= 60000]
    ready = max(expected['next'], expected['cooldown'],
                expected['reads'][0] + 60001 if len(expected['reads']) >= 100 else 0)
    require(clock >= ready, 'READ_WAS_NOT_ADMISSIBLE')
    expected['next'] = clock + 600
    expected['reads'].append(clock)
    expected['active'] = flight
    record = {'sequence': sequence + 1, 'previous': previous, 'state': expected,
              'hash': sha(raw([sequence + 1, previous, expected]))}
    encoded = raw(record) + b'\n'
    require(len(tail) < len(encoded) and encoded.startswith(tail), 'EXACT_UNSTARTED_READ_PREFIX_REQUIRED')
    return {'completeRecordsVerified': count, 'lastCompleteSequence': sequence,
            'lastCompletePrevious': previous, 'retainedPrefixBytes': prefix,
            'retainedPrefixSha256': sha(journal_raw[:prefix]),
            'originalJournalBytes': len(journal_raw), 'originalJournalSha256': sha(journal_raw),
            'uncommittedTailBytes': len(tail), 'uncommittedTailSha256': sha(tail),
            'unstartedReadRequest': flight['request'], 'unknownCount': 0,
            'originalIntentCount': len(state['intents']), 'originalKeyCount': len(state['keys']),
            'financialStateChanged': False, 'completeRecordsRemoved': False}


def repair(root, *, execute=False):
    root = Path(root)
    require(os.name == 'posix' and root == ROOT and root.resolve() == root, 'ORIGINAL_CANONICAL_LINUX_ROOT')
    import fcntl
    queue = root / 'account-queue'
    paths = [root / 'EXECUTION_STOP', root.parent / 'NETWORK_DISABLED',
             queue / 'account-queue-manifest.json',
             root.parent / 'moloch-live-tartarus-r57-g12-20261019.json']
    for p in paths:
        require(p.is_file() and not p.is_symlink(), 'ORIGINAL_STOP_OR_MANIFEST_REQUIRED')
    pins = {p: p.read_bytes() for p in paths}
    require(sha(pins[paths[0]]) == '1b1b24f1379d9e3a11c13991afd588d085dc16d99c392eda2a3920769c0b2043',
            'ORIGINAL_EXECUTION_STOP_CHANGED')
    require(pins[paths[1]] == b'', 'ORIGINAL_NETWORK_MARKER_CHANGED')
    require(sha(pins[paths[3]]) == 'bed178634e699f969a344bb3c1aa3a595c37ef40db1198ee9935a87cce00467a',
            'ORIGINAL_CONTROL_CHANGED')
    owner_path = Path(json.loads(pins[paths[3]])['ownerLock'])
    require(owner_path == root.parent / 'capture.lock' and owner_path.is_file()
            and not owner_path.is_symlink(), 'ORIGINAL_EXECUTION_OWNER_LOCK')
    manifest = json.loads(pins[paths[2]])
    for key in ('journal', 'checkpoint'):
        name = manifest[key]
        require(isinstance(name, str) and Path(name).name == name and '..' not in name,
                'LOCAL_SELECTED_GENERATION_ONLY')
    journal, checkpoint = (queue / manifest[k] for k in ('journal', 'checkpoint'))
    require(not journal.is_symlink() and not checkpoint.is_symlink(), 'REGULAR_SELECTED_FILES')
    lock = queue / 'offline-account-queue.lock'
    identity = raw({'scope': manifest['scope'], 'token': 'read-tail-recovery-' + str(os.getpid()), 'pid': os.getpid()})
    # Same create-new exclusion used by the SDK; no stale-lock removal.
    with owner_path.open('rb') as owner:
        fcntl.flock(owner, fcntl.LOCK_EX | fcntl.LOCK_NB)
        with lock.open('xb') as handle:
            handle.write(identity)
            handle.flush()
            os.fsync(handle.fileno())
        try:
            original = journal.read_bytes()
            pins[checkpoint] = checkpoint.read_bytes()
            proof = analyze(pins[paths[2]], pins[checkpoint], original)
            if not execute:
                return proof
            incident = root.parent / 'incidents' / ('history-read-tail-' +
                       datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%S%fZ'))
            incident.mkdir(exist_ok=False)
            archive = incident / 'original-journal.jsonl.gz'
            with archive.open('xb') as output:
                with gzip.GzipFile(fileobj=output, mode='wb', compresslevel=3, mtime=0) as compressed:
                    compressed.write(original)
                output.flush()
                os.fsync(output.fileno())
            with gzip.open(archive, 'rb') as source:
                require(sha(source.read()) == proof['originalJournalSha256'], 'ARCHIVE_DECODE_MISMATCH')
            proof.update(archive=str(archive), archiveSha256=sha(archive.read_bytes()),
                         archiveVerifiedBeforeTruncate=True)
            with (incident / 'before.json').open('xb') as report:
                report.write(raw(proof)); report.flush(); os.fsync(report.fileno())
            directory = os.open(incident, os.O_RDONLY)
            try: os.fsync(directory)
            finally: os.close(directory)
            require(all(p.read_bytes() == value for p, value in pins.items()), 'PINNED_CONTROL_CHANGED')
            require(lock.read_bytes() == identity and journal.read_bytes() == original, 'ORIGINAL_JOURNAL_OR_LEASE_CHANGED')
            with journal.open('r+b') as output:
                output.truncate(proof['retainedPrefixBytes'])
                output.flush()
                os.fsync(output.fileno())
            require(sha(journal.read_bytes()) == proof['retainedPrefixSha256'], 'REPAIRED_PREFIX_MISMATCH')
            require(all(p.read_bytes() == value for p, value in pins.items()), 'PINNED_CONTROL_CHANGED')
            proof.update(repairComplete=True, controlsUnchanged=True,
                         observedUtc=datetime.now(timezone.utc).isoformat())
            with (incident / 'complete.json').open('xb') as report:
                report.write(raw(proof)); report.flush(); os.fsync(report.fileno())
            return proof
        finally:
            if lock.exists() and lock.read_bytes() == identity:
                lock.unlink()


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--execute', action='store_true')
    args = parser.parse_args()
    print(json.dumps(repair(ROOT, execute=args.execute), indent=2))
