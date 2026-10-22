"""Original-source, freshness and gap cases for research-only book discovery."""
import base64
from copy import deepcopy
from hashlib import sha256
import json
from pathlib import Path
import tempfile
import unittest
import research_book_observations as obs


AT = '2026-10-22T17:00:00Z'
REGISTRY = [dict(exchangeId='101', marketId='201')]
MAPPING = {'101': '201'}


def raw_row(*, kind='markets_batch', price='0.4', source=AT, dirty=False):
    book = dict(exchangeId='101', asOf=dict(sequence=7, at=source),
        bids=[dict(price=price, quantity=10)], asks=[dict(price='0.45', quantity=20)])
    e = dict(source='official_sig_realtime', event=kind, receivedAt=AT,
        payload=dict(markets=[dict(marketId='201', batch=dict(
            delivery=dict(model='best-effort-authoritative-resync', revision=1, previousRevision=0),
            books=[book], trades=[], bookDirty=[dict(exchangeId='101', marketId='201',
                tournamentId='cup', at='2026-10-22T17:00:01Z')] if dirty else [], marketSettled=[]))]))
    if kind == 'stream_reset':
        e['payload'] = {'localBoundary': True}
    row = dict(sequence=1, previous='', event=e)
    return obs.canonical(dict(row, hash=sha256(obs.canonical(row)).hexdigest())) + b'\n'


class ResearchBookTests(unittest.TestCase):
    def fixture(self, lines):
        temp = tempfile.TemporaryDirectory(); self.addCleanup(temp.cleanup)
        root = Path(temp.name); (root / 'breadth-123-1.jsonl').write_bytes(b''.join(lines))
        compiled = dict(reportSha256='a' * 64, viewSha256='b' * 64, originalBooks=[])
        view = dict(scope=dict(tournamentId='cup', exchanges=MAPPING))
        result = obs.capture(root, 123, compiled, view, REGISTRY, obs.clock(AT),
            root / 'research-book-observations.original.json')
        return root, compiled, result

    def test_fresh_original_book_is_research_only_and_no_native_mutation(self):
        root, compiled, result = self.fixture([raw_row()])
        before = deepcopy(compiled); books, sources = obs.load(root, compiled, REGISTRY)
        self.assertEqual(books['101']['bids'][0]['price'], '0.400000')
        self.assertEqual(sources['101'], raw_row())
        self.assertEqual(compiled, before)
        self.assertFalse(result['nativeAdmissionClaimed'])
        self.assertFalse(result['completeTradeHistoryClaimed'])
        self.assertFalse(result['executionAuthority'])

    def test_changed_recorder_row_is_rejected(self):
        x = json.loads(raw_row()); x['event']['receivedAt'] = '2026-10-22T16:00:00Z'
        with self.assertRaisesRegex(AssertionError, 'ORIGINAL_RECORDER_ROW_HASH'):
            obs.row_books(obs.canonical(x), MAPPING, 'cup', obs.clock(AT))

    def test_old_or_future_source_is_not_a_fresh_observation(self):
        for stamp in ['2026-10-22T16:59:49Z', '2026-10-22T17:00:01Z']:
            self.assertEqual(obs.row_books(raw_row(source=stamp), MAPPING, 'cup', obs.clock(AT)), {})

    def test_crossed_or_off_tick_book_is_not_available(self):
        for price in ['0.46', '0.401']:
            self.assertEqual(obs.row_books(raw_row(price=price), MAPPING, 'cup', obs.clock(AT)), {})

    def test_reset_and_dirty_remove_previous_observation(self):
        for second in [raw_row(kind='stream_reset'), raw_row(dirty=True)]:
            _, _, result = self.fixture([raw_row(), second])
            self.assertEqual(result['observations'], {})

    def test_changed_normalized_price_is_rejected(self):
        root, compiled, result = self.fixture([raw_row()])
        result['observations']['101']['book']['bids'][0]['price'] = '0.405000'
        (root / 'research-book-observations.original.json').write_bytes(obs.canonical(result))
        with self.assertRaisesRegex(AssertionError, 'ORIGINAL_RESEARCH_BOOK_CHANGED'):
            obs.load(root, compiled, REGISTRY)

    def test_full_book_can_supersede_an_earlier_dirty_notice(self):
        x = json.loads(raw_row(dirty=True)); x['event']['payload']['markets'][0]['batch']['bookDirty'][0]['at'] = '2026-10-22T16:59:59Z'
        x.pop('hash'); x['hash'] = sha256(obs.canonical(x)).hexdigest()
        _, _, result = self.fixture([obs.canonical(x) + b'\n'])
        self.assertEqual(len(result['observations']), 1)

    def test_changed_financial_binding_is_rejected(self):
        root, compiled, _ = self.fixture([raw_row()]); compiled['reportSha256'] = 'c' * 64
        with self.assertRaises(AssertionError):
            obs.load(root, compiled, REGISTRY)

    def test_exact_original_bytes_are_rechecked(self):
        root, compiled, result = self.fixture([raw_row()])
        h = next(iter(result['originalRows']))
        result['originalRows'][h] = base64.b64encode(raw_row() + b' ').decode()
        (root / 'research-book-observations.original.json').write_bytes(obs.canonical(result))
        with self.assertRaisesRegex(AssertionError, 'ORIGINAL_RESEARCH_ROW_CHANGED'):
            obs.load(root, compiled, REGISTRY)

    def test_later_empty_book_does_not_leave_phantom_liquidity(self):
        x = json.loads(raw_row()); x['event']['payload']['markets'][0]['batch']['books'][0]['asks'] = []
        x.pop('hash'); x['hash'] = sha256(obs.canonical(x)).hexdigest()
        _, _, result = self.fixture([raw_row(), obs.canonical(x) + b'\n'])
        self.assertEqual(result['observations'], {})

    def test_stale_or_invalid_native_fallback_does_not_trigger_repeated_research(self):
        root, compiled, _ = self.fixture([])
        fresh = obs.row_books(raw_row(), MAPPING, 'cup', obs.clock(AT))['101']
        for bad in [dict(fresh, sourceAt='2026-10-22T16:59:49Z'), dict(fresh, invalid=True)]:
            compiled['originalBooks'] = [bad]
            self.assertEqual(obs.load(root, compiled, REGISTRY)[0], {})
        compiled['originalBooks'] = [fresh]
        self.assertEqual(obs.load(root, compiled, REGISTRY)[0], {'101': fresh})


if __name__ == '__main__':
    unittest.main()
