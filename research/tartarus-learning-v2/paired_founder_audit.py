"""Finite, read-only founder/trained comparison using the original frozen source.

Example: python -I paired_founder_audit.py --source FROZEN_SOURCE --run RUN
         --output NEW_PRIVATE_FOLDER --member 0
No account requests, optimizer or independent test. Both checkpoints use CPU
sampling with the run's same frozen virtual decision clock, not CPU wall time.
Requires12GiB free RAM by default: loading a second full replay can otherwise
page the active trainer. A dedicated evaluation machine can set another value.
"""
import argparse
import gc
import hashlib
import json
import os
from pathlib import Path
import sys
import time
from datetime import datetime, timezone


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def free_memory_gib():
    if os.name=='nt':
        import ctypes
        class Memory(ctypes.Structure):
            _fields_=[('length',ctypes.c_uint32),('load',ctypes.c_uint32)]+[
                (k,ctypes.c_uint64)for k in ('total','available','total_page','available_page',
                                           'total_virtual','available_virtual','extended')]
        value=Memory();value.length=ctypes.sizeof(value)
        if not ctypes.windll.kernel32.GlobalMemoryStatusEx(ctypes.byref(value)):
            raise OSError('MEMORY_CAPACITY_CHECK_FAILED')
        return value.available/2**30
    return os.sysconf('SC_PAGE_SIZE')*os.sysconf('SC_AVPHYS_PAGES')/2**30


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--source', type=Path, required=True)
    parser.add_argument('--run', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--member', type=int, default=0)
    parser.add_argument('--minimum-free-ram-gib',type=float,default=12.)
    args = parser.parse_args()
    source, run, output = args.source.resolve(), args.run.resolve(), args.output.resolve()
    if output.exists():
        raise ValueError('AUDIT_OUTPUT_MUST_BE_NEW')
    import math
    if not math.isfinite(args.minimum_free_ram_gib)or args.minimum_free_ram_gib<0:
        parser.error('minimum free RAM must be finite and nonnegative')
    available=free_memory_gib()
    if available<args.minimum_free_ram_gib:
        parser.error(f'CPU_AUDIT_CAPACITY_UNAVAILABLE: {available:.2f}GiB free; requires {args.minimum_free_ram_gib:.2f}GiB')
    os.chdir(source)
    sys.path.insert(0, str(source))
    from moloch.learning_actions import experiment as loaded_source
    from moloch.learning_actions.continue_population import verify_source
    from moloch.learning_actions.selection import frozen
    from moloch.learning_actions.training import load_checkpoint
    if Path(loaded_source.__file__).resolve().parent != source/'moloch/learning_actions':
        raise ValueError('ORIGINAL_FROZEN_SOURCE_IMPORT_REQUIRED')
    manifest = verify_source(source)
    proof = json.loads((run/'experiment.json').read_text())
    expected_pins = {k:v for k,v in manifest['files'].items() if len(Path(k).parts)==3
        and Path(k).parts[1] in ('learning_actions','learning_realism','learning_v2','learning_flow')
        and not Path(k).name.startswith('test_')}
    if {k.replace('\\','/'):v for k,v in proof['source_sha256'].items()} != expected_pins:
        raise ValueError('TRAINING_SOURCE_IDENTITY_MISMATCH')
    config = proof['config']
    if config['mode'] != 'inspected_development' or config.get('batched_evaluation',False):
        raise ValueError('EXACT_SERIAL_INSPECTED_DEVELOPMENT_PROFILE_REQUIRED')
    original = json.loads((run/f'result-000-{args.member:03d}.json').read_text())
    paths = {'founder':run/f'founder-{args.member:03d}.pt',
             'trained':Path(original['checkpoint']['path']).resolve()}
    if paths['trained'].parent != run or sha(paths['trained']) != original['checkpoint']['sha256']:
        raise ValueError('EXACT_COMPLETED_MEMBER_CHECKPOINT_REQUIRED')
    checkpoints = {k:sha(p) for k,p in paths.items()}
    output.mkdir(parents=True)
    plan = {'classification':'predeclared_cpu_paired_inspected_development_not_gpu_score_reproduction',
        'sourceCommit':manifest['commit'],'sourceManifestSha256':sha(source/'source-snapshot.json'),
        'originalExperimentSha256':sha(run/'experiment.json'),'checkpointSha256':checkpoints,
        'originalGpuResultSha256':sha(run/f'result-000-{args.member:03d}.json'),
        'configurationOverrides':{'device':'cpu'},'member':args.member,'cycle':0,
        'executionOrder':['founder','trained'],'independentTestUsed':False,
        'optimizerUpdates':0,'productionChanged':False,'scriptSha256':sha(__file__)}
    frozen(output/'PLAN.json',plan)

    def status(**fields):
        value={'observedUtc':datetime.now(timezone.utc).isoformat(),**fields}
        pending=output/'status.pending.json'
        pending.write_text(json.dumps(value,indent=2)+'\n')
        pending.replace(output/'status.json')
        print(json.dumps(value),flush=True)

    class Audit(loaded_source.Experiment):
        label=None
        def world(self, role, start, end, finalist=None, stress=False):
            status(phase='case_started',checkpoint=self.label,startMs=start,endMs=end,stress=stress)
            return super().world(role,start,end,finalist,stress)

        def stream(self, model, env, training=False):
            if training:raise ValueError('AUDIT_HAS_NO_OPTIMIZER')
            stream=super().stream(model,env,training=False)
            collect=stream.run_chunk;last=[time.monotonic()]
            def chunk(*a,**kw):
                result=collect(*a,**kw)
                if time.monotonic()-last[0]>=30:
                    last[0]=time.monotonic()
                    status(phase='case_progress',checkpoint=self.label,decisions=stream.decisions,
                           virtualMs=env.now,endMs=env.end)
                return result
            stream.run_chunk=chunk
            return stream

    try:
        audit=Audit({**config,'device':'cpu'},output/'experiment')
        if audit.dataset_sha != proof['dataset_manifest_sha256']:
            raise ValueError('EXACT_DATASET_REQUIRED')
        audit.calibration=json.loads((run/'compute-calibration.json').read_text())
        frozen(output/'virtual-compute-clock.json',audit.calibration)
        results={}
        for label in plan['executionOrder']:
            audit.label=label
            status(phase='checkpoint_loading',checkpoint=label)
            model,bundle=load_checkpoint(paths[label],checkpoints[label],device='cpu')
            del bundle;gc.collect()
            result=audit.evaluate(model,{'path':str(paths[label]),'sha256':checkpoints[label]})
            frozen(output/(label+'-result.json'),result)
            results[label]=result
            del model;gc.collect()
            status(phase='checkpoint_complete',checkpoint=label,worstRoi=result['marked_roi_min'])
        pairs=[]
        for before,after in zip(results['founder']['cases'],results['trained']['cases']):
            keys=('start_ms','end_ms','stress','selection_seed')
            if any(before[k]!=after[k]for k in keys):raise ValueError('PAIRED_CASES_MUST_MATCH')
            pairs.append({**{k:before[k]for k in keys},'founderRoi':before['marked_roi'],
                'trainedRoi':after['marked_roi'],'roiDelta':after['marked_roi']-before['marked_roi'],
                'founderDecisions':before['decisions'],'trainedDecisions':after['decisions']})
        report={**plan,'completedUtc':datetime.now(timezone.utc).isoformat(),
            'virtualDecisionLatencyMs':audit.calibration['decision_latency_ms'],
            'datasetManifestSha256':audit.dataset_sha,'pairs':pairs,
            'founderWorstRoi':results['founder']['marked_roi_min'],
            'trainedWorstRoi':results['trained']['marked_roi_min'],
            'worstRoiDelta':results['trained']['marked_roi_min']-results['founder']['marked_roi_min'],
            'independentProfitableRoiEstablished':False,
            'limitations':['single predeclared member; four correlated development cases',
                'CPU stochastic samples; not reproduction of GPU samples',
                'same frozen23ms virtual model clock, not a CPU speed benchmark',
                'uncalibrated execution and midpoint marking; inspected short history']}
        frozen(output/'RESULT.json',report)
        status(phase='complete',worstRoiDelta=report['worstRoiDelta'])
    except BaseException as exc:
        status(phase='failed',exception=type(exc).__name__,reason=str(exc))
        raise


if __name__=='__main__':main()
