"""One local training-generation handoff, never a production deployment.

Run only from a private snapshot of checked code. The existing trainer stays
alive until its completed generation and selected neural checkpoints verify.
"""
import argparse,hashlib,io,json,subprocess,tarfile,time
from datetime import datetime,timezone
from pathlib import Path
from .population_handoff import boundary,select_population,next_configuration,sha_file
from .training_process import TrainingProcess
from .selection import frozen


def freeze_source(repository,commit,destination):
    repository=Path(repository);destination=Path(destination).resolve()
    resolved=subprocess.check_output(['git','rev-parse',commit],cwd=repository,text=True).strip()
    if resolved!=commit or len(commit)!=40:raise ValueError('EXACT_CHECKED_COMMIT_REQUIRED')
    raw=subprocess.check_output(['git','archive',commit,'--','moloch','research/tartarus-learning-v2/development-mechanics-v2.json'],cwd=repository)
    destination.mkdir(parents=True,exist_ok=False);pins={}
    with tarfile.open(fileobj=io.BytesIO(raw))as archive:
        for item in archive:
            if not item.isfile()or not(item.name.endswith('.py')or item.name=='research/tartarus-learning-v2/development-mechanics-v2.json'):continue
            target=(destination/item.name).resolve()
            if not target.is_relative_to(destination):raise ValueError('SOURCE_PATH_OUTSIDE_PRIVATE_SNAPSHOT')
            value=archive.extractfile(item).read();target.parent.mkdir(parents=True,exist_ok=True);target.write_bytes(value)
            pins[item.name]=hashlib.sha256(value).hexdigest()
    manifest={'commit':commit,'files':pins,'productionAuthority':False}
    frozen(destination/'source-snapshot.json',manifest);return manifest


def verify_source(root):
    root=Path(root);manifest=json.loads((root/'source-snapshot.json').read_text())
    actual={str(p.relative_to(root)).replace('\\','/')for p in(root/'moloch').rglob('*.py')}
    if actual!={p for p in manifest['files']if p.endswith('.py')}:raise ValueError('FROZEN_SOURCE_FILE_SET_CHANGED')
    for name,digest in manifest['files'].items():
        target=(root/name).resolve()
        if not target.is_relative_to(root.resolve())or sha_file(target)!=digest:raise ValueError('FROZEN_SOURCE_CHANGED')
    return manifest


def verify_dataset(directory,expected):
    directory=Path(directory);raw=(directory/'dataset-manifest.json').read_bytes();manifest=json.loads(raw)
    if hashlib.sha256(raw).hexdigest()!=expected or json.loads((directory/'COMPLETE.json').read_text())['manifestSha256']!=expected:
        raise ValueError('NEW_DATASET_MANIFEST_CHANGED')
    base=Path(manifest['baseDataset']).resolve()
    if not base.is_relative_to(directory.resolve()):raise ValueError('EXACT_NEW_DATASET_BASE_REQUIRED')
    if sha_file(base/'dataset-manifest.json')!=manifest['baseManifestSha256']:raise ValueError('NEW_BASE_MANIFEST_CHANGED')
    metadata=json.loads((base/'dataset-manifest.json').read_text())
    if sha_file(base/'observations.jsonl.gz')!=metadata['observationsSha256']or sha_file(directory/'trade-flow.jsonl.gz')!=manifest['tradeRowsSha256']:
        raise ValueError('NEW_NORMALIZED_DATASET_BYTES_CHANGED')


def started_learning(run,config,source,dataset_sha):
    run=Path(run);progress=run/'progress.jsonl'
    if not progress.exists():return False
    accepted=False
    with progress.open('rb')as f:
        for raw in f:
            if not raw.endswith(b'\n'):break
            row=json.loads(raw)
            if row.get('event')in('train_chunk','parallel_train_chunk')and row.get('total_decisions',0)>0:
                accepted=accepted or any(r.get('accepted')is True for r in row['updates'])
    if not accepted:return False
    proof=json.loads((run/'experiment.json').read_text())
    if proof['config']!=config or proof['dataset_manifest_sha256']!=dataset_sha or proof['production_authority']is not False:
        raise ValueError('NEW_RUNNING_EXPERIMENT_IDENTITY_CHANGED')
    pins={k.replace('\\','/'):v for k,v in proof['source_sha256'].items()}
    manifest=verify_source(source)
    expected={k:v for k,v in manifest['files'].items()if len(Path(k).parts)==3 and Path(k).parts[1]in
              ('learning_actions','learning_realism','learning_v2','learning_flow')and not Path(k).name.startswith('test_')}
    if pins!=expected:raise ValueError('NEW_TRAINER_SOURCE_PINS_CHANGED')
    return True


