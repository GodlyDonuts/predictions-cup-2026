import importlib.util
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location('queue_analysis', Path(__file__).with_name('analyze.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class ExactProjectionTests(unittest.TestCase):
    def test_closed_form_matches_every_record_of_small_read_write_trace(self):
        for reads, writes in [(100, 1), (100, 5), (100, 24), (0, 24), (95, 1)]:
            for minutes in [1, 2, 7]:
                initial, base, increment = 98, 429, 390
                n, actual = initial, 0
                for _ in range(minutes):
                    actual += reads * 3 * (base + increment * n)
                    for _ in range(writes):
                        n += 1
                        actual += 3 * (base + increment * n)
                self.assertEqual(actual, module.journal_bytes(reads, writes, minutes, initial, base, increment))

    def test_storage_budget_search_is_first_whole_minute_reaching_budget(self):
        budget = module.journal_bytes(100, 5, 100, 98, 429, 390)
        self.assertEqual(module.budget_minute(100, 5, 98, 429, 390, budget), 100)

    def test_growth_is_quadratic_not_constant_bytes_per_hour(self):
        one = module.journal_bytes(100, 5, 1440, 98, 429, 390)
        two = module.journal_bytes(100, 5, 2880, 98, 429, 390)
        self.assertGreater(two, one * 3.9)


if __name__ == '__main__':
    unittest.main()
