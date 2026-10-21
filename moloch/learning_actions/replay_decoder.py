"""Optional faster replay parsing after exhaustive, exact cache-specific parity.

An audit is an engineering equivalence check, never source/calendar admission.
Original SQLite bodies, integer economics and the default decoder are unchanged.
"""
import argparse,hashlib,json,platform,sqlite3,struct,time
from pathlib import Path

FORMAT='exact_full_replay_decoder_parity_v1'


def sha(path):
    with Path(path).open('rb')as f:return hashlib.file_digest(f,'sha256').hexdigest()


def implementation():
    import orjson
    folder=Path(orjson.__file__).parent
    files={p.name:sha(p)for p in sorted(folder.iterdir())if p.suffix in('.pyd','.so','.py')}
    return orjson.loads,{'version':orjson.__version__,'filesSha256':files,
                         'pythonVersion':platform.python_version()}


def typed_equal(a,b):
    if type(a)is not type(b):return False
    if isinstance(a,dict):return a.keys()==b.keys()and all(typed_equal(a[k],b[k])for k in a)
    if isinstance(a,list):return len(a)==len(b)and all(typed_equal(x,y)for x,y in zip(a,b))
    if isinstance(a,float):return struct.pack('>d',a)==struct.pack('>d',b)
    return a==b


def audited_decoder(manifest,manifest_sha,audit_path=None,audit_sha=None):
    if audit_path is None and audit_sha is None:return json.loads
    if audit_path is None or audit_sha is None:raise ValueError('EXPLICIT_DECODER_AUDIT_PIN_REQUIRED')
    p=Path(audit_path)
    if sha(p)!=audit_sha:raise ValueError('DECODER_AUDIT_CHANGED')
    proof=json.loads(p.read_bytes());decode,actual=implementation()
    if (proof.get('format')!=FORMAT or proof.get('complete')is not True
        or proof.get('cacheManifestSha256')!=manifest_sha
        or proof.get('databaseSha256')!=manifest['databaseSha256']
        or proof.get('implementation')!=actual
        or proof.get('allTypedDecodedRowsEqual')is not True
        or proof.get('rows')!={t:manifest['counts'][t]for t in('market','flow')}):
        raise ValueError('EXACT_CACHE_AND_DECODER_PARITY_REQUIRED')
    return decode


def audit(cache,manifest_sha,output,*,bounds,heldout_start,data_use=None):
    from .selection import DataUse
    from .disk_tape import FORMAT as CACHE_FORMAT
    if (len(bounds)!=2 or any(type(v)is not int for v in bounds)
            or type(heldout_start)is not int or not 0<=bounds[0]<bounds[1]<=heldout_start):
        raise ValueError('EXPLICIT_PRE_TEST_DECODER_AUDIT_BOUNDS_REQUIRED')
    cache=Path(cache).resolve();output=Path(output).resolve();raw=(cache/'manifest.json').read_bytes()
    if hashlib.sha256(raw).hexdigest()!=manifest_sha:raise ValueError('REPLAY_MANIFEST_CHANGED')
    m=json.loads(raw)
    if (m['format']!=CACHE_FORMAT or m['productionAuthority']is not False
            or json.loads((cache/'COMPLETE.json').read_bytes())!={'manifestSha256':manifest_sha}):
        raise ValueError('ORIGINAL_COMPLETED_REPLAY_REQUIRED')
    dbpath=cache/'replay.sqlite'
    if dbpath.is_symlink()or sha(dbpath)!=m['databaseSha256']:raise ValueError('REPLAY_DATABASE_CHANGED')
    decode,impl=implementation();db=sqlite3.connect(dbpath.as_uri()+'?mode=ro',uri=True)
    db.execute('PRAGMA cache_size=-16384');rows={};started=time.perf_counter()
    try:
        # Clocks/counts only before recording exposure; never decode a crossing
        # cache then quietly discard its protected price rows.
        for table in('market','flow'):
            count,lo,hi=db.execute(f'SELECT count(*),min(at),max(at) FROM {table}').fetchone()
            if count!=m['counts'][table]or(count and not bounds[0]<=lo<=hi<bounds[1]):
                raise ValueError('WHOLE_REPLAY_CACHE_MUST_PRECEDE_TEST')
        registry=DataUse()if data_use is None else data_use
        registry.record('inspected_development',*bounds,manifest_sha)
        output.mkdir(parents=True,exist_ok=False)
        for table in('market','flow'):
            rows[table]=0
            for i,body in db.execute(f'SELECT i,body FROM {table} ORDER BY i'):
                if i!=rows[table]or not typed_equal(json.loads(body),decode(body)):
                    raise ValueError('EXACT_TYPED_REPLAY_DECODE_PARITY_FAILED')
                rows[table]+=1
                if rows[table]%100000==0:
                    temp=output/'progress.pending'
                    temp.write_text(json.dumps({'rows':rows,'wallSeconds':time.perf_counter()-started}))
                    temp.replace(output/'progress.json')
        if rows!={t:m['counts'][t]for t in('market','flow')}:
            raise ValueError('EVERY_ORIGINAL_REPLAY_ROW_REQUIRED')
        if sha(dbpath)!=m['databaseSha256']or sha(cache/'manifest.json')!=manifest_sha:
            raise ValueError('REPLAY_SOURCE_CHANGED_DURING_AUDIT')
        if implementation()[1]!=impl:raise ValueError('DECODER_CHANGED_DURING_AUDIT')
        result={'format':FORMAT,'complete':True,'cacheManifestSha256':manifest_sha,
                'databaseSha256':m['databaseSha256'],'implementation':impl,'rows':rows,
                'allTypedDecodedRowsEqual':True,'receiptBoundsMs':list(bounds),
                'wallSeconds':time.perf_counter()-started,'productionChanged':False,
                'independentTestUsed':False,'physicalAccountRequests':0,
                'classification':'full_cache_parse_equivalence_not_training_speed_or_ROI'}
        (output/'RESULT.json').write_text(json.dumps(result,indent=2)+'\n');return result
    finally:db.close()


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    for name in('cache','output'):p.add_argument('--'+name,type=Path,required=True)
    p.add_argument('--manifest-sha',required=True);p.add_argument('--bounds',type=int,nargs=2,required=True)
    p.add_argument('--heldout-start',type=int,required=True);a=p.parse_args()
    print(json.dumps(audit(a.cache,a.manifest_sha,a.output,bounds=a.bounds,heldout_start=a.heldout_start)))
