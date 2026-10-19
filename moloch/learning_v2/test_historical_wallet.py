import hashlib,json,tempfile,unittest
from pathlib import Path
from types import SimpleNamespace
from .historical_wallet import install_wallet


class HistoricalWalletTests(unittest.TestCase):
    def test_causal_original_fifo_no_cash_or_fills_created_and_independent_copies(self):
        with tempfile.TemporaryDirectory() as d:
            p=Path(d)/'wallet.json'
            x={'observedAt':'2026-10-19T04:12:46Z','cash':'158.360000','ownedCost':'1.400000',
               'positions':{'x':{'YES':[{'price':'0.600000','quantity':1},{'price':'0.800000','quantity':1}]}},
               'fundedCapitalMicros':100000000000,'liabilitiesMicros':0,'reservationsMicros':0,
               'executionAuthority':False,'originalFifoBasisPreserved':True}
            p.write_text(json.dumps(x));spec={'path':str(p),'sha256':hashlib.sha256(p.read_bytes()).hexdigest()}
            def env(now=1792383360000):return SimpleNamespace(now=now,initial_wealth=100000000000,cash=100000000000,lots={},orders={},fill_events=[],budget=SimpleNamespace(active=None,unknown={}))
            a,b=env(),env();r=install_wallet(a,spec);install_wallet(b,spec)
            self.assertEqual(a.cash,158360000);self.assertEqual(a.fill_events,[])
            self.assertIsNone(r['initial_marked_wealth_micros']);self.assertEqual(a.lots[('x','YES')],[[1,600000],[1,800000]])
            a.lots[('x','YES')][0][0]=0;self.assertEqual(b.lots[('x','YES')][0][0],1)
            with self.assertRaisesRegex(ValueError,'FUTURE_WALLET'):install_wallet(env(0),spec)
            p.write_text(json.dumps(dict(x,cash='159.360000')))
            with self.assertRaisesRegex(ValueError,'WALLET_CHANGED'):install_wallet(env(),spec)

    def test_relative_interval_score_does_not_fabricate_absolute_roi(self):
        from .run_population import score
        e={'split_role':'search_development','qualified_accounting':False,'qualified_relative_interval_accounting':True,
           'interval_lower_excess_return':.001,'observable_market_frames':1,'reference_return':None}
        self.assertEqual(score([e],'interval_lower_excess'),.001)
        self.assertEqual(score([e],'reference_return'),-1e99)


if __name__=='__main__':unittest.main()
