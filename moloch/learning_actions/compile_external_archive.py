"""Stream original archived REST references; never invent missing WS state.

Each immutable snapshot must pass gzip completion and the existing reference
adapters. A truncated newer snapshot is rolled back before an older complete
snapshot is tried. Missing source coverage remains missing.
"""
import argparse,gzip,hashlib,json,math,shutil,sqlite3
from collections import defaultdict
from pathlib import Path
from .compile_archived_cohort import reference_rows,verify_snapshot
from .audit_breadth_inventory import write,clock
from .breadth_frame_index import sha


def compile_archive(archive,index_path,index_sha,kalshi,kalshi_sha,polymarket,polymarket_sha,
                    output,*,roles,heldout_start,minimum_free_bytes=32*1024**3):
    archive=Path(archive).resolve();index_path=Path(index_path);output=Path(output)
    paths=[index_path,Path(kalshi),Path(polymarket)];raws=[p.read_bytes()for p in paths]
    if [hashlib.sha256(b).hexdigest()for b in raws]!=[index_sha,kalshi_sha,polymarket_sha]:
        raise ValueError('EXACT_FROZEN_ARCHIVE_AND_UNIVERSES_REQUIRED')
    if(set(roles)!={'fit','search'}or not 0<=roles['fit'][0]<roles['fit'][1]<=roles['search'][0]<roles['search'][1]<=heldout_start):
        raise ValueError('CHRONOLOGICAL_NON_HELDOUT_ROLES_REQUIRED')
    if type(minimum_free_bytes)is not int or minimum_free_bytes<1:raise ValueError('POSITIVE_EXTERNAL_DISK_RESERVE')
    if shutil.disk_usage(output.parent).free<minimum_free_bytes:raise ValueError('EXTERNAL_DISK_HEADROOM')
    groups=defaultdict(dict)
    for line in raws[0].splitlines():
        e=json.loads(line)
        captured=e['captured_unix']
        if type(captured)not in(int,float)or not math.isfinite(captured)or not 0<=captured*1000<heldout_start:
            raise ValueError('NO_HELDOUT_ARCHIVE_CAPTURE_ALLOWED')
        source=e['source'];venue='Kalshi'if source=='Kalshi'else'Polymarket'if source.startswith('forward_polymarket_')else None
        if venue is None:raise ValueError('EXPLICIT_ARCHIVED_REFERENCE_SOURCE')
        p=(archive/e['archive_relative_path']).resolve()
        if not p.is_relative_to(archive):raise ValueError('ARCHIVE_PATH_OUTSIDE_ROOT')
        groups[source,e['filename']][e['sha256']]={**e,'path':str(p),'venue':venue}
    output.mkdir(parents=True,exist_ok=False);db=sqlite3.connect(output/'references.sqlite')
    db.execute('PRAGMA temp_store=FILE');db.execute('PRAGMA cache_size=-16384')
    db.execute('CREATE TABLE observations(hash TEXT PRIMARY KEY,at INTEGER,venue TEXT,kind TEXT,market TEXT,body TEXT)')
    db.commit();pins=[];excluded=[];duplicates=0;ku,pu=map(json.loads,raws[1:])
    try:
        for number,(identity,versions)in enumerate(sorted(groups.items())):
            ordered=sorted(versions.values(),key=lambda x:(-x['source_size'],x['sha256']))
            # Filename reuse is not permission to discard an older history.
            # Every recorded variant must be an exact prefix of the largest.
            largest,_=verify_snapshot(ordered[0]);digest=hashlib.sha256();offset=0
            with largest.open('rb')as f:
                for prior in reversed(ordered):
                    remaining=prior['source_size']-offset
                    while remaining:
                        body=f.read(min(1024*1024,remaining))
                        if not body:raise ValueError('ARCHIVE_PREFIX_TRUNCATED')
                        digest.update(body);offset+=len(body);remaining-=len(body)
                    if digest.hexdigest()!=prior['sha256']:raise ValueError('ARCHIVED_FILENAME_PREFIX_CONFLICT')
            selected=False
            for e in ordered:
                candidate=[];candidate_duplicates=0;db.execute('BEGIN')
                try:
                    for count,row in enumerate(reference_rows({'files':[e]},ku,pu,roles,candidate),1):
                        if count%10000==0 and shutil.disk_usage(output).free<minimum_free_bytes:
                            raise ValueError('EXTERNAL_DISK_HEADROOM')
                        body=json.dumps(row,sort_keys=True,separators=(',',':'),allow_nan=False)
                        key=hashlib.sha256(body.encode()).hexdigest()
                        changed=db.execute('INSERT OR IGNORE INTO observations VALUES(?,?,?,?,?,?)',
                            (key,row['available_at_ms'],row['venue'],row['kind'],row['market_id'],body)).rowcount
                        candidate_duplicates+=not changed
                    db.commit();pins.extend(candidate);duplicates+=candidate_duplicates;selected=True;break
                except (EOFError,ValueError)as error:
                    db.rollback()
                    if not isinstance(error,EOFError)and str(error)!='EXTERNAL_PARTIAL_CLOSED_ROW':raise
                    excluded.append({'source':e['source'],'filename':e['filename'],'sha256':e['sha256'],
                                     'reason':'incomplete_gzip_or_final_row_no_rows_admitted'})
            if not selected:excluded.append({'source':identity[0],'filename':identity[1],'reason':'no_complete_snapshot_available'})
            write(output/'status.json',{'phase':'validating_original_external_receipts','filesCompleted':number+1,
                'filesTotal':len(groups),'completeSnapshots':len(pins),'excludedSnapshots':len(excluded),
                'trainingAdmitted':False,'accountRequests':0})
        db.execute('CREATE INDEX chronology ON observations(at,hash)');db.commit()
        with gzip.GzipFile(filename=str(output/'references.jsonl.gz'),mode='wb',mtime=0,compresslevel=1)as f:
            for index,(body,)in enumerate(db.execute('SELECT body FROM observations ORDER BY at,hash')):
                if index%10000==0 and shutil.disk_usage(output).free<minimum_free_bytes:raise ValueError('EXTERNAL_DISK_HEADROOM')
                f.write((body+'\n').encode())
        coverage=[{'venue':v,'kind':k,'rows':n,'firstAvailableMs':lo,'lastAvailableMs':hi,'mappedMarkets':markets}
            for v,k,n,lo,hi,markets in db.execute('SELECT venue,kind,COUNT(*),MIN(at),MAX(at),COUNT(DISTINCT market) FROM observations GROUP BY venue,kind')]
        if not all(any(v['venue']==venue and v['kind']=='reference'for v in coverage)for venue in('Kalshi','Polymarket')):
            raise ValueError('BOTH_RECORDED_REFERENCE_VENUES_REQUIRED')
        if any(p.read_bytes()!=raw for p,raw in zip(paths,raws)):raise ValueError('FROZEN_REFERENCE_INPUT_CHANGED')
        result={'format':'original_archived_REST_references_v1','sourceIndexSha256':index_sha,
            'kalshiUniverseSha256':kalshi_sha,'polymarketUniverseSha256':polymarket_sha,
            'normalizedRowsSha256':sha(output/'references.jsonl.gz'),'calendarRolesMs':roles,
            'coverage':coverage,'completeSnapshots':pins,'excludedSnapshots':excluded,'exactDuplicatesRemoved':duplicates,
            'websocketDeltasReconstructed':False,'originalWebsocketFramesRetained':True,
            'globalSourceCoverageComplete':False,'historicalMappingAvailabilityVerified':False,
            'trainingAdmitted':False,'independentTestUsed':False,'productionChanged':False,'accountRequests':0}
        write(output/'complete.json',result);write(output/'status.json',{'phase':'complete_partial_reference_research',
            'coverage':coverage,'trainingAdmitted':False,'accountRequests':0});return result
    except BaseException as error:
        db.rollback();write(output/'FAILED.json',{'errorType':type(error).__name__,'reason':str(error),
                                                'trainingAdmitted':False,'accountRequests':0});raise
    finally:db.close()


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    for k in('archive','index','index-sha256','kalshi','kalshi-sha256','polymarket','polymarket-sha256',
             'output','fit-start','fit-end','search-end'):p.add_argument('--'+k,required=True)
    a=p.parse_args();cut=clock(a.search_end);roles={'fit':[clock(a.fit_start),clock(a.fit_end)],'search':[clock(a.fit_end),cut]}
    r=compile_archive(a.archive,a.index,a.index_sha256,a.kalshi,a.kalshi_sha256,a.polymarket,a.polymarket_sha256,
        a.output,roles=roles,heldout_start=cut)
    print(json.dumps({'coverage':r['coverage'],'completeSnapshots':len(r['completeSnapshots']),'trainingAdmitted':False}))
