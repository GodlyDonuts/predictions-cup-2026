"""One completed-generation handoff into cooperative research islands.

Retains the exact old trainer handle and every selected checkpoint. This does
not contact an exchange, alter production, or consume an independent test.
"""
import argparse
import copy
from datetime import datetime, timezone
import json
from pathlib import Path
import subprocess
import time

from .continue_population import verify_source
from .population_handoff import boundary, select_population, next_configuration, sha_file
from .selection import frozen
from .training_process import TrainingProcess


def continuation_configuration(template, plan, *, managed_profile=None):
    config = next_configuration(copy.deepcopy(template), plan,
                                compute_steps=template['compute_chunk_decisions'])
    # These are one-time initialization mutations. Applying them again to an
    # already evolved checkpoint changes the selected parent on every handoff.
    removed = {}
    for key in ['inherited_wait_logit_shifts', 'founder_family_logit_shifts',
                'founder_price_logit_shifts']:
        if key in config: removed[key] = config.pop(key)
    config['test_finalist_once'] = False
    if config['mode'] != 'inspected_development' or config.get('device') != 'cuda':
        raise ValueError('EXPLICIT_CUDA_DEVELOPMENT_CONTINUATION_REQUIRED')
    if managed_profile is not None:
        from .managed_data_service import INTENT_PROFILE,HISTORY_PROFILE
        service=config.get('managed_data_service')
        if (managed_profile!=HISTORY_PROFILE or not isinstance(service,dict)
                or service.get('profile')!=INTENT_PROFILE or not config.get('historical_information')):
            raise ValueError('EXPLICIT_V2_TO_V3_BOUND_HISTORY_MIGRATION_REQUIRED')
        config['managed_data_service']={**service,'profile':HISTORY_PROFILE}
        config['migrate_managed_data_profile']=True
    return config, removed


