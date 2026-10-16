"""Frozen prospective crossing-return analysis admitted by original input replay.

No data acquisition or financial/model authority. Fixed checked repository
modules only; manifest bytes are data, never a code path or a tuning option.
"""
import argparse
import hashlib
import json
from pathlib import Path
import types

HERE=Path(__file__).resolve().parent
READER_LF_SHA='6fb96218a54e1d6aa2b46d524cda1db1afab35f5f06c5bdaf8c16b324ad484e2'
REPLAY_LF_SHA='a51f89856d4848d4bc576d8aa15e0e51db942513db21fd8dac8a09d53702d57b'


def sha(raw):return hashlib.sha256(raw).hexdigest()

def need(value,code):
    if not value:raise ValueError('PROSPECTIVE_ANALYSIS_'+code)


def checked_module(path,expected,name):
    raw=path.read_bytes()
    need(len(raw)<=1024**2 and sha(raw.replace(b'\r\n',b'\n'))==expected,'FIXED_SOURCE_CHANGED')
    # Never import a manifest path or trust unrelated cached .pyc bytes.
    module=types.ModuleType(name);module.__file__=str(path)
    exec(compile(raw,str(path),'exec'),module.__dict__)
    return module


READER=checked_module(HERE.parent/'analyze.py',READER_LF_SHA,'_frozen_trade_math')
REPLAY=checked_module(HERE.parent/'witness/trace_replay.py',REPLAY_LF_SHA,'_frozen_input_replay')


def pin(base,spec,maximum):
    need(isinstance(spec,dict) and set(spec)=={'path','sha256','bytes'},'PIN_FIELDS')
    need(isinstance(spec['path'],str),'PIN_PATH')
    path=Path(spec['path']);path=path if path.is_absolute() else base/path
    need(type(spec['bytes']) is int and 0<=spec['bytes']<=maximum,'PIN_BOUND')
    return REPLAY.Pinned({**spec,'path':str(path.absolute())})


def admitted_inputs(path,expected_sha):
    path=Path(path).absolute()
    manifest=REPLAY.Pinned({'path':str(path),'sha256':expected_sha,'bytes':path.stat().st_size})
    value=READER.decode(manifest.small(1024**2))
    need(type(value.get('schemaVersion')) is int and value['schemaVersion']==1 and value.get('sealed') is True and value.get('analysisProtocolSha256')==READER.PROTOCOL_SHA,'FROZEN_MANIFEST')
    cutoff=READER.clock(value['asOfUtc'])
    need(isinstance(value.get('cohorts'),list) and 0<len(value['cohorts'])<=128,'COHORT_BOUND')
    witnesses=[];pins=[manifest];identities=set()
    for cohort in value['cohorts']:
        cid=cohort.get('cohortId');need(isinstance(cid,str) and cid and cid not in identities,'COHORT_ID');identities.add(cid)
        need(cohort.get('samplingPolicySha256')==READER.POLICY_SHA,'SAMPLING_POLICY')
        proof=cohort.get('inputReplay')
        need(isinstance(proof,dict) and set(proof)=={'manifest','result'},'ORIGINAL_INPUT_REPLAY_REQUIRED')
        source=pin(path.parent,proof['manifest'],1024**2);report=pin(path.parent,proof['result'],4*1024**2)
        original_manifest=READER.decode(source.small(1024**2))
        actual=REPLAY.verify(source.path,source.sha)
        retained=READER.decode(report.small(4*1024**2))
        need(READER.compact(actual)==READER.compact(retained),'REPLAY_RESULT_CHANGED')
        need(actual.get('samplerInputReplayVerified') is True and actual.get('samplingMode')=='trade','WRONG_SAMPLER')
        # A positive stored flag alone cannot mint admission: it must equal the
        # independently rerun fixed verifier bound to these original bytes.
        producer=pin(path.parent,cohort['producer'],1024**2)
        need(actual['producerSha256']==producer.sha and producer.sha==original_manifest['producer']['sha256'],'PRODUCER_BINDING')
        need(isinstance(cohort.get('files'),list) and len(cohort['files'])==1,'ONE_ORIGINAL_TRACE_SAMPLE_FILE')
        samples=pin(path.parent,cohort['files'][0],REPLAY.MAX_COMPRESSED)
        need((samples.sha,samples.bytes)==(original_manifest['samples']['sha256'],original_manifest['samples']['bytes']),'SAMPLE_MEMBER_BINDING')
        trace=REPLAY.Pinned(original_manifest['trace']);seal=REPLAY.Pinned(original_manifest['seal'])
        start=next(REPLAY.lines(trace))['event'];end=READER.decode(seal.small(65536))
        need(start['effectiveSamplerConfig']['samplingMode']=='trade' and start['samplingPolicySha256']==READER.POLICY_SHA,'RUN_POLICY_BINDING')
        need(cohort.get('configSha256')==start['effectiveSamplerConfigSha256'] and cohort.get('mappingSha256')==start['mappingFileReference']['sha256'],'CONFIG_OR_MAPPING_BINDING')
        need(end['sealedAtMs']<=cutoff,'SEAL_AFTER_FROZEN_CUTOFF')
        status=cohort.get('status');skip={'available':False,'reason':'ORIGINAL_STOPPED_STATUS_NOT_PINNED'}
        if status is not None:
            status_pin=pin(path.parent,status,1024**2);pins.append(status_pin)
            body=READER.decode(status_pin.small(1024**2))
            need(body.get('state')=='stopped' and body.get('samplingPolicySha256')==READER.POLICY_SHA and body.get('triggerDisabled') is False and isinstance(body.get('counters'),dict),'STOPPED_STATUS')
            need(all(type(n) is int and n>=0 for n in body['counters'].values()),'STATUS_COUNTERS')
            skip={'available':True,'counters':body['counters'],'sourceSha256':status_pin.sha,'externallyPinnedNotSealAttested':True}
        witnesses.append({'cohortId':cid,'inputReplayManifestSha256':source.sha,'retainedResultSha256':report.sha,
            'producerSha256':producer.sha,'samplesSha256':samples.sha,'configSha256':cohort['configSha256'],
            'mappingSha256':cohort['mappingSha256'],'traceSealedAtMs':end['sealedAtMs'],
            'verification':actual,'producerSkipCounters':skip})
        pins.extend([source,report,producer,samples,trace,seal])
    for original in pins:original.recheck()
    return witnesses,pins


