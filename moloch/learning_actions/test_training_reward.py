"""Synthetic reward/calendar integration; no market-performance claims."""
import tempfile,unittest
from pathlib import Path
import torch
from moloch.learning_realism.protocol import DAY
from .training_reward import horizon,ledger
from .test_actions import environment
from .test_episode_tape import worlds
from .economics import GrowthLedger
from .policy import ApiPolicy
from .batched import BatchStream
from .episode_tape import EpisodeWriter,EpisodeReader


class TrainingRewardTests(unittest.TestCase):
    def test_explicit_daily_horizon_is_not_clipped_into_short_proxy(self):
        c={'development_training_daily_growth':True,'training_days':2}
        self.assertEqual(horizon(c,(1,3*DAY+1),development=True),2*DAY)
        for change in ({'training_days':1},{'training_days':2.5},
                       {'development_episode_ms':3600000}):
            with self.subTest(change=change),self.assertRaises(ValueError):
                horizon(dict(c,**change),(1,3*DAY+1),development=True)
        with self.assertRaisesRegex(ValueError,'HISTORY_REQUIRED'):
            horizon(c,(1,2*DAY),development=True)

    def test_short_development_remains_explicit_proxy(self):
        self.assertEqual(horizon({},(1,1001),development=True),1000)
        e=environment();l=ledger(e,{},development=True);e.advance(e.end)
        self.assertIsNone(l.finish()['D'])
        for value in (0,-1,True,1.5):
            with self.subTest(value=value),self.assertRaises(ValueError):
                horizon({'development_episode_ms':value},(1,1001),development=True)

    def test_daily_profile_cannot_claim_full_history_or_change_world_length(self):
        c={'development_training_daily_growth':True,'training_days':2}
        with self.assertRaises(ValueError):horizon(c,(1,3*DAY),development=False)
        for value in (1,'yes',None):
            with self.subTest(value=value),self.assertRaises(ValueError):
                horizon(dict(c,development_training_daily_growth=value),(1,3*DAY),development=True)
        for length in (DAY,2*DAY-1,2*DAY+1,3*DAY):
            e=environment();e.end=e.now+length
            with self.subTest(length=length),self.assertRaises(ValueError):ledger(e,c,development=True)

    def test_complete_day_collection_replays_same_human_reward(self):
        torch.set_num_threads(1);torch.manual_seed(118)
        config={'development_training_daily_growth':True,'training_days':2,
                'development_last_observed_marks':True,'maximum_mark_age_ms':3*DAY}
        def make_worlds():
            envs=worlds()
            for e in envs:
                e.end=e.now+2*DAY
                # After all source-fixture books, a synthetic day-one gain.
                e.clock.schedule(e.now+DAY//4,10,'fixture_late_mark',None)
            return envs
        model=ApiPolicy(16)
        with torch.no_grad():
            model.heads['family'].weight.zero_();model.heads['family'].bias.zero_()
            model.heads['family'].bias[11]=100
        envs=make_worlds();ledgers=[ledger(e,config,development=True)for e in envs]
        stream=BatchStream(envs,model,ledgers,calibration={'decision_latency_ms':DAY//2},
                           behavior_guard='tensor_versions_v1',compact_finished=True)
        with tempfile.TemporaryDirectory()as folder:
            path=Path(folder)/'episode';writer=EpisodeWriter(path,stream,
                dataset_sha256='a'*64,specification_sha256='b'*64)
            try:
                while any(e.now<e.end for e in envs):
                    writer.record(stream.collect(2,bootstrap=False))
                sha=writer.seal()
            finally:writer.close()
            for r in stream.completed.values():
                self.assertEqual(r['D'],2);self.assertFalse(r['eligible'])
                self.assertTrue(r['development_daily_growth'])
                self.assertLess(r['reward'],0) # one gain day, then flat: consistency penalty
            reader=EpisodeReader(path,sha)
            try:
                replay_envs=make_worlds()
                replay_ledgers=[ledger(e,config,development=True)for e in replay_envs]
                chunks=list(reader.replay(replay_envs,replay_ledgers))
                self.assertTrue(chunks)
                self.assertTrue(all(l.closed for l in replay_ledgers))
                for original,replayed in zip(ledgers,replay_ledgers):
                    self.assertEqual(original.boundaries,replayed.boundaries)
                    self.assertEqual(original.reward_sum,replayed.reward_sum)
            finally:reader.close()
