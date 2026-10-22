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
import subprocess
import time
from review_generation_window import publication_boundary, require_current_generation


# Load the checked sibling without changing the public research module path.
import importlib.util
_projection_spec = importlib.util.spec_from_file_location('money_reservation_projection', Path(__file__).with_name('money-reservation-projection.py'))
_projection_module = importlib.util.module_from_spec(_projection_spec)
_projection_spec.loader.exec_module(_projection_module)
reservation_projection = _projection_module.project


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


def completed_prior_review(root, view, old_sha, qid, completion=None):
    assert qid not in view['knownUnclosedAcknowledgments'] and not any(q['id'] == qid for q in view['activeNativeOrders']), 'OLD_REVIEW_STILL_HAS_NATIVE_OR_PHYSICAL_LIABILITY'
    intent, closed, not_sent = False, None, None
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
            if event['kind'] == 'admitted_pre_network_not_sent':
                not_sent = digest(line)
    assert intent, 'ORIGINAL_MATCHING_RANK_INTENT_REQUIRED'
    if closed is not None:
        return dict(qid=qid, priorRawSha256=old_sha, originalClosedLineSha256=closed)
    assert not_sent is not None and completion is not None, 'ORIGINAL_MATCHING_RANK_INTENT_AND_TERMINAL_REQUIRED'
    # This is explicitly a research view, not an SDK capability. The verifier
    # below independently restores the original opaque proof from SDK files.
    assert view['unresolvedOrdinaryAttempt'] is False
    verifier, expected = completion['verifier'], completion['verifierSha256']
    assert verifier is not None and expected is not None, 'PRIOR_TYPED_NOT_SENT_VERIFIER_REQUIRED'
    verifier = Path(verifier)
    assert verifier.is_absolute() and verifier.resolve(strict=True) == verifier and not verifier.is_symlink()
    info = verifier.stat()
    assert info.st_uid == os.getuid() and info.st_nlink == 1 and info.st_mode & 0o022 == 0
    assert digest(verifier.read_bytes()) == expected, 'PRIOR_NOT_SENT_VERIFIER_CHANGED'
    report_raw = (root/'bank-all-markets-v1/report.json').read_bytes()
    report = json.loads(report_raw)
    quote = next(q for q in report['orders'] if q['id'] == qid)
    assert quote['retired'] is True and quote['filled'] == 0
    assert not any(row['orderId'] == qid for row in report['fills']), 'PRIOR_NOT_SENT_HAS_FILL'
    raw = (root/'dispatch.ndjson').read_bytes()
    assert raw.endswith(b'\n'), 'PRIOR_DISPATCH_SNAPSHOT_TORN'
    snapshot = completion['prepared']/('prior-completion-' + str(qid) + '-' + digest(raw)[:16] + '.ndjson')
    if not snapshot.exists():
        create(snapshot, raw)
    private(snapshot)
    assert snapshot.read_bytes() == raw
    checked = subprocess.run([str(verifier),str(completion['control']),str(snapshot),str(qid),old_sha],
        capture_output=True,text=True,timeout=90)
    assert checked.returncode == 0, 'PRIOR_TYPED_NOT_SENT_VERIFICATION_REJECTED'
    proof = json.loads(checked.stdout)
    assert proof['qid'] == qid and proof['priorRawSha256'] == old_sha
    assert proof['kind'] == 'positively_proved_pre_network_not_sent' and proof['opaqueSdkProofVerified'] is True
    assert proof['dispatchSnapshotSha256'] == digest(raw) and proof['retryAuthorized'] is False
    assert proof['exchangeClosedClaimed'] is False and proof['accountRequests'] == 0 and proof['dispatchSnapshotModified'] is False
    assert digest(verifier.read_bytes()) == expected and snapshot.read_bytes() == raw
    return dict(qid=qid,priorRawSha256=old_sha,completionKind=proof['kind'],originalNotSentLineSha256=not_sent,
        originalReportSha256=digest(report_raw),verifierSha256=expected,typedVerification=proof,
        exchangeClosedClaimed=False,retryAuthorized=False)


