import hashlib,json,unittest
from decimal import Decimal
from pathlib import Path

P=Path(__file__).resolve().parent


class Candidate(unittest.TestCase):
    def setUp(self):
        self.r=json.loads((P/'review-v2-terminal-candidate.json').read_bytes())
        self.e=json.loads((P/'draft-evidence.json').read_bytes())

    def test_actual_raw_candidate_pin(self):
        self.assertEqual(hashlib.sha256((P/'review-v2-terminal-candidate.json').read_bytes()).hexdigest(),self.e['candidateRawSha256'])
        self.assertFalse(self.e['executionAuthority']);self.assertFalse(self.e['productionActivated'])

    def test_exact_democratic_target_not_inherited_republican_no(self):
        self.assertEqual((self.r['marketId'],self.r['exchangeId'],self.r['contractId'],self.r['side']),('277','966','278','YES'))
        self.assertEqual(self.r['officialContractRootSha256'],'56670f0ec4ec7e9837de07ca07cc7573996a89e94d578f9db675252613936da1')

    def test_unchanged_economics_and_explicit_principal_budget(self):
        self.assertEqual([self.r[k]for k in ['ordinaryProbability','exceptionalProbabilityAllowance','minimumExceptionalPayout','marginPerShare']],['0.919000','0.030000','0.000000','0.010000'])
        self.assertEqual(self.r['maximumPrincipal'],'2000.000000');self.assertEqual(self.r['maximumQuantity'],10000)
        value=(1-Decimal(self.r['exceptionalProbabilityAllowance']))*Decimal(self.r['ordinaryProbability'])
        self.assertEqual(value,Decimal('.89143'));ceiling=value-Decimal(self.r['marginPerShare'])
        self.assertEqual(ceiling,Decimal('.88143'));self.assertEqual(ceiling//Decimal('.005')*Decimal('.005'),Decimal('.880'))

    def test_original_node_clock_cannot_be_renewed_by_review(self):
        n=next(s for s in self.r['sources']if s['kind']=='official_contract_node')
        self.assertEqual(n['observedAtMs'],1792107921269);self.assertEqual(n['maximumAgeMs'],86400000)
        self.assertEqual(self.r['expiresAtMs'],n['observedAtMs']+n['maximumAgeMs'])
        self.assertLess(self.r['reviewedAtMs'],self.r['expiresAtMs'])
        for s in self.r['sources']:
            self.assertLessEqual(s['publishedAtMs'],s['observedAtMs']);self.assertLessEqual(s['observedAtMs'],self.r['reviewedAtMs']);self.assertLessEqual(self.r['expiresAtMs'],s['observedAtMs']+s['maximumAgeMs'])

    def test_sources_match_genuine_new_public_original_pins(self):
        p=json.loads((P/'source-pins.json').read_bytes());pins={s.get('sha256')for s in p['sources']if s.get('status')==200}
        for s in self.r['sources']:
            if s['kind']!='official_contract_node':self.assertIn(s['sha256'],pins)
        forecast=next(s for s in self.r['sources']if s['kind']=='forecast')
        self.assertEqual(forecast['maximumAgeMs'],86400000)

    def test_actual_terminal_delay_objective_and_modes_preserved(self):
        v=self.r['valuationObjective'];self.assertEqual(v['kind'],'terminalSettlement');self.assertEqual(v['delayPolicy'],'holdForActualSettlement');self.assertEqual(v['timingJsonPointer'],'/data/root/settlement_date');self.assertEqual(v['scheduledSettlementAtMs'],self.r['holdUntilMs'])
        self.assertEqual(self.r['executionPolicy']['allowedModes'],['boundedLimitMayRest','improveOneTick','joinBid'])


if __name__=='__main__':unittest.main()
