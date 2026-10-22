"""Publish one exact Moloch execution-owner review under existing human authority.

No account client or order. The official worker independently reconciles the
account, open orders, sources and physical capability. A different unexpired
nomination needs its exact original intent and closed record; no clock renewal.
"""
import argparse
from datetime import datetime, timezone
from hashlib import sha256
import json
import os
from pathlib import Path
import time


def digest(raw):
    return sha256(raw).hexdigest()


def private(path):
    for ancestor in path.parents:
        assert not ancestor.is_symlink(), 'PRIVATE_PARENT_LINK'
    assert not path.is_symlink(), 'PRIVATE_LINK'
    stat = path.stat()
    assert stat.st_uid == os.getuid() and stat.st_nlink == 1 and stat.st_mode & 0o077 == 0, 'PRIVATE_OWNER_MODE'


def create(path, raw):
    with path.open('xb') as stream:
        stream.write(raw)
        stream.flush()
        os.fsync(stream.fileno())


def completed_prior_review(root, view, old_sha, qid):
    assert qid not in view['knownUnclosedAcknowledgments'] and not any(q['id'] == qid for q in view['activeNativeOrders']), 'OLD_REVIEW_STILL_HAS_NATIVE_OR_PHYSICAL_LIABILITY'
    intent, closed = False, None
    private(root / 'dispatch.ndjson')
    with (root / 'dispatch.ndjson').open('rb') as stream:
        for line in stream:
            assert len(line) <= 16 * 1024 * 1024, 'DISPATCH_MEMBER_BOUND'
            event = json.loads(line)['event']
            if event.get('qid') != qid:
                continue
            if event['kind'] == 'intent':
                intent = (event.get('rankCampaign') or {}).get('rawSha256') == old_sha
            if event['kind'] == 'closed':
                closed = digest(line)
    assert intent and closed is not None, 'ORIGINAL_MATCHING_RANK_INTENT_AND_TERMINAL_REQUIRED'
    return dict(qid=qid, priorRawSha256=old_sha, originalClosedLineSha256=closed)


