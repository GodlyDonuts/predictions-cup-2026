import importlib.util
from hashlib import sha256
import json
import os
from pathlib import Path
from types import SimpleNamespace
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('publisher_gate_test', Path(__file__).with_name('publish-current-money-review.py'))
publisher = importlib.util.module_from_spec(spec); spec.loader.exec_module(publisher)


@unittest.skipUnless(os.name == 'posix', 'Actual publication boundary is checked on the production host')
class PublisherGateTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory(); self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        for name in ['rank-campaign-owner','prepared','reference','fundamental-reload']:
            (self.root/name).mkdir()
        self.generation = 'a'*64
        raw = json.dumps(dict(managedInventory=dict(generationSha256=self.generation))).encode()
        (self.root/'prepared/nomination.json').write_bytes(raw)
        (self.root/'reference/pending.json').write_text(json.dumps(dict(resolved=True)))
        (self.root/'reference/state.json').write_text(json.dumps(dict(generationSha256=self.generation)))
        (self.root/'fundamental-reload/status.json').write_text(json.dumps(dict(workerPid=42,previousGenerationSha256=self.generation)))
        self.args = SimpleNamespace(runtime=self.root, prepared=self.root/'prepared', expected_pid=42,
            expected_raw_sha256=sha256(raw).hexdigest(), source_reference_root=self.root/'reference')

    def test_current_resolved_epoch_allows_the_existing_publisher_checks(self):
        with patch.object(publisher, '_run', return_value='existing_checks') as checked:
            self.assertEqual(publisher.run(self.args), 'existing_checks')
            checked.assert_called_once_with(self.args)

    def test_unresolved_source_request_cannot_reach_approval_writes(self):
        (self.root/'reference/pending.json').write_text(json.dumps(dict(resolved=False)))
        with patch.object(publisher, '_run') as checked:
            with self.assertRaisesRegex(AssertionError, 'SOURCE_RELOAD_RECONCILIATION_REQUIRED'):
                publisher.run(self.args)
            checked.assert_not_called()

    def test_changed_owner_generation_cannot_reach_approval_writes(self):
        (self.root/'fundamental-reload/status.json').write_text(json.dumps(dict(workerPid=42,previousGenerationSha256='b'*64)))
        with patch.object(publisher, '_run') as checked:
            with self.assertRaisesRegex(AssertionError, 'SOURCE_REVIEW_GENERATION_CHANGED'):
                publisher.run(self.args)
            checked.assert_not_called()

    def test_source_check_cannot_be_substituted_with_other_nomination_bytes(self):
        self.args.expected_raw_sha256 = '0'*64
        with patch.object(publisher, '_run') as checked:
            with self.assertRaisesRegex(AssertionError, 'EXACT_REVIEWED_NOMINATION'):
                publisher.run(self.args)
            checked.assert_not_called()


if __name__ == '__main__':
    unittest.main()
