import copy
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec=importlib.util.spec_from_file_location("bench",Path(__file__).with_name("benchmark.py"))
b=importlib.util.module_from_spec(spec);spec.loader.exec_module(b)

class Guards(unittest.TestCase):
    def proposal(self):
        original={"upgrades":[{"hash":str(n).zfill(64),"original":n} for n in range(12)]}
        head={"sequence":308166};report={"sequence":308166,"report":{"cash":"unchanged"}}
        prefix={"bytes":952000000,"sha256":"a"*64}
        proposed=copy.deepcopy(original);proposed["upgrades"].append({"fromEngineIdentity":"b"*64,"toEngineIdentity":"c"*64,"oldHead":head,"oldReportEnvelope":report,"journalPrefix":prefix,"previousUpgradeHash":original["upgrades"][-1]["hash"]})
        return proposed,original,head,report,prefix,"b"*64,"c"*64
    def test_exact_original_twelve_and_distinct_thirteenth(self):
        b.proposal_check(*self.proposal())
    def test_old_entry_mutation_cannot_be_resealed_as_offline_parity(self):
        args=list(self.proposal());args[0]["upgrades"][4]["original"]=99
        with self.assertRaisesRegex(ValueError,"ORIGINAL_BRIDGES_CHANGED"):b.proposal_check(*args)
    def test_proposal_rejects_changed_cash_head_prefix_and_identity(self):
        for field in ("oldHead","oldReportEnvelope","journalPrefix","toEngineIdentity"):
            args=list(self.proposal());args[0]["upgrades"][-1][field]={} if field!="toEngineIdentity" else "b"*64
            with self.subTest(field=field),self.assertRaises(ValueError):b.proposal_check(*args)
    def test_absent_or_extra_original_bridge_rejects(self):
        for count in (11,13):
            args=list(self.proposal());args[1]["upgrades"]=[{"hash":"a"*64}]*count
            with self.subTest(count=count),self.assertRaisesRegex(ValueError,"EXACT_TWELVE_PLUS_ONE"):b.proposal_check(*args)
    def test_source_identity_requires_all_sixteen_exact_inputs(self):
        with self.assertRaisesRegex(ValueError,"SOURCE_INPUT_SET"):b.source_identity(Path.cwd(),{})
    def test_private_roots_reject_production_and_relative_paths(self):
        for path in ("relative", "/home/archon/apps/moloch/shared/live-production-v1", "/tmp/anything-else"):
            with self.subTest(path=path),self.assertRaises(ValueError):b.private_root(path,"/tmp/moloch-native-flow-benchmark-")
    def test_results_are_create_new_not_overwrite(self):
        with tempfile.TemporaryDirectory() as d:
            path=Path(d)/"result.json";b.write_new(path,{"original":True});original=path.read_bytes()
            with self.assertRaises(FileExistsError):b.write_new(path,{"changed":True})
            self.assertEqual(path.read_bytes(),original);self.assertEqual(json.loads(original),{"original":True})

if __name__=="__main__":unittest.main()
