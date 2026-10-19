"""Read a completed development generation; never operate an account worker."""
import hashlib,json,math
from pathlib import Path
from .training import load_checkpoint


def sha_file(path):
    with Path(path).open('rb')as f:return hashlib.file_digest(f,'sha256').hexdigest()


def boundary(run,cycle=0):
    """A complete marker after all replacements, never just completed members."""
    run=Path(run);digest=hashlib.sha256();offset=0;current=None;replacements=[];retentions=[];seen=set();result_hashes={}
    with (run/'progress.jsonl').open('rb')as f:
        for raw in f:
            if not raw.endswith(b'\n'):break
            digest.update(raw);offset+=len(raw);row=json.loads(raw)
            if row['event']=='member_evaluated':
                current=row['cycle']
                if current==cycle:
                    if row['member']in seen:raise ValueError('DUPLICATE_COMPLETED_MEMBER_EVENT')
                    seen.add(row['member'])
                    value={k:row[k]for k in('member','cycle','checkpoint','training','selection')}
                    result_hashes[str(row['member'])]=hashlib.sha256(json.dumps(value,sort_keys=True,separators=(',',':'),allow_nan=False).encode()).hexdigest()
            elif row['event']=='population_replacement'and current==cycle:replacements.append(row)
            elif row['event']=='research_champion_retained'and row['cycle']==cycle:retentions.append(row)
            elif row['event']=='cycle_complete'and row['cycle']==cycle:
                return {'cycle':cycle,'members':sorted(seen),'replacements':replacements,'retentions':retentions,
                        'completedResultSemanticSha256':result_hashes,
                        'progressPrefixBytes':offset,'progressPrefixSha256':digest.hexdigest()}
    return None


def select_population(run,*,cycle=0,members=8,hidden_size=4096):
    run=Path(run).resolve();b=boundary(run,cycle)
    if b is None:return None
    source_raw=(run/'experiment.json').read_bytes();source=json.loads(source_raw);source_sha=hashlib.sha256(source_raw).hexdigest()
    if source.get('production_authority')is not False or source.get('full_history_experiment')is not False or source.get('independent_test_used')is not False:
        raise ValueError('ONLY_DEVELOPMENT_POPULATION_HANDOFF')
    config=source['config']
    if config.get('mode')!='inspected_development'or config['members']!=members or b['members']!=list(range(members)):
        raise ValueError('EVERY_MEMBER_MUST_COMPLETE_THE_SAME_GENERATION')
    archive=run/('research-archive-%03d.json'%cycle)
    if config.get('population_search')and not archive.is_file():raise ValueError('COMPLETED_RESEARCH_ARCHIVE_REQUIRED')
    slots=[];results=[]
    for member in range(members):
        p=run/('result-%03d-%03d.json'%(cycle,member));row=json.loads(p.read_text())
        semantic=hashlib.sha256(json.dumps(row,sort_keys=True,separators=(',',':'),allow_nan=False).encode()).hexdigest()
        if semantic!=b['completedResultSemanticSha256'].get(str(member)):raise ValueError('COMPLETED_RESULT_DOES_NOT_MATCH_PROGRESS')
        if row['member']!=member or row['cycle']!=cycle or row['selection']['checkpoint_sha256']!=row['checkpoint']['sha256']:
            raise ValueError('COMPLETED_MEMBER_IDENTITY_CHANGED')
        if not math.isfinite(row['selection']['reward']):raise ValueError('FINITE_COMPLETED_SELECTION_REQUIRED')
        slots.append(row['checkpoint']);results.append({'member':member,'resultSha256':sha_file(p),'selectionReward':row['selection']['reward']})
    if config.get('population_search')and len(b['replacements'])!=config['population_search']['islands']:
        raise ValueError('EVERY_ISLAND_REPLACEMENT_MUST_BE_COMPLETE')
    retained=set()
    champions=run/('island-champions-%03d.json'%cycle)
    if b['retentions']:
        proof=json.loads(champions.read_text())
        if proof['experiment_sha256']!=source_sha or proof['dataset_sha256']!=source['dataset_manifest_sha256']:
            raise ValueError('RETAINED_CHAMPION_EXPERIMENT_CHANGED')
        for row in b['retentions']:
            slot=row['slot'];cp=row['retained_checkpoint']
            if type(slot)is not int or not 0<=slot<members or slot in retained:
                raise ValueError('UNIQUE_RETAINED_CHAMPION_SLOT_REQUIRED')
            champion=proof['champions'][str(row['island'])]
            if champion['member']!=slot or champion['checkpoint']!=cp:
                raise ValueError('RETAINED_CHAMPION_CHECKPOINT_CHANGED')
            retained.add(slot);slots[slot]=cp
    losers=set()
    for row in b['replacements']:
        slot=row['loser']
        if type(slot)is not int or not 0<=slot<members or slot in losers:raise ValueError('UNIQUE_OFFSPRING_SLOT_REQUIRED')
        losers.add(slot);slots[slot]=row['checkpoint']
    parents=[]
    for member,cp in enumerate(slots):
        p=Path(cp['path']).resolve()
        if p.parent!=run or not p.is_file():raise ValueError('EXACT_OWN_RUN_CHECKPOINT_REQUIRED')
        model,bundle=load_checkpoint(p,cp['sha256'],'cpu')
        if bundle['split_specification_sha256']!=source_sha or bundle['dataset_manifest_sha256']!=source['dataset_manifest_sha256']:
            raise ValueError('CHECKPOINT_EXPERIMENT_OR_DATASET_CHANGED')
        if bundle['hidden_size']!=hidden_size:raise ValueError('EXPECTED_LEARNER_SIZE_REQUIRED')
        if bundle.get('global_state_encoding','raw')!=config.get('global_state_encoding','raw'):
            raise ValueError('PARENT_CHECKPOINT_FEATURE_ENCODING_CHANGED')
        group=bundle['optimizer']['param_groups'][0];rate=group['lr'];entropy=group['tartarus_entropy_coefficient']
        if not math.isfinite(rate)or rate<=0 or not math.isfinite(entropy)or entropy<0:raise ValueError('FINITE_PARENT_HYPERPARAMETERS')
        parents.append({'member':member,'path':str(p),'sha256':cp['sha256'],'learning_rate':rate,'entropy_coefficient':entropy,
                        'origin':'checked_evolved_child'if member in losers else
                        'checked_historical_island_champion'if member in retained else'checked_retained_generation_member'})
        del model,bundle
    # Check the exact observed prefix again after reading all checkpoint bytes.
    with (run/'progress.jsonl').open('rb')as f:raw=f.read(b['progressPrefixBytes'])
    if hashlib.sha256(raw).hexdigest()!=b['progressPrefixSha256']:raise ValueError('GENERATION_PREFIX_CHANGED_DURING_HANDOFF')
    return {**b,'sourceExperimentSha256':source_sha,'sourceDatasetSha256':source['dataset_manifest_sha256'],
            'parents':parents,'completedResults':results,'researchArchiveSha256':sha_file(archive)if archive.exists()else None,
            'islandChampionsSha256':sha_file(champions)if champions.exists()else None,
            'oldScoresReusedForNewSelection':False,
            'oldFinancialWorldsResumed':False,'productionAuthority':False}


