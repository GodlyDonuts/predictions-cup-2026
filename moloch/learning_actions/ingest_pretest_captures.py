"""Recover completed pre-test captures through the original producer/semantic checks.

No network, account client, financial writer or independent-test admission.
Every price-bearing read follows the shared calendar exposure registration.
"""
import argparse,hashlib,json,shutil,subprocess,tarfile
from collections import Counter
from pathlib import Path
from .compile_prospective_source import compile_capture,sha,pinned_json,lines
from .selection import DataUse
from moloch.learning_v2.reference_adapters import millis

FORMAT='completed_pretest_SIG_segments_private_export_v1'
FILES=('result.json','capture/complete.json','capture/stream/manifest.json',
       'capture/stream/stats.json','capture/stream/events.jsonl','capture/receipts.jsonl')


def ingest(package,manifest_pin,output,*,bounds,heldout_start,node='node',data_use=None):
    package=Path(package).resolve();output=Path(output).resolve()
    if (len(bounds)!=2 or any(type(v)is not int or v<0 for v in bounds)
            or type(heldout_start)is not int or not bounds[0]<bounds[1]<=heldout_start):
        raise ValueError('EXPLICIT_PRE_TEST_DEVELOPMENT_BOUNDS_REQUIRED')
    m=pinned_json(package/'manifest.json',manifest_pin)
    if (m.get('format')!=FORMAT or m.get('accountRequests')!=0 or m.get('productionChanged')is not False
            or m.get('independentTestUsed')is not False or not m.get('segments')):
        raise ValueError('EXPLICIT_COMPLETED_PRE_TEST_EXPORT_REQUIRED')
    if json.loads((package/'COMPLETE.json').read_bytes())!={'manifestSha256':manifest_pin}:
        raise ValueError('COMPLETED_EXPORT_MANIFEST_CHANGED')
    expected={};segments=set()
    for row in m['segments']:
        i=row['segment']
        if type(i)is not int or i<0 or i in segments:raise ValueError('UNIQUE_CAPTURE_SEGMENTS_REQUIRED')
        segments.add(i)
        if not bounds[0]<=millis(row['until'])<=millis(row['finishedUtc'])<bounds[1]:
            raise ValueError('WHOLE_CAPTURE_MUST_PRECEDE_TEST')
        if set(row['files'])!=set(FILES):raise ValueError('EXACT_PUBLIC_CAPTURE_FILES_REQUIRED')
        for rel,record in row['files'].items():expected[f'segment-{i:05d}/{rel}']=record
    registry=DataUse()if data_use is None else data_use
    registry.record('inspected_development',*bounds,manifest_pin)
    archive=package/'segments.tar.gz'
    if archive.is_symlink()or archive.stat().st_size!=m['archiveBytes']or sha(archive)!=m['archiveSha256']:
        raise ValueError('ORIGINAL_EXPORT_ARCHIVE_CHANGED')
    output.mkdir(parents=True,exist_ok=False);extracted=output/'extracted';seen=set()
    with tarfile.open(archive,'r:gz')as stream:
        for member in stream:
            if member.name not in expected or member.name in seen or not member.isfile():
                raise ValueError('EXACT_REGULAR_CAPTURE_MEMBER_REQUIRED')
            record=expected[member.name];p=(extracted/member.name).resolve()
            if not p.is_relative_to(extracted.resolve())or member.size!=record['bytes']:
                raise ValueError('CAPTURE_MEMBER_PATH_OR_SIZE_CHANGED')
            p.parent.mkdir(parents=True,exist_ok=True)
            with stream.extractfile(member)as source,p.open('xb')as target:shutil.copyfileobj(source,target)
            if sha(p)!=record['sha256']:raise ValueError('CAPTURE_MEMBER_HASH_CHANGED')
            seen.add(member.name)
    if seen!=set(expected):raise ValueError('MISSING_CAPTURE_EXPORT_MEMBER')
    script=Path(__file__).parents[1]/'research/audit_completed_capture.mjs'
    ledger=Path(__file__).parents[2]/'execution/ledger.mjs'
    code_pins={str(p):sha(p)for p in(script,ledger)};results=[]
    for row in sorted(m['segments'],key=lambda row:row['segment']):
        i=row['segment'];source=extracted/f'segment-{i:05d}'
        result=json.loads((source/'result.json').read_bytes());complete=json.loads((source/'capture/complete.json').read_bytes())
        session=source/'capture/stream/manifest.json';session_value=json.loads(session.read_bytes())
        if (result['exitCode']!=0 or result['financialExecution']is not False
                or result['finishedUtc']!=row['finishedUtc']or complete['until']!=row['until']
                or complete['financialExecution']is not False or complete['trainingAdmitted']is not False
                or session_value['tokenPersistence']is not False or session_value['contestOrders']!=0
                or session_value['paperOrders']!=0):raise ValueError('ORIGINAL_PUBLIC_SESSION_REQUIRED')
        root=output/'wrapped'/f'segment-{i:05d}';originals=root/'originals';originals.mkdir(parents=True)
        paths={'events.jsonl':source/'capture/stream/events.jsonl','receipts.jsonl':source/'capture/receipts.jsonl'}
        counts={};files={}
        for name,p in paths.items():
            shutil.copyfile(p,originals/name)
            counts[name]=dict(Counter((json.loads(raw)['event']if name=='events.jsonl'else json.loads(raw))['kind']for raw in lines(p)))
            files[name]={'bytes':p.stat().st_size,'sha256':sha(p)}
        shutil.copyfile(session,originals/'session-manifest.json')
        # This is a derived byte-preserving wrapper archive, not a newly observed
        # market capture. Its source package/segment bindings remain explicit.
        with tarfile.open(root/'originals.tar.gz','w:gz',compresslevel=1)as f:
            for name in paths:f.add(originals/name,arcname=name,recursive=False)
        manifest={'format':'original_completed_sig_forward_fixture_v1','trainingAdmitted':False,
                  'independentTestUsed':False,'productionChanged':False,'files':files,'counts':counts,
                  'sourceStreamManifestSha256':sha(session),'archiveSha256':sha(root/'originals.tar.gz'),
                  'sourcePackageManifestSha256':manifest_pin,'sourcePackageArchiveSha256':m['archiveSha256'],
                  'sourceSegment':i,'derivedWrapperNotNewObservation':True}
        (root/'complete.json').write_text(json.dumps(manifest,indent=2)+'\n')
        audit=output/'audits'/f'segment-{i:05d}';audit.mkdir(parents=True)
        canonical=audit/'canonical-events.jsonl'
        subprocess.run([node,str(script),str(originals/'events.jsonl'),str(root/'complete.json'),str(canonical),
                        str(bounds[0]),str(bounds[1])],check=True,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        decoded=compile_capture(root,sha(root/'complete.json'),canonical,audit/'RESULT.json',sha(audit/'RESULT.json'),
                                originals/'session-manifest.json',output/'normalized'/f'segment-{i:05d}',
                                bounds=tuple(bounds),role='search',heldout_start=heldout_start,data_use=registry)
        results.append({'segment':i,'normalizedCompleteSha256':sha(output/'normalized'/f'segment-{i:05d}'/'complete.json'),**decoded})
        (output/'progress.json').write_text(json.dumps({'completedSegments':len(results),'totalSegments':len(segments),'segment':i})+'\n')
    if sha(archive)!=m['archiveSha256']or any(sha(Path(p))!=pin for p,pin in code_pins.items()):
        raise ValueError('INGEST_SOURCE_OR_CODE_CHANGED')
    result={'classification':'original_pretest_capture_normalization_not_model_performance_or_admission',
            'sourcePackageManifestSha256':manifest_pin,'archiveSha256':m['archiveSha256'],'receiptBoundsMs':list(bounds),
            'heldoutStartMs':heldout_start,'codeSha256':code_pins,'segments':results,
            'trainingAdmitted':False,'independentTestUsed':False,'productionChanged':False,'accountRequests':0}
    (output/'COMPLETE.json').write_text(json.dumps(result,indent=2)+'\n');return result


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    for name in('package','output'):p.add_argument('--'+name,type=Path,required=True)
    p.add_argument('--manifest-pin',required=True);p.add_argument('--bounds',type=int,nargs=2,required=True)
    p.add_argument('--heldout-start',type=int,required=True);p.add_argument('--node',default='node');a=p.parse_args()
    result=ingest(a.package,a.manifest_pin,a.output,bounds=a.bounds,heldout_start=a.heldout_start,node=a.node)
    print(json.dumps({'completedSegments':len(result['segments']),'trainingAdmitted':False,'independentTestUsed':False}))
