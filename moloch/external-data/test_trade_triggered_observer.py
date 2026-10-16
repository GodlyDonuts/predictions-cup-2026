"""Prospective sampler tests use the original public trade/book wire shapes."""
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from crossvenue_observer import Study, compact, TRADE_SAMPLING_POLICY_SHA256, require_trade_output_fresh
from test_crossvenue_observer import row, external, websocket, public_flow


def envelope(at=40000, flow=None, **kwargs):
    result = websocket(at, **kwargs)
    result['markets'][0]['tradeFlow'] = flow or public_flow(at)
    result['websocket'] = {'coverage': {'connections': 1}}
    return result


class TradeTriggeredObserverTests(unittest.TestCase):
    def study(self):
        out = []
        return Study(out.append, sampling_mode='trade'), out

    def test_default_book_mode_bytes_match_original_preimplementation_golden(self):
        out=[]; s=Study(out.append,horizons=(1000,3000))
        s.external_update(external(9000),10000); s.accept(row(10000),10000)
        s.external_update(external(11000),11000); s.accept(row(11000,2,bid='0.430000',ask='0.450000'),11000)
        self.assertEqual(len(out),3)
        self.assertEqual(hashlib.sha256(('\n'.join(map(compact,out))+'\n').encode()).hexdigest(),'475a5499e3705ade2da67942c853b94ccd488be18c3d53211b84f99bd4b428cb')
        self.assertNotIn('samplingPolicy',out[0])

    def test_genuine_shapes_pin_first_availability_and_future_labels_only_use_later_books(self):
        s,out=self.study(); s.accept(row(39950),39950); self.assertEqual(out,[])
        s.external_update(envelope(),40010)
        self.assertEqual(len(out),1); sample=out[0]; trigger=sample['trigger']
        self.assertEqual((sample['decisionMs'],trigger['sourceExportMs'],trigger['observerFirstAvailableMs'],trigger['record']['sourceTsMs'],trigger['record']['receivedMs'],trigger['record']['availableMs']),(40010,40000,40010,39900,39896,39910))
        self.assertEqual(sample['sig']['sourceAt'],row(39950)['event']['book']['sourceAt'])
        self.assertEqual(sample['samplingPolicySha256'],TRADE_SAMPLING_POLICY_SHA256)
        self.assertFalse(trigger['resolutionEquivalenceVerified']); self.assertFalse(sample['executionAuthority'])
        s.accept(row(41010,2,bid='0.430000',ask='0.450000'),41010)
        labels=[r for r in out if r['kind']=='horizon_label']; self.assertEqual(len(labels),1)
        self.assertEqual(labels[0]['sampleId'],sample['sampleId']); self.assertEqual(labels[0]['horizonMs'],1000)
        self.assertEqual(labels[0]['samplingPolicySha256'],TRADE_SAMPLING_POLICY_SHA256)
        self.assertFalse(labels[0]['actualFill'])

    def test_poll_duplicates_do_not_renew_first_availability_or_resample_original_trade(self):
        s,out=self.study(); s.accept(row(39950),39950); e=envelope(); s.external_update(e,40010)
        original=compact(out[0]); s.external_update(e,40110)
        self.assertEqual(len(out),1); self.assertEqual(compact(out[0]),original)
        self.assertEqual(s.external_flow['1']['availableMs'],40010); self.assertGreater(s.counts['tradeDuplicateSkip'],0)

    def test_stale_sig_skips_identity_once_without_waiting_for_later_book(self):
        s,out=self.study(); s.accept(row(35000),35000); e=envelope(); s.external_update(e,40010)
        self.assertEqual(s.counts['tradeStaleSigSkip'],1); self.assertEqual(out,[])
        s.accept(row(40100,2),40100); s.external_update(e,40110)
        self.assertEqual(out,[]); self.assertEqual(len(s.trigger_seen),1)

    def test_missing_external_old_trigger_and_future_evidence_are_explicit_missing(self):
        s,out=self.study(); s.accept(row(39950),39950); e=envelope(); e['markets'][0].update(valid=False,invalidReason='stale')
        s.external_update(e,40010); self.assertEqual(s.counts['tradeNoCurrentExternalSkip'],1); self.assertEqual(out,[])
        s,out=self.study(); s.accept(row(41900),41900); s.external_update(envelope(),41901)
        self.assertEqual(s.counts['tradeOldSkip'],1); self.assertEqual(out,[])
        s,out=self.study(); s.accept(row(39950),39950); s.external_update(envelope(45100),40000)
        self.assertGreater(s.counts['tradeFutureSkip'],0); self.assertEqual(out,[])

    def test_gap_invalid_and_future_sig_cannot_supply_current_executable_book(self):
        for mode in ('gap','invalid','future'):
            s,out=self.study(); s.accept(row(39900),39900)
            if mode=='gap': s.accept({'event':{'type':'gap','marketId':'1'}},39950)
            else:
                bad=row(39950,2)
                if mode=='invalid': bad['event']['book']['invalid']=True
                else: bad['event']['book']['sourceAt']=row(41000)['event']['book']['sourceAt']
                s.accept(bad,39950)
            s.external_update(envelope(),40010)
            self.assertEqual(out,[],mode); self.assertEqual(s.counts['tradeNoCurrentSigSkip'],1,mode)

    def test_identity_conflict_disables_sampling_and_does_not_invent_healthy_coverage(self):
        s,out=self.study(); s.accept(row(39950),39950); s.external_update(envelope(),40010)
        changed=public_flow(40000)
        for w in changed['windows'].values():
            w['evidence'][0]['bodySha256']='e'*64
            w['evidenceSha256']=hashlib.sha256(json.dumps(w['evidence'],separators=(',',':'),ensure_ascii=False).encode()).hexdigest()
        changed['asOfMs']=40100
        s.external_update(envelope(40100,flow=changed,sequence=2,sha='d'*64),40110)
        self.assertTrue(s.trigger_disabled); self.assertEqual(s.counts['tradeIdentityConflict'],1)
        s.external_update(envelope(41100,sequence=3,sha='f'*64),41110)
        self.assertEqual(len(out),1); self.assertEqual(s.counts['tradeDisabledPollSkip'],1)

    def test_seen_capacity_never_evicts_original_ids_or_enables_retrigger(self):
        s,out=self.study(); s.accept(row(39950),39950); s.trigger_seen={('1','RACE-26-D',str(i)):'a'*64 for i in range(32768)}
        s.external_update(envelope(),40010)
        self.assertTrue(s.trigger_disabled); self.assertEqual(len(s.trigger_seen),32768)
        self.assertEqual(s.counts['tradeIdentityCapReached'],1); self.assertEqual(out,[])

    def test_analog_references_are_not_promoted_to_trade_samples(self):
        s,out=self.study(); s.accept(row(39950),39950); s.external_update(envelope(mapping='candidate_reference_analog'),40010)
        self.assertEqual(out,[]); self.assertEqual(s.counts['tradeAnalogSkip'],1)

    def test_invalid_book_censors_pending_labels_and_malformed_flow_stays_missing(self):
        s,out=self.study(); s.accept(row(39950),39950); s.external_update(envelope(),40010)
        bad=row(40500,2); bad['event']['book']['invalid']=True; s.accept(bad,40500)
        s.accept(row(41010,3),41010)
        self.assertFalse(any(r['kind']=='horizon_label' for r in out))
        self.assertEqual(s.counts['labelsMissingTimelyBook'],5)
        for windows in (None, [], {'1000': {'evidence': None}}):
            e=envelope(41100); e['markets'][0]['tradeFlow']['windows']=windows
            s.external_update(e,41110)
        self.assertEqual(s.counts['tradeInvalidEvidenceSkip'],3)
        self.assertEqual(len(out),1)

    def test_policy_bounds_and_fresh_cohort_directory_are_explicit(self):
        protocol=json.loads((Path(__file__).resolve().parents[2]/'research/moloch-trade-trigger-20261016/protocol.json').read_text(encoding='utf-8'))
        self.assertEqual(protocol['samplingPolicySha256'],TRADE_SAMPLING_POLICY_SHA256)
        self.assertEqual(hashlib.sha256(compact(protocol['samplingPolicy']).encode()).hexdigest(),TRADE_SAMPLING_POLICY_SHA256)
        with self.assertRaisesRegex(ValueError,'OBSERVER_TRADE_FIXED_POLICY'):
            Study(lambda _:None,sampling_mode='trade',sig_age=10000)
        with tempfile.TemporaryDirectory() as directory:
            p=Path(directory); require_trade_output_fresh(p)
            (p/'status.json').write_text('{}',encoding='utf-8')
            with self.assertRaisesRegex(ValueError,'NEW_COHORT_REQUIRED'): require_trade_output_fresh(p)
            (p/'status.json').unlink(); (p/'old.jsonl.gz').write_bytes(b'original')
            with self.assertRaisesRegex(ValueError,'NEW_COHORT_REQUIRED'): require_trade_output_fresh(p)
            (p/'old.jsonl.gz').unlink(); (p/'sampling-policy.json').write_text('{}',encoding='utf-8')
            with self.assertRaisesRegex(ValueError,'NEW_COHORT_REQUIRED'): require_trade_output_fresh(p)


if __name__=='__main__':
    unittest.main()
