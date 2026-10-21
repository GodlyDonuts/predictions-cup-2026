import unittest
from unittest.mock import patch,Mock
from .gpu_diagnostic_guard import require_idle_gpu,training_pids


class DiagnosticGpuGuardTests(unittest.TestCase):
    def test_one_parent_island_runner_is_in_the_existing_owner_inventory(self):
        with patch('moloch.learning_actions.gpu_diagnostic_guard.os.name','nt'),\
             patch('moloch.learning_actions.gpu_diagnostic_guard.subprocess.run',return_value=Mock(stdout='[]'))as run:
            self.assertEqual(training_pids(),[])
        script=run.call_args.args[0][-1]
        self.assertIn('asynchronous_islands',script);self.assertIn('process_capacity',script)
    def test_cpu_never_queries_or_reserves_training_gpu(self):
        with patch('moloch.learning_actions.gpu_diagnostic_guard.training_pids',side_effect=AssertionError):require_idle_gpu('cpu')

    def test_active_worker_is_rejected_before_gpu_tool_call(self):
        with patch('moloch.learning_actions.gpu_diagnostic_guard.training_pids',return_value=[42]),\
             patch('moloch.learning_actions.gpu_diagnostic_guard.subprocess.run',side_effect=AssertionError):
            with self.assertRaisesRegex(ValueError,'ACTIVE_ML_WORKERS'):require_idle_gpu('cuda:0')

    def test_other_compute_is_rejected_and_empty_device_is_allowed(self):
        with patch('moloch.learning_actions.gpu_diagnostic_guard.training_pids',return_value=[]),\
             patch('moloch.learning_actions.gpu_diagnostic_guard.subprocess.run',return_value=Mock(stdout='424242\n')):
            with self.assertRaisesRegex(ValueError,'OTHER_COMPUTE'):require_idle_gpu('cuda')
        with patch('moloch.learning_actions.gpu_diagnostic_guard.training_pids',return_value=[]),\
             patch('moloch.learning_actions.gpu_diagnostic_guard.subprocess.run',return_value=Mock(stdout='')):require_idle_gpu('cuda')

    def test_capacity_exception_is_exact_verified_host_and_keeps_strict_default(self):
        with patch('moloch.learning_actions.gpu_diagnostic_guard.training_pids',return_value=[]),\
             patch('moloch.learning_actions.gpu_diagnostic_guard.verified_desktop_pids',return_value=[101]),\
             patch('moloch.learning_actions.gpu_diagnostic_guard.subprocess.run',return_value=Mock(stdout='101\n')):
            with self.assertRaisesRegex(ValueError,'OTHER_COMPUTE'):require_idle_gpu('cuda')
            require_idle_gpu('cuda',allow_verified_desktop=True)
        with patch('moloch.learning_actions.gpu_diagnostic_guard.training_pids',return_value=[]),\
             patch('moloch.learning_actions.gpu_diagnostic_guard.verified_desktop_pids',return_value=[101]),\
             patch('moloch.learning_actions.gpu_diagnostic_guard.subprocess.run',return_value=Mock(stdout='101\n202\n')):
            with self.assertRaisesRegex(ValueError,'OTHER_COMPUTE_PROCESSES:202'):
                require_idle_gpu('cuda',allow_verified_desktop=True)

    def test_desktop_opt_in_never_bypasses_active_learning_worker(self):
        with patch('moloch.learning_actions.gpu_diagnostic_guard.training_pids',return_value=[42]),\
             patch('moloch.learning_actions.gpu_diagnostic_guard.verified_desktop_pids',side_effect=AssertionError):
            with self.assertRaisesRegex(ValueError,'ACTIVE_ML_WORKERS'):
                require_idle_gpu('cuda',allow_verified_desktop=True)
