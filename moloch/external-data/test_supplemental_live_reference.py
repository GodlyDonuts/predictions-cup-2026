import base64
import copy
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import sys
import tempfile
import types
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from moloch.tartarus.test_party_reference import fixture
from moloch.tartarus import party_reference as reference
from supplemental_live_reference import append


class SupplementalReferenceTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        row, originals, inventory, quotes, metadata = fixture()
        self.ticker = inventory[0]['ticker']
        self.seed = dict(version=1, rows=[dict(registry=row, mappingClaim=inventory[0],
            officialRecordBase64=base64.b64encode(originals[row['semanticSourceSha256']]).decode(),
            metadataRecordBase64=base64.b64encode(metadata[self.ticker]).decode())])
        party_path = Path(reference.__file__)
        self.config = dict(exchanges=['1067', '983'], supplementalReference=dict(exchangeIds=['1067'],
            seedPath=str(self.root/'seed.json'), seedSha256='', partyReferencePath=str(party_path),
            partyReferenceSha256=hashlib.sha256(party_path.read_bytes()).hexdigest()))
        self.cache = dict(markets=[json.loads(quotes[self.ticker])])
        self.base = dict(rows=[dict(exchangeId='983')], capturedAtMs=123)
        self.now = reference.clock('2026-10-18T07:00:02Z')
        self.pin()

    def pin(self):
        raw = json.dumps(self.seed).encode()
        Path(self.config['supplementalReference']['seedPath']).write_bytes(raw)
        self.config['supplementalReference']['seedSha256'] = hashlib.sha256(raw).hexdigest()

    def test_original_quote_and_metadata_clocks_survive_new_capture(self):
        result = append(self.config, self.base, self.now, self.cache)
        r = result['rows'][1]
        self.assertEqual(r['quoteObservedAtMs'], self.now-1000)
        self.assertEqual(r['mapping']['metadataReceivedAt'], '2026-10-18T07:00:01Z')
        self.assertEqual(r['yesMidpoint'], '0.2550')
        self.assertFalse(r['sourceClocksRenewed'])
        self.assertEqual(self.base['rows'], [dict(exchangeId='983')])

    def test_fresh_export_timestamp_does_not_make_stale_quote_current(self):
        self.cache['at'] = '2026-10-18T07:01:00Z'
        r = append(self.config, self.base, self.now+60000, self.cache)
        self.assertEqual(r['rows'], self.base['rows'])
        self.assertIn('QUOTE_STALE', r['supplementalUnavailable'][0]['reason'])

    def test_stale_metadata_disables_only_supplement_even_with_fresh_quote(self):
        self.cache['markets'][0].update(requestStartedAt='2026-10-19T07:00:02Z', receivedAt='2026-10-19T07:00:02Z')
        r = append(self.config, self.base, self.now+86400000, self.cache)
        self.assertEqual(r['rows'], self.base['rows'])
        self.assertIn('METADATA_STALE', r['supplementalUnavailable'][0]['reason'])

    def test_original_body_tampering_and_duplicate_ticker_do_not_refresh(self):
        changed = copy.deepcopy(self.cache)
        changed['markets'][0]['bodyText'] += ' '
        for cache in (changed, dict(markets=self.cache['markets']*2)):
            r = append(self.config, self.base, self.now, cache)
            self.assertEqual(r['rows'], self.base['rows'])
            self.assertEqual(len(r['supplementalUnavailable']), 1)

    def test_pinned_identity_and_party_remain_binding(self):
        self.seed['rows'][0]['mappingClaim']['party'] = 'Democratic'
        self.pin()
        r = append(self.config, self.base, self.now, self.cache)
        self.assertIn('IDENTITY_MISSING', r['supplementalUnavailable'][0]['reason'])
        self.assertEqual(r['rows'], self.base['rows'])

    def test_seed_tampering_and_scope_overlap_reject(self):
        Path(self.config['supplementalReference']['seedPath']).write_text('{}')
        with self.assertRaisesRegex(ValueError, 'PIN_CHANGED'):
            append(self.config, self.base, self.now, self.cache)
        self.pin()
        with self.assertRaisesRegex(ValueError, 'EXACT_SCOPE'):
            append(self.config, dict(rows=[dict(exchangeId='1067')]), self.now, self.cache)

    def test_existing_capture_scope_and_rank_export_are_separate_requests(self):
        path = Path(__file__).with_name('run-live-reference-reload.py')
        spec = importlib.util.spec_from_file_location('test_updater', path)
        updater = importlib.util.module_from_spec(spec)
        # Only capture_values is exercised; no Linux lock operation is mocked
        # into a successful deployment on Windows.
        try:
            import fcntl
            spec.loader.exec_module(updater)
        except ModuleNotFoundError as error:
            if error.name != 'fcntl':
                raise
            with patch.dict(sys.modules, {'fcntl': types.ModuleType('fcntl')}):
                spec.loader.exec_module(updater)
        module_path = path.with_name('supplemental_live_reference.py')
        self.config['captureUrl'] = 'http://127.0.0.1:18964/capture'
        self.config['supplementalReference'].update(modulePath=str(module_path),
            moduleSha256=hashlib.sha256(module_path.read_bytes()).hexdigest(),
            rankCacheUrl='http://127.0.0.1:18964/rank-cache')
        urls = []

        def response(url, timeout):
            urls.append(url)
            value = self.cache if url.endswith('/rank-cache') else self.base
            stream = io.BytesIO(json.dumps(value).encode())
            stream.headers = {}
            return stream

        with patch('urllib.request.urlopen', side_effect=response), patch.object(updater.time, 'time_ns', return_value=self.now*1000000):
            result = updater.capture_values(self.config)
        self.assertEqual({r['exchangeId'] for r in result['rows']}, {'983', '1067'})
        self.assertEqual(urls, ['http://127.0.0.1:18964/capture?exchanges=983',
                               'http://127.0.0.1:18964/rank-cache'])


if __name__ == '__main__':
    unittest.main()
