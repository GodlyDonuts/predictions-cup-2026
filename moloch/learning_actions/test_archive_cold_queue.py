import json
import os
from pathlib import Path
import shutil
import tempfile
import time
import unittest
from .archive_cold_queue import ColdQueue
from .preserve_superseded_queue import archive_batch


class ColdArchiveTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve(); self.queue = self.root/'queue'; self.queue.mkdir()
        self.opened = set(); self.owner = True
        for seq in (1, 2, 3, 4, 10, 11):
            for prefix, suffix in (('queue','jsonl'), ('checkpoint','json')):
                p = self.queue/f'{prefix}-{seq}-{seq}.{suffix}'; p.write_bytes(b'original\n'*50)
                os.utime(p, (time.time()-200000, time.time()-200000))
        self.select(10, 'queue-4-4.jsonl')
        os.utime(self.queue/'queue-3-3.jsonl', None)
        p = self.queue/'queue-2-2.jsonl'; s = p.stat(); self.opened.add((s.st_dev,s.st_ino))
        self.guard = ColdQueue(self.queue, owner_check=self.check_owner, open_files=lambda:self.opened)

    def check_owner(self):
        if not self.owner: raise ValueError('OWNER_CHANGED')

    def select(self, seq, source):
        (self.queue/f'checkpoint-{seq}-{seq}.json').write_text(json.dumps({'anchor':{'source_journal':source}}))
        (self.queue/'account-queue-manifest.json').write_text(json.dumps(dict(schema_version=1,scope='fixture',
            checkpoint=f'checkpoint-{seq}-{seq}.json',journal=f'queue-{seq}-{seq}.jsonl')))

    def test_current_previous_new_recent_and_open_files_are_excluded(self):
        names={p.name for p in self.guard.choose(100000)}
        self.assertIn('queue-1-1.jsonl',names)
        for name in ('queue-10-10.jsonl','checkpoint-10-10.json','queue-11-11.jsonl',
                     'queue-4-4.jsonl','queue-3-3.jsonl','queue-2-2.jsonl'):
            self.assertNotIn(name,names)

    def test_manifest_can_advance_but_never_regress(self):
        paths=self.guard.choose(100000); self.select(11,'queue-10-10.jsonl');self.guard.check(paths)
        self.select(10,'queue-4-4.jsonl')
        with self.assertRaisesRegex(ValueError,'REGRESSED'):self.guard.check(paths)

    def test_owner_loss_or_file_open_blocks_unlink(self):
        paths=self.guard.choose(100000);self.owner=False
        with self.assertRaisesRegex(ValueError,'OWNER_CHANGED'):self.guard.check(paths)
        self.owner=True;p=paths[0];s=p.stat();self.opened.add((s.st_dev,s.st_ino))
        with self.assertRaisesRegex(ValueError,'OPEN'):self.guard.check(paths)

    @unittest.skipUnless(shutil.which('zstd'), 'requires real zstd CLI')
    def test_complete_archive_keeps_active_bytes_and_decodes_old_originals(self):
        paths=self.guard.choose(100000); originals={p.name:p.read_bytes() for p in paths}
        selected=(self.queue/'queue-10-10.jsonl').read_bytes()
        result=archive_batch(self.queue,self.root/'archive',100000,cold_guard=self.guard)
        self.assertTrue(result['fullDecompressionHashesVerified'])
        self.assertEqual(result['fileCount'],len(originals))
        self.assertEqual((self.queue/'queue-10-10.jsonl').read_bytes(),selected)
        for name in originals:self.assertFalse((self.queue/name).exists())

    @unittest.skipUnless(shutil.which('zstd'), 'requires real zstd CLI')
    def test_live_rotation_during_archive_preserves_new_selected_generation(self):
        result=archive_batch(self.queue,self.root/'archive',100000,cold_guard=self.guard,
                             recheck=lambda:self.select(11,'queue-10-10.jsonl'))
        self.assertTrue(result['complete'])
        self.assertTrue((self.queue/'queue-10-10.jsonl').exists())
        self.assertTrue((self.queue/'queue-11-11.jsonl').exists())

    @unittest.skipUnless(shutil.which('zstd'), 'requires real zstd CLI')
    def test_owner_change_before_unlink_preserves_every_original(self):
        paths=self.guard.choose(100000)
        def lose_owner():self.owner=False
        with self.assertRaisesRegex(ValueError,'OWNER_CHANGED'):
            archive_batch(self.queue,self.root/'archive',100000,cold_guard=self.guard,recheck=lose_owner)
        self.assertTrue(all(p.exists() for p in paths))


if __name__=='__main__':unittest.main()