def analyze(manifest_path,manifest_sha256):
    witnesses,pins=admitted_inputs(manifest_path,manifest_sha256)
    # The original597 reader performs global economic dedup BEFORE features or
    # labels, fixed calendar boundaries BEFORE completeness and common cohorts.
    # Ridge1, ten columns, costs, purge, tie/selection/coverage rules are unchanged.
    result=READER.analyze(manifest_path,manifest_sha256)
    result['reusedFrozenMathSourceSha256']=result['sourceCodeSha256']
    result['sourceCodeSha256']=sha(Path(__file__).read_bytes())
    result['fixedMathLfSha256']=READER_LF_SHA;result['fixedReplayLfSha256']=REPLAY_LF_SHA
    result['originalInputReplay']=witnesses
    result['samplerInputReplayVerified']=True
    result['wholeWitnessVerified']=False
    result['rawSourceWitnessesIndependentlyVerified']=False
    result['sourceCaveat']='Exact known-sampler original input membership/callback consumption is verified. Whole provider tape, collector processing/epoch, tournament topics and contract equivalence remain unverified. Unread source observations and independent segment boundaries are reported, never silently repaired or relabeled.'
    result['innovationRuntimeModelExported']=False
    for horizon in result['horizons'].values():
        for metrics in horizon['models'].values():
            # Decimal division may repeat. Preserve exact sufficient statistics
            # instead of falsely calling a rounded decimal expansion exact.
            mean=metrics.pop('selectedNetCrossingMeanExact')
            metrics['selectedNetCrossingMeanDecimal']=mean
            metrics['selectedNetCrossingMeanRational']=({'numeratorExact':metrics['selectedNetCrossingSumExact'],
                'denominator':metrics['selectedClusters']} if metrics['selectedClusters'] else None)
            metrics['meanDecimalPrecisionDigits']=28
            metrics['selectedEconomicsAvailability']=('unavailable_no_oos' if not metrics['outOfSampleClusters'] else
                'no_selected_events' if not metrics['selectedClusters'] else 'observed_counterfactual')
            metrics['modelApproval']=False
    for original in pins:original.recheck()
    return result


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--manifest',required=True,type=Path);parser.add_argument('--manifest-sha256',required=True)
    parser.add_argument('--output',required=True,type=Path);args=parser.parse_args()
    result=analyze(args.manifest,args.manifest_sha256)
    with args.output.open('x',encoding='utf-8',newline='\n') as target:
        target.write(READER.compact(result)+'\n')

if __name__=='__main__':main()
