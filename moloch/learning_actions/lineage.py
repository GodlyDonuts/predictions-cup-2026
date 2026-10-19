"""Audited development weight transfer across independently compiled datasets."""
import hashlib
import json
from pathlib import Path


def development_parent(bundle,checkpoint_path,*,new_dataset_sha256,allow_cross_dataset=False):
    same_dataset=bundle['dataset_manifest_sha256']==new_dataset_sha256
    if not same_dataset and allow_cross_dataset is not True:raise ValueError('DEVELOPMENT_PARENT_DATASET_CHANGED')
    path=Path(checkpoint_path).parent/'experiment.json';raw=path.read_bytes();source=json.loads(raw)
    source_sha=hashlib.sha256(raw).hexdigest()
    if source_sha!=bundle['split_specification_sha256']:raise ValueError('PARENT_EXPERIMENT_SOURCE_CHANGED')
    if source['dataset_manifest_sha256']!=bundle['dataset_manifest_sha256']:
        raise ValueError('PARENT_EXPERIMENT_DATASET_CHANGED')
    if source.get('full_history_experiment')is not False or source.get('independent_test_used')is not False or source.get('production_authority')is not False:
        raise ValueError('ONLY_AUDITED_DEVELOPMENT_PARENTS_CAN_TRANSFER')
    if source.get('config',{}).get('mode')!='inspected_development':raise ValueError('PARENT_DEVELOPMENT_MODE_REQUIRED')
    bounds={role:source['actual_'+role+'_bounds_ms']for role in ('train','evolution')}
    for value in bounds.values():
        if len(value)!=2 or any(type(v)is not int or v<0 for v in value)or value[0]>=value[1]:
            raise ValueError('AUDITED_PARENT_CALENDARS_REQUIRED')
    return {'classification':'development_weights_only_new_scores_required','source_experiment_sha256':source_sha,
            'source_dataset_sha256':bundle['dataset_manifest_sha256'],'new_dataset_sha256':new_dataset_sha256,
            'prior_calendar_bounds_ms':bounds,'same_dataset':same_dataset,
            'optimizer_moments_reset':True,'independent_test_qualification':False}
