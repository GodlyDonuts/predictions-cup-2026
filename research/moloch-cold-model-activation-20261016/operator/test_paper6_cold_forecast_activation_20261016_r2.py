"""Offline guards only: never invokes stage/stop/promote or any remote operation."""
import copy
import importlib.util
import json
import pathlib
import tempfile
import unittest
from unittest import mock
import zipfile

PATH = pathlib.Path(__file__).with_name('paper6_cold_forecast_activation_20261016_r2.py')
spec = importlib.util.spec_from_file_location('cold_activation', PATH)
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)


class Guards(unittest.TestCase):
    def setUp(self):
        self.before = {'activation': {'binarySha256': m.BINARY, 'maximumCommitted': '95579.915000'},
                       'fundamentalPolicy': copy.deepcopy(m.OLD_SETTINGS), 'policy': {'maximumMarketCost': '10000.000000'},
                       'root': '/unchanged', 'defensiveCancel': True, 'credentials': '/private-original-reference'}
        self.after = copy.deepcopy(self.before)
        self.after['fundamentalPolicy'] = copy.deepcopy(m.NEW_SETTINGS)
        self.typed = {'settingsLoadPassed': True, 'reviewAccepted': True, 'sourcesVerified': True,
                      'settingsCanonicalSha256': m.NEW_SETTINGS_SHA, 'executionAuthority': False,
                      'forecastCalibrationVerified': False}
        self.boundary = {'manifestSha256': '1'*64, 'requestSha256': '2'*64, 'stopSha256': '3'*64,
                         'workerStatusSha256': '4'*64,
                         'typedSettings': {'path': '/tmp/actual-staged-rust-proof.json', 'sha256': '5'*64}}

    def test_actual_current_four_and_only_new_paths(self):
        m.settings_delta(m.OLD_SETTINGS, m.NEW_SETTINGS)
        self.assertEqual(m.NEW_SETTINGS['reviews'][:4], m.OLD_SETTINGS['reviews'])
        self.assertEqual(m.sha_value(m.NEW_SETTINGS), 'e7714333a9467b6bf69144905f1b74f1143d85e2c682a7a236d723eb936f37f8')
        for r in m.NEW_SETTINGS['reviews'][4:]:
            self.assertTrue(r['path'].startswith(m.GENERATION+'/'))
            self.assertTrue(all(s['path'].startswith(m.GENERATION+'/') for s in r['sourceFiles']))

    def test_old_entry_change_rejected_even_if_other_three_preserved(self):
        changed = copy.deepcopy(m.NEW_SETTINGS)
        changed['reviews'][0]['sourceFiles'][0]['sha256'] = 'f'*64
        with self.assertRaises(RuntimeError): m.settings_delta(m.OLD_SETTINGS, changed)

    def test_only_model_settings_control_delta(self):
        m.controls(self.before, self.after)
        for key in ['root', 'defensiveCancel', 'credentials', 'policy', 'activation']:
            changed = copy.deepcopy(self.after)
            changed[key] = None
            with self.subTest(key=key), self.assertRaises(RuntimeError): m.controls(self.before, changed)

    def test_settings_path_initialization_and_binary_cannot_change(self):
        changed = copy.deepcopy(self.after)
        changed['fundamentalPolicy']['allowInitialization'] = not changed['fundamentalPolicy']['allowInitialization']
        with self.assertRaises(RuntimeError): m.controls(self.before, changed)
        changed = copy.deepcopy(self.after)
        changed['activation']['binarySha256'] = 'f'*64
        with self.assertRaises(RuntimeError): m.controls(self.before, changed)

    def test_actual_review_clocks_not_renewed_and_expiry_strict(self):
        at = max(r['reviewedAtMs'] for r in m.NEW_REVIEWS)
        m.source_lifetimes(m.NEW_REVIEWS, at)
        expiry = min(r['expiresAtMs'] for r in m.NEW_REVIEWS)
        with self.assertRaises(RuntimeError): m.source_lifetimes(m.NEW_REVIEWS, expiry)
        with self.assertRaises(RuntimeError): m.source_lifetimes(m.NEW_REVIEWS, at-1)
        changed = copy.deepcopy(m.NEW_REVIEWS)
        changed[0]['sources'][2]['observedAtMs'] += 1
        with self.assertRaises(RuntimeError): m.source_lifetimes(changed, at)

    def test_staged_settings_proof_old_tmp_digest_cannot_be_reused(self):
        m.typed_settings_proof(self.typed)
        old = dict(self.typed, settingsCanonicalSha256='9e9b9039781da9fe4d7762b7b50455d96428994fee119b65dcbbce8fa3777b5a3')
        with self.assertRaises(RuntimeError): m.typed_settings_proof(old)
        for key in ['settingsLoadPassed', 'reviewAccepted', 'sourcesVerified']:
            with self.subTest(key=key), self.assertRaises(RuntimeError): m.typed_settings_proof(dict(self.typed, **{key: False}))
        with self.assertRaises(RuntimeError): m.typed_settings_proof(dict(self.typed, executionAuthority=True))

    def approval(self):
        a = m.approval_template('6'*64, '7'*64, self.boundary['typedSettings']['path'], '5'*64, m.stop_witness(self.boundary))
        a['executionOwnerReviewed'] = True
        return a

    def test_approval_template_is_deliberately_unapproved(self):
        a = self.approval()
        a['executionOwnerReviewed'] = None
        with self.assertRaisesRegex(RuntimeError, 'INDEPENDENT_OWNER_APPROVAL'): m.validate_approval(a, '6'*64, '7'*64, self.boundary)
        m.validate_approval(self.approval(), '6'*64, '7'*64, self.boundary)

    def test_approval_pins_boundary_status_stop_and_raw_staged_settings(self):
        for mutate in [lambda a: a.update(boundarySha256='a'*64),
                       lambda a: a['ownedStopWitness'].update(stopSha256='a'*64),
                       lambda a: a['ownedStopWitness'].update(workerStatusSha256='a'*64),
                       lambda a: a['typedSettings'].update(sha256='a'*64),
                       lambda a: a.update(binarySha256='a'*64),
                       lambda a: a.update(settingsCanonicalSha256='a'*64)]:
            a = self.approval(); mutate(a)
            with self.assertRaises(RuntimeError): m.validate_approval(a, '6'*64, '7'*64, self.boundary)

    def test_exact_unit_single_argument_change_preserves_other_bytes(self):
        for newline in [b'\n', b'\r\n']:
            raw = b'[Service]'+newline+('ExecStart='+m.OLD_RELEASE+' run '+m.OLD_CONTROL).encode()+newline+b'Restart=no'+newline
            expected = raw.replace(m.OLD_CONTROL.encode(), m.NEW_CONTROL.encode())
            self.assertEqual(m.unit_candidate(raw), expected)
        with self.assertRaises(RuntimeError): m.unit_candidate(raw+raw)
        with self.assertRaises(RuntimeError): m.unit_candidate(raw.replace(b'ExecStart=', b'#ExecStart='))

    def test_exact_current_capital_and_no_double_credit(self):
        a = {'originalInitialCash': '40000.000000', 'netContributions': '55579.915000',
             'effectiveInitialCash': '95579.915000', 'effectiveMaximumCommitted': '95579.915000'}
        m.capital_unchanged({'ownerCapitalAllocation': a})
        for key in a:
            changed = dict(a); changed[key] = '95580.000000'
            with self.subTest(key=key), self.assertRaises(RuntimeError): m.capital_unchanged({'ownerCapitalAllocation': changed})

    def test_root_manifest_exact_and_operator_self_pin(self):
        m.validate_manifest(m.manifest_template())
        changed = m.manifest_template(); changed['oldPid'] += 1
        with self.assertRaises(RuntimeError): m.validate_manifest(changed)
        changed = m.manifest_template(); changed['operatorSha256'] = 'f'*64
        with self.assertRaises(RuntimeError): m.validate_manifest(changed)

    def test_regular_file_hash_and_bound_refusals(self):
        with tempfile.TemporaryDirectory() as d:
            p = pathlib.Path(d)/'proof.json'; p.write_bytes(b'original')
            self.assertEqual(m.read_pinned(p, m.digest(b'original')), b'original')
            with self.assertRaises(RuntimeError): m.read_pinned(p, 'f'*64)
            with self.assertRaises(RuntimeError): m.read_pinned(p, m.digest(b'original'), 2)
            with self.assertRaises(RuntimeError): m.read_pinned(pathlib.Path(d), 'f'*64)

    def test_symlink_selected_artifact_or_parent_is_refused(self):
        with mock.patch.object(pathlib.Path, 'is_symlink', return_value=True):
            with self.assertRaisesRegex(RuntimeError, 'SYMLINK_PATH_REFUSED'):
                m.no_symlink_path(pathlib.Path(__file__).absolute())

    def test_reviewed_packet_original_payload_bytes(self):
        base = pathlib.Path(r'C:\Users\csram\AppData\Local\Temp')
        zpath = base/'moloch-cold-candidate-typed-proof-20261016.zip'
        raw = m.read_pinned(zpath, m.PACKET_PROOF_SHA)
        import io
        with zipfile.ZipFile(io.BytesIO(raw)) as z:
            for name, expected in m.ORIGINAL_PROOF_HASHES.items(): self.assertEqual(m.digest(z.read(name)), expected)
            for name in ['alaska-review.json', 'maine-review.json', 'authority.json']:
                self.assertEqual(m.digest(z.read(name)), m.PAYLOAD_HASHES[name])
            self.assertEqual(json.loads(z.read('authority.json')), m.NEW_AUTHORITY)
        for relative, expected in m.PAYLOAD_HASHES.items():
            if relative.startswith('sources/'):
                m.read_pinned(base/'moloch-cold-approved-candidates-20261016'/relative, expected)

    def test_windows_guard_precedes_any_operator_io_or_process_action(self):
        with mock.patch.object(m.sys, 'platform', 'win32'), mock.patch.object(m.subprocess, 'run') as run:
            with self.assertRaisesRegex(RuntimeError, 'LINUX_EXECUTION_OWNER_ONLY'):
                m.main(['operator', 'stage', '/not-read.json', '1'*64])
            run.assert_not_called()

    def test_malformed_cli_or_missing_proofs_cannot_stop(self):
        with self.assertRaises(RuntimeError): m.main(['operator', 'stop', '/manifest', '1'*64])
        with self.assertRaises(RuntimeError): m.main(['operator', 'promote', '/manifest', '1'*64])


    def test_fixed_pin_widths_and_transcription_regression(self):
        m.validate_manifest(m.manifest_template())
        for value in [m.ENGINE, m.BINARY, m.OLD_CONTROL_SHA, m.OLD_UNIT_SHA, m.OLD_SUMMARY_SHA, m.HELPER, m.DROPIN, m.BACKUP_SHA, m.BRIDGE_SHA, m.BANK_MANIFEST, m.NEW_SETTINGS_SHA, m.OLD_SETTINGS_SHA, m.PACKET_PROOF_SHA, *m.REVIEW_CANONICAL, *m.PAYLOAD_HASHES.values(), *m.ORIGINAL_PROOF_HASHES.values()]:
            m.checked_sha(value)
        malformed = m.manifest_template()
        malformed['oldUnitSha256'] = '091797ff9ce24ba6633bfc42db111eaf4a1344452fa1ce815505f68614e87146a'
        with self.assertRaisesRegex(RuntimeError, 'SHA256_REQUIRED'):
            m.validate_manifest(malformed)


if __name__ == '__main__':
    unittest.main()
