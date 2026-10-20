"""Read-only inventory of original SIG frame files; not semantic admission."""
import argparse
from collections import Counter
from datetime import datetime,timezone
import hashlib,json,os
from pathlib import Path
from moloch.research.verify_learning_transport import source_hash


def clock(stamp):
    value=datetime.fromisoformat(stamp.replace('Z','+00:00'))
    if value.utcoffset() is None:raise ValueError('AWARE_SOURCE_CLOCK_REQUIRED')
    delta=value-datetime(1970,1,1,tzinfo=timezone.utc)
    return delta.days*86400000+delta.seconds*1000+delta.microseconds//1000


def identity(path):
    s=path.stat();return(s.st_dev,s.st_ino,s.st_size,s.st_mtime_ns)


def audit_file(path,cutoff):
    path=Path(path)
    if path.is_symlink()or not path.is_file():raise ValueError('REGULAR_ORIGINAL_FILE_REQUIRED')
    before=identity(path);digest=hashlib.sha256();first=last=None;rows=0
    events=Counter();hours=Counter();exchanges={};error=None;offset=0
    with path.open('rb')as source:
        while True:
            raw=source.readline(16*1024*1024+1)
            if not raw:break
            digest.update(raw);offset+=len(raw)
            if error is not None:continue
            try:
                if len(raw)>16*1024*1024:raise ValueError('OVERSIZED_ORIGINAL_ROW')
                row=json.loads(raw);source_hash(raw,row)
                seq=row['sequence'];previous=row['previous']
                if (type(seq)is not int or seq<1 or not isinstance(previous,str)
                        or not(len(previous)==64 or(seq==1 and previous==''))):
                    raise ValueError('ORIGINAL_SEQUENCE_REQUIRED')
                event=row['event'];at=clock(event['receivedAt'])
                if event.get('source')!='official_sig_realtime':raise ValueError('OFFICIAL_SOURCE_REQUIRED')
                if at>=cutoff:raise ValueError('RESERVED_DATA_BOUNDARY')
                if last is not None and(seq!=last['sequence']+1 or previous!=last['hash']or at<last['atMs']):
                    raise ValueError('ORIGINAL_CHAIN_OR_CLOCK_DISCONTINUITY')
                stamp={'sequence':seq,'hash':row['hash'],'previous':previous,'atMs':at}
                if first is None:first=stamp
                last=stamp;rows+=1;events[event['event']]+=1
                if event['event']=='markets_batch':
                    for market in event['payload']['markets']:
                        for book in market['batch'].get('books',[]):
                            ex=str(book['exchangeId']);source_at=clock(book['asOf']['at'])
                            if source_at>at:raise ValueError('FUTURE_BOOK_HEADER')
                            h=hours[str(at//3600000)];hours[str(at//3600000)]=h+1
                            item=exchanges.setdefault(ex,{'rawBookOccurrences':0,'firstReceivedMs':at,'lastReceivedMs':at})
                            item['rawBookOccurrences']+=1;item['firstReceivedMs']=min(item['firstReceivedMs'],at)
                            item['lastReceivedMs']=max(item['lastReceivedMs'],at)
            except Exception as caught:
                error={'code':str(caught)if isinstance(caught,ValueError)else type(caught).__name__,
                       'rowStartOffset':offset-len(raw)}
    unchanged=identity(path)==before and offset==before[2]
    if not unchanged:error={'code':'ORIGINAL_CHANGED_DURING_READ'}
    return {'file':path.name,'bytes':before[2],'sha256':digest.hexdigest(),
            'status':'verified_whole_file'if error is None and rows else'quarantined',
            'error':error,'verifiedPrefixRows':rows,'first':first,'last':last,
            'eventCounts':dict(events),'rawBookOccurrencesByHour':dict(hours),'exchanges':exchanges,
            'bookSemanticsValidated':False,'duplicatesRemoved':False,'trainingAdmitted':False}


def write(path,value):
    temporary=path.with_suffix('.pending');temporary.write_text(json.dumps(value,indent=2));os.replace(temporary,path)


def audit(root,output,cutoff):
    root=Path(root).resolve();output=Path(output).resolve();output.mkdir(exist_ok=False,parents=True)
    files=sorted(root.glob('breadth-*.jsonl'));records=[]
    for i,path in enumerate(files):
        result=audit_file(path,cutoff);records.append(result)
        write(output/('file-%04d.json'%i),result)
        write(output/'status.json',{'phase':'auditing','filesCompleted':i+1,'filesTotal':len(files),
              'bytesRead':sum(x['bytes']for x in records),'accountRequests':0})
    valid=[r for r in records if r['status']=='verified_whole_file'];tails={r['last']['hash']for r in valid}
    counts=Counter();hours=Counter();scope={}
    for r in valid:
        counts.update(r['eventCounts']);hours.update(r['rawBookOccurrencesByHour'])
        for ex,item in r['exchanges'].items():
            old=scope.setdefault(ex,dict(item,rawBookOccurrences=0))
            old['rawBookOccurrences']+=item['rawBookOccurrences']
            old['firstReceivedMs']=min(old['firstReceivedMs'],item['firstReceivedMs'])
            old['lastReceivedMs']=max(old['lastReceivedMs'],item['lastReceivedMs'])
    result={'classification':'original_frame_hash_chain_inventory_not_semantic_replay_admission',
            'cutoffExclusiveMs':cutoff,'files':len(records),'verifiedWholeFiles':len(valid),
            'quarantinedFiles':len(records)-len(valid),'bytes':sum(r['bytes']for r in records),
            'verifiedRows':sum(r['verifiedPrefixRows']for r in valid),
            'firstReceivedMs':min((r['first']['atMs']for r in valid),default=None),
            'lastReceivedMs':max((r['last']['atMs']for r in valid),default=None),
            'filesLinkedToAnInventoriedTail':sum(r['first']['previous']in tails for r in valid),
            'eventCounts':dict(counts),'rawBookOccurrencesByHour':dict(sorted(hours.items())),
            'exchanges':scope,'inventory':[{'file':r['file'],'sha256':r['sha256'],'status':r['status']}for r in records],
            'duplicatesRemoved':False,'globalHistoryCompletenessEstablished':False,
            'bookSemanticsValidated':False,'trainingAdmitted':False,'independentTestUsed':False,
            'productionChanged':False,'accountRequests':0}
    write(output/'complete.json',result);write(output/'status.json',{'phase':'complete_inventory_not_training_admission',
          'verifiedWholeFiles':len(valid),'quarantinedFiles':len(records)-len(valid),'accountRequests':0})
    return result


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--root',required=True);p.add_argument('--output',required=True);p.add_argument('--cutoff',required=True)
    a=p.parse_args();r=audit(a.root,a.output,clock(a.cutoff))
    print(json.dumps({k:r[k]for k in('files','verifiedWholeFiles','quarantinedFiles','verifiedRows')}))
