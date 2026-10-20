from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from .selection import DataUse


class ProspectiveHoldoutTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory()
        self.registry=DataUse(self.temp.name)
        self.clock=patch('moloch.learning_actions.selection.now_ms',return_value=1000)
        self.clock.start()

    def tearDown(self):
        self.clock.stop()
        self.temp.cleanup()

    def test_reserved_period_rejects_every_optimization_kind(self):
        path=self.registry.reserve_calendar(2000,6000,'a'*64)
        self.assertEqual(path,self.registry.reserve_calendar(2000,6000,'a'*64))
        for kind in('train','selection','inspected_development'):
            with self.subTest(kind=kind),self.assertRaisesRegex(ValueError,'CANNOT_TRAIN_OR_SELECT'):
                self.registry.record(kind,1999,2001,'b'*64)
        self.registry.record('train',1000,2000,'b'*64)
        self.registry.record('selection',6000,7000,'b'*64)

    def test_test_is_exact_complete_and_single_use(self):
        self.registry.reserve_calendar(2000,6000,'a'*64)
        with self.assertRaisesRegex(ValueError,'NOT_COMPLETE'):
            self.registry.record('test',2000,6000,'b'*64)
        with patch('moloch.learning_actions.selection.now_ms',return_value=6000):
            with self.assertRaisesRegex(ValueError,'EXACT_RESERVED'):
                self.registry.record('test',2000,5999,'b'*64)
            self.registry.record('test',2000,6000,'b'*64)
            with self.assertRaisesRegex(ValueError,'PREVIOUSLY_USED'):
                self.registry.record('test',2000,6000,'c'*64)

    def test_exposure_and_overlap_prevent_new_reservation(self):
        self.registry.record('selection',2000,2100,'b'*64)
        with self.assertRaisesRegex(ValueError,'PREVIOUSLY_USED'):
            self.registry.reserve_calendar(2000,6000,'a'*64)
        self.registry.reserve_calendar(3000,7000,'a'*64)
        with self.assertRaisesRegex(ValueError,'OVERLAP'):
            self.registry.reserve_calendar(4000,8000,'c'*64)
        with self.assertRaisesRegex(ValueError,'START_IN_FUTURE'):
            self.registry.reserve_calendar(1000,2000,'a'*64)

    def test_os_lock_serializes_admission_and_releases_after_exception(self):
        with self.registry._lock():
            with self.assertRaisesRegex(ValueError,'REGISTRY_BUSY'):
                DataUse(self.temp.name).record('train',1000,2000,'a'*64)
        self.registry.record('train',1000,2000,'a'*64)
        self.assertEqual(len(list(Path(self.temp.name).glob('*.json'))),1)


if __name__=='__main__':
    unittest.main()
