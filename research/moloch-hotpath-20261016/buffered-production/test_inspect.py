import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec=importlib.util.spec_from_file_location('buffered_hotpath',Path(__file__).with_name('inspect.py'))
m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
base=m.load_base()


class ComparisonTests(unittest.TestCase):
    def test_source_commit_and_original_bytes_both_pinned(self):
        with tempfile.TemporaryDirectory() as d:
            p=Path(d)/'source.json';p.write_bytes(b'{"sourceCommit":"checked"}')
            digest=m.sha(p.read_bytes())
            self.assertEqual(m.pinned_source(p,digest,'checked')['sourceCommit'],'checked')
            with self.assertRaises(ValueError):m.pinned_source(p,digest,'other')
            p.write_bytes(b'{"sourceCommit":"checked"}\n')
            with self.assertRaises(ValueError):m.pinned_source(p,digest,'checked')

    def test_historical_status_is_excluded_not_assumed_completed(self):
        since=base.clock('2026-10-16T12:00:00Z');now=since+1000
        proof={'sha256':'a'*64,'bytes':20}
        self.assertFalse(m.diagnostic({'pid':1,'at':'2026-10-16T12:00:00Z','cycles':99},proof,2,since,now,base)['accepted'])
        result=m.diagnostic({'pid':2,'at':'2026-10-16T11:59:59Z','cycles':99},proof,2,since,now,base)
        self.assertEqual(result['reason'],'before_readiness')
        self.assertNotIn('cycles',result)

    def test_future_clock_excluded_and_private_raw_fields_removed(self):
        since=base.clock('2026-10-16T12:00:00Z')
        value={'pid':2,'at':'2026-10-16T12:00:00Z','cycles':3,'report':{'profile':'private'},'selectedReadScopes':['a','b']}
        result=m.diagnostic(value,{},2,since,since+1000,base)
        self.assertTrue(result['accepted']);self.assertEqual(result['selectedReadScopeCount'],2)
        self.assertNotIn('private',json.dumps(result))
        self.assertFalse(m.diagnostic(value,{},2,since,since-1,base)['accepted'])

    def test_rollover_never_fabricates_scope_throughput(self):
        def sample(completed,cycles,selected=35):return {'files':{'progress.json':{'accepted':True,'completedReadScopes':completed,'selectedReadScopeCount':selected},'status.json':{'accepted':True,'cycles':cycles}}}
        result=m.progress_change(sample(34,0),sample(2,1,39))
        self.assertEqual(result['completedCycleDelta'],1)
        self.assertIsNone(result['scopesCompletedWithinSameSelection'])
        self.assertEqual(m.progress_change(sample(2,1),sample(5,1))['scopesCompletedWithinSameSelection'],3)
        a=sample(2,1);a['files']['status.json']={'accepted':False,'reason':'different_pid'}
        self.assertIsNone(m.progress_change(a,sample(5,1))['completedCycleDelta'])

    def test_process_counter_regression_and_pid_reuse_fail(self):
        a={'startTicks':1,'monotonic':1,'ticksPerSecond':100,'userTicks':1,'systemTicks':1,'io':{'syscw':100,'wchar':1000,'read_bytes':0,'write_bytes':4096}}
        b={**a,'monotonic':11,'io':{**a['io'],'syscw':101,'wchar':66536}}
        self.assertEqual(m.checked_delta(a,b,base)['writeSyscalls'],1)
        with self.assertRaises(ValueError):m.checked_delta(a,{**b,'startTicks':2},base)
        with self.assertRaises(ValueError):m.checked_delta(a,{**b,'io':{**b['io'],'syscw':99}},base)

    def test_comparison_never_claims_whole_loop_speedup_from_sample(self):
        def result(pid,seconds,calls):return {'pid':pid,'notBefore':'a','observedCutoff':'b','processDeltas':[{'elapsedSeconds':seconds,'oneCoreCpuPercent':50,'writeSyscalls':calls,'writeChars':10000,'bytesPerWriteSyscall':10000/calls}]}
        out=m.comparison(result(1,10,1000),result(2,10,10),'a'*64)
        self.assertTrue(out['approximatelyMatchingTenSecondSamples']);self.assertFalse(out['wholeLoopSpeedupEstablished'])
        self.assertEqual(out['current']['writeSyscallsPerSecond'],1)

    def test_output_cannot_replace_runtime_or_source_and_symlink_refused(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d)/'runtime';root.mkdir();baseline=Path(d)/'base.json';source=Path(d)/'source.json';source.write_bytes(b'{}')
            with self.assertRaises(ValueError):m.output_allowed(root/'report.json',root,baseline,source)
            with self.assertRaises(ValueError):m.output_allowed(source,root,baseline,source)
            m.output_allowed(Path(d)/'new.json',root,baseline,source)
            link=Path(d)/'link.json'
            try:link.symlink_to(source)
            except OSError:return
            with self.assertRaises(ValueError):m.original_json(link)

    def test_exact_original_aggregate_baseline_is_present(self):
        original=Path(__file__).resolve().parent.parent/'report.json'
        self.assertEqual(m.sha(original.read_bytes()),m.BASELINE_SHA256)


if __name__=='__main__':unittest.main()
