"""Bound automatic latest snapshots without truncating explicit paid history."""
import tempfile
import unittest
from pathlib import Path
import numpy as np
import torch
from .test_managed_history_service import install
from .test_historical_neural import KEY, wait
from .managed_data_service import HISTORY_PROFILE, SNAPSHOT_PROFILE, advance_alarm
from .historical_neural import HistoricalNeuralPolicy
from .historical_memory import LATEST
from .training import save_checkpoint, load_checkpoint
from .state_encoding import matching
from .behavior_guard import contract, require_contract


class ManagedSnapshotTests(unittest.TestCase):
    def setUp(self): torch.set_num_threads(1)

    def test_long_uninterruptible_wait_keeps_identical_paid_schedule_and_latest_views(self):
        old, _, _ = install(HISTORY_PROFILE)
        new, _, _ = install(SNAPSHOT_PROFILE)
        for env in (old, new):
            advance_alarm(env, env.now, {'ready_ms':60000, 'wake_on_observation':False})
        self.assertEqual(new.now, 60000)
        self.assertEqual(new.feed_requests, old.feed_requests)
        self.assertEqual(new.feed_receipts, old.feed_receipts)
        self.assertEqual(new.feed_request_counts, old.feed_request_counts)
        self.assertEqual(new.known_historical_information, old.known_historical_information)
        self.assertGreater(len(old.pending_history_receipts), 2500)
        self.assertEqual(new.pending_history_receipts, [])
        self.assertEqual(len(new.pending_managed_history), 2)
        view = new.observe()
        self.assertEqual(sum(k=='historical_record' for k,_ in view['entities']), 2)
        # Repeat observations do not consume the mailbox or alter time/finances.
        np.testing.assert_array_equal(view['contracts'], new.observe()['contracts'])
        self.assertEqual(new.cash, old.cash)
        self.assertEqual(new.api_fills, old.api_fills)
        self.assertFalse(new.budget.writes)

    def test_new_receipt_during_inference_survives_observed_sequence_consumption(self):
        env, _, service = install(SNAPSHOT_PROFILE)
        env.advance(151); observed = env.observe()
        first = env.pending_managed_history['Kalshi', KEY][0]
        env.advance(251); latest = env.pending_managed_history['Kalshi', KEY]
        self.assertGreater(latest[0], first)
        env.act_raw(wait(), observed)
        self.assertEqual(env.pending_managed_history['Kalshi', KEY], latest)
        service.toggle(); env.advance(501)
        final = env.observe(); env.act_raw(wait(), final)
        self.assertFalse(env.pending_managed_history)

    def test_explicit_actor_history_pages_remain_complete_and_separate(self):
        env, reader, service = install(SNAPSHOT_PROFILE)
        env.advance(1); service.toggle(); env.advance(51)
        self.assertEqual(env.request_observation('Kalshi', KEY)['status'], 'IN_FLIGHT')
        env.advance(201)
        self.assertEqual(reader.calls, [(1,1),(51,1000)])
        self.assertEqual(len(env.pending_history_receipts), 1)
        self.assertEqual(len(env.pending_history_receipts[0][3]['payload']), 3)
        self.assertEqual(len(env.pending_managed_history), 2)
        view = env.observe()
        self.assertEqual(sum(k=='historical_record' for k,_ in view['entities']), 5)
        with self.assertRaisesRegex(ValueError, 'REGISTERED_BACKGROUND'):
            env.request_observation('Kalshi', KEY, history_latest_only=True)

    def test_profile_is_checkpoint_bound_and_old_policy_cannot_silently_change(self):
        env, _, _ = install(SNAPSHOT_PROFILE); model=HistoricalNeuralPolicy(16)
        model.managed_data_profile=HISTORY_PROFILE; model.historical_memory_profile=LATEST
        old=contract(model)
        with self.assertRaisesRegex(ValueError, 'MANAGED_DATA_MISMATCH'): matching(env,model)
        model.managed_data_profile=SNAPSHOT_PROFILE; matching(env,model)
        with self.assertRaisesRegex(ValueError, 'PROTOCOL_CHANGED'): require_contract(model,old)
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory)/'policy.pt'
            pin=save_checkpoint(path,model,torch.optim.Adam(model.parameters()),
                                dataset_manifest_sha256='a'*64,split_specification_sha256='b'*64)
            loaded,_=load_checkpoint(path,pin,'cpu'); matching(env,loaded)
            self.assertEqual(loaded.managed_data_profile,SNAPSHOT_PROFILE)
