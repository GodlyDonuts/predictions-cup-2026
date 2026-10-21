"""Independent policy RNG and actual on-policy evolution, not ROI evidence."""
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
import hashlib
import random
import tempfile
import threading
import unittest
import torch
from .neural_sessions import NeuralCoordinator, neural_section, seed_neural


def tree_sha(value):
    digest = hashlib.sha256()
    def walk(node):
        if isinstance(node, torch.Tensor):
            tensor = node.detach().cpu().contiguous()
            digest.update((str(tensor.dtype) + repr(tuple(tensor.shape))).encode())
            digest.update(tensor.numpy().tobytes())
        elif isinstance(node, dict):
            for key in sorted(node, key=repr): digest.update(repr(key).encode()); walk(node[key])
        elif isinstance(node, (list, tuple)):
            digest.update(type(node).__name__.encode())
            for item in node: walk(item)
        else: digest.update(repr(node).encode())
    walk(value)
    return digest.hexdigest()


class NeuralSessionTests(unittest.TestCase):
    def setUp(self): torch.set_num_threads(1)

    def test_nested_sections_exceptions_and_reseeding_preserve_outer_rng(self):
        torch.manual_seed(71); original = torch.get_rng_state().clone()
        session = NeuralCoordinator().session('a', 81)
        with session.bind():
            with self.assertRaisesRegex(RuntimeError, 'fixture'):
                with neural_section():
                    first = torch.rand(5)
                    with neural_section(): second = torch.rand(5)
                    raise RuntimeError('fixture')
            with neural_section(): third = torch.rand(5)
            seed_neural(81)
            with neural_section(): repeated = torch.rand(5)
        self.assertTrue(torch.equal(original, torch.get_rng_state()))
        self.assertTrue(torch.equal(first, repeated))
        with torch.random.fork_rng(devices=[]):
            torch.manual_seed(81)
            for result in [first, second, third]: self.assertTrue(torch.equal(result, torch.rand(5)))

    def test_waiting_source_does_not_own_compute_and_policy_rng_stays_private(self):
        coordinator = NeuralCoordinator(); a = coordinator.session('a', 11); b = coordinator.session('b', 12)
        ready = threading.Event(); responded = threading.Event()
        original = torch.get_rng_state().clone(); cuda_before = torch.cuda.is_initialized()
        def work_a():
            with a.bind():
                with neural_section(): first = torch.rand(9)
                ready.set()
                if not responded.wait(10): raise AssertionError('source wait held compute')
                with neural_section(): return first, torch.rand(9)
        def work_b():
            if not ready.wait(10): raise AssertionError('source did not reach fixture wait')
            with b.bind():
                with neural_section(): values = torch.rand(18)
                responded.set(); return values
        with ThreadPoolExecutor(2) as pool:
            fa = pool.submit(work_a); fb = pool.submit(work_b)
            values_a, values_b = fa.result(), fb.result()
        self.assertTrue(torch.equal(original, torch.get_rng_state()))
        self.assertEqual(cuda_before, torch.cuda.is_initialized())
        for seed, values in [(11, torch.cat(values_a)), (12, values_b)]:
            expected = torch.rand(18, generator=torch.Generator().manual_seed(seed))
            self.assertTrue(torch.equal(values, expected))

    def test_duplicate_binding_and_unbound_neural_calls_fail_closed(self):
        coordinator = NeuralCoordinator(); session = coordinator.session('same', 81)
        with self.assertRaisesRegex(ValueError, 'UNIQUE'): coordinator.session('same', 82)
        ready = threading.Event(); finish = threading.Event()
        def wait():
            with session.bind():
                ready.set()
                if not finish.wait(10): raise AssertionError('fixture did not release source')
        with ThreadPoolExecutor(1) as pool:
            future = pool.submit(wait); self.assertTrue(ready.wait(10))
            try:
                with self.assertRaisesRegex(ValueError, 'UNBOUND'):
                    with neural_section(): pass
                with self.assertRaisesRegex(ValueError, 'ONE_THREAD'):
                    with session.bind(): pass
            finally: finish.set()
            future.result()
        with self.assertRaisesRegex(ValueError, 'BOUND_PARENT'):
            with session.compute(): pass

    def test_explicit_cancellation_blocks_new_compute_without_rewinding_or_fake_completion(self):
        coordinator = NeuralCoordinator(); session = coordinator.session('a', 81)
        original = torch.get_rng_state().clone()
        with session.bind():
            with neural_section(): value = torch.rand(5)
            private = session.cpu_state.clone()
            coordinator.cancel('another owned island failed')
            with self.assertRaisesRegex(RuntimeError, 'CANCELLED'):
                with neural_section(): torch.rand(5)
            self.assertTrue(torch.equal(private, session.cpu_state))
        self.assertTrue(torch.equal(original, torch.get_rng_state()))
        with self.assertRaisesRegex(RuntimeError, 'CANCELLED'): coordinator.session('b', 82)
        with self.assertRaisesRegex(RuntimeError, 'CANCELLED'):
            with session.bind(): pass
        self.assertTrue(torch.equal(value, torch.rand(5, generator=torch.Generator().manual_seed(81))))

    def test_two_actual_population_runs_match_serial_models_adam_scores_and_mutation(self):
        from .test_population_experiment import FixtureExperiment
        def setup(root, index):
            root.mkdir(); experiment = FixtureExperiment(root)
            experiment.seed = 81 + index * 19; experiment.rng = random.Random(experiment.seed)
            experiment.config.update(members=2, cycles=2, fresh_founders=2)
            experiment.search_config['islands'] = 1
            return experiment
        def signature(experiment, summary):
            bundles = [torch.load(item['path'], map_location='cpu', weights_only=True)
                       for item in experiment.population]
            return dict(models=[tree_sha(x['model']) for x in bundles],
                optimizers=[tree_sha(x['optimizer']) for x in bundles],
                score=summary['best']['selection']['reward'],
                cases=summary['best']['selection']['cases'],
                updates=summary['total_accepted_updates'], decisions=summary['total_decisions'],
                mutations=[(x['operator'], x['loser'], x['donor']) for x in experiment.events
                           if x['event'] == 'population_replacement'])
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); expected = []
            for index in range(2):
                experiment = setup(root/f'serial-{index}', index)
                summary = experiment.run(); expected.append(signature(experiment, summary))
            coordinator = NeuralCoordinator(); experiments = [setup(root/f'parallel-{i}', i) for i in range(2)]
            sessions = [coordinator.session(str(i), e.seed) for i, e in enumerate(experiments)]
            original = torch.get_rng_state().clone()
            def run(index):
                with sessions[index].bind(): return experiments[index].run()
            with ThreadPoolExecutor(2) as pool: summaries = list(pool.map(run, range(2)))
            self.assertTrue(torch.equal(original, torch.get_rng_state()))
            actual = [signature(e, summary) for e, summary in zip(experiments, summaries)]
            self.assertEqual(expected, actual)
            self.assertTrue(all(x['updates'] > 0 and x['mutations'] for x in actual))

    def test_concurrent_spawned_worlds_match_serial_ppo_and_original_finances(self):
        from .process_worlds import ProcessCohort
        from .test_process_worlds import factory
        from .policy import ApiPolicy
        from .batched import BatchStream
        from .training import update
        from .episode_tape import tensor_digest, observation_fields
        def run(seed):
            backend = None
            try:
                with neural_section():
                    seed_neural(seed); model = ApiPolicy(16)
                    model.acting_validation = 'consolidated_v1'
                    with torch.no_grad():
                        model.heads['family'].weight.zero_(); model.heads['family'].bias.zero_()
                        model.heads['family'].bias[11] = 100
                    optimizer = torch.optim.Adam(model.parameters(), lr=1e-5)
                backend = ProcessCohort([(factory, (index, False)) for index in range(2)])
                self.assertTrue(all(x['cudaInitialized'] is False for x in backend.identities))
                stream = BatchStream(backend.worlds, model, [None]*2,
                    calibration={'decision_latency_ms':100}, compact_finished=True,
                    behavior_guard='tensor_versions_v1', rollout_only=True, world_backend=backend)
                frames = []; reports = []
                while any(e.now < e.end for e in backend.worlds):
                    chunk = stream.collect(2)
                    for frame in chunk.frames:
                        frames.append((tensor_digest(observation_fields(frame)), tree_sha({
                            k:frame[k] for k in ['actions','old_logprob','old_value','reset','learn',
                                                'terminal','initial_hidden','economic_reward','elapsed_ms']})))
                    reports += update(model, optimizer, chunk, [f['economic_reward']for f in chunk.frames],
                                      role='train', epochs=1, checkpoint_encoder=True)
                audits = [backend.request(view, 'audit', None) for view in backend.worlds]
                self.assertTrue(any(view.details['fill_count'] > 0 for view in backend.worlds))
                return dict(model=tree_sha(model.state_dict()), optimizer=tree_sha(optimizer.state_dict()),
                            hidden=tree_sha(stream.hidden), frames=frames, reports=reports,
                            finals=stream.completed, audits=audits)
            finally:
                if backend is not None: backend.close()
        expected = [run(seed) for seed in [61, 71]]
        coordinator = NeuralCoordinator(); sessions = [coordinator.session(str(seed), seed) for seed in [61,71]]
        original = torch.get_rng_state().clone()
        def concurrent(index):
            with sessions[index].bind(): return run([61,71][index])
        with ThreadPoolExecutor(2) as pool: actual = list(pool.map(concurrent, range(2)))
        self.assertTrue(torch.equal(original, torch.get_rng_state()))
        self.assertEqual(expected, actual)

    def test_concurrent_evaluation_preserves_case_generators_and_exact_outcomes(self):
        from .evaluation_batch import EvaluationBatch
        from .test_episode_tape import worlds
        from .episode_tape import financial_digest
        from .policy import ApiPolicy
        def run(seed):
            with neural_section():
                seed_neural(seed); model = ApiPolicy(16)
                with torch.no_grad():
                    model.heads['family'].weight.zero_(); model.heads['family'].bias.zero_()
                    model.heads['family'].bias[11] = 100
            envs = worlds()[:2]
            batch = EvaluationBatch(envs, model, [81,91],
                                    calibration={'decision_latency_ms':100}, batch_action_heads=True)
            rows = []
            while batch.run_chunk(1): rows += batch.last_rows
            return dict(rows=rows, hidden=tree_sha(batch.hidden),
                        financial=[financial_digest(e)for e in envs],
                        generators=[tree_sha(g.get_state())for g in batch.generators])
        expected = [run(seed)for seed in [81,91]]
        coordinator = NeuralCoordinator(); sessions = [coordinator.session(str(seed),seed)for seed in [81,91]]
        original = torch.get_rng_state().clone()
        def concurrent(index):
            with sessions[index].bind(): return run([81,91][index])
        with ThreadPoolExecutor(2) as pool: actual = list(pool.map(concurrent,range(2)))
        self.assertTrue(torch.equal(original,torch.get_rng_state()))
        self.assertEqual(expected,actual)


if __name__ == '__main__': unittest.main()
