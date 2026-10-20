"""Package completed original SIG normalization for bounded research replay.

This changes storage metadata only. It supplies neither missing external
observations nor execution calibration, full history or training authority.
"""
import argparse,hashlib,json,shutil
from pathlib import Path
from .breadth_frame_index import sha
from .audit_breadth_inventory import write,clock
from moloch.learning_flow.episodes import FEATURE_SCHEMA
from moloch.learning_flow.features import FEATURE_NAMES


def package(source,manifest_sha,output,*,heldout_start,minimum_free_bytes=8*1024**3):
    source=Path(source).resolve();output=Path(output).resolve()
    raw=(source/'complete.json').read_bytes()
    if hashlib.sha256(raw).hexdigest()!=manifest_sha:raise ValueError('EXACT_COMPLETED_NORMALIZATION_REQUIRED')
    m=json.loads(raw)
    if(m['format']!='normalized_original_sig_frames_v1'or m['originalFrameSemanticsValidated']is not True
            or m['trainingAdmitted']is not False or m['independentTestUsed']is not False
            or m['productionChanged']is not False):raise ValueError('UNADMITTED_RESEARCH_NORMALIZATION_REQUIRED')
    if not m['firstReceivedMs']<m['fitEndExclusiveMs']<m['lastReceivedMs']<m['cutoffExclusiveMs']<=heldout_start:
        raise ValueError('CHRONOLOGICAL_NON_HELDOUT_SOURCE_REQUIRED')
    inputs=[(source/'observations.jsonl.gz',m['observationsSha256']),
            (source/'trade-flow.jsonl.gz',m['tradeRowsSha256'])]
    for p,digest in inputs:
        if p.is_symlink()or not p.is_file()or sha(p)!=digest:raise ValueError('COMPLETED_NORMALIZATION_BYTES_CHANGED')
    if type(minimum_free_bytes)is not int or minimum_free_bytes<1:raise ValueError('POSITIVE_PACKAGE_DISK_RESERVE')
    if shutil.disk_usage(output.parent).free<minimum_free_bytes+sum(p.stat().st_size for p,_ in inputs):
        raise ValueError('PACKAGE_DISK_HEADROOM')
    output.mkdir(exist_ok=False);base=output/'base';base.mkdir()
    common={'classification':'partial_original_multiday_SIG_research_requires_external_binding_and_qualification',
        'sourceNormalizationSha256':manifest_sha,'calendarRolesMs':{
            'fit':[m['firstReceivedMs'],m['fitEndExclusiveMs']],
            'search':[m['fitEndExclusiveMs'],m['lastReceivedMs']+1]},
        'observationCounts':m['counts'],'requiredWorldBookLifetimeMs':60000,
        'sourceCoveragePolicy':'conditional_observed_snapshot_world_with_original_gaps',
        'fullHistoryReady':False,'completeObservedSourceCoverage':False,
        'referenceContractEquivalenceVerified':False,'externalObservationsIncluded':False,
        'historicalCatalogAvailabilityVerified':False,'executionCalibrated':False,
        'trainingAdmitted':False,'independentTestEligible':False,'heldoutFeaturesIncluded':False,
        'accountRequests':0,'productionAuthority':False}
    try:
        for (p,digest),target in zip(inputs,(base/'observations.jsonl.gz',output/'trade-flow.jsonl.gz')):
            with p.open('rb')as src,target.open('xb')as dst:shutil.copyfileobj(src,dst,1024*1024)
            if sha(target)!=digest or sha(p)!=digest:raise ValueError('SOURCE_CHANGED_DURING_PACKAGE')
        bm={**common,'observationsSha256':m['observationsSha256'],
            'totalRows':sum(v for k,v in m['counts'].items()if k.endswith('/book')or k.endswith('/gap'))}
        write(base/'dataset-manifest.json',bm);bsha=sha(base/'dataset-manifest.json')
        write(base/'COMPLETE.json',{'manifestSha256':bsha})
        fm={**common,'featureSchema':FEATURE_SCHEMA,'additionalFeatures':FEATURE_NAMES,
            'baseDataset':str(base),'baseManifestSha256':bsha,'tradeRowsSha256':m['tradeRowsSha256'],
            'tradeRows':sum(v for k,v in m['counts'].items()if k.endswith('/trade')),
            'originalSIGExchanges':len(m['exchanges']),'allHistoricalOwnTradesExcluded':False}
        write(output/'dataset-manifest.json',fm)
        if(source/'complete.json').read_bytes()!=raw:raise ValueError('NORMALIZATION_MANIFEST_CHANGED')
        write(output/'COMPLETE.json',{'manifestSha256':sha(output/'dataset-manifest.json')})
        return fm
    except BaseException as error:
        write(output/'FAILED.json',{'errorType':type(error).__name__,'reason':str(error),'trainingAdmitted':False})
        raise


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    for k in('source','manifest-sha256','output','heldout-start'):p.add_argument('--'+k,required=True)
    a=p.parse_args();r=package(a.source,a.manifest_sha256,a.output,heldout_start=clock(a.heldout_start))
    print(json.dumps({'tradeRows':r['tradeRows'],'calendarRolesMs':r['calendarRolesMs'],'trainingAdmitted':False}))
