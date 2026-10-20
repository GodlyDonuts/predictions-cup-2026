import unittest,torch
from .test_actions import environment
from .policy import ApiPolicy
from .stream import Stream
from .batched import BatchStream
from .economics import GrowthLedger
from .training import update
from .contract import OrderSpec


class BatchTests(unittest.TestCase):
    def test_omitting_unused_bootstrap_keeps_actions_rng_worlds_and_compaction_exact(self):
        from .api_affordances import ApiAffordancePolicy
        from .test_api_affordances import world
        from .episode_tape import tensor_digest,observation_fields,financial_digest
        torch.set_num_threads(1)
        for factory,policy in((environment,ApiPolicy),(world,ApiAffordancePolicy)):
            for rollout in(False,True):
                for compact in(False,True):
                    with self.subTest(policy=policy.__name__,rollout=rollout,compact=compact):
                        torch.manual_seed(431);model=policy(16);model.acting_validation='consolidated_v1'
                        with torch.no_grad():
                            model.heads['family'].weight.zero_();model.heads['family'].bias.zero_()
                            model.heads['family'].bias[11]=100
                        outputs=[]
                        for bootstrap in(True,False):
                            torch.manual_seed(833);envs=[factory(),factory()];envs[0].end=20;envs[1].end=3000
                            stream=BatchStream(envs,model,[GrowthLedger(e,development=True)for e in envs],
                                calibration={'decision_latency_ms':50},compact_finished=compact,rollout_only=rollout)
                            encodes=[];hook=model.encoder.register_forward_hook(lambda *args:encodes.append(True))
                            frames=[]
                            try:
                                for _ in range(3):
                                    chunk=stream.collect(3,bootstrap=bootstrap)
                                    self.assertEqual(chunk.bootstrap_value is None,not bootstrap)
                                    for frame in chunk.frames:
                                        fields={k:frame[k]for k in('actions','old_logprob','old_value','initial_hidden','reset','learn','terminal')}
                                        frames.append((tensor_digest(observation_fields(frame)),tensor_digest(fields),
                                                       frame['economic_reward'],frame['elapsed_ms']))
                            finally:hook.remove()
                            outputs.append((frames,torch.get_rng_state().clone(),[financial_digest(e)for e in envs],
                                            stream.hidden.clone(),len(encodes)))
                        a,b=outputs;self.assertEqual(a[0],b[0]);self.assertTrue(torch.equal(a[1],b[1]))
                        self.assertEqual(a[2],b[2]);self.assertTrue(torch.equal(a[3],b[3]))
                        self.assertEqual(a[4]-b[4],3)

    def test_bootstrap_free_chunks_cannot_silently_train_as_open_streaming_returns(self):
        from moloch.learning_v2.train_inner import fingerprint
        torch.set_num_threads(1);torch.manual_seed(51);model=ApiPolicy(16);env=environment()
        stream=BatchStream([env],model,[GrowthLedger(env,development=True)],calibration={'decision_latency_ms':2})
        chunk=stream.collect(2,bootstrap=False);before=fingerprint(model);optimizer=torch.optim.Adam(model.parameters())
        with self.assertRaisesRegex(ValueError,'SEALED_EPISODE_TARGETS_REQUIRED'):
            update(model,optimizer,chunk,[f['economic_reward']for f in chunk.frames],role='train')
        self.assertEqual(fingerprint(model),before);self.assertEqual(optimizer.state_dict()['state'],{})

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
