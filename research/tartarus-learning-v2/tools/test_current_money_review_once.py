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


if __name__=='__main__': unittest.main()
