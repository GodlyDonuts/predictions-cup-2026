import gzip,hashlib,json,tempfile,unittest,os,time
from pathlib import Path
from .archive import RecorderArchive


class ArchiveTests(unittest.TestCase):
    def test_preserves_hashes_skips_existing_and_retains_changed_versions(self):
        with tempfile.TemporaryDirectory()as d:
            root=Path(d);source=root/'source';source.mkdir()
            p=source/'one.gz';raw=gzip.compress(b'original observation\n');p.write_bytes(raw)
            archive=RecorderArchive(root/'archive',{'SIG':source},minimum_age_seconds=0,reserve_bytes=0)
            try:
                self.assertEqual(archive.cycle()['copied_files'],1)
                self.assertEqual(archive.cycle()['copied_files'],0)
                second=gzip.compress(b'next original observation\n');p.write_bytes(raw+second)
                self.assertEqual(archive.cycle()['copied_files'],1)
                rows=[json.loads(x)for x in (root/'archive/manifest.jsonl').read_text().splitlines()]
                self.assertEqual(len(rows),2)
                for row in rows:
                    data=(root/'archive'/row['archive_relative_path']).read_bytes()
                    self.assertEqual(hashlib.sha256(data).hexdigest(),row['sha256'])
                self.assertEqual(p.read_bytes(),raw+second)
            finally:archive.close()

    def test_active_files_are_deferred_and_disk_reserve_does_not_delete_anything(self):
        with tempfile.TemporaryDirectory()as d:
            root=Path(d);source=root/'source';source.mkdir();p=source/'active.gz';p.write_bytes(b'raw')
            archive=RecorderArchive(root/'archive',{'SIG':source},minimum_age_seconds=3600,reserve_bytes=0)
            try:
                self.assertEqual(archive.cycle()['copied_files'],0)
                self.assertEqual(archive.cycle()['deferred_active_files'],1)
                archive.age=0;archive.reserve=10**30
                with self.assertRaisesRegex(ValueError,'DISK_RESERVE'):archive.cycle()
                self.assertEqual(p.read_bytes(),b'raw')
            finally:archive.close()

    def test_archive_cannot_be_nested_in_source(self):
        with tempfile.TemporaryDirectory()as d:
            root=Path(d)
            with self.assertRaises(ValueError):RecorderArchive(root/'bad',{'SIG':root})


if __name__=='__main__':unittest.main()
