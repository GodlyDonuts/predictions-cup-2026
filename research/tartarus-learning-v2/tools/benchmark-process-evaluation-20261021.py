"""Interleaved local/spawned CPU evaluation with exact trajectory comparison."""
import argparse,gc,hashlib,json,os,sys,time
from pathlib import Path
from datetime import datetime,timezone


def main(job):
    job=Path(job).resolve();plan=json.loads((job/'PLAN.json').read_text())
    source=Path(plan['sourceRoot']);sys.path.insert(0,str(source));os.chdir(source)
    from moloch.learning_actions.continue_population import verify_source
    from moloch.learning_actions.population_handoff import sha_file
    from moloch.learning_actions.experiment import Experiment
    from moloch.learning_actions.training import load_checkpoint
    from moloch.learning_v2.train_inner import fingerprint
    from moloch.learning_actions import evaluation_batch
    import numpy as np
    assert verify_source(source)['commit']==plan['sourceCommit']
    assert sha_file(source/'source-snapshot.json')==plan['sourceManifestSha256']
    for name,key in [('config.json','configurationSha256'),('actor-calibration.json','actorCalibrationSha256')]:
        assert sha_file(job/name)==plan[key]
    config=json.loads((job/'config.json').read_text());calibration=json.loads((job/'actor-calibration.json').read_text())
    assert config['mode']=='inspected_development' and plan['independentTestUsed']is False
    def digest(h,value):
        if isinstance(value,np.ndarray):h.update(str(value.dtype).encode()+repr(value.shape).encode()+value.tobytes())
        elif isinstance(value,dict):
            for k in sorted(value):digest(h,k);digest(h,value[k])
        elif isinstance(value,(tuple,list)):
            h.update(str(type(value)).encode())
            for item in value:digest(h,item)
        else:h.update(repr(value).encode()+b'\0')
    results=[];original_step=evaluation_batch.EvaluationBatch.step;original_inputs=evaluation_batch.inputs
    for index,remote in enumerate((False,True,True,False)):
        owner=Experiment(dict(config,process_evaluation_worlds=remote),job/('case-%d'%index))
        owner.calibration=calibration
        model,bundle=load_checkpoint(plan['checkpoint']['path'],plan['checkpoint']['sha256'],'cpu')
        assert bundle['dataset_manifest_sha256']==owner.dataset_sha;del bundle
        before=fingerprint(model);model.eval();observations=hashlib.sha256();actions=hashlib.sha256();last=time.monotonic()
        def prepared(values,device):digest(observations,values);return original_inputs(values,device)
        def step(batch,**kwargs):
            nonlocal last
            value=original_step(batch,**kwargs);digest(actions,batch.last_rows)
            if time.monotonic()-last>=20:
                (job/'progress.json').write_text(json.dumps(dict(case=index,remote=remote,
                    virtualMs=[e.now for e in batch.envs],decisions=batch.decisions)),encoding='utf-8');last=time.monotonic()
            return value
        evaluation_batch.inputs=prepared;evaluation_batch.EvaluationBatch.step=step
        try:
            began=time.perf_counter();result=owner.evaluate(model,plan['checkpoint']);elapsed=time.perf_counter()-began
            for case in result['cases']:case.pop('source_compute_mode',None)
            assert fingerprint(model)==before
            canonical=json.dumps(result,sort_keys=True,separators=(',',':'),allow_nan=False)
            row=dict(remote=remote,wallSeconds=elapsed,observationSha256=observations.hexdigest(),
                actionSha256=actions.hexdigest(),resultSha256=hashlib.sha256(canonical.encode()).hexdigest(),
                decisions=[c['decisions']for c in result['cases']],fills=[c['fill_count']for c in result['cases']],
                boundsMs=[[c['start_ms'],c['end_ms']]for c in result['cases']],weightsUnchanged=True)
            if results:
                for key in ('observationSha256','actionSha256','resultSha256','decisions','fills','boundsMs'):
                    assert row[key]==results[0][key],key
            results.append(row);(job/('case-%d.json'%index)).write_text(json.dumps(row,indent=2),encoding='utf-8')
        finally:
            evaluation_batch.inputs=original_inputs;evaluation_batch.EvaluationBatch.step=original_step;owner.close()
            del model,owner;gc.collect()
    from statistics import median
    old=median(r['wallSeconds']for r in results if not r['remote']);new=median(r['wallSeconds']for r in results if r['remote'])
    verify_source(source)
    result=dict(observedUtc=datetime.now(timezone.utc).isoformat(),sourceCommit=plan['sourceCommit'],
        planSha256=sha_file(job/'PLAN.json'),sourceManifestSha256=plan['sourceManifestSha256'],results=results,
        speedupIncludingProcessStartup=old/new,exactObservationActionAndEconomicResultParity=True,
        optimizerRun=False,independentTestUsed=False,productionChanged=False,
        diagnosticWindowMs=config['development_selection_episode_ms'],
        classification='interleaved_cpu_development_diagnostic_alongside_original_gpu_training_not_fitness_or_roi')
    (job/'RESULT.json').write_text(json.dumps(result,indent=2),encoding='utf-8');print(json.dumps(result),flush=True)


if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--job',required=True);args=parser.parse_args()
    try:main(args.job)
    except BaseException as error:
        (Path(args.job)/'FAILED.json').write_text(json.dumps(dict(type=type(error).__name__,reason=str(error))),encoding='utf-8');raise
