import unittest
from unittest.mock import patch,Mock
from .gpu_diagnostic_guard import require_idle_gpu


class DiagnosticGpuGuardTests(unittest.TestCase):
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
