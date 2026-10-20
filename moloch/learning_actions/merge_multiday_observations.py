"""Merge pinned original SIG observations and external REST references.

Original normalized rows, clocks and gaps remain intact. This produces an
explicitly partial research dataset, never source qualification or orders.
"""
import argparse,gzip,hashlib,heapq,json,shutil
from collections import Counter
from pathlib import Path
from .audit_breadth_inventory import write,clock
from .breadth_frame_index import sha
from moloch.learning_flow.episodes import FEATURE_SCHEMA
from moloch.learning_flow.features import FEATURE_NAMES


def merge(sig,sig_sha,external,external_sha,output,*,heldout_start,minimum_free_bytes=32*1024**3):
    sig=Path(sig);external=Path(external);output=Path(output).resolve()
    parents=[sig/'complete.json',external/'complete.json'];raws=[p.read_bytes()for p in parents]
    if [hashlib.sha256(r).hexdigest()for r in raws]!=[sig_sha,external_sha]:raise ValueError('PINNED_COMPLETED_SOURCES_REQUIRED')
    s,e=map(json.loads,raws)
    if(s['format']!='normalized_original_sig_frames_v1'or not s['originalFrameSemanticsValidated']
            or e['format']!='original_archived_REST_references_v1'
            or any(m['trainingAdmitted']is not False or m['independentTestUsed']is not False or m['productionChanged']is not False for m in(s,e))):
        raise ValueError('UNQUALIFIED_ORIGINAL_RESEARCH_SOURCES_REQUIRED')
    cut=s['fitEndExclusiveMs'];roles=e['calendarRolesMs']
    if not cut==roles['fit'][1]==roles['search'][0]or not s['lastReceivedMs']<s['cutoffExclusiveMs']<=heldout_start or roles['search'][1]>heldout_start:
        raise ValueError('MATCHING_NON_HELDOUT_CHRONOLOGICAL_CUT_REQUIRED')
    inputs=[(sig/'observations.jsonl.gz',s['observationsSha256']),
            (external/'references.jsonl.gz',e['normalizedRowsSha256']),
            (sig/'trade-flow.jsonl.gz',s['tradeRowsSha256'])]
    for p,h in inputs:
        if p.is_symlink()or not p.is_file()or sha(p)!=h:raise ValueError('ORIGINAL_NORMALIZED_BYTES_CHANGED')
    if type(minimum_free_bytes)is not int or minimum_free_bytes<1:raise ValueError('POSITIVE_MERGE_DISK_RESERVE')
    if shutil.disk_usage(output.parent).free<minimum_free_bytes+sum(p.stat().st_size for p,_ in inputs):raise ValueError('MERGE_DISK_HEADROOM')
    output.mkdir(exist_ok=False);base=output/'base';base.mkdir();counts=Counter();coverage={};total=0;first=None;last=None
    def rows(path,is_sig):
        previous=-1
        with gzip.open(path,'rb')as f:
            for raw in f:
                if not raw.endswith(b'\n')or len(raw)>16*1024*1024:raise ValueError('COMPLETE_BOUNDED_NORMALIZED_ROW_REQUIRED')
                r=json.loads(raw);at=r['available_at_ms'];venue=r['venue'];kind=r['kind']
                if type(at)is not int or not 0<=at<heldout_start or at<previous:raise ValueError('ORIGINAL_NON_HELDOUT_ROW_CLOCK_REQUIRED')
                if(is_sig and(venue!='SIG'or kind not in('book','gap'))or not is_sig and(venue not in('Kalshi','Polymarket')or kind not in('reference','reference_gap'))):
                    raise ValueError('ORIGINAL_TYPED_SOURCE_SCOPE_REQUIRED')
                role='fit'if at<cut else'search'
                if r['calendar_role']!=role:raise ValueError('ORIGINAL_ROLE_MUST_MATCH_FROZEN_CUT')
                previous=at;yield at,raw,venue,kind,role
    try:
        with gzip.GzipFile(filename=str(base/'observations.jsonl.gz'),mode='wb',mtime=0,compresslevel=1)as out:
            for at,raw,venue,kind,role in heapq.merge(rows(inputs[0][0],True),rows(inputs[1][0],False),key=lambda r:r[0]):
                if total%100000==0:
                    if shutil.disk_usage(output).free<minimum_free_bytes:raise ValueError('MERGE_DISK_HEADROOM')
                    write(output/'status.json',{'phase':'merging_original_observations','rows':total,'counts':dict(counts),'accountRequests':0})
                out.write(raw);total+=1;first=at if first is None else first;last=at;counts[role+'/'+venue+'/'+kind]+=1
                if kind in('book','reference'):
                    key=role+'/'+venue;c=coverage.setdefault(key,{'rows':0,'firstAvailableMs':at,'lastAvailableMs':at,'completeSourceCoverage':False})
                    c['rows']+=1;c['lastAvailableMs']=at
        if first is None or not first<cut<=last<heldout_start:raise ValueError('BOTH_CHRONOLOGICAL_ROLES_REQUIRED')
        for key,n in s['counts'].items():
            if '/'in key:
                role,kind=key.split('/')
                if role in('fit','search')and kind in('book','gap')and counts[role+'/SIG/'+kind]!=n:
                    raise ValueError('ORIGINAL_SIG_ROW_COUNTS_CHANGED')
        for c in e['coverage']:
            if sum(counts[role+'/'+c['venue']+'/'+c['kind']]for role in('fit','search'))!=c['rows']:
                raise ValueError('ORIGINAL_EXTERNAL_ROW_COUNTS_CHANGED')
        for role in('fit','search'):
            for venue in('SIG','Kalshi','Polymarket'):
                coverage.setdefault(role+'/'+venue,{'rows':0,'firstAvailableMs':None,'lastAvailableMs':None,'completeSourceCoverage':False})
        shutil.copyfile(inputs[2][0],output/'trade-flow.jsonl.gz')
        common={'classification':'partial_original_three_venue_research_not_independent_qualification',
            'sourceSigNormalizationSha256':sig_sha,'sourceExternalNormalizationSha256':external_sha,
            'calendarRolesMs':{'fit':[first,cut],'search':[cut,last+1]},'observationCounts':dict(counts),'sourceCoverage':coverage,
            'requiredWorldBookLifetimeMs':60000,'sourceCoveragePolicy':'conditional_observed_snapshot_world_with_original_gaps',
            'externalObservationsIncluded':True,'fullHistoryReady':False,'completeObservedSourceCoverage':False,
            'historicalCatalogAvailabilityVerified':False,'referenceContractEquivalenceVerified':False,'executionCalibrated':False,
            'trainingAdmitted':False,'independentTestEligible':False,'heldoutFeaturesIncluded':False,'accountRequests':0,'productionAuthority':False}
        write(base/'dataset-manifest.json',{**common,'observationsSha256':sha(base/'observations.jsonl.gz'),'totalRows':total})
        bsha=sha(base/'dataset-manifest.json');write(base/'COMPLETE.json',{'manifestSha256':bsha})
        result={**common,'featureSchema':FEATURE_SCHEMA,'additionalFeatures':FEATURE_NAMES,'baseDataset':str(base),
            'baseManifestSha256':bsha,'tradeRowsSha256':s['tradeRowsSha256'],'tradeRows':sum(v for k,v in s['counts'].items()if k.endswith('/trade')),
            'originalSIGExchanges':len(s['exchanges']),'allHistoricalOwnTradesExcluded':False}
        for p,h in inputs:
            if sha(p)!=h:raise ValueError('SOURCE_CHANGED_DURING_MERGE')
        if sha(output/'trade-flow.jsonl.gz')!=s['tradeRowsSha256']or any(p.read_bytes()!=r for p,r in zip(parents,raws)):
            raise ValueError('SOURCE_OR_TRADE_IDENTITY_CHANGED')
        write(output/'dataset-manifest.json',result);write(output/'COMPLETE.json',{'manifestSha256':sha(output/'dataset-manifest.json')})
        write(output/'status.json',{'phase':'complete_partial_three_venue_research','rows':total,'trainingAdmitted':False,'accountRequests':0});return result
    except BaseException as error:
        write(output/'FAILED.json',{'errorType':type(error).__name__,'reason':str(error),'trainingAdmitted':False});raise


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    for k in('sig','sig-sha256','external','external-sha256','output','heldout-start'):p.add_argument('--'+k,required=True)
    a=p.parse_args();r=merge(a.sig,a.sig_sha256,a.external,a.external_sha256,a.output,heldout_start=clock(a.heldout_start))
    print(json.dumps({'counts':r['observationCounts'],'sourceCoverage':r['sourceCoverage'],'trainingAdmitted':False}))