def run(a):
    os.umask(0o077)
    assert len(a.source_commit) == 40 and all(c in '0123456789abcdef' for c in a.source_commit), 'CHECKED_SOURCE_COMMIT'
    root = a.runtime.resolve(strict=True)
    assert root == a.runtime and not (root / 'EXECUTION_STOP').exists() and not (root.parent / 'EXECUTION_STOP').exists()
    process = Path('/proc', str(a.expected_pid))
    assert digest((process / 'exe').read_bytes()) == a.expected_binary_sha256, 'WORKER_BINARY'
    command = (process / 'cmdline').read_bytes().split(b'\0')[:-1]
    assert len(command) == 3 and command[1] == b'run', 'WORKER_ARGUMENTS'
    control = json.loads(Path(os.fsdecode(command[2])).read_bytes())
    assert control['root'] == str(root), 'WORKER_ROOT'
    settings = control['rankCampaign']
    owner = root / 'rank-campaign-owner'
    assert settings == dict(nominationPath=str(owner / 'nomination.json'),
        originalsDirectory=str(owner / 'originals'), ownerApprovalPath=str(owner / 'owner-approval.json')), 'CONFIGURED_OWNER_FILES'
    raw = (a.prepared / 'nomination.json').read_bytes()
    assert digest(raw) == a.expected_raw_sha256, 'EXACT_REVIEWED_NOMINATION'
    summary = json.loads((a.prepared / 'prepared.public.json').read_bytes())
    assert summary['rawSha256'] == a.expected_raw_sha256 and not summary['approvalPublished'] and not summary['submitted']
    campaign = json.loads(raw)
    now = time.time_ns() // 1000000
    until = campaign['validUntilMs']
    assert campaign['objective'] == 'declared_positive_expected_money' and campaign['campaignStage'] == 'direct'
    assert campaign['reviewedAtMs'] <= now < until, 'NO_EXPIRED_REVIEW_RENEWAL'
    for source in campaign['sources']:
        assert source['observedAtMs'] <= now < source['observedAtMs'] + source['maximumAgeMs'], 'ORIGINAL_SOURCE_EXPIRED'
        body = (a.prepared / 'originals' / (source['sha256'] + '.bin')).read_bytes()
        assert digest(body) == source['sha256'], 'ORIGINAL_SOURCE_PIN'
    nomination = owner / 'nomination.json'
    approval = owner / 'owner-approval.json'
    private(nomination)
    private(approval)
    old_nomination, old_approval = nomination.read_bytes(), approval.read_bytes()
    old = json.loads(old_approval)
    assert old['schemaVersion'] == 1 and old['runId'] == control['activation']['runId'] and old['accountScope'] == control['accountScope']
    active = [row for row in old['approvedCampaigns'] if row['approvedAtMs'] <= now < row['validUntilMs']]
    if digest(old_nomination) == a.expected_raw_sha256 and any(row['rawSha256'] == a.expected_raw_sha256 for row in active):
        print(json.dumps(dict(state='already_published', rawSha256=a.expected_raw_sha256, clocksRenewed=False)))
        return
    view_raw = (root / 'tartarus-current-native-state.json').read_bytes()
    view = json.loads(view_raw)
    assert not view['unresolvedOrdinaryAttempt'], 'UNRESOLVED_ORIGINAL_ATTEMPT'
    superseded = None
    if active:
        qid = a.completed_qid
        old_sha = digest(old_nomination)
        assert qid is not None and all(row['rawSha256'] == old_sha for row in active), 'DIFFERENT_UNEXPIRED_REVIEW_REQUIRES_EXACT_COMPLETION_REVIEW'
        superseded = completed_prior_review(root, view, old_sha, qid)
    assert all(campaign['account'][key] == view['account'][key] for key in
        ('positionsSha256', 'cash', 'reservedCash', 'actualFillsSha256')), 'CURRENT_NATIVE_FINANCIAL_CHANGED'
    # A known resting order is retained and disclosed. It is not a permission
    # to dispatch: the worker still requires its actual terminal reconciliation.
    original_directory = owner / 'originals'
    for source in campaign['sources']:
        path = original_directory / (source['sha256'] + '.bin')
        body = (a.prepared / 'originals' / path.name).read_bytes()
        if path.exists():
            private(path)
            assert digest(path.read_bytes()) == source['sha256']
        else:
            create(path, body)
    stamp = datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%S')
    history = owner / 'review-history'
    history.mkdir(mode=0o700, exist_ok=True)
    archive = history / (stamp + '-' + a.expected_raw_sha256[:16])
    archive.mkdir(mode=0o700)
    create(archive / 'nomination.previous.json', old_nomination)
    create(archive / 'owner-approval.previous.json', old_approval)
    reviewed = dict(old, approvedCampaigns=[dict(rawSha256=a.expected_raw_sha256,
        approvedAtMs=now, validUntilMs=until)], reviewerId='Viserion-Moloch-execution-owner',
        sourceCommit=a.source_commit, basis='Existing direct human Moloch production authority; exact new monetary review, not an agent handoff or renewal.')
    next_approval = archive / 'owner-approval.next.json'
    next_nomination = archive / 'nomination.next.json'
    create(next_approval, (json.dumps(reviewed, indent=2) + '\n').encode())
    create(next_nomination, raw)
    assert nomination.read_bytes() == old_nomination and approval.read_bytes() == old_approval, 'OWNER_FILES_CHANGED'
    assert (root / 'tartarus-current-native-state.json').read_bytes() == view_raw, 'NATIVE_VIEW_CHANGED_BEFORE_PUBLICATION'
    assert not (root / 'EXECUTION_STOP').exists()
    os.replace(next_approval, approval)
    os.replace(next_nomination, nomination)
    for directory in (owner, original_directory, archive, history):
        fd = os.open(directory, os.O_RDONLY)
        os.fsync(fd)
        os.close(fd)
    result = dict(state='exact_execution_owner_review_published', rawSha256=digest(nomination.read_bytes()),
        approvalSha256=digest(approval.read_bytes()), approvedAtMs=now, validUntilMs=until,
        expectedWorkerPid=a.expected_pid, sourceCommit=a.source_commit,
        knownPhysicalOrdersStillRequireReconciliation=view['knownUnclosedAcknowledgments'],
        completedPriorReview=superseded,
        sourceClocksRenewed=False, historicalOwnerFilesArchived=True, accountClientsCreated=0,
        ordersSubmittedByPublisher=0, officialWorkerMayActAfterIndependentChecks=True)
    create(a.prepared / 'owner-publication.public.json', (json.dumps(result, indent=2) + '\n').encode())
    print(json.dumps(result), flush=True)


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--runtime', type=Path, required=True)
    p.add_argument('--prepared', type=Path, required=True)
    p.add_argument('--expected-pid', type=int, required=True)
    p.add_argument('--expected-binary-sha256', required=True)
    p.add_argument('--expected-raw-sha256', required=True)
    p.add_argument('--source-commit', required=True)
    p.add_argument('--completed-qid', type=int)
    run(p.parse_args())
