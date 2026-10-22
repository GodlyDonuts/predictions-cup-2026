"""Local review/source publication coordination, never execution authority."""
from contextlib import contextmanager
from hashlib import sha256
import json
import os
from pathlib import Path
import stat
import time

try:
    import fcntl
except ImportError:
    fcntl = None

MAXIMUM_HOLD_MS = 300_000


class PublicationBusy(RuntimeError):
    pass


@contextmanager
def publication_boundary(root, inherited_fd=None, wait_seconds=0):
    if fcntl is None:
        raise RuntimeError('SOURCE_PUBLICATION_LOCK_REQUIRES_POSIX')
    parent = Path(root) / 'rank-campaign-owner'
    assert parent.is_dir() and not parent.is_symlink(), 'SOURCE_PUBLICATION_OWNER_DIRECTORY'
    path = parent / 'generation-publication.lock'
    fd = os.open(path, os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600) if inherited_fd is None else os.dup(inherited_fd)
    try:
        info = os.fstat(fd)
        assert stat.S_ISREG(info.st_mode) and info.st_uid == os.getuid() and info.st_nlink == 1 and info.st_mode & 0o077 == 0
        current = path.lstat()
        assert stat.S_ISREG(current.st_mode) and (info.st_dev,info.st_ino)==(current.st_dev,current.st_ino), 'SOURCE_PUBLICATION_DESCRIPTOR_CHANGED'
        deadline = time.monotonic()+wait_seconds
        while True:
            try:
                fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
                break
            except BlockingIOError as error:
                if time.monotonic() >= deadline:
                    raise PublicationBusy('SOURCE_GENERATION_PUBLICATION_BUSY') from error
                time.sleep(min(.05,max(0,deadline-time.monotonic())))
        yield fd
    finally:
        os.close(fd)


def active_managed_review(root, control, generation, now):
    """Delay a NEW source request only until the original bounded review ends."""
    try:
        paths = control['rankCampaign']
        owner = Path(root) / 'rank-campaign-owner'
        assert paths['nominationPath'] == str(owner / 'nomination.json')
        assert paths['ownerApprovalPath'] == str(owner / 'owner-approval.json')
        raw = (owner / 'nomination.json').read_bytes()
        approval_raw = (owner / 'owner-approval.json').read_bytes()
        assert len(raw) <= 2 * 1024 * 1024 and len(approval_raw) <= 64 * 1024
        review, approval = json.loads(raw), json.loads(approval_raw)
        proof = review.get('managedInventory')
        if not isinstance(proof, dict) or proof.get('generationSha256') != generation:
            return None
        assert approval['schemaVersion'] == 1 and approval['runId'] == control['activation']['runId']
        assert approval['accountScope'] == control['accountScope']
        reviewed, until = review['reviewedAtMs'], review['validUntilMs']
        assert type(reviewed) is int and type(until) is int and reviewed <= now < until
        raw_sha = sha256(raw).hexdigest()
        rows = [row for row in approval['approvedCampaigns'] if row['rawSha256'] == raw_sha
            and type(row['approvedAtMs']) is int and type(row['validUntilMs']) is int
            and row['approvedAtMs'] <= now < row['validUntilMs']]
        assert len(rows) == 1
        until = min(until, rows[0]['validUntilMs'], reviewed + MAXIMUM_HOLD_MS)
        assert isinstance(review['sources'], list) and 1 <= len(review['sources']) <= 64
        for source in review['sources']:
            observed, maximum_age = source['observedAtMs'], source['maximumAgeMs']
            assert type(observed) is int and type(maximum_age) is int and observed <= now and maximum_age > 0
            until = min(until, observed + maximum_age)
        assert now < until and proof['exchangeId'] == review['action']['exchangeId']
        assert (owner / 'nomination.json').read_bytes() == raw
        assert (owner / 'owner-approval.json').read_bytes() == approval_raw
        return dict(rawSha256=raw_sha, exchangeId=proof['exchangeId'], generationSha256=generation,
            holdUntilMs=until, originalValidUntilMs=review['validUntilMs'], sourceClocksRenewed=False)
    except (AssertionError, KeyError, ValueError, TypeError, OSError):
        return None


def require_current_generation(root, reference, review, expected_pid):
    """An already-issued reload must reconcile before a new review is published."""
    proof = review.get('managedInventory')
    if not isinstance(proof, dict):
        return
    assert reference is not None, 'MANAGED_REVIEW_REQUIRES_SOURCE_REFERENCE_ROOT'
    reference = Path(reference)
    pending = json.loads((reference / 'pending.json').read_bytes())
    assert pending.get('resolved') is True, 'SOURCE_RELOAD_RECONCILIATION_REQUIRED'
    state = json.loads((reference / 'state.json').read_bytes())
    owner = json.loads((Path(root) / 'fundamental-reload/status.json').read_bytes())
    assert owner['workerPid'] == expected_pid, 'SOURCE_GENERATION_WORKER_CHANGED'
    assert proof['generationSha256'] == state['generationSha256'] == owner['previousGenerationSha256'], 'SOURCE_REVIEW_GENERATION_CHANGED'
