"""Stream original indexed SIG frames into normalized research observations.

This validates known frame semantics, not historical execution calibration,
catalog completeness or continuous coverage. External data is not invented.
"""
import argparse,gzip,hashlib,json,sqlite3,shutil
from collections import Counter
from contextlib import ExitStack
from pathlib import Path
from .breadth_frame_index import FORMAT,sha
from .breadth_source import Decoder
from .audit_breadth_inventory import identity,clock,write
from moloch.research.verify_learning_transport import source_hash
from moloch.learning_flow.features import Flow


def compile_index(root,index,index_sha,output,*,tournament,train_end,minimum_free_bytes=8*1024**3):
    root=Path(root).resolve();index=Path(index);raw=(index/'complete.json').read_bytes()
    if hashlib.sha256(raw).hexdigest()!=index_sha:raise ValueError('PINNED_FRAME_OFFSET_MANIFEST_REQUIRED')
    source=json.loads(raw)
    if source['format']!=FORMAT or source['trainingAdmitted']is not False:
        raise ValueError('UNADMITTED_ORIGINAL_OFFSETS_REQUIRED')
    if sha(index/'frames.sqlite')!=source['databaseSha256']:raise ValueError('FRAME_OFFSET_DATABASE_CHANGED')
    if not source['firstReceivedMs']<train_end<source['lastReceivedMs']<source['cutoffExclusiveMs']:
        raise ValueError('EXACT_CHRONOLOGICAL_RESEARCH_CUT_REQUIRED')
    output=Path(output)
    if type(minimum_free_bytes)is not int or minimum_free_bytes<1:raise ValueError('POSITIVE_COMPILER_DISK_RESERVE')
    if shutil.disk_usage(output.parent).free<minimum_free_bytes:raise ValueError('COMPILER_DISK_HEADROOM')
    output.mkdir(parents=True,exist_ok=False)
    db=sqlite3.connect((index/'frames.sqlite').resolve().as_uri()+'?mode=ro&immutable=1',uri=True)
    seen=sqlite3.connect(output/'print-facts.sqlite')
    seen.execute('CREATE TABLE prints(k TEXT PRIMARY KEY,signature TEXT)')
    decoder=Decoder(tournament);counts=Counter();last_hash=None;processed=0
    try:
        with ExitStack()as stack:
            handles={};files={}
            for i,name,digest,size in db.execute('SELECT i,name,sha,bytes FROM files'):
                p=root/name
                if Path(name).name!=name or p.is_symlink()or not p.is_file()or p.stat().st_size!=size:
                    raise ValueError('REGULAR_PINNED_SOURCE_REQUIRED')
                handles[i]=stack.enter_context(p.open('rb'));files[i]=(p,digest,identity(p))
            books=stack.enter_context(gzip.GzipFile(filename=str(output/'observations.jsonl.gz'),mode='wb',mtime=0,compresslevel=1))
            prints=stack.enter_context(gzip.GzipFile(filename=str(output/'trade-flow.jsonl.gz'),mode='wb',mtime=0,compresslevel=1))
            def emit(handle,row):handle.write((json.dumps(row,separators=(',',':'),allow_nan=False)+'\n').encode())
            for claimed,at,file,offset,length,seq,previous in db.execute('SELECT hash,at,file,offset,bytes,sequence,previous FROM frames ORDER BY at,sequence,hash'):
                if processed%128==0 and shutil.disk_usage(output).free<minimum_free_bytes:
                    raise ValueError('COMPILER_DISK_HEADROOM')
                handle=handles[file];handle.seek(offset);body=handle.read(length)
                row=json.loads(body);source_hash(body,row)
                if row['hash']!=claimed or row['sequence']!=seq or row['previous']!=previous:
                    raise ValueError('ORIGINAL_FRAME_OFFSET_CHANGED')
                event=row['event']
                if clock(event['receivedAt'])!=at or at>=source['cutoffExclusiveMs']:
                    raise ValueError('ORIGINAL_FRAME_CLOCK_CHANGED')
                role='fit'if at<train_end else'search'
                if last_hash!=previous and event['event']!='stream_reset':
                    gap={'venue':'SIG','kind':'gap','market_id':None,'exchange_id':None,
                        'available_at_ms':at,'received_at_ms':at,'original_realtime_received_at_ms':at,
                        'snapshot_origin':'original_official_sig_realtime_frame','original_sha256':claimed,
                        'calendar_role':role,'gap_reason':'unlinked_original_chain_boundary',
                        'derived_from_source_chain':True}
                    emit(books,gap);counts[role+'/gap']+=1;counts['derived_chain_boundaries']+=1
                    decoder.revisions.clear();decoder.needs_baseline.update(decoder.mapping)
                normalized,trades=decoder.decode(event,claimed,at,role)
                for record in normalized:emit(books,record);counts[role+'/'+record['kind']]+=1
                for record in trades:
                    # Reuse the exact fact/unit validator, but retain the full
                    # corpus deduplication set on disk rather than in an episode.
                    validator=Flow();validator.accept(at,record)
                    key,signature=next(iter(validator.seen.items()))
                    key=json.dumps(key,separators=(',',':'));signature=json.dumps(signature,separators=(',',':'))
                    prior=seen.execute('SELECT signature FROM prints WHERE k=?',(key,)).fetchone()
                    if prior is not None:
                        if prior[0]!=signature:raise ValueError('ORIGINAL_MATCH_FACTS_CHANGED')
                        counts['duplicate_engine_prints']+=1
                    else:
                        seen.execute('INSERT INTO prints VALUES(?,?)',(key,signature))
                        emit(prints,record);counts[role+'/trade']+=1
                last_hash=claimed;processed+=1
                if processed%1000==0:
                    seen.commit()
                    books.flush();prints.flush();write(output/'status.json',{'phase':'normalizing_original_frames',
                        'framesCompleted':processed,'framesTotal':source['uniqueFrames'],'counts':dict(counts),
                        'latestReceivedMs':at,'accountRequests':0})
            if processed!=source['uniqueFrames']:raise ValueError('ALL_INDEXED_FRAMES_REQUIRED')
            seen.commit()
            for p,digest,before in files.values():
                if identity(p)!=before or sha(p)!=digest:raise ValueError('ORIGINAL_SOURCE_CHANGED_DURING_COMPILE')
        if(index/'complete.json').read_bytes()!=raw:raise ValueError('FRAME_MANIFEST_CHANGED_DURING_COMPILE')
        result={'format':'normalized_original_sig_frames_v1','sourceOffsetManifestSha256':index_sha,
            'observationsSha256':sha(output/'observations.jsonl.gz'),'tradeRowsSha256':sha(output/'trade-flow.jsonl.gz'),
            'frames':processed,'counts':dict(counts),'decoderCounts':dict(decoder.counts),
            'exchanges':decoder.mapping,'firstReceivedMs':source['firstReceivedMs'],'lastReceivedMs':source['lastReceivedMs'],
            'fitEndExclusiveMs':train_end,'cutoffExclusiveMs':source['cutoffExclusiveMs'],
            'originalFrameSemanticsValidated':True,'globalHistoryCompletenessEstablished':False,
            'historicalCatalogAvailabilityVerified':False,'settlementPayoutsInferred':False,
            'minimumFreeDiskBytes':minimum_free_bytes,
            'externalObservationsIncluded':False,'executionCalibrationEstablished':False,
            'trainingAdmitted':False,'independentTestUsed':False,'productionChanged':False,'accountRequests':0}
        write(output/'complete.json',result);write(output/'status.json',{'phase':'complete_semantic_review_still_required',
            'framesCompleted':processed,'counts':dict(counts),'accountRequests':0})
        return result
    except BaseException as error:
        write(output/'FAILED.json',{'phase':'failed_no_training_admission','errorType':type(error).__name__,
            'reason':str(error),'framesCompleted':processed,'accountRequests':0});raise
    finally:seen.close();db.close()


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    for k in('root','index','index-sha256','output','tournament','train-end'):p.add_argument('--'+k,required=True)
    a=p.parse_args();r=compile_index(a.root,a.index,a.index_sha256,a.output,tournament=a.tournament,train_end=clock(a.train_end))
    print(json.dumps({'frames':r['frames'],'counts':r['counts']}))
