"""Pinned original frame offsets for bounded-memory chronological decoding.

Only the producer's exact terminal envelope is parsed. Original whole-file hashes
must match the prior row/chain audit. Prices are not decoded by this indexer.
"""
import argparse,hashlib,json,re,sqlite3
from pathlib import Path
from .audit_breadth_inventory import clock,identity,write

FORMAT='evidenced_sig_frame_offsets_v1'
HEADER=re.compile(rb',"receivedAt":"([^"\r\n]{1,50})","source":"official_sig_realtime"},"hash":"([0-9a-f]{64})","previous":"([0-9a-f]{0,64})","sequence":([0-9]+)}\n$')


def sha(path):
    with Path(path).open('rb')as f:return hashlib.file_digest(f,'sha256').hexdigest()


def build(root,audit_path,audit_sha,output):
    root=Path(root).resolve();audit_path=Path(audit_path);raw=audit_path.read_bytes()
    if hashlib.sha256(raw).hexdigest()!=audit_sha:raise ValueError('PINNED_FRAME_INVENTORY_REQUIRED')
    proof=json.loads(raw)
    if (proof['classification']!='original_frame_hash_chain_inventory_not_semantic_replay_admission'
            or proof['trainingAdmitted']is not False):raise ValueError('UNADMITTED_SOURCE_INVENTORY_REQUIRED')
    entries=[r for r in proof['inventory']if r['status']=='verified_whole_file']
    if not entries:raise ValueError('VERIFIED_WHOLE_SOURCES_REQUIRED')
    output=Path(output);output.mkdir(parents=True,exist_ok=False)
    db=sqlite3.connect(output/'frames.sqlite');rows=0;bytes_read=0
    try:
        db.executescript('''
CREATE TABLE files(i INTEGER PRIMARY KEY,name TEXT UNIQUE,sha TEXT,bytes INTEGER);
CREATE TABLE frames(hash TEXT PRIMARY KEY,at INTEGER,file INTEGER,offset INTEGER,bytes INTEGER,sequence INTEGER,previous TEXT);
CREATE INDEX chronology ON frames(at,sequence,hash);
''')
        for i,entry in enumerate(entries):
            name=entry['file'];path=root/name
            if (Path(name).name!=name or not name.startswith('breadth-')or not name.endswith('.jsonl')
                    or path.is_symlink()or not path.is_file()):raise ValueError('REGULAR_SCOPED_ORIGINAL_REQUIRED')
            before=identity(path);digest=hashlib.sha256();offset=0
            db.execute('INSERT INTO files VALUES(?,?,?,?)',(i,name,entry['sha256'],before[2]))
            with path.open('rb')as source:
                while True:
                    body=source.readline(16*1024*1024+1)
                    if not body:break
                    digest.update(body)
                    if len(body)>16*1024*1024:raise ValueError('ORIGINAL_FRAME_ROW_BOUND')
                    match=HEADER.search(body[-512:])
                    if match is None:raise ValueError('EXACT_PRODUCER_ENVELOPE_SUFFIX_REQUIRED')
                    stamp,claimed,previous,seq=match.groups();at=clock(stamp.decode('ascii'))
                    if at>=proof['cutoffExclusiveMs']:raise ValueError('RESERVED_DATA_BOUNDARY')
                    db.execute('INSERT OR IGNORE INTO frames VALUES(?,?,?,?,?,?,?)',
                        (claimed.decode(),at,i,offset,len(body),int(seq),previous.decode()))
                    offset+=len(body);rows+=1
            if digest.hexdigest()!=entry['sha256']or identity(path)!=before or offset!=before[2]:
                raise ValueError('AUDITED_ORIGINAL_CHANGED')
            bytes_read+=offset;db.commit()
            write(output/'status.json',{'phase':'indexing_original_offsets','filesCompleted':i+1,
                'filesTotal':len(entries),'bytesRead':bytes_read,'sourceRows':rows,'accountRequests':0})
        count,first,last=db.execute('SELECT COUNT(*),MIN(at),MAX(at) FROM frames').fetchone()
    finally:db.close()
    if audit_path.read_bytes()!=raw:raise ValueError('SOURCE_INVENTORY_CHANGED')
    result={'format':FORMAT,'sourceAuditSha256':audit_sha,'databaseSha256':sha(output/'frames.sqlite'),
        'databaseBytes':(output/'frames.sqlite').stat().st_size,'originalFiles':len(entries),
        'sourceBytes':bytes_read,'sourceRows':rows,'uniqueFrames':count,'identicalEnvelopeCopiesRemoved':rows-count,
        'firstReceivedMs':first,'lastReceivedMs':last,'cutoffExclusiveMs':proof['cutoffExclusiveMs'],
        'originalBytesRetained':True,'rawPayloadsDecoded':False,'bookSemanticsValidated':False,
        'globalHistoryCompletenessEstablished':False,'trainingAdmitted':False,
        'productionChanged':False,'independentTestUsed':False,'accountRequests':0}
    write(output/'complete.json',result);write(output/'status.json',{'phase':'complete_offsets_not_semantic_admission',
        'uniqueFrames':count,'sourceRows':rows,'accountRequests':0})
    return result


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    for key in('root','audit','audit-sha256','output'):p.add_argument('--'+key,required=True)
    a=p.parse_args();r=build(a.root,a.audit,a.audit_sha256,a.output)
    print(json.dumps({k:r[k]for k in('originalFiles','uniqueFrames','sourceBytes','databaseBytes')}))
