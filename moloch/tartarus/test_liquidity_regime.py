"""Synthetic causal/price-depth falsification; never opens an actual cohort."""
from copy import deepcopy
from datetime import datetime, timezone
import importlib.util
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch
try:
    from . import liquidity_regime as L
except ImportError:
    import liquidity_regime as L


def stamp(n):
    return datetime.fromtimestamp(n/1000,timezone.utc).isoformat().replace('+00:00','Z')


def book(at, ordinal, hash, ask='0.50', bid='0.49', qty='20', **updates):
    raw={'marketId':'1','exchangeId':'2','sequence':str(ordinal),'sourceAt':stamp(at),'receivedAt':stamp(at),
         'bids':[{'price':bid,'quantity':qty}], 'asks':[{'price':ask,'quantity':qty}]}
    raw.update(updates)
    return {'book':raw,'available_at_ms':at,'ordinal':ordinal,'journal_hash':hash}


def decision(at=1000, sample='s1', trade='t1'):
    return {'sampleId':sample,'decisionMs':at,'marketId':'1','exchangeId':'2','sig':{'journalHash':'entry'},
            'trigger':{'record':{'ticker':'race','tradeId':trade,'sourceTsMs':at,'quantity':'2','yesPrice':'0.5',
                                'noPrice':'0.5','direction':1,'isBlockTrade':False}}}


def fixture():
    return {'decisions':[decision()], 'books':[book(500,1,'before',qty='10'),book(1000,2,'entry'),
                    book(11000,3,'future',bid='0.55',ask='0.56')],
            'barriers':[],'evidence_class':'synthetic','input_replay':None}


