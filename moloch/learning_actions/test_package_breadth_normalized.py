import tempfile,unittest
from pathlib import Path
from . import test_compile_breadth_index as fixture
from .compile_breadth_index import compile_index
from .package_breadth_normalized import package
from .breadth_frame_index import sha
from .audit_breadth_inventory import clock
from .disk_tape import build,source_files


class PackageNormalizedTests(unittest.TestCase):
    def source(self,root):
        raw,pin=fixture.CompileIndexedFramesTests().source(root,duplicate_print=True)
        compile_index(raw,root/'index',pin,root/'normalized',tournament='cup',train_end=clock('2026-10-17T00:00:00Z'))
        return sha(root/'normalized/complete.json')

    def test_original_bytes_roles_and_gaps_survive_existing_disk_cache(self):
        with tempfile.TemporaryDirectory()as d:
            r=Path(d);pin=self.source(r)
            m=package(r/'normalized',pin,r/'dataset',heldout_start=clock('2026-10-21T00:00:00Z'))
            self.assertFalse(m['trainingAdmitted']);self.assertFalse(m['externalObservationsIncluded'])
            self.assertEqual(m['calendarRolesMs']['fit'][1],clock('2026-10-17T00:00:00Z'))
            self.assertEqual((r/'dataset/base/observations.jsonl.gz').read_bytes(),(r/'normalized/observations.jsonl.gz').read_bytes())
            source_files(r/'dataset');build(r/'dataset',r/'cache')
            import sqlite3
            db=sqlite3.connect(r/'cache/replay.sqlite')
            try:
                self.assertEqual(db.execute('SELECT COUNT(*) FROM market').fetchone()[0],4)
                self.assertEqual(db.execute("SELECT COUNT(*) FROM market WHERE kind='gap'").fetchone()[0],2)
                self.assertEqual(db.execute('SELECT COUNT(*) FROM flow').fetchone()[0],1)
            finally:db.close()

    def test_changed_compressed_source_fails_before_creating_dataset(self):
        with tempfile.TemporaryDirectory()as d:
            r=Path(d);pin=self.source(r);p=r/'normalized/observations.jsonl.gz';p.write_bytes(p.read_bytes()+b'changed')
            with self.assertRaisesRegex(ValueError,'BYTES_CHANGED'):
                package(r/'normalized',pin,r/'dataset',heldout_start=clock('2026-10-21T00:00:00Z'))
            self.assertFalse((r/'dataset').exists())

    def test_independent_calendar_and_disk_reserve_are_enforced(self):
        with tempfile.TemporaryDirectory()as d:
            r=Path(d);pin=self.source(r)
            with self.assertRaisesRegex(ValueError,'NON_HELDOUT'):
                package(r/'normalized',pin,r/'dataset',heldout_start=clock('2026-10-18T00:00:00Z'))
            with self.assertRaisesRegex(ValueError,'DISK_HEADROOM'):
                package(r/'normalized',pin,r/'dataset',heldout_start=clock('2026-10-21T00:00:00Z'),minimum_free_bytes=2**80)
            self.assertFalse((r/'dataset').exists())


if __name__=='__main__':unittest.main()
