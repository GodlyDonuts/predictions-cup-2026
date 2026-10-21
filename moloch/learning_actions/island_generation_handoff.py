"""Preserve each island's latest completed generation before a source upgrade.

This is a one-shot local research continuation, not an account worker. Original
runs and partial later-world checkpoints remain on disk; worlds are not resumed.
"""
import argparse
import copy
from datetime import datetime, timezone
import json
from pathlib import Path
import subprocess
import time

from .asynchronous_islands import configurations
from .continue_population import verify_source
from .population_handoff import select_population, sha_file
from .selection import frozen
from .training_process import TrainingProcess


def select_completed_generations(run, expected_configuration, expected_calibration):
    run=Path(run).resolve()
    global_path=run/'global-experiment.json'
    base=json.loads(global_path.read_text(encoding='utf-8'))
    if (sha_file(global_path)!=expected_configuration or base.get('mode')!='inspected_development'
            or base.get('test_finalist_once',False)):
        raise ValueError('EXACT_DEVELOPMENT_ISLAND_CONFIGURATION_REQUIRED')
    if sha_file(run/'shared-actor-calibration.json')!=expected_calibration:
        raise ValueError('EXACT_SHARED_ACTOR_CALIBRATION_REQUIRED')
    variants=configurations(base)
    membership=json.loads((run/'membership.json').read_text(encoding='utf-8'))
    ids=[c['asynchronous_global_member_ids']for c in variants]
    if membership.get('islands')!=ids or membership.get('globalPopulation')!=base['members']:
        raise ValueError('EXACT_GLOBAL_ISLAND_MEMBERSHIP_REQUIRED')
    directories=[run/f'island-{i:03d}'for i in range(len(variants))]
    cycles=[]
    for directory in directories:
        path=directory/'progress.jsonl'
        if not path.exists():return None
        completed=[]
        with path.open('rb')as stream:
            for raw in stream:
                if not raw.endswith(b'\n'):break
                row=json.loads(raw)
                if row.get('event')=='cycle_complete':completed.append(row['cycle'])
        if not completed:return None
        cycles.append(max(completed))
    plans=[];parents=[];dataset=None;source=None
    for island,(directory,config)in enumerate(zip(directories,variants)):
        original=json.loads((directory/'experiment.json').read_text(encoding='utf-8'))
        if original['config']!=config:
            raise ValueError('ISLAND_CONFIGURATION_CHANGED')
        if sha_file(directory/'compute-calibration.json')!=expected_calibration:
            raise ValueError('ISLAND_ACTOR_CALIBRATION_CHANGED')
        if dataset is None:dataset=original['dataset_manifest_sha256'];source=original['source_sha256']
        if dataset!=original['dataset_manifest_sha256']or source!=original['source_sha256']:
            raise ValueError('ISLAND_SOURCE_OR_DATASET_MISMATCH')
        plan=select_population(directory,cycle=cycles[island],members=config['members'],hidden_size=base['hidden_size'])
        if plan is None:raise ValueError('COMPLETED_ISLAND_BOUNDARY_DISAPPEARED')
        plans.append(plan)
        for parent in plan['parents']:
            parents.append({**parent,'local_member':parent['member'],'island':island,
                            'member':ids[island][parent['member']]})
    parents.sort(key=lambda p:p['member'])
    if [p['member']for p in parents]!=list(range(base['members'])):
        raise ValueError('EVERY_GLOBAL_PARENT_REQUIRED')
    return dict(classification='latest_completed_island_generations_for_source_upgrade',cycles=cycles,
        sourceGlobalConfigurationSha256=expected_configuration,
        sourceIslandExperimentsSha256=[p['sourceExperimentSha256']for p in plans],
        sourceDatasetSha256=dataset,calibrationSha256=expected_calibration,
        parents=parents,islandPlans=plans,completedGeneration=True,
        oldScoresReusedForNewSelection=False,oldFinancialWorldsResumed=False,productionAuthority=False)


def next_configuration(base,plan,*,target_query_profile=None):
    if base.get('mode')!='inspected_development'or not plan['completedGeneration']:
        raise ValueError('COMPLETED_DEVELOPMENT_ISLAND_GENERATION_REQUIRED')
    c=copy.deepcopy(base);parents=plan['parents']
    if [p['member']for p in parents]!=list(range(c['members'])):
        raise ValueError('EVERY_GLOBAL_PARENT_REQUIRED')
    c.pop('development_initial_checkpoint',None)
    for key in ('inherited_wait_logit_shifts','founder_family_logit_shifts','founder_price_logit_shifts',
                'transferred_source_experiment_sha256','transferred_partial_cycle',
                'transferred_partial_snapshot_updates','transferred_completed_cycle','corrective_transfer_reason'):
        c.pop(key,None)
    c.update(development_initial_checkpoints=[{k:p[k]for k in ('path','sha256','origin')}for p in parents],
        initial_learning_rates=[p['learning_rate']for p in parents],
        initial_entropy_coefficients=[p['entropy_coefficient']for p in parents],
        fresh_founders=0,first_cycle_member_order=list(range(c['members'])),
        transferred_source_island_experiments_sha256=plan['sourceIslandExperimentsSha256'],
        transferred_source_global_configuration_sha256=plan['sourceGlobalConfigurationSha256'],
        transferred_completed_island_cycles=plan['cycles'],test_finalist_once=False,
        training_chunk_transitions=64,evaluate_founders_before_training=True)
    if target_query_profile is not None:
        from .family_target_query import validate,LEGACY
        requested=validate(target_query_profile)
        if requested==LEGACY and c.get('target_query_profile',LEGACY)!=LEGACY:
            raise ValueError('CANNOT_DISCARD_LEARNED_FAMILY_TARGET_QUERY')
        c['target_query_profile']=requested
    from .training_chunks import validate
    validate(c)
    configurations(c)
    return c


