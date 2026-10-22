"""Publisher completion gates; mocked process output is not SDK qualification."""
import importlib.util
from hashlib import sha256
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('prior_completion_test', Path(__file__).with_name('publish-current-money-review.py'))
publisher = importlib.util.module_from_spec(spec); spec.loader.exec_module(publisher)


@unittest.skipUnless(os.name == 'posix', 'Production file ownership and executable pins are POSIX')
class PriorCompletionTests(unittest.TestCase):
    def setUp(self):
        previous_mask = os.umask(0o077); self.addCleanup(os.umask,previous_mask)
        directory = tempfile.TemporaryDirectory(); self.addCleanup(directory.cleanup)
        self.root = Path(directory.name); self.raw_sha = 'a'*64; self.qid = 42
        (self.root/'bank-all-markets-v1').mkdir(); (self.root/'prepared').mkdir()
        self.write_events([dict(kind='intent',qid=42,rankCampaign=dict(rawSha256=self.raw_sha)),
            dict(kind='admitted_pre_network_not_sent',qid=42)])
        self.write_report()
        self.view = dict(knownUnclosedAcknowledgments=[],activeNativeOrders=[],sdkQueueBoundaryChecked=True,
            unresolvedOrdinaryAttempt=False)
        helper = self.root/'verifier'; helper.write_bytes(b'test-only executable identity'); helper.chmod(0o700)
        self.context = dict(prepared=self.root/'prepared',control=self.root/'control.json',verifier=helper,
            verifierSha256=sha256(helper.read_bytes()).hexdigest())

    def write_events(self, events):
        path = self.root/'dispatch.ndjson'
        path.write_text(''.join(json.dumps(dict(event=event))+'\n' for event in events)); path.chmod(0o600)

    def write_report(self, filled=0):
        (self.root/'bank-all-markets-v1/report.json').write_text(json.dumps(dict(
            orders=[dict(id=42,retired=True,filled=filled)],fills=[])))

    def complete(self, context=True):
        return publisher.completed_prior_review(self.root,self.view,self.raw_sha,42,self.context if context else None)

    def verified_output(self, args, **kwargs):
        proof = dict(qid=42,priorRawSha256=self.raw_sha,kind='positively_proved_pre_network_not_sent',
            opaqueSdkProofVerified=True,dispatchSnapshotSha256=sha256(Path(args[2]).read_bytes()).hexdigest(),
            retryAuthorized=False,exchangeClosedClaimed=False,accountRequests=0,dispatchSnapshotModified=False)
        return subprocess.CompletedProcess(args,0,json.dumps(proof),'')

    def test_marker_json_without_typed_verifier_does_not_complete_review(self):
        self.context['verifier'] = None
        with self.assertRaisesRegex(AssertionError,'PRIOR_TYPED_NOT_SENT_VERIFIER_REQUIRED'):
            self.complete()

    def test_changed_verifier_is_rejected_before_process_invocation(self):
        self.context['verifierSha256'] = '0'*64
        with patch.object(publisher.subprocess,'run') as process:
            with self.assertRaisesRegex(AssertionError,'PRIOR_NOT_SENT_VERIFIER_CHANGED'):
                self.complete()
            process.assert_not_called()

    def test_failed_opaque_verification_does_not_complete_review(self):
        with patch.object(publisher.subprocess,'run',return_value=subprocess.CompletedProcess([],1,'','rejected')):
            with self.assertRaisesRegex(AssertionError,'PRIOR_TYPED_NOT_SENT_VERIFICATION_REJECTED'):
                self.complete()

    def test_non_network_completion_does_not_invent_exchange_closure_or_retry(self):
        with patch.object(publisher.subprocess,'run',side_effect=self.verified_output):
            with patch.dict(os.environ,{}):
                old = os.umask(0o077)
                try: result = self.complete()
                finally: os.umask(old)
        self.assertEqual(result['completionKind'],'positively_proved_pre_network_not_sent')
        self.assertFalse(result['exchangeClosedClaimed']); self.assertFalse(result['retryAuthorized'])
        self.assertNotIn('originalClosedLineSha256',result)

    def test_fill_or_unresolved_attempt_prevents_non_network_completion(self):
        self.write_report(filled=1)
        with self.assertRaises(AssertionError):self.complete()
        self.write_report(); self.view['unresolvedOrdinaryAttempt'] = True
        with self.assertRaises(AssertionError):self.complete()

    def test_original_exchange_closed_predecessor_keeps_its_existing_path(self):
        self.write_events([dict(kind='intent',qid=42,rankCampaign=dict(rawSha256=self.raw_sha)),dict(kind='closed',qid=42)])
        self.assertIn('originalClosedLineSha256',self.complete(context=False))


if __name__ == '__main__':
    unittest.main()