def run(spec_path):
    spec_path = Path(spec_path).resolve(); spec = json.loads(spec_path.read_text())
    job = spec_path.parent; source = Path(spec['sourceRoot']).resolve()
    old = Path(spec['oldRun']).resolve(); new = Path(spec['newRun']).resolve()
    handle = None
    def status(phase, **details):
        value = dict(phase=phase, observedUtc=datetime.now(timezone.utc).isoformat(),
                     productionChanged=False, independentTestUsed=False, **details)
        pending = job/'status.pending'; pending.write_text(json.dumps(value, indent=2)+'\n')
        pending.replace(job/'status.json'); print(json.dumps(value), flush=True)
    def checked_source():
        if (verify_source(source)['commit'] != spec['sourceCommit'] or
                sha_file(source/'source-snapshot.json') != spec['sourceManifestSha256']):
            raise ValueError('EXACT_CHECKED_SOURCE_CHANGED')
    try:
        checked_source()
        if new.parent != old.parent or new.exists():
            raise ValueError('FRESH_RUN_IN_EXISTING_RESEARCH_DIRECTORY_REQUIRED')
        original = json.loads((old/'experiment.json').read_text())
        if original['dataset_manifest_sha256'] != spec['datasetManifestSha256']:
            raise ValueError('EXACT_EXISTING_DATASET_REQUIRED')
        if original['config'] != spec['template']:
            raise ValueError('CONTINUATION_TEMPLATE_MUST_BE_EXACT_RUNNING_CONFIGURATION')
        calibration = old/'compute-calibration.json'
        if sha_file(calibration) != spec['calibrationSha256']:
            raise ValueError('EXACT_EXISTING_ACTOR_CALIBRATION_REQUIRED')
        handle = TrainingProcess(spec['oldPid'], old, spec['oldConfig'],
                                 expected_creation=spec['oldCreation'])
        partial=spec.get('preservedPartialPopulation')
        if partial is not None:
            if sha_file(partial['path'])!=partial['sha256']:
                raise ValueError('PRESERVED_PARTIAL_PLAN_CHANGED')
            from .partial_population import verify
            status('verifying_preserved_unfinished_population',trainerPid=handle.pid)
            plan=verify(json.loads(Path(partial['path']).read_text()),old,
                members=spec['template']['members'],hidden_size=spec['template']['hidden_size'])
        else:
            status('waiting_for_completed_generation', trainerPid=handle.pid, cycle=spec['cycle'])
            while boundary(old, spec['cycle']) is None:
                if (job/'cancel').exists(): status('cancelled_before_handoff'); return
                if not handle.alive(): raise ValueError('EXISTING_TRAINER_EXITED_WITHOUT_HANDOFF')
                time.sleep(20)
            plan = select_population(old, cycle=spec['cycle'], members=spec['template']['members'],
                                     hidden_size=spec['template']['hidden_size'])
            if plan is None: raise ValueError('COMPLETED_GENERATION_DISAPPEARED')
        frozen(job/'population-plan.json', plan)
        config, removed = continuation_configuration(spec['template'], plan,
            managed_profile=spec.get('managedDataProfile'))
        frozen(job/'removed-one-time-mutations.json', removed)
        next_path = job/'next-config.json'; frozen(next_path, config)
        next_calibration = job/'inherited-actor-calibration.json'
        actor_calibration=json.loads(calibration.read_text())
        replacement=spec.get('replacementCalibration')
        if spec.get('managedDataProfile')is not None and replacement is None:
            raise ValueError('HISTORY_MIGRATION_REQUIRES_FRESH_ACTOR_CALIBRATION')
        if replacement is not None:
            if (not isinstance(replacement,dict)or set(replacement)!={'path','sha256'}
                    or sha_file(replacement['path'])!=replacement['sha256']):
                raise ValueError('PINNED_REPLACEMENT_ACTOR_CALIBRATION_REQUIRED')
            candidate=json.loads(Path(replacement['path']).read_text())
            if (candidate.get('managed_data_profile')!=spec.get('managedDataProfile')
                    or candidate.get('device')!='cpu' or candidate.get('policy_rng_preserved')is not True
                    or type(candidate.get('decision_latency_ms'))is not int
                    or candidate['decision_latency_ms']<actor_calibration['decision_latency_ms']):
                raise ValueError('CAUSAL_HISTORY_ACTOR_CALIBRATION_REQUIRED')
            actor_calibration=candidate
        frozen(next_calibration, actor_calibration)
        checked_source()
        if (job/'cancel').exists(): status('cancelled_before_handoff'); return
        # The immutable selected population is already saved. This retained OS
        # handle identifies only the authorized simulated training process.
        handle.terminate_for_handoff()
        deadline = time.monotonic()+60
        while handle.alive() and time.monotonic()<deadline: time.sleep(.2)
        if handle.alive(): raise ValueError('EXACT_OLD_TRAINER_STILL_RUNNING')
        # Let the existing venv redirector observe its child's exit; it is
        # independently included in the process inventory below.
        time.sleep(2)
        from .gpu_diagnostic_guard import require_idle_gpu
        require_idle_gpu('cuda', allow_verified_desktop=True)
        status('unfinished_population_weights_preserved_launching_islands'if partial is not None else
               'selected_generation_preserved_launching_islands',parents=len(plan['parents']))
        command = [spec['python'], '-u', '-m', 'moloch.learning_actions.asynchronous_islands',
                   '--config', str(next_path), '--config-sha', sha_file(next_path),
                   '--output', str(new), '--calibration', str(next_calibration),
                   '--calibration-sha', sha_file(next_calibration), '--workers', '2']
        with (job/'islands.log').open('x') as log:
            process = subprocess.Popen(command, cwd=source, stdout=log, stderr=subprocess.STDOUT,
                                       creationflags=subprocess.CREATE_NO_WINDOW)
        frozen(job/'launch.json', dict(pid=process.pid, command=command,
               sourceCommit=spec['sourceCommit'], sourceManifestSha256=spec['sourceManifestSha256'],
               parentPlanSha256=sha_file(job/'population-plan.json'), productionChanged=False,
               calibration=('fresh pinned CPU history-context actor cost, not remote hardware or shared-island measurement'
                   if replacement is not None else 'inherited measured single-policy actor cost, not a new shared-load measurement')))
        status('island_process_launched_learning_not_yet_verified', launcherPid=process.pid)
    except BaseException as error:
        status('handoff_failed_no_automatic_restart', error=type(error).__name__, reason=str(error))
        raise
    finally:
        if handle is not None: handle.close()


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--spec', type=Path, required=True)
    run(parser.parse_args().spec)
