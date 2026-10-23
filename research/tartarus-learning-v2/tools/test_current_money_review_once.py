import importlib.util
from pathlib import Path
import unittest

spec=importlib.util.spec_from_file_location('current_money_owner',Path(__file__).with_name('run-current-money-review-once.py'))
owner=importlib.util.module_from_spec(spec); spec.loader.exec_module(owner)


class RecoveryAndSelectionTests(unittest.TestCase):
    def setUp(self):
        self.deployment=dict(pid=42)
        self.proof=dict(pid=42,freshTradingVerified=True,fullOriginalFinancialPrefixPreserved=True,
                        fullOriginalFundamentalPrefixPreserved=True,originalFillPrefixPreserved=True)
        self.progress=dict(pid=42,phase='trading',realtimeHealthy=True)

    def test_full_verified_recovery_is_eligible_for_fresh_research(self):
        self.assertTrue(owner.checked_recovery(self.deployment,self.proof,self.progress,True))

    def test_old_process_progress_and_missing_proof_cannot_trigger_a_review(self):
        self.assertFalse(owner.checked_recovery(self.deployment,None,self.progress,True))
        self.assertFalse(owner.checked_recovery(self.deployment,self.proof,dict(self.progress,pid=41),True))

    def test_dead_worker_is_not_an_observation_timeout(self):
        with self.assertRaisesRegex(AssertionError,'EXPECTED_WORKER_EXITED'):
            owner.checked_recovery(self.deployment,self.proof,self.progress,False)

    def test_wrong_worker_and_missing_history_evidence_are_rejected(self):
        for key,value in [('pid',41),('fullOriginalFinancialPrefixPreserved',False),
                          ('fullOriginalFundamentalPrefixPreserved',False),('originalFillPrefixPreserved',False)]:
            with self.subTest(key=key), self.assertRaises(AssertionError):
                owner.checked_recovery(self.deployment,dict(self.proof,**{key:value}),self.progress,True)

    def test_new_deployment_reuses_whole_universe_research_and_original_horizon(self):
        template=['python3','research.py','--deployment','old.json','--output','old-dir',
                  '--decision-profile-policy','current-prices','--horizon-ms','300000']
        result=owner.research_command(template,Path('new.json'),Path('new-dir'))
        self.assertEqual(result[3],'new.json'); self.assertEqual(result[5],'new-dir')
        self.assertEqual(result[-2:],['--horizon-ms','300000'])
        self.assertNotIn('--exchange',result)
        self.assertEqual(template[3],'old.json')

    def test_no_silent_change_of_price_assumptions(self):
        with self.assertRaises(AssertionError):
            owner.research_command(['--deployment','old','--output','old',
                '--decision-profile-policy','all-declared'],Path('new'),Path('new'))

    def test_unmanaged_value_buy_cannot_fall_into_the_legacy_forced_exit_policy(self):
        candidates=[dict(leg=dict(ex='859',side='NO',action='BUY')),
                    dict(leg=dict(ex='1051',side='NO',action='BUY'))]
        selected,deferred=owner.select_candidate(candidates,{'1051':{'side':'NO'}})
        self.assertEqual(selected['leg']['ex'],'1051')
        self.assertEqual(deferred[0]['exchangeId'],'859')

    def test_managed_selection_has_no_fixed_exchange_and_unmanaged_sells_are_allowed(self):
        candidates=[dict(leg=dict(ex='other',side='YES',action='SELL')),
                    dict(leg=dict(ex='1051',side='NO',action='BUY'))]
        selected,deferred=owner.select_candidate(candidates,{'1051':{'side':'NO'}})
        self.assertEqual(selected['leg']['ex'],'other'); self.assertEqual(deferred,[])

    def test_no_eligible_managed_value_buy_does_not_authorize_a_forced_loss_cycle(self):
        selected,deferred=owner.select_candidate([dict(leg=dict(ex='859',side='NO',action='BUY'))],{})
        self.assertIsNone(selected); self.assertEqual(len(deferred),1)

    def test_opposite_side_does_not_inherit_the_other_sides_exit_manager(self):
        selected,deferred=owner.select_candidate([dict(leg=dict(ex='1051',side='YES',action='BUY'))],
                                                {'1051':{'side':'NO'}})
        self.assertIsNone(selected); self.assertEqual(len(deferred),1)

    def test_only_an_explicit_positive_prior_order_is_forwarded_for_independent_completion(self):
        self.assertEqual(owner.prior_completion_arguments(None),[])
        self.assertEqual(owner.prior_completion_arguments(23417),['--completed-qid','23417'])
        for qid in [0,-1,True,'23417']:
            with self.subTest(qid=qid),self.assertRaises(AssertionError):
                owner.prior_completion_arguments(qid)


if __name__=='__main__': unittest.main()
