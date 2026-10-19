import unittest
import numpy as np
from .test_actions import environment
from .account_reads import capture,publish
from .contract import OrderSpec,UNIT


def hidden_finance():
    e=environment()
    e._execute('ORDER',[OrderSpec('x','YES','BUY',5,450000)])
    e._execute('ORDER',[OrderSpec('x','YES','BUY',3,200000)])
    e.finance_revision+=1
    return e


class AccountReadTests(unittest.TestCase):
    def test_endpoint_projection_does_not_publish_other_resources(self):
        expected={'account':{'cash','advance'},'positions':{'lots'},
                  'owned_orders':{'orders'},'collateral':{'advance'}}
        for resource,fields in expected.items():
            with self.subTest(resource=resource):
                e=hidden_finance();initial_cash=e.known_cash
                self.assertEqual(set(capture(e,resource)['fields']),fields)
                self.assertEqual(e.request_observation('SIG',(resource,))['status'],'IN_FLIGHT')
                self.assertFalse(e.known_lots);self.assertFalse(e.known_orders)
                self.assertEqual(e.known_cash,initial_cash)
                e.advance(101)
                self.assertEqual(bool(e.known_lots),'lots'in fields)
                self.assertEqual(bool(e.known_orders),'orders'in fields)
                self.assertEqual(e.known_cash,e.cash if 'cash'in fields else initial_cash)
                self.assertEqual(e.finance_observation_count,1)
                self.assertEqual(len(e.budget.reads),1);self.assertFalse(e.budget.writes)
                self.assertEqual(e.feed_request_counts['SIG'],1)
                self.assertEqual(e.feed_receipt_counts['SIG'],1)

    def test_resource_versions_do_not_suppress_an_independent_older_response(self):
        e=environment()
        publish(e,{'revision':7,'fields':{'cash':70}})
        self.assertTrue(publish(e,{'revision':2,'fields':{'lots':{('x','YES'):[[1,100]]}}}))
        self.assertFalse(publish(e,{'revision':6,'fields':{'cash':60}}))
        self.assertEqual(e.known_cash,70);self.assertEqual(e.known_lots[('x','YES')],[[1,100]])
        e._publish_account((3,30,{('x','YES'):[[2,200]]},{},0))
        self.assertEqual(e.known_cash,70);self.assertEqual(e.known_lots[('x','YES')],[[2,200]])
        self.assertEqual(e.known_resource_revisions['cash'],7)
        self.assertEqual(e.known_resource_revisions['lots'],3)
        self.assertEqual(e.finance_observation_count,3)

    def test_hidden_financial_mutation_count_is_not_a_network_feature(self):
        observations=[]
        for revision in (2,900000):
            e=hidden_finance();e.finance_revision=revision
            e.request_observation('SIG',('account',));e.advance(101)
            self.assertEqual(e.known_finance_revision,revision)
            self.assertEqual(e.observe()['global_state'][15],1)
            observations.append(e.observe())
        np.testing.assert_array_equal(observations[0]['global_state'],observations[1]['global_state'])
        np.testing.assert_array_equal(observations[0]['contracts'],observations[1]['contracts'])

    def test_snapshot_and_published_state_do_not_alias_physical_finance(self):
        e=hidden_finance();snapshot=capture(e,'positions');n=e.held('x','YES')
        e.lots[('x','YES')][0][0]+=10
        publish(e,snapshot)
        self.assertEqual(sum(q for q,p in e.known_lots[('x','YES')]),n)
        snapshot['fields']['lots'][('x','YES')][0][0]+=20
        self.assertEqual(sum(q for q,p in e.known_lots[('x','YES')]),n)

    def test_all_four_endpoints_share_the_same_serialized_read_budget(self):
        e=environment();keys=('account','positions','owned_orders','collateral')
        for i in range(100):
            self.assertEqual(e.request_observation('SIG',(keys[i%4],))['status'],'IN_FLIGHT')
            self.assertEqual(e.request_observation('SIG',('account',))['status'],'BUSY')
            e.advance(e.now+100)
        self.assertEqual(len(e.budget.reads),100)
        self.assertEqual(e.feed_request_counts['SIG'],100)
        self.assertEqual(e.feed_receipt_counts['SIG'],100)
        self.assertEqual(e.request_observation('SIG',('positions',))['status'],'RATE_WAIT')
        self.assertEqual(e.finance_observation_count,100)


if __name__=='__main__':unittest.main()