def expired_unused_review(root, view, old_sha, until):
    # Observe the serialized worker's RETURNING expiry path, rather than
    # replacing owner pins while an old callback might still be using them.
    status_path = root/'rank-campaign-status.json'
    private(status_path)
    status_raw = status_path.read_bytes()
    status = json.loads(status_raw)
    at = int(datetime.fromisoformat(status['at'].replace('Z', '+00:00')).timestamp()*1000)
    assert at >= until and status.get('state') == 'deferred' and status.get('stage') == 'owner_review' and status.get('physicalAttempted') is False and status.get('reason') in ('RANK_CAMPAIGN_NOT_OWNER_APPROVED', 'ORIGINAL_REVIEW_EXPIRED_BEFORE_PHYSICAL_ATTEMPT'), 'EXPIRED_REVIEW_RUNTIME_BOUNDARY_REQUIRED'
    assert not any(q.get('reviewedRankCampaign') for q in view['activeNativeOrders']), 'NATIVE_RANK_RESERVATION_STILL_ACTIVE'
    private(root/'dispatch.ndjson')
    chain = __import__('hashlib').sha256()
    with (root/'dispatch.ndjson').open('rb') as stream:
        for line in stream:
            assert line.endswith(b'\n') and len(line) <= 16*1024*1024
            chain.update(line)
            event = json.loads(line)['event']
            assert not (event['kind'] == 'intent' and (event.get('rankCampaign') or {}).get('rawSha256') == old_sha), 'EXPIRED_REVIEW_HAS_INTENT_REQUIRES_EXACT_COMPLETION'
    return dict(priorRawSha256=old_sha, expiryObservedByWorkerAt=status['at'],
                originalStatusSha256=digest(status_raw), originalDispatchSha256=chain.hexdigest(),
                originalIntentAbsent=True, sourceClocksRenewed=False)


def run(a):
    # The source updater uses this same local boundary before issuing a reload.
    # It is not an account lock and grants no order or source-clock authority.
    with publication_boundary(a.runtime):
        raw = (a.prepared / 'nomination.json').read_bytes()
        assert digest(raw) == a.expected_raw_sha256, 'EXACT_REVIEWED_NOMINATION'
        review = json.loads(raw)
        require_current_generation(a.runtime, getattr(a, 'source_reference_root', None), review, a.expected_pid)
        return _run(a)


