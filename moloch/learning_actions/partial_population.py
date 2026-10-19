"""Explicit corrective transfer of an unfinished development generation.

Preserve every member, not a new ranking. Never label a recovery snapshot an
evaluated candidate, completed episode, financial-world resume or offspring.
"""
import hashlib,json,math,shutil
from pathlib import Path
from .population_handoff import sha_file
from .training import load_checkpoint

CLASSIFICATION='preserved_unfinished_development_generation_weights_only'


def semantic(row):
    return hashlib.sha256(json.dumps(row,sort_keys=True,separators=(',',':'),allow_nan=False).encode()).hexdigest()


def verify(plan,run,*,members,hidden_size):
    run=Path(run).resolve();source=(run/'experiment.json').read_bytes();experiment=json.loads(source)
    if any(experiment.get(k)is not False for k in('full_history_experiment','independent_test_used','production_authority')):
        raise ValueError('ONLY_PARTIAL_DEVELOPMENT_GENERATIONS_MAY_TRANSFER')
    if plan.get('classification')!=CLASSIFICATION or plan.get('completedGeneration')is not False:
        raise ValueError('EXPLICIT_UNFINISHED_GENERATION_REQUIRED')
    if plan.get('sourceRun')!=str(run)or hashlib.sha256(source).hexdigest()!=plan['sourceExperimentSha256']:
        raise ValueError('PARTIAL_SOURCE_EXPERIMENT_CHANGED')
    if plan['sourceDatasetSha256']!=experiment['dataset_manifest_sha256']:
        raise ValueError('PARTIAL_SOURCE_DATASET_CHANGED')
    if any(plan.get(k)is not False for k in('oldScoresReusedForNewSelection','oldFinancialWorldsResumed','productionAuthority')):
        raise ValueError('PARTIAL_TRANSFER_CANNOT_REUSE_SCORES_OR_AUTHORITY')
    with(run/'progress.jsonl').open('rb')as f:prefix=f.read(plan['progressPrefixBytes'])
    if hashlib.sha256(prefix).hexdigest()!=plan['progressPrefixSha256']:raise ValueError('PARTIAL_PROGRESS_PREFIX_CHANGED')
    if sorted(p['member']for p in plan['parents'])!=list(range(members)):raise ValueError('EVERY_PARTIAL_MEMBER_REQUIRED')
    for parent in plan['parents']:
        if sha_file(parent['path'])!=parent['sha256']:raise ValueError('PRESERVED_PARTIAL_CHECKPOINT_CHANGED')
        if parent['hiddenSize']!=hidden_size:raise ValueError('PARTIAL_MODEL_SIZE_CHANGED')
    return plan


