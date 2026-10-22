"""Economic refresh boundaries; no network, worker or GPU."""
import importlib.util
import json
import os
from pathlib import Path
import sys
import tempfile
import types
import unittest

if sys.platform == 'win32':
    sys.modules.setdefault('fcntl', types.SimpleNamespace())
spec = importlib.util.spec_from_file_location('refresh', Path(__file__).with_name('run-live-reference-reload.py'))
refresh = importlib.util.module_from_spec(spec)
spec.loader.exec_module(refresh)
D = refresh.Decimal


class RefreshTests(unittest.TestCase):
    def recovery(self, root, generation='previous'):
        previous='a'*64;out=root/'updater';out.mkdir();g=out/'generation';g.mkdir();control=g/'control.json';control.write_text('{}')
        config={'workerPid':42,'workerStartTicks':123,'baseControlSha256':'b'*64,
            'ownerLease':'test-lease','bankManifestSha256':'c'*64,'runId':'run'}
        request={**config,'previousGenerationSha256':previous,'controlPath':str(control),
            'controlSha256':refresh.sha(control),'expiresAtMs':200,'bankManifestSha256':'c'*64,'runId':'run'}
        path=g/'request.json';path.write_text(json.dumps(request));wanted=refresh.sha(path)
        pending={'expectedGenerationSha256':wanted,'previousGenerationSha256':previous,'expiresAtMs':200,
            'staged':{'controlPath':str(control),'controlSha256':refresh.sha(control)}}
        state={'generationSha256':previous,'controlPath':'original','controlSha256':'d'*64,'index':3,'commits':2}
        active=wanted if generation=='committed' else previous
        (root/'fundamental-reload').mkdir();owner=root/'fundamental-reload/status.json';owner.write_text(json.dumps({**config,'previousGenerationSha256':active}));os.utime(owner,ns=(220_000_000,220_000_000))
        (root/'tartarus-current-native-state.json').write_text(json.dumps({'writtenAt':'1970-01-01T00:00:00.221Z','generationSha256':active}))
        rows=[{'manifestHash':'c'*64,'runId':'run','event':{'kind':'review_generation','generation':{'generationSha256':x}}}for x in ([previous,wanted]if generation=='committed'else[previous])]
        (root/'fundamental.ndjson').write_text(''.join(json.dumps(x)+'\n' for x in rows));return config,state,pending,out

    def test_expired_request_requires_later_matching_owner_view_and_original_prefix(self):
        with tempfile.TemporaryDirectory()as d:
            root=Path(d);c,s,p,out=self.recovery(root)
            self.assertIsNone(refresh.reload_resolution(root,c,s,p,199))
            owner=root/'fundamental-reload/status.json';os.utime(owner,ns=(199_000_000,199_000_000));self.assertIsNone(refresh.reload_resolution(root,c,s,p,250))
            os.utime(owner,ns=(220_000_000,220_000_000));view=root/'tartarus-current-native-state.json';original=view.read_bytes();v=json.loads(original);v['writtenAt']='1970-01-01T00:00:00.199Z';view.write_text(json.dumps(v));self.assertIsNone(refresh.reload_resolution(root,c,s,p,250));view.write_bytes(original)
            proof=refresh.reload_resolution(root,c,s,p,250);self.assertEqual(proof['outcome'],'expired_uncommitted');self.assertFalse(proof['requestResent']);self.assertEqual(proof['accountRequests'],0)
            original_journal=(root/'fundamental.ndjson').read_bytes();refresh.record_resolution(out,s,p,proof);self.assertEqual(s['generationSha256'],'a'*64);self.assertEqual((s['commits'],s['index']),(2,4));refresh.record_resolution(out,s,p,proof);self.assertEqual((s['commits'],s['index']),(2,4));self.assertEqual((root/'fundamental.ndjson').read_bytes(),original_journal)

    def test_late_commit_uses_exact_staged_control_once_and_preserves_request(self):
        with tempfile.TemporaryDirectory()as d:
            root=Path(d);c,s,p,out=self.recovery(root,'committed');proof=refresh.reload_resolution(root,c,s,p,250);self.assertEqual(proof['outcome'],'committed')
            refresh.record_resolution(out,s,p,proof);self.assertEqual(s['generationSha256'],p['expectedGenerationSha256']);self.assertEqual(s['controlPath'],p['staged']['controlPath']);self.assertEqual((s['commits'],s['index']),(3,4));refresh.record_resolution(out,s,p,proof);self.assertEqual((s['commits'],s['index']),(3,4))
            self.assertEqual(json.loads((out/('reload-request-'+p['expectedGenerationSha256']+'.json')).read_bytes()),p)

    def test_partial_or_conflicting_evidence_never_resolves(self):
        for case in ['request','control','owner','owner_scope','foreign_generation','foreign_cohort','view','journal']:
            with self.subTest(case=case),tempfile.TemporaryDirectory()as d:
                root=Path(d);c,s,p,out=self.recovery(root)
                if case=='request':(Path(p['staged']['controlPath']).parent/'request.json').write_text('{}')
                elif case=='control':Path(p['staged']['controlPath']).write_text('{"changed":true}')
                elif case in ['owner','owner_scope','foreign_generation']:
                    path=root/'fundamental-reload/status.json';v=json.loads(path.read_bytes())
                    key={'owner':'workerPid','owner_scope':'ownerLease','foreign_generation':'previousGenerationSha256'}[case]
                    v[key]=99 if case=='owner'else'e'*64;path.write_text(json.dumps(v))
                elif case=='foreign_cohort':
                    path=root/'fundamental.ndjson';path.write_text(path.read_text().replace('"runId": "run"','"runId": "other"'))
                elif case=='view':
                    path=root/'tartarus-current-native-state.json';v=json.loads(path.read_bytes());v['generationSha256']=p['expectedGenerationSha256'];path.write_text(json.dumps(v))
                else:(root/'fundamental.ndjson').write_text('')
                if case in ['view','journal']:self.assertIsNone(refresh.reload_resolution(root,c,s,p,250))
                else:
                    with self.assertRaises(ValueError):refresh.reload_resolution(root,c,s,p,250)
                self.assertFalse((out/'pending.json').exists());self.assertEqual(s['index'],3)

    def test_restart_after_state_commit_does_not_double_count_commit(self):
        with tempfile.TemporaryDirectory()as d:
            root=Path(d);c,s,p,out=self.recovery(root,'committed')
            s.update(generationSha256=p['expectedGenerationSha256'],controlPath=p['staged']['controlPath'],
                controlSha256=p['staged']['controlSha256'],commits=3)
            proof=refresh.reload_resolution(root,c,s,p,250)
            refresh.record_resolution(out,s,p,proof)
            s=json.loads((out/'state.json').read_bytes())
            refresh.record_resolution(out,s,p,proof)
            self.assertEqual((s['commits'],s['index']),(3,4))

    def review(self):
        return {'exchangeId': '1067', 'side': 'YES', 'ordinaryProbability': '0.5000',
            'expiresAtMs': 900_000, 'exceptionalProbabilityAllowance': '0.01',
            'minimumExceptionalPayout': '0', 'marginPerShare': '0.01',
            'passiveValueExit': {'version': 1, 'maximumQuantity': 10_000,
                'pricingObjective': {'kind': 'fifoProfitTurnover', 'minimumNetGain': '.005'}}}

    def test_unchanged_executable_ticks_do_not_force_new_review(self):
        r = self.review()
        self.assertEqual(refresh.executable_ticks(r, D('.5000')), refresh.executable_ticks(r, D('.5001')))
        self.assertNotEqual(refresh.executable_ticks(r, D('.5000')), refresh.executable_ticks(r, D('.506')))

    def test_no_complement_and_value_exit(self):
        r = self.review(); r['side'] = 'NO'
        self.assertEqual(refresh.executable_ticks(r, D('.5000')), refresh.executable_ticks(r, D('.4999')))
        r['passiveValueExit'].pop('pricingObjective')
        self.assertEqual(refresh.executable_ticks(r, D('.5'))[1], 99)

    def test_expiry_and_open_order_filter(self):
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / 'review.json'; p.write_text(json.dumps(self.review()))
            control = {'fundamentalPolicy': {'reviews': [{'path': str(p)}]}}
            capture = {'rows': [{'exchangeId': '1067', 'yesMidpoint': '.5001'}]}
            self.assertEqual(refresh.changed_scopes(capture, control, set(), 1000), [])
            self.assertEqual(refresh.changed_scopes(capture, control, set(), 599_999), [])
            self.assertEqual(refresh.changed_scopes(capture, control, set(), 600_000), ['1067'])
            self.assertEqual(refresh.changed_scopes(capture, control, set(), 780_000), ['1067'])
            self.assertEqual(refresh.changed_scopes(capture, control, {'1067'}, 780_000), [])

    def test_earliest_original_source_clock_drives_refresh_without_extending_it(self):
        review = self.review()
        review['sources'] = [{'observedAtMs': 100_000, 'maximumAgeMs': 350_000},
                             {'observedAtMs': 100_000, 'maximumAgeMs': 900_000}]
        self.assertEqual(refresh.review_current_until(review), 450_001)
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / 'review.json'; original = json.dumps(review); p.write_text(original)
            control = {'fundamentalPolicy': {'reviews': [{'path': str(p)}]}}
            capture = {'rows': [{'exchangeId': '1067', 'yesMidpoint': '.5000'}]}
            self.assertEqual(refresh.changed_scopes(capture, control, set(), 150_000), [])
            self.assertEqual(refresh.changed_scopes(capture, control, set(), 150_001), ['1067'])
            self.assertEqual(refresh.changed_scopes(capture, control, {'1067'}, 150_001), [])
            self.assertEqual(p.read_text(), original)


if __name__ == '__main__':
    unittest.main()