def require_process_capacity(proof,config,config_path,source_manifest_sha):
    """A local-only green probe cannot admit a spawned-world configuration."""
    if config.get('process_source_worlds')is not True:return
    workers=proof.get('workerIdentities');count=config['training_worlds']
    if (proof.get('processSourceWorlds')is not True
            or proof.get('sourceWorkerCount')!=count
            or proof.get('sourceWorkersCPUOnly')is not True
            or proof.get('exactObservationActionsHiddenRewardsAndReceipts')is not True
            or proof.get('configurationSha256')!=sha_file(config_path)
            or proof.get('sourceManifestSha256')!=source_manifest_sha
            or proof.get('effectiveCudaMemoryFraction')!=config['cuda_memory_fraction']
            or type(workers)is not list or len(workers)!=count
            or any(type(w)is not dict or type(w.get('pid'))is not int or w['pid']<=0 or w.get('cudaInitialized')is not False for w in workers)
            or len({w['pid']for w in workers})!=count):
        raise ValueError('ACTUAL_PROCESS_BACKEND_CAPACITY_PROOF_REQUIRED')


def run(spec_path):
    spec_path=Path(spec_path);spec=json.loads(spec_path.read_text());job=spec_path.parent
    source=Path(spec['sourceRoot']);cancel=job/'cancel';handle=None
    def check_sources():
        if verify_source(source)['commit']!=spec['sourceCommit']or sha_file(source/'source-snapshot.json')!=spec['sourceManifestSha256']:
            raise ValueError('CHECKED_SOURCE_IDENTITY_CHANGED')
    def status(phase,**details):
        value={'phase':phase,'observedUtc':datetime.now(timezone.utc).isoformat(),
               'productionChanged':False,'independentTestUsed':False,**details}
        pending=job/'status.pending';pending.write_text(json.dumps(value,indent=2)+'\n');pending.replace(job/'status.json')
        print(json.dumps(value),flush=True)
    try:
        check_sources();verify_dataset(spec['template']['dataset'],spec['datasetManifestSha256'])
        if Path(spec['newRun']).parent.resolve()!=Path(spec['oldRun']).parent.resolve()or Path(spec['newRun']).exists():
            raise ValueError('FRESH_RUN_IN_SAME_ML_DIRECTORY_REQUIRED')
        handle=TrainingProcess(spec['oldPid'],spec['oldRun'],spec['oldConfig'],expected_creation=spec['oldCreation'])
        partial=spec.get('preservedPartialPopulation')
        if partial is not None:
            if sha_file(partial['path'])!=partial['sha256']:raise ValueError('PRESERVED_PARTIAL_PLAN_CHANGED')
            from .partial_population import verify
            status('verifying_preserved_unfinished_population',trainerPid=spec['oldPid'])
            plan=verify(json.loads(Path(partial['path']).read_text()),spec['oldRun'],
                members=spec['template']['members'],hidden_size=spec['template']['hidden_size'])
        else:
            status('waiting_for_completed_generation',trainerPid=spec['oldPid'],cycle=spec['cycle'])
            while True:
                if cancel.exists():status('cancelled_before_handoff');return
                if not handle.alive():raise ValueError('EXISTING_TRAINER_STOPPED_WITHOUT_VERIFIED_HANDOFF')
                if boundary(spec['oldRun'],spec['cycle'])is not None:break
                time.sleep(20)
            status('verifying_completed_population')
            plan=select_population(spec['oldRun'],cycle=spec['cycle'],members=spec['template']['members'],hidden_size=spec['template']['hidden_size'])
        if plan is None:raise ValueError('GENERATION_BOUNDARY_DISAPPEARED')
        frozen(job/'population-plan.json',plan);check_sources();verify_dataset(spec['template']['dataset'],spec['datasetManifestSha256'])
        if cancel.exists():status('cancelled_before_handoff');return
        # The retained handle targets the exact simulated trainer, even if a
        # numeric PID later becomes reusable. No account process is selected.
        handle.terminate_for_handoff()
        deadline=time.monotonic()+30
        while handle.alive()and time.monotonic()<deadline:time.sleep(.2)
        if handle.alive():raise ValueError('EXACT_OLD_TRAINER_STILL_LIVE')
        status('unfinished_population_weights_preserved_capacity_probe_pending'if partial is not None else
               'old_generation_preserved_capacity_probe_pending',parents=len(plan['parents']))
        chosen=None;checks=[]
        for steps in spec['capacityChunks']:
            if cancel.exists():status('cancelled_after_saved_generation');return
            config=next_configuration(spec['template'],plan,compute_steps=steps)
            config_path=job/('capacity-%d-config.json'%steps);frozen(config_path,config)
            output=job/('capacity-%d'%steps);log=job/('capacity-%d.log'%steps)
            module=('moloch.learning_actions.process_capacity'if config.get('process_source_worlds')is True
                    else'moloch.learning_actions.batch_benchmark')
            command=[spec['python'],'-m',module,'--config',str(config_path),
                '--output',str(output),'--worlds',str(config['training_worlds']),'--steps',str(steps),
                '--guard','tensor_versions_v1','--checkpoint-encoder','--start-offset-ms','1200000',
                '--memory-fraction',str(config['cuda_memory_fraction']),'--prime-books','8']
            status('probing_gpu_capacity',worlds=config['training_worlds'],computeSteps=steps)
            with log.open('x')as stream:
                result=subprocess.run(command,cwd=source,stdout=stream,stderr=subprocess.STDOUT,timeout=900,
                    creationflags=subprocess.CREATE_NO_WINDOW)
            if result.returncode:
                text=log.read_text(errors='replace')
                checks.append({'computeSteps':steps,'returncode':result.returncode,'outOfMemory':'out of memory'in text.lower()})
                if 'out of memory'not in text.lower():raise ValueError('CAPACITY_PROBE_FAILED_WITH_NON_MEMORY_ERROR')
                continue
            proof=json.loads((output/'RESULT.json').read_text())
            if proof['device']!='cuda:0'or not proof['identical_wallets_orders_fills_and_clocks']or not any(p['accepted']for p in proof['ppo']):
                raise ValueError('REAL_GPU_UPDATE_AND_EXACT_FINANCE_REQUIRED')
            if not proof['on_policy_guard_changed_after_optimizer_step']:raise ValueError('OPTIMIZER_GUARD_PROOF_REQUIRED')
            require_process_capacity(proof,config,config_path,spec['sourceManifestSha256'])
            chosen=config;checks.append({'computeSteps':steps,'accepted':True,'resultSha256':sha_file(output/'RESULT.json')});break
        if chosen is None:raise ValueError('NO_MEASURED_GPU_CAPACITY_PROFILE')
        if chosen.get('closed_episode_return_replay',False):
            # Chunk capacity alone does not exercise frozen collection,
            # sealing, source reconstruction or full-return optimization.
            path=job/'closed-capacity-config.json';frozen(path,chosen)
            output=job/'closed-capacity';log=job/'closed-capacity.log'
            status('probing_complete_episode_gpu_capacity',worlds=chosen['training_worlds'])
            with log.open('x')as stream:
                result=subprocess.run([spec['python'],'-m','moloch.learning_actions.closed_episode_capacity',
                    '--config',str(path),'--output',str(output)],cwd=source,stdout=stream,stderr=subprocess.STDOUT,
                    timeout=900,creationflags=subprocess.CREATE_NO_WINDOW)
            if result.returncode:raise ValueError('COMPLETE_EPISODE_GPU_CAPACITY_FAILED')
            proof=json.loads((output/'RESULT.json').read_text())
            if (proof['device']!='cuda:0'or not proof['fullReturnTargets']or not proof['optimizerChangedWeights']
                    or proof['acceptedUpdates']<=0 or proof['worlds']!=chosen['training_worlds']):
                raise ValueError('COMPLETE_EPISODE_GPU_CAPACITY_PROOF_REQUIRED')
            checks.append({'closedEpisodeCapacityAccepted':True,'resultSha256':sha_file(output/'RESULT.json')})
        frozen(job/'capacity-checks.json',checks)
        if cancel.exists():status('cancelled_after_capacity_probe');return
        check_sources();verify_dataset(spec['template']['dataset'],spec['datasetManifestSha256'])
        config_path=job/'next-config.json';frozen(config_path,chosen)
        log=job/'next-training.log';error=job/'next-training-error.log'
        with log.open('x')as out,error.open('x')as err:
            process=subprocess.Popen([spec['python'],'-u','-m','moloch.learning_actions.experiment',
                '--config',str(config_path),'--output',spec['newRun']],cwd=source,stdout=out,stderr=err,
                creationflags=subprocess.CREATE_NO_WINDOW)
        status('new_population_launched',launcherPid=process.pid,newRun=spec['newRun'],sourceCommit=spec['sourceCommit'])
        deadline=time.monotonic()+900
        while time.monotonic()<deadline:
            if process.poll()is not None:raise ValueError('NEW_POPULATION_EXITED_DURING_STARTUP')
            if started_learning(spec['newRun'],chosen,source,spec['datasetManifestSha256']):
                status('new_population_training_verified',launcherPid=process.pid,newRun=spec['newRun'],
                       sourceCommit=spec['sourceCommit'],computeSteps=chosen['compute_chunk_decisions']);return
            time.sleep(5)
        status('new_population_live_learning_unverified',launcherPid=process.pid,newRun=spec['newRun'])
    except BaseException as exc:
        status('handoff_failed',exception=type(exc).__name__,reason=str(exc));raise
    finally:
        if handle is not None:handle.close()


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--spec',type=Path,required=True)
    path=p.parse_args().spec;spec=json.loads(path.read_text())
    if not Path(__file__).resolve().is_relative_to(Path(spec['sourceRoot']).resolve()):
        raise ValueError('EXECUTE_HANDOFF_FROM_CHECKED_PRIVATE_SOURCE')
    run(path)