def preserve(run,destination,*,members=8,hidden_size=4096,reason):
    run=Path(run).resolve();destination=Path(destination).resolve()
    if type(members)is not int or members<2:raise ValueError('AT_LEAST_TWO_PARTIAL_MEMBERS_REQUIRED')
    if not isinstance(reason,str)or not reason.strip():raise ValueError('EXPLICIT_CORRECTIVE_REASON_REQUIRED')
    source=(run/'experiment.json').read_bytes();experiment=json.loads(source);config=experiment['config']
    if any(experiment.get(k)is not False for k in('full_history_experiment','independent_test_used','production_authority')):
        raise ValueError('ONLY_PARTIAL_DEVELOPMENT_GENERATIONS_MAY_TRANSFER')
    if config.get('mode')!='inspected_development'or config['members']!=members:
        raise ValueError('EXACT_DEVELOPMENT_POPULATION_REQUIRED')
    if config.get('first_cycle_member_order',list(range(members)))!=list(range(members)):
        raise ValueError('PARTIAL_TRANSFER_REQUIRES_EXACT_SEQUENTIAL_FIRST_CYCLE')
    # Source file writes are append-only. Ignore only an incomplete final line.
    with(run/'progress.jsonl').open('rb')as f:raw=f.read()
    prefix=raw[:raw.rfind(b'\n')+1];events=[json.loads(line)for line in prefix.splitlines()]
    if any(e['event']in('cycle_complete','population_replacement')for e in events):
        raise ValueError('USE_COMPLETED_GENERATION_OR_LATER_CYCLE_HANDOFF')
    completed=[(i,e)for i,e in enumerate(events)if e['event']=='member_evaluated']
    if [e['member']for _,e in completed]!=list(range(members-1))or any(e['cycle']!=0 for _,e in completed):
        raise ValueError('ALL_BUT_LAST_FIRST_CYCLE_MEMBERS_MUST_BE_EVALUATED')
    candidates=[]
    for path in(run/'training-recovery').glob('slot-*.json'):
        metadata=json.loads(path.read_text())
        if metadata.get('financial_world_resume_supported')is not False or metadata.get('selection_or_profitability_result')is not False or metadata.get('rotating_recovery_copy')is not True:
            raise ValueError('RECOVERY_IS_NOT_AN_EVALUATION')
        cp=metadata['checkpoint'];p=Path(cp['path']).resolve()
        if p.parent!=(run/'training-recovery')or p.stem!=path.stem:raise ValueError('EXACT_RECOVERY_SLOT_REQUIRED')
        matches=[i for i,e in enumerate(events)if e['event']=='training_weight_snapshot'
            and e['checkpoint']==cp and e['total_updates']==metadata['total_updates']]
        if matches and matches[-1]>completed[-1][0]:candidates.append((metadata['total_updates'],metadata))
    if not candidates:raise ValueError('LAST_ACTIVE_MEMBER_ACCEPTED_SNAPSHOT_REQUIRED')
    _,recovery=max(candidates,key=lambda item:item[0]);destination.mkdir(parents=True,exist_ok=False)
    active=destination/'active-parent';active.mkdir();(active/'experiment.json').write_bytes(source)
    selected=active/('member-%03d.pt'%(members-1));shutil.copyfile(recovery['checkpoint']['path'],selected)
    if sha_file(selected)!=recovery['checkpoint']['sha256']:raise ValueError('RECOVERY_ROTATED_DURING_COPY')
    checkpoints=[]
    for _,event in completed:
        member=event['member'];row=json.loads((run/('result-000-%03d.json'%member)).read_text())
        if semantic(row)!=semantic({k:event[k]for k in('member','cycle','checkpoint','training','selection')}):
            raise ValueError('COMPLETED_PARTIAL_MEMBER_RESULT_CHANGED')
        expected=run/('cycle-000-member-%03d.pt'%member)
        if Path(row['checkpoint']['path']).resolve()!=expected:raise ValueError('EXACT_COMPLETED_MEMBER_CHECKPOINT_REQUIRED')
        checkpoints.append(row['checkpoint'])
    checkpoints.append({'path':str(selected),'sha256':recovery['checkpoint']['sha256']})
    parents=[];source_sha=hashlib.sha256(source).hexdigest()
    for member,cp in enumerate(checkpoints):
        model,bundle=load_checkpoint(cp['path'],cp['sha256'],'cpu')
        if bundle['split_specification_sha256']!=source_sha or bundle['dataset_manifest_sha256']!=experiment['dataset_manifest_sha256']:
            raise ValueError('PARTIAL_CHECKPOINT_SOURCE_CHANGED')
        if bundle['hidden_size']!=hidden_size:raise ValueError('PARTIAL_MODEL_SIZE_CHANGED')
        if bundle.get('global_state_encoding','raw')!=config.get('global_state_encoding','raw'):
            raise ValueError('PARTIAL_PARENT_ENCODING_CHANGED')
        group=bundle['optimizer']['param_groups'][0];rate=group['lr'];entropy=group['tartarus_entropy_coefficient']
        if not math.isfinite(rate)or rate<=0 or not math.isfinite(entropy)or entropy<0:raise ValueError('FINITE_PARTIAL_HYPERPARAMETERS')
        parents.append({'member':member,**cp,'hiddenSize':hidden_size,'learning_rate':rate,'entropy_coefficient':entropy,
            'origin':'checked_unfinished_member_accepted_weights'if member==members-1 else'checked_evaluated_partial_generation_member'})
        del model,bundle
    plan={'classification':CLASSIFICATION,'completedGeneration':False,'cycle':0,'sourceRun':str(run),
        'reason':reason,'sourceExperimentSha256':source_sha,'sourceDatasetSha256':experiment['dataset_manifest_sha256'],
        'completedMembers':list(range(members-1)),'unfinishedMember':members-1,'activeSnapshotUpdates':recovery['total_updates'],
        'activeSnapshotDecisions':recovery['total_decisions'],'parents':parents,'progressPrefixBytes':len(prefix),
        'progressPrefixSha256':hashlib.sha256(prefix).hexdigest(),'oldScoresReusedForNewSelection':False,
        'oldFinancialWorldsResumed':False,'productionAuthority':False}
    return verify(plan,run,members=members,hidden_size=hidden_size)
