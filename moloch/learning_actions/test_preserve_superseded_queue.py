import json
from pathlib import Path
import shutil
import tempfile
import unittest

from moloch.learning_actions.preserve_superseded_queue import archive_batch, choose, selected


class PreservationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.queue = self.root / 'queue'
        self.queue.mkdir()
        self.manifest = self.queue / 'account-queue-manifest.json'
        self.manifest.write_text(json.dumps({'journal': 'queue-20-2.jsonl',
                                           'checkpoint': 'checkpoint-20-2.json'}))
        self.originals = {'queue-10-1.jsonl': b'old journal\n' * 100,
                          'checkpoint-10-1.json': b'{"old": true}',
                          'queue-20-2.jsonl': b'active', 'checkpoint-20-2.json': b'active',
                          'queue-21-3.jsonl': b'future', 'proof.json': b'proof'}
        for name, value in self.originals.items():
            (self.queue / name).write_bytes(value)

    def test_selection_only_strictly_old_generations(self):
        self.assertEqual({p.name for p in choose(self.queue, 4096)},
                         {'queue-10-1.jsonl', 'checkpoint-10-1.json'})
        self.assertEqual(selected(self.queue)[2], 20)

    def test_mismatched_selected_pair_rejected(self):
        self.manifest.write_text(json.dumps({'journal': 'queue-20-2.jsonl',
                                           'checkpoint': 'checkpoint-19-2.json'}))
        with self.assertRaisesRegex(ValueError, 'PAIR_REQUIRED'):
            selected(self.queue)

    @unittest.skipUnless(shutil.which('zstd'), 'requires real zstd CLI')
    def test_archive_roundtrip_and_preserved_current(self):
        result = archive_batch(self.queue, self.root / 'archive', 4096)
        self.assertTrue(result['fullDecompressionHashesVerified'])
        self.assertEqual(result['fileCount'], 2)
        for name, value in self.originals.items():
            if '-10-' in name:
                self.assertFalse((self.queue / name).exists())
            else:
                self.assertEqual((self.queue / name).read_bytes(), value)
        self.assertTrue((self.root / 'archive' / 'complete.json').exists())

    @unittest.skipUnless(shutil.which('zstd'), 'requires real zstd CLI')
    def test_changed_source_is_retained(self):
        def mutate():
            (self.queue / 'queue-10-1.jsonl').write_bytes(b'changed')
        with self.assertRaisesRegex(ValueError, 'ORIGINAL_CHANGED'):
            archive_batch(self.queue, self.root / 'archive', 4096, recheck=mutate)
        self.assertTrue((self.queue / 'queue-10-1.jsonl').exists())
        self.assertTrue((self.queue / 'checkpoint-10-1.json').exists())

    @unittest.skipUnless(shutil.which('zstd'), 'requires real zstd CLI')
    def test_changed_manifest_prevents_unlink(self):
        def mutate():
            self.manifest.write_text(self.manifest.read_text() + ' ')
        with self.assertRaisesRegex(ValueError, 'GENERATION_CHANGED'):
            archive_batch(self.queue, self.root / 'archive', 4096, recheck=mutate)
        for name in self.originals:
            self.assertTrue((self.queue / name).exists())


if __name__ == '__main__':
    unittest.main()
