import unittest
import torch
from .behavior_audit import BehaviorAudit


class BehaviorAuditTests(unittest.TestCase):
    def test_audit_preserves_stochastic_actions_rng_finances_clocks_and_receipts(self):
        from .test_actions import environment
        from .policy import ApiPolicy
        from .stream import Stream
        from .episode_tape import financial_digest
        torch.set_num_threads(1);torch.manual_seed(87);model=ApiPolicy(16)
        results=[]
        for enabled in (False,True):
            env=environment();env.end=10000;torch.manual_seed(127)
            audit=BehaviorAudit(env)if enabled else None
            stream=Stream(env,model,calibration={'decision_latency_ms':20},actions_only=True)
            while env.now<env.end:stream.step()
            results.append((list(stream.rows),financial_digest(env),env.now,list(env.feed_requests),
                            list(env.feed_receipts),torch.get_rng_state()))
            if audit:
                report=audit.report();self.assertGreater(report['counters']['observations'],0)
                self.assertFalse(report['actionsModified']);audit.close()
        self.assertEqual(results[0][:-1],results[1][:-1])
        self.assertTrue(torch.equal(results[0][-1],results[1][-1]))

    def test_sell_conversion_is_distinct_from_fills_and_received_finance(self):
        from .test_actions import environment
        from .contract import OrderSpec
        env=environment();env.lots[('a','YES')]=[[10,400000]];env.known_lots={('a','YES'):[[4,400000]]}
        audit=BehaviorAudit(env)
        try:
            self.assertEqual(env._canonical(OrderSpec('a','YES','SELL',8,500000)).action,'SELL')
            self.assertEqual(env._canonical(OrderSpec('a','YES','SELL',20,500000)).action,'BUY')
            result=audit.report()
            self.assertEqual(result['counters']['canonical_sell_calls'],2)
            self.assertEqual(result['counters']['sell_calls_converted_to_complement_buy'],1)
            self.assertEqual(result['actualFillActions'],{})
            self.assertEqual(env.known_lots[('a','YES')][0][0],4)
        finally:audit.close()


if __name__=='__main__':unittest.main()
