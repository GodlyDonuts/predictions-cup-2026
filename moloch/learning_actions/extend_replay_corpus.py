"""Extend a pinned three-venue research corpus with decoded pre-test SIG captures.

Original observation bytes/order/clocks survive; repeated trade reports never
create additional liquidity. This builds a new corpus, not a live-run mutation.
"""
import argparse,gzip,hashlib,heapq,json,shutil
from collections import Counter
from pathlib import Path
from .compile_prospective_source import pinned_json,sha,FORMAT as CAPTURE_FORMAT
from .selection import DataUse
from .audit_breadth_inventory import write
from moloch.learning_flow.episodes import FEATURE_SCHEMA
from moloch.learning_flow.features import FEATURE_NAMES,Flow


def extend(parent,parent_pin,captures,capture_pin,output,*,heldout_start,
           minimum_free_bytes=32*1024**3,data_use=None):
    parent=Path(parent).resolve();captures=Path(captures).resolve();output=Path(output).resolve()
    if type(heldout_start)is not int or heldout_start<=0:raise ValueError('EXPLICIT_HELDOUT_BOUNDARY_REQUIRED')
    p=pinned_json(parent/'dataset-manifest.json',parent_pin)
    c=pinned_json(captures/'COMPLETE.json',capture_pin)
    if (p.get('featureSchema')!=FEATURE_SCHEMA or p.get('externalObservationsIncluded')is not True
            or p.get('productionAuthority')is not False or p.get('trainingAdmitted')is not False
            or p.get('heldoutFeaturesIncluded')is not False or c.get('accountRequests')!=0
            or c.get('productionChanged')is not False or c.get('independentTestUsed')is not False
            or c.get('trainingAdmitted')is not False or not c.get('segments')):
        raise ValueError('PINNED_PARTIAL_ORIGINAL_RESEARCH_CORPORA_REQUIRED')
    if json.loads((parent/'COMPLETE.json').read_bytes())!={'manifestSha256':parent_pin}:
        raise ValueError('PARENT_COMPLETE_MANIFEST_CHANGED')
    roles=p['calendarRolesMs'];lo,cut=roles['fit'];search_lo,parent_end=roles['search']
    start,end=c['receiptBoundsMs']
    if (any(type(v)is not int for v in(lo,cut,search_lo,parent_end,start,end))
            or not 0<=lo<cut==search_lo<parent_end<=heldout_start
            or not cut<=start<end<=heldout_start or c['heldoutStartMs']!=heldout_start):
        raise ValueError('CHRONOLOGICAL_NON_HELDOUT_SOURCE_BOUNDS_REQUIRED')
    if (output==parent or output.is_relative_to(parent)or output==captures or output.is_relative_to(captures)):
        raise ValueError('EXTENSION_MUST_NOT_REPLACE_OR_MODIFY_SOURCE_CORPUS')
    registry=DataUse()if data_use is None else data_use
    registry.record('inspected_development',lo,max(parent_end,end),capture_pin)
    base=Path(p['baseDataset']).resolve()
    b=pinned_json(base/'dataset-manifest.json',p['baseManifestSha256'])
    if json.loads((base/'COMPLETE.json').read_bytes())!={'manifestSha256':p['baseManifestSha256']}:
        raise ValueError('PARENT_BASE_COMPLETE_CHANGED')
    inputs=[(base/'observations.jsonl.gz',b['observationsSha256'],False),
            (parent/'trade-flow.jsonl.gz',p['tradeRowsSha256'],True)]
    metadata=[(parent/'dataset-manifest.json',parent_pin),(parent/'COMPLETE.json',sha(parent/'COMPLETE.json')),
              (base/'dataset-manifest.json',p['baseManifestSha256']),(base/'COMPLETE.json',sha(base/'COMPLETE.json')),
              (captures/'COMPLETE.json',capture_pin)]
    seen=set();tournaments=set()
    for record in c['segments']:
        i=record['segment']
        if type(i)is not int or i<0 or i in seen:raise ValueError('UNIQUE_CAPTURE_SEGMENTS_REQUIRED')
        seen.add(i);root=captures/'normalized'/f'segment-{i:05d}'
        m=pinned_json(root/'complete.json',record['normalizedCompleteSha256'])
        declared={k:v for k,v in record.items()if k not in('segment','normalizedCompleteSha256')}
        if (m!=declared or m.get('format')!=CAPTURE_FORMAT or m.get('calendarRole')!='search'
                or m.get('receiptBoundsMs')!=[start,end]or m.get('heldoutStartMs')!=heldout_start
                or m.get('allInputsRehashedAfterRead')is not True or m.get('trainingAdmitted')is not False
                or m.get('independentTestUsed')is not False or m.get('productionChanged')is not False):
            raise ValueError('EXACT_COMPLETED_PRE_TEST_SEMANTIC_CAPTURE_REQUIRED')
        session_path=captures/'wrapped'/f'segment-{i:05d}'/'originals/session-manifest.json'
        session=pinned_json(session_path,m['sessionManifestSha256'])
        if session.get('tokenPersistence')is not False or session.get('contestOrders')!=0 or session.get('paperOrders')!=0:
            raise ValueError('ORIGINAL_PUBLIC_CAPTURE_SCOPE_REQUIRED')
        tournaments.add(session['tournamentId'])
        metadata.extend([(root/'complete.json',record['normalizedCompleteSha256']),
                         (session_path,m['sessionManifestSha256'])])
        inputs.extend([(root/'observations.jsonl.gz',m['observationsSha256'],False),
                       (root/'trades.jsonl.gz',m['tradeRowsSha256'],True)])
    if len(tournaments)!=1:raise ValueError('ONE_SIG_TOURNAMENT_REQUIRED')
    def recheck():
        for path,pin,*_ in [*inputs,*metadata]:
            if path.is_symlink()or not path.is_file()or sha(path)!=pin:raise ValueError('ORIGINAL_EXTENSION_SOURCE_CHANGED')
    recheck()
    if type(minimum_free_bytes)is not int or minimum_free_bytes<1:raise ValueError('POSITIVE_EXTENSION_DISK_RESERVE')
    if shutil.disk_usage(output.parent).free<minimum_free_bytes+sum(path.stat().st_size for path,_,_ in inputs):
        raise ValueError('EXTENSION_DISK_HEADROOM')
    output.mkdir(exist_ok=False);outbase=output/'base';outbase.mkdir();counts=Counter();coverage={}
    source_counts=Counter();first=None;last=None;total=0;duplicates=0;flow=Flow();exchanges=set()
    def rows(path,*,trade=False,extension=False):
        previous=-1
        with gzip.open(path,'rb')as f:
            for raw in f:
                if not raw.endswith(b'\n')or len(raw)>16*1024*1024:raise ValueError('COMPLETE_BOUNDED_NORMALIZED_ROW_REQUIRED')
                row=json.loads(raw);at=row['available_at_ms']
                lower,upper=(start,end)if extension else(lo,parent_end)
                if type(at)is not int or not lower<=at<upper or at<previous:raise ValueError('ORIGINAL_EXTENSION_ROW_CLOCK')
                previous=at
                if trade:
                    if row['venue']!='SIG'or row['tournament_id']not in tournaments:raise ValueError('ORIGINAL_SIG_TRADE_SCOPE')
                else:
                    venue,kind=row['venue'],row['kind']
                    if (venue=='SIG'and kind not in('book','gap')or venue in('Kalshi','Polymarket')and kind not in('reference','reference_gap')
                            or venue not in('SIG','Kalshi','Polymarket')or extension and venue!='SIG'):
                        raise ValueError('ORIGINAL_EXTENSION_TYPED_SCOPE')
                    if row['calendar_role']!=('fit'if at<cut else'search'):raise ValueError('ORIGINAL_CALENDAR_ROLE_CHANGED')
                source_counts[('extension'if extension else'parent')+('/trade'if trade else'/observation')]+=1
                yield at,raw,row
    try:
        observations=[rows(path,extension=n>1)for n,(path,_,trade)in enumerate(inputs)if not trade]
        with gzip.GzipFile(filename=str(outbase/'observations.jsonl.gz'),mode='wb',mtime=0,compresslevel=1)as f:
            for at,raw,row in heapq.merge(*observations,key=lambda v:v[0]):
                f.write(raw);total+=1;first=at if first is None else first;last=at
                key=row['calendar_role']+'/'+row['venue']+'/'+row['kind'];counts[key]+=1
                if row['kind']in('book','reference'):
                    cover=coverage.setdefault(row['calendar_role']+'/'+row['venue'],{'rows':0,'firstAvailableMs':at,'lastAvailableMs':at,'completeSourceCoverage':False})
                    cover['rows']+=1;cover['lastAvailableMs']=at
                if row['venue']=='SIG'and row['kind']=='book':exchanges.add(row['exchange_id'])
                if total%100000==0:
                    if shutil.disk_usage(output).free<minimum_free_bytes:raise ValueError('EXTENSION_DISK_HEADROOM')
                    write(output/'status.json',{'phase':'merging_original_observation_bytes','rows':total,'counts':dict(counts),'accountRequests':0})
        trades=[rows(path,trade=True,extension=n>1)for n,(path,_,trade)in enumerate(inputs)if trade]
        with gzip.GzipFile(filename=str(output/'trade-flow.jsonl.gz'),mode='wb',mtime=0,compresslevel=1)as f:
            for at,raw,row in heapq.merge(*trades,key=lambda v:v[0]):
                if flow.accept(at,row):f.write(raw)
                else:duplicates+=1
        if first is None or not first<cut<=last<heldout_start:raise ValueError('WHOLE_CHRONOLOGICAL_CORPUS_REQUIRED')
        if source_counts['parent/observation']!=sum(p['observationCounts'].values()):raise ValueError('PARENT_OBSERVATION_COUNT_CHANGED')
        if source_counts['parent/trade']!=p['tradeRows']:raise ValueError('PARENT_TRADE_COUNT_CHANGED')
        if source_counts['extension/observation']!=sum(sum(s['normalizedCounts'].values())for s in c['segments']):
            raise ValueError('CAPTURE_OBSERVATION_COUNT_CHANGED')
        for role in('fit','search'):
            for venue in('SIG','Kalshi','Polymarket'):
                coverage.setdefault(role+'/'+venue,{'rows':0,'firstAvailableMs':None,'lastAvailableMs':None,'completeSourceCoverage':False})
        common={k:p[k]for k in('requiredWorldBookLifetimeMs','sourceCoveragePolicy')}
        common.update(classification='extended_partial_original_three_venue_research_not_independent_qualification',
            sourceParentManifestSha256=parent_pin,sourceCaptureNormalizationSha256=capture_pin,
            calendarRolesMs={'fit':[first,cut],'search':[cut,last+1]},observationCounts=dict(counts),sourceCoverage=coverage,
            sourceRowsConsumed=dict(source_counts),repeatedTradeReportsSuppressed=duplicates,
            rawObservationBytesPreserved=True,observationTieOrder='parent_first_then_declared_capture_order',
            externalObservationsIncluded=True,fullHistoryReady=False,completeObservedSourceCoverage=False,
            historicalCatalogAvailabilityVerified=False,referenceContractEquivalenceVerified=False,executionCalibrated=False,
            trainingAdmitted=False,independentTestEligible=False,heldoutFeaturesIncluded=False,accountRequests=0,productionAuthority=False)
        write(outbase/'dataset-manifest.json',{**common,'observationsSha256':sha(outbase/'observations.jsonl.gz'),'totalRows':total})
        bpin=sha(outbase/'dataset-manifest.json');write(outbase/'COMPLETE.json',{'manifestSha256':bpin})
        result={**common,'featureSchema':FEATURE_SCHEMA,'additionalFeatures':FEATURE_NAMES,'baseDataset':str(outbase),
                'baseManifestSha256':bpin,'tradeRowsSha256':sha(output/'trade-flow.jsonl.gz'),'tradeRows':len(flow.seen),
                'originalSIGExchanges':len(exchanges),'allHistoricalOwnTradesExcluded':False}
        recheck();write(output/'dataset-manifest.json',result);write(output/'COMPLETE.json',{'manifestSha256':sha(output/'dataset-manifest.json')})
        write(output/'status.json',{'phase':'complete_partial_extension_not_training_admission','rows':total,'trades':len(flow.seen),'accountRequests':0})
        return result
    except BaseException as error:
        write(output/'FAILED.json',{'errorType':type(error).__name__,'reason':str(error),'trainingAdmitted':False});raise


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    for name in('parent','captures','output'):p.add_argument('--'+name,type=Path,required=True)
    p.add_argument('--parent-pin',required=True);p.add_argument('--capture-pin',required=True);p.add_argument('--heldout-start',type=int,required=True)
    a=p.parse_args();v=extend(a.parent,a.parent_pin,a.captures,a.capture_pin,a.output,heldout_start=a.heldout_start)
    print(json.dumps({'tradeRows':v['tradeRows'],'sourceRows':v['sourceRowsConsumed'],'trainingAdmitted':False}))
