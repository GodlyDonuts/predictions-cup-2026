import unittest,torch
from .test_actions import environment
from .policy import ApiPolicy
from .stream import Stream
from .batched import BatchStream
from .economics import GrowthLedger
from .training import update
from .contract import OrderSpec


class BatchTests(unittest.TestCase):
    def test_batched_worlds_match_individual_deterministic_inference_and_accounts(self):
        torch.set_num_threads(1);torch.manual_seed(51)
        model=ApiPolicy(16)
        with torch.no_grad():model.fractions.bias[2]=-10.;model.fractions.bias[3]=100.
        serial=[environment(),environment()];parallel=[environment(),environment()]
        for group in (serial,parallel):
            group[1]._execute('ORDER',[OrderSpec('x','YES','BUY',1,5000)])
            group[1].known_orders=group[1].api_orders.copy()
        streams=[Stream(e,model,calibration={'decision_latency_ms':2})for e in serial]
        batch=BatchStream(parallel,model,[GrowthLedger(e,development=True)for e in parallel],
                          calibration={'decision_latency_ms':2})
        for _ in range(8):
            for stream in streams:stream.step(deterministic=True)
            batch.step(deterministic=True)
        for i,(a,b)in enumerate(zip(serial,parallel)):
            self.assertEqual((a.now,a.cash,a.lots,a.api_id),(b.now,b.cash,b.lots,b.api_id))
            torch.testing.assert_close(streams[i].hidden[0],batch.hidden[i],atol=1e-6,rtol=1e-5)
            self.assertIsNot(parallel[0].books,parallel[1].books)

    def test_finished_world_is_excluded_while_other_world_continues_and_updates(self):
        torch.set_num_threads(1);torch.manual_seed(55)
        model=ApiPolicy(16)
        with torch.no_grad():model.fractions.bias[2]=-10.;model.fractions.bias[3]=100.
        a=environment();a.end=a.now+1;b=environment();b.end=b.now+10000
        stream=BatchStream([a,b],model,[GrowthLedger(e,development=True)for e in(a,b)],
                           calibration={'decision_latency_ms':2})
        chunk=stream.collect(8)
        self.assertIn(0,stream.completed)
        self.assertTrue(chunk.frames[0]['learn'][0]);self.assertFalse(chunk.frames[1]['learn'][0])
        self.assertTrue(chunk.frames[-1]['learn'][1]);self.assertLess(b.now,b.end)
        opt=torch.optim.Adam(model.parameters(),lr=1e-6)
        result=update(model,opt,chunk,[f['economic_reward']for f in chunk.frames],role='train',epochs=1)
        self.assertTrue(result[0]['accepted']);before=b.now
        stream.collect(4);self.assertGreater(b.now,before)


if __name__=='__main__':unittest.main()
