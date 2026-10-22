import json
from hashlib import sha256
import os
from pathlib import Path
import tempfile
import unittest
import subprocess
import sys

import review_generation_window as window


class ReviewGenerationTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.owner = self.root / 'rank-campaign-owner'; self.owner.mkdir()
        self.generation = 'a' * 64
        self.control = dict(activation=dict(runId='test-run'), accountScope=dict(tournamentId='test'),
            rankCampaign=dict(nominationPath=str(self.owner/'nomination.json'),
                ownerApprovalPath=str(self.owner/'owner-approval.json')))
        self.review = dict(reviewedAtMs=1000, validUntilMs=10000,
            managedInventory=dict(generationSha256=self.generation, exchangeId='1051'),
            action=dict(exchangeId='1051'), sources=[dict(observedAtMs=1000, maximumAgeMs=9000)])
        self.write_review()

    def write_review(self):
        raw = json.dumps(self.review).encode(); (self.owner/'nomination.json').write_bytes(raw)
        self.approval = dict(schemaVersion=1, runId='test-run', accountScope=self.control['accountScope'],
            approvedCampaigns=[dict(rawSha256=sha256(raw).hexdigest(), approvedAtMs=1500,
                validUntilMs=self.review['validUntilMs'])])
        self.write_approval()

    def write_approval(self):
        (self.owner/'owner-approval.json').write_text(json.dumps(self.approval))

    def hold(self, at=2000, generation=None):
        return window.active_managed_review(self.root, self.control, generation or self.generation, at)

    def test_approved_review_holds_same_generation_without_clock_renewal(self):
        first, later = self.hold(), self.hold(2500)
        self.assertEqual(first['holdUntilMs'], 10000)
        self.assertEqual(first, later)
        self.assertFalse(first['sourceClocksRenewed'])

    def test_original_expiry_and_source_expiry_end_the_window(self):
        self.assertIsNone(self.hold(10000))
        self.review['sources'][0]['maximumAgeMs'] = 1500; self.write_review()
        self.assertEqual(self.hold()['holdUntilMs'], 2500)
        self.assertIsNone(self.hold(2500))

    def test_expired_approval_does_not_hold_a_still_current_nomination(self):
        self.approval['approvedCampaigns'][0]['validUntilMs'] = 2000; self.write_approval()
        self.assertIsNone(self.hold())

    def test_changed_generation_or_unapproved_bytes_do_not_hold(self):
        self.assertIsNone(self.hold(generation='b'*64))
        (self.owner/'nomination.json').write_text(json.dumps({**self.review, 'changed': True}))
        self.assertIsNone(self.hold())

    def test_unmanaged_or_future_review_does_not_hold(self):
        self.review.pop('managedInventory'); self.write_review(); self.assertIsNone(self.hold())
        self.review['managedInventory'] = dict(generationSha256=self.generation, exchangeId='1051')
        self.review['reviewedAtMs'] = 3000; self.write_review(); self.assertIsNone(self.hold())

    def test_long_review_cannot_freeze_new_sources_beyond_five_minutes(self):
        self.review['validUntilMs'] = 1_000_000
        self.review['sources'][0]['maximumAgeMs'] = 1_000_000; self.write_review()
        self.assertEqual(self.hold()['holdUntilMs'], 301000)
        self.assertIsNone(self.hold(301000))

    def test_source_reload_must_resolve_and_match_exact_current_owner(self):
        reference = self.root/'reference'; reference.mkdir()
        (self.root/'fundamental-reload').mkdir()
        (reference/'pending.json').write_text(json.dumps(dict(resolved=False)))
        (reference/'state.json').write_text(json.dumps(dict(generationSha256=self.generation)))
        status = self.root/'fundamental-reload/status.json'
        status.write_text(json.dumps(dict(workerPid=42, previousGenerationSha256=self.generation)))
        with self.assertRaisesRegex(AssertionError, 'SOURCE_RELOAD_RECONCILIATION_REQUIRED'):
            window.require_current_generation(self.root, reference, self.review, 42)
        (reference/'pending.json').write_text(json.dumps(dict(resolved=True)))
        window.require_current_generation(self.root, reference, self.review, 42)
        status.write_text(json.dumps(dict(workerPid=42, previousGenerationSha256='b'*64)))
        with self.assertRaisesRegex(AssertionError, 'SOURCE_REVIEW_GENERATION_CHANGED'):
            window.require_current_generation(self.root, reference, self.review, 42)

    @unittest.skipUnless(os.name == 'posix', 'Actual POSIX descriptor locking runs on the production host')
    def test_publication_and_source_request_cannot_hold_the_boundary_together(self):
        with window.publication_boundary(self.root):
            with self.assertRaises(window.PublicationBusy):
                with window.publication_boundary(self.root):
                    self.fail('concurrent publication entered')
        with window.publication_boundary(self.root):
            pass

    @unittest.skipUnless(os.name == 'posix', 'Actual inherited POSIX descriptor')
    def test_checked_child_can_borrow_the_same_held_descriptor_without_unlocking_parent(self):
        code = ('import sys; from pathlib import Path; import review_generation_window as w; '
            'boundary=w.publication_boundary(Path(sys.argv[1]),inherited_fd=int(sys.argv[2])); '
            'boundary.__enter__(); boundary.__exit__(None,None,None); print("shared-held-descriptor")')
        with window.publication_boundary(self.root) as fd:
            result = subprocess.run([sys.executable,'-c',code,str(self.root),str(fd)],
                cwd=Path(__file__).parent,pass_fds=(fd,),capture_output=True,text=True)
            self.assertEqual(result.returncode,0,result.stderr)
            self.assertIn('shared-held-descriptor',result.stdout)
            with self.assertRaises(window.PublicationBusy):
                with window.publication_boundary(self.root): pass
        with window.publication_boundary(self.root): pass

    @unittest.skipUnless(os.name == 'posix', 'Actual descriptor identity')
    def test_unrelated_descriptor_cannot_replace_the_publication_lock(self):
        other = self.root/'unrelated'; other.write_bytes(b''); other.chmod(0o600)
        with window.publication_boundary(self.root):
            with other.open('rb') as handle:
                with self.assertRaisesRegex(AssertionError,'SOURCE_PUBLICATION_DESCRIPTOR_CHANGED'):
                    with window.publication_boundary(self.root,inherited_fd=handle.fileno()): pass

    @unittest.skipUnless(os.name == 'posix', 'Actual timed lock contention')
    def test_waiting_for_a_busy_boundary_is_bounded(self):
        with window.publication_boundary(self.root):
            with self.assertRaises(window.PublicationBusy):
                with window.publication_boundary(self.root,wait_seconds=.01): pass


if __name__ == '__main__':
    unittest.main()