def run(spec_path):
    spec_path=Path(spec_path).resolve();job=spec_path.parent
    spec=json.loads(spec_path.read_text(encoding='utf-8'));source=Path(spec['sourceRoot']).resolve()
    old=Path(spec['oldRun']).resolve();new=Path(spec['newRun']).resolve();handle=None
    def status(phase,**details):
        value=dict(phase=phase,observedUtc=datetime.now(timezone.utc).isoformat(),
                   productionChanged=False,independentTestUsed=False,**details)
        pending=job/'status.pending';pending.write_text(json.dumps(value,indent=2)+'\n',encoding='utf-8')
        pending.replace(job/'status.json');print(json.dumps(value),flush=True)
    def checked():
        if (verify_source(source)['commit']!=spec['sourceCommit']
                or sha_file(source/'source-snapshot.json')!=spec['sourceManifestSha256']):
            raise ValueError('CHECKED_CONTINUATION_SOURCE_CHANGED')
        if sha_file(spec['oldConfig'])!=spec['oldConfigSha256']:
            raise ValueError('ORIGINAL_TRAINER_CONFIG_CHANGED')
    try:
        if spec.get('targetQueryProfile') is not None:
            from .family_target_query import validate
            validate(spec['targetQueryProfile'])
        checked()
        if old.parent!=new.parent or new.exists():raise ValueError('NEW_PRIVATE_RESEARCH_RUN_REQUIRED')
        base=json.loads((old/'global-experiment.json').read_text(encoding='utf-8'))
        if base!=json.loads(Path(spec['oldConfig']).read_text(encoding='utf-8')):
            raise ValueError('RUNNING_GLOBAL_CONFIGURATION_CHANGED')
        if base.get('device')!='cuda':raise ValueError('EXACT_GPU_ISLAND_CONTINUATION_REQUIRED')
        handle=TrainingProcess(spec['oldPid'],old,spec['oldConfig'],expected_creation=spec['oldCreation'],
                               expected_module='moloch.learning_actions.asynchronous_islands')
        status('waiting_for_all_first_generation_boundaries',trainerPid=handle.pid)
        while True:
            if (job/'cancel').exists():status('cancelled_original_training_unchanged');return
            if not handle.alive():raise ValueError('ORIGINAL_TRAINER_EXITED_WITHOUT_HANDOFF')
            plan=select_completed_generations(old,spec['globalConfigurationSha256'],spec['calibrationSha256'])
            if plan is not None:break
            time.sleep(20)
        # All selected checkpoints verify before any process control. The new
        # source must evaluate them afresh rather than importing the old scores.
        frozen(job/'population-plan.json',plan)
        config=next_configuration(base,plan,target_query_profile=spec.get('targetQueryProfile'))
        config_path=job/'next-config.json';frozen(config_path,config)
        calibration=json.loads((old/'shared-actor-calibration.json').read_text(encoding='utf-8'))
        calibration_path=job/'actor-calibration.json';frozen(calibration_path,calibration)
        checked()
        if (job/'cancel').exists():status('cancelled_selected_parents_preserved');return
        handle.terminate_for_handoff();deadline=time.monotonic()+60
        while handle.alive()and time.monotonic()<deadline:time.sleep(.2)
        if handle.alive():raise ValueError('ORIGINAL_TRAINER_STILL_ALIVE')
        # Keep original files, including later partial recovery checkpoints. A
        # complete financial-world continuation is not claimed or fabricated.
        checkpoints=[]
        for path in old.rglob('*.pt'):
            checkpoints.append(dict(path=str(path),sha256=sha_file(path),bytes=path.stat().st_size))
        frozen(job/'preserved-original-checkpoints.json',dict(checkpoints=checkpoints,
            selectedIslandGenerations=plan['cycles'],unfinishedWorldsResumed=False,originalFilesDeleted=False))
        time.sleep(2)
        from .gpu_diagnostic_guard import require_idle_gpu
        require_idle_gpu('cuda',allow_verified_desktop=True)
        checked()
        command=[spec['python'],'-u','-m','moloch.learning_actions.asynchronous_islands',
                 '--config',str(config_path),'--config-sha',sha_file(config_path),'--output',str(new),
                 '--calibration',str(calibration_path),'--calibration-sha',sha_file(calibration_path),'--workers','2']
        with (job/'islands.log').open('x',encoding='utf-8')as log:
            child=subprocess.Popen(command,cwd=source,stdout=log,stderr=subprocess.STDOUT,
                                   creationflags=subprocess.CREATE_NO_WINDOW)
        frozen(job/'launch.json',dict(pid=child.pid,command=command,sourceCommit=spec['sourceCommit'],
            sourceManifestSha256=spec['sourceManifestSha256'],parentPlanSha256=sha_file(job/'population-plan.json'),
            originalCheckpointsDeleted=False,worldsResumed=False,productionChanged=False))
        status('new_islands_launched_founder_evaluation_pending',launcherPid=child.pid,
               sourceCommit=spec['sourceCommit'],newRun=str(new))
    except BaseException as error:
        status('handoff_failed_no_automatic_restart',error=type(error).__name__,reason=str(error));raise
    finally:
        if handle is not None:handle.close()


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--spec',type=Path,required=True)
    arguments=parser.parse_args();spec=json.loads(arguments.spec.read_text(encoding='utf-8'))
    if not Path(__file__).resolve().is_relative_to(Path(spec['sourceRoot']).resolve()):
        raise ValueError('RUN_HANDOFF_FROM_CHECKED_PRIVATE_SOURCE')
    run(arguments.spec)