def next_configuration(template,plan,*,compute_steps):
    config=dict(template)
    if config.get('mode')!='inspected_development':raise ValueError('DEVELOPMENT_HANDOFF_ONLY')
    parents=sorted(plan['parents'],key=lambda p:p['member'])
    if sorted(p['member']for p in parents)!=list(range(config['members'])):raise ValueError('COMPLETE_NEW_PARENT_SET_REQUIRED')
    if type(compute_steps)is not int or compute_steps<=0:raise ValueError('POSITIVE_MEMORY_CHUNK')
    config.pop('development_initial_checkpoint',None)
    config.update(development_initial_checkpoints=[{'path':p['path'],'sha256':p['sha256'],'source_member':p['member'],'origin':p['origin']}for p in parents],
        initial_learning_rates=[p['learning_rate']for p in parents],initial_entropy_coefficients=[p['entropy_coefficient']for p in parents],
        fresh_founders=0,first_cycle_member_order=list(range(config['members'])),compute_chunk_decisions=compute_steps,
        development_allow_cross_dataset_transfer=True,migrate_full_depth_encoder=True,
        transferred_source_experiment_sha256=plan['sourceExperimentSha256'])
    if plan.get('completedGeneration')is False:
        from .partial_population import CLASSIFICATION
        if plan.get('classification')!=CLASSIFICATION:raise ValueError('EXPLICIT_PARTIAL_TRANSFER_IDENTITY_REQUIRED')
        config.pop('transferred_completed_cycle',None)
        config.update(transferred_partial_cycle=plan['cycle'],
            transferred_partial_snapshot_updates=plan['activeSnapshotUpdates'],
            corrective_transfer_reason=plan['reason'])
    else:config['transferred_completed_cycle']=plan['cycle']
    return config