def _run(a):
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
    if campaign['objective'] == 'declared_positive_expected_money_rotation':
        assert getattr(a, 'allow_conditional_rotation', False), 'EXPLICIT_CONDITIONAL_ROTATION_SELECTION_REQUIRED'
        support = getattr(a, 'conditional_support_proof', None)
        assert support is not None, 'QUALIFIED_CONDITIONAL_ROTATION_BINARY_REQUIRED'
        capability = json.loads(support.read_bytes())
        assert capability['binarySha256'] == a.expected_binary_sha256 and capability['conditionalMonetaryRotationObjectiveQualified'] is True, 'QUALIFIED_CONDITIONAL_ROTATION_BINARY_REQUIRED'
        assert campaign['campaignStage'] == 'rotation_prerequisite' and campaign['action']['action'] == 'SELL'
        plan = campaign['contingentPlan']
        assert plan['executionAuthority'] is False and plan['actualFillAndFreshReapprovalRequired'] is True
        assert plan['minimumTargetGainPpm'] == campaign['risk']['minimumTargetGainPpm'] == 0
        assert summary['objective'] == campaign['objective'] and summary['monetaryFloorAppliesTo'] == 'full_conditional_rotation'
        assert summary['conditionalFuturePhysicalLegs'] == len(plan['tradeLegs']) > 0
        assert summary['futureLiquidityCertified'] is False and summary['actualFutureProceedsCredited'] is False
        assert summary['weakestDeclaredExpectedChange'] > 0 and float(summary['ordinaryMinimumExpectedChange']) > 0
    else:
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
    completion = dict(prepared=a.prepared,control=Path(os.fsdecode(command[2])),
        verifier=getattr(a,'prior_not_sent_verifier',None),verifierSha256=getattr(a,'prior_not_sent_verifier_sha256',None))
    if active:
        qid = a.completed_qid
        old_sha = digest(old_nomination)
        assert qid is not None and all(row['rawSha256'] == old_sha for row in active), 'DIFFERENT_UNEXPIRED_REVIEW_REQUIRES_EXACT_COMPLETION_REVIEW'
        superseded = completed_prior_review(root, view, old_sha, qid, completion)
    elif a.completed_qid is not None:
        superseded = completed_prior_review(root, view, digest(old_nomination), a.completed_qid, completion)
    else:
        superseded = expired_unused_review(root, view, digest(old_nomination), json.loads(old_nomination)['validUntilMs'])
    assert all(campaign['account'][key] == view['account'][key] for key in
        ('positionsSha256', 'cash', 'actualFillsSha256')), 'CURRENT_NATIVE_FINANCIAL_CHANGED'
    if 'unsubmittedDraftYield' in campaign['account']:
        projection = reservation_projection(view, campaign['action']['exchangeId'])
        # The nomination retains its ORIGINAL capture IDs/clocks. Ordinary
        # never-sent drafts can change without changing cash/holdings/fills.
        # Recompute the exact CURRENT yield independently; only an identical
        # post-yield reservation can support the unchanged financial model.
        original_projection = campaign['account']['unsubmittedDraftYield']
        assert original_projection['actualCashCredited'] == '0.000000' and original_projection['executionAuthority'] is False
        assert campaign['account']['reservedCash'] == original_projection['expectedReservedCash'] == projection['expectedReservedCash'], 'POST_YIELD_RESERVATION_MISMATCH'
    else:
        assert campaign['account']['reservedCash'] == view['account']['reservedCash'], 'CURRENT_NATIVE_RESERVATION_CHANGED'
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
    now = time.time_ns() // 1000000
    assert campaign['reviewedAtMs'] <= now < until, 'ORIGINAL_REVIEW_EXPIRED_DURING_PUBLICATION'
    assert all(source['observedAtMs'] <= now < source['observedAtMs'] + source['maximumAgeMs']
        for source in campaign['sources']), 'ORIGINAL_SOURCE_EXPIRED_DURING_PUBLICATION'
    reviewed = dict(old, approvedCampaigns=[dict(rawSha256=a.expected_raw_sha256,
        approvedAtMs=now, validUntilMs=until)], reviewerId='Viserion-Moloch-execution-owner',
        sourceCommit=a.source_commit, monetaryReviewObjective=campaign['objective'],
        basis='Existing direct human Moloch production authority; exact new monetary review, not an agent handoff or renewal.')
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
        completedPriorReview=superseded if superseded and 'qid' in superseded else None,
        expiredUnusedPriorReview=superseded if superseded and 'qid' not in superseded else None,
        currentReservationProjection=projection if 'unsubmittedDraftYield' in campaign['account'] else None,
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
    p.add_argument('--prior-not-sent-verifier',type=Path)
    p.add_argument('--prior-not-sent-verifier-sha256',
        help='Exact checked file-only SDK verifier hash; required when the completed predecessor never reached the network.')
    p.add_argument('--source-reference-root', type=Path,
        help='Existing source updater root; required for a managed review so unresolved reloads and changed generations are rejected before publication.')
    p.add_argument('--allow-conditional-rotation', action='store_true',
        help='Explicitly select a conditional positive-money funding SELL; later orders require actual fills and fresh reviews.')
    p.add_argument('--conditional-support-proof', type=Path,
        help='Checked qualification of this exact worker binary for conditional monetary rotations.')
    run(p.parse_args())