class LiquidityRegimeTests(unittest.TestCase):
    def test_full_depth_costs_not_whole_position_top_quote(self):
        e=book(1000,1,'e')['book'];f=book(11000,2,'f',bid='0.6',ask='0.61')['book']
        e['asks']=[{'price':'0.5','quantity':2},{'price':'0.55','quantity':3}]
        f['bids']=[{'price':'0.6','quantity':1},{'price':'0.58','quantity':4}]
        r=L.crossing(e,f,'YES',5)
        self.assertEqual(r['entry_cost_micros'],'2650000')
        self.assertEqual(r['exit_proceeds_micros'],'2920000')
        self.assertEqual(r['net_micros'],'245000')
        self.assertIsNone(L.crossing(e,f,'YES',6))
        self.assertFalse(r['actual_fill'])

    def test_no_complement_and_fractional_depth_exact(self):
        e=book(1000,1,'e',bid='0.49',ask='0.5',qty='1.5')['book']
        f=book(11000,2,'f',bid='0.44',ask='0.45',qty='1.5')['book']
        r=L.crossing(e,f,'NO',1)
        self.assertEqual(r['entry_cost_micros'],'510000')
        self.assertEqual(r['exit_proceeds_micros'],'550000')
        self.assertEqual(r['net_micros'],'35000')
        self.assertIsNone(L.crossing(e,f,'NO',2))
        self.assertIsNone(L.crossing(e,f,'NO',1,5000))

    def test_causal_regime_and_capacity_model_absence_honest(self):
        report=L.evaluate_capture(fixture());e=report['episodes'][0]
        self.assertEqual(e['status'],'labeled_display_counterfactual')
        self.assertEqual(e['regimes'],{'YES':1,'NO':1})
        self.assertEqual(e['capacity']['YES']['10']['normal']['net_micros'],'450000')
        self.assertIsNone(e['capacity']['YES']['100']['normal'])
        self.assertIsNone(report['trained_artifact']);self.assertEqual(report['source_model_baseline']['status'],'unavailable')
        self.assertFalse(report['execution_authorized']);self.assertIsNone(e['actual_execution'])

    def test_first_missing_event_consumes_cluster_no_rescue(self):
        f=fixture();f['decisions']=[decision(sample='missing'),decision(1500,'better','t2')]
        f['decisions'][0]['sig']['journalHash']='absent'
        rows,counts=L.build_episodes(f)
        self.assertEqual(len(rows),1);self.assertEqual(rows[0]['reason'],'missing_original_entry_book')
        self.assertEqual(counts['cluster_skips'],1)

    def test_duplicate_equivalent_decimals_but_conflict_quarantines_all(self):
        f=fixture();d=deepcopy(f['decisions'][0]);d['sampleId']='later';d['trigger']['record']['quantity']='2.00';f['decisions'].append(d)
        rows,c=L.build_episodes(f);self.assertEqual(len(rows),1);self.assertEqual(c['duplicate_events'],1)
        d['trigger']['record']['direction']=-1
        rows,c=L.build_episodes(f);self.assertEqual(rows,[]);self.assertEqual(c['event_conflicts'],1)

    def test_late_first_future_book_not_replaced_by_favorable_later(self):
        f=fixture();f['books'][2]=book(15000,3,'too_late');f['books'].append(book(11001,4,'later_better'))
        rows,_=L.build_episodes(f)
        self.assertEqual(rows[0]['reason'],'first_future_book_outside_slack')

    def test_gaps_rotation_and_missing_anchor_censor_not_zero(self):
        for barriers in [[{'ordinal':3,'scope':['1','2']}],[{'ordinal':3,'scope':None}]]:
            f=fixture();f['barriers']=barriers
            self.assertEqual(L.build_episodes(f)[0][0]['reason'],'gap_or_rotation')
        f=fixture();f['books'].pop(0)
        self.assertEqual(L.build_episodes(f)[0][0]['reason'],'missing_causal_replenishment_anchor')

    def test_source_clock_stale_or_future_and_empty_capacity_not_profit_zero(self):
        f=fixture();f['decisions'][0]['decisionMs']=5000
        self.assertEqual(L.build_episodes(f)[0][0]['reason'],'stale_entry')
        f=fixture();f['books'][1]['available_at_ms']=900
        rows,c=L.build_episodes(f);self.assertEqual(c['invalid_books'],1)
        self.assertEqual(rows[0]['reason'],'missing_original_entry_book')
        report=L.evaluate_capture({'decisions':[],'books':[],'evidence_class':'synthetic'})
        self.assertIsNone(report['capacity_curve'][0]['counterfactual_net_sum_micros'])

    def test_actual_replay_requires_matching_first_original_horizon_label(self):
        f=fixture();f['evidence_class']='replayed_original_callbacks';f['labels']=[]
        self.assertEqual(L.build_episodes(f)[0][0]['reason'],'missing_or_different_original_horizon_label')
        f['labels']=[{'sampleId':'s1','horizonMs':10000,'sigJournalHash':'future','sourceAt':stamp(11000),'receivedAt':stamp(11000)}]
        self.assertEqual(L.build_episodes(f)[0][0]['status'],'labeled_display_counterfactual')

    def test_haircut_limits_capacity_without_inventing_queue_fills(self):
        report=L.evaluate_capture(fixture())
        normal=next(v for v in report['capacity_curve'] if v['side']=='YES' and v['quantity']==10 and v['control']=='normal')
        haircut=next(v for v in report['capacity_curve'] if v['side']=='YES' and v['quantity']==100 and v['control']=='depth_haircut')
        self.assertEqual(normal['displayed_capacity_episodes'],1)
        self.assertEqual(haircut['displayed_capacity_episodes'],0)
        self.assertIsNone(haircut['actual_execution_count'])
        self.assertIn('adverse_queue',report['missing_controls'])

    def test_informative_cancellation_failed_and_censored_execution_rows_not_erased(self):
        f=fixture();f['execution_outcomes']=[
            {'class':'confirmed_zero','requested_quantity':10,'observed_filled_quantity':0,'conditional_gain_units':0,'source_sha256':'a'*64,'terminal_status':'canceled'},
            {'class':'confirmed_partial','requested_quantity':10,'observed_filled_quantity':3,'conditional_gain_units':None,'value_censored_quantity':3,'source_sha256':'b'*64},
            {'class':'unresolved','requested_quantity':10,'observed_filled_quantity':2,'conditional_gain_units':None,'source_sha256':'c'*64}]
        result=L.evaluate_capture(f)
        self.assertEqual(result['actual_execution_observations']['rows'],f['execution_outcomes'])
        self.assertIsNone(result['quantity_model'])
        self.assertFalse(result['actual_execution_observations']['original_receipt_membership_independently_verified'])
        f['execution_outcomes'][0]['observed_filled_quantity']=1
        with self.assertRaisesRegex(ValueError,'EXECUTION_CLASS_QUANTITY'):L.evaluate_capture(f)

    def test_scope_gap_does_not_invalidate_other_market(self):
        f=fixture();f['barriers']=[{'ordinal':3,'scope':['other_market',None]}]
        self.assertEqual(L.build_episodes(f)[0][0]['status'],'labeled_display_counterfactual')
        f['barriers'][0]['scope']=['1',None]
        self.assertEqual(L.build_episodes(f)[0][0]['reason'],'gap_or_rotation')

    def test_split_uses_missing_representatives_and_purges_unavailable_training_labels(self):
        f=fixture();f['decisions']=[decision(1000,'one','t1'),decision(15000,'missing','t2'),decision(29000,'missing2','t3')]
        result=L.evaluate_capture(f)
        self.assertEqual(result['chronology']['boundary_decision_ms'],15000)
        self.assertEqual(result['chronology']['training_sample_ids'],['one'])
        self.assertEqual(result['chronology']['heldout_sample_ids'],[])
        self.assertEqual(result['chronology']['common_all_control_episode_count'],0)
        self.assertIn('INSUFFICIENT_OOS_EPISODES',result['insufficiency_reasons'])

    def test_protocol_changed_and_duplicate_level_refused(self):
        with patch.object(L,'PROTOCOL_LF_SHA','f'*64):
            with self.assertRaisesRegex(ValueError,'FROZEN_PROTOCOL_CHANGED'):L.protocol()
        b=book(1,1,'b')['book'];b['bids'].append(deepcopy(b['bids'][0]))
        with self.assertRaisesRegex(ValueError,'DUPLICATE_PRICE'):L.levels(b,'bids')

    def test_original_shaped_sealed_trace_replays_and_tamper_refuses(self):
        folder=L.ROOT/'research/moloch-trade-trigger-20261016/witness'
        sys.path.insert(0,str(folder))
        try:
            spec=importlib.util.spec_from_file_location('_regime_fixture_only',folder/'test_trace_replay.py')
            module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
            with tempfile.TemporaryDirectory() as temporary:
                f=module.Fixture(Path(temporary))
                try:
                    f.polls();f.sig(10000);previous=f.tail.previous;f.clock=11000;f.polls();f.sig(11000,2,previous)
                    f.finish();result=L.analyze_capture(f.path,f.hash)
                    self.assertTrue(result['episode_dataset_manifest']['input_replay']['samplerInputReplayVerified'])
                    self.assertEqual(result['status'],'insufficient_evidence')
                    self.assertEqual(result['episode_dataset_manifest']['episode_count'],0)
                    source=f.source.read_bytes();f.source.write_bytes(source.replace(b'0.420000',b'0.430000',1))
                    # Any changed original source bytes must fail membership/hash.
                    if f.source.read_bytes()==source:f.source.write_bytes(source+b'changed')
                    with self.assertRaises(Exception):L.analyze_capture(f.path,f.hash)
                finally:f.close()
        finally:sys.path.remove(str(folder))


if __name__=='__main__':unittest.main()
