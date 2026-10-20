import copy
import unittest
from .repair_history_read_tail import analyze, raw, sha


def fixture(write=False, network=False):
    state = {'schema_version': 1, 'scope': 'fixture', 'last_clock': 100,
             'next': 100, 'cooldown': 0, 'manual_review': False, 'reads': [],
             'writes': [], 'active': None, 'unknown': {}, 'intents': {}, 'keys': {},
             'read_profile': {'cap': 100, 'gap_ms': 600}}
    anchor = {'sequence': 0, 'previous': '', 'source_journal': 'old.jsonl',
              'source_bytes': 0, 'source_sha256': 'a' * 64, 'prior_checkpoint_hash': None}
    cp = {'schema_version': 1, 'anchor': anchor, 'state': state,
          'hash': sha(raw([1, anchor, state]))}
    m = {'scope': 'fixture', 'checkpoint_hash': cp['hash']}
    row = {'sequence': 1, 'previous': '', 'state': state,
           'hash': sha(raw([1, '', state]))}
    changed = copy.deepcopy(state)
    changed.update(last_clock=101, next=701, reads=[101],
                   active={'request': 'one', 'write': write, 'binding': None, 'network_started': network})
    tail = raw({'sequence': 2, 'previous': row['hash'], 'state': changed,
                'hash': sha(raw([2, row['hash'], changed]))})[:-20]
    return raw(m), raw(cp), raw(row) + b'\n' + tail


class ReadTailTests(unittest.TestCase):
    def test_exact_unstarted_read_preserves_complete_prefix(self):
        m, cp, journal = fixture()
        result = analyze(m, cp, journal)
        self.assertEqual(result['lastCompleteSequence'], 1)
        self.assertEqual(journal[:result['retainedPrefixBytes']].count(b'\n'), 1)
        self.assertFalse(result['financialStateChanged'])

    def test_write_or_started_read_cannot_be_removed(self):
        for write, network in ((True, False), (False, True)):
            with self.assertRaisesRegex(ValueError, 'ONLY_UNSTARTED_READ'):
                analyze(*fixture(write, network))

    def test_corruption_and_other_partial_operations_are_rejected(self):
        m, cp, journal = fixture()
        with self.assertRaises(ValueError): analyze(m, cp, journal.replace(b'"next":701', b'"next":702'))
        with self.assertRaises(ValueError): analyze(m, cp, journal.replace(b'"last_clock":100', b'"last_clock":99', 1))
        with self.assertRaises(ValueError): analyze(m, cp, journal[:journal.index(b'\n') + 1])
