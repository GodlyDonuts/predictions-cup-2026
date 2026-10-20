"""Immutable, source-pinned SQLite replay. No HTTP or financial-state writer.

JSON bodies remain original. Indices, role views and bounded book caches are
engineering artifacts, never new market observations or source coverage.
"""
import argparse
import gzip
import hashlib
import json
from pathlib import Path
import sqlite3
import atexit
from collections import OrderedDict
from collections.abc import Sequence
from moloch.learning_flow.episodes import FlowTape,FEATURE_SCHEMA
from moloch.learning_flow.features import Flow

FORMAT='tartarus_sqlite_replay_v1'


def sha(path):
    h=hashlib.sha256()
    with Path(path).open('rb')as f:
        for block in iter(lambda:f.read(1024*1024),b''):h.update(block)
    return h.hexdigest()


def source_files(dataset):
    dataset=Path(dataset);raw=(dataset/'dataset-manifest.json').read_bytes();m=json.loads(raw)
    digest=hashlib.sha256(raw).hexdigest()
    if json.loads((dataset/'COMPLETE.json').read_text())['manifestSha256']!=digest or m['featureSchema']!=FEATURE_SCHEMA or m['productionAuthority']is not False:
        raise ValueError('ORIGINAL_FLOW_MANIFEST_REQUIRED')
    base=Path(m['baseDataset']);braw=(base/'dataset-manifest.json').read_bytes()
    if hashlib.sha256(braw).hexdigest()!=m['baseManifestSha256']or json.loads((base/'COMPLETE.json').read_text())['manifestSha256']!=m['baseManifestSha256']:
        raise ValueError('ORIGINAL_BASE_MANIFEST_REQUIRED')
    bm=json.loads(braw)
    files=[(base/'observations.jsonl.gz',bm['observationsSha256']),
           (dataset/'trade-flow.jsonl.gz',m['tradeRowsSha256'])]
    for p,h in files:
        if sha(p)!=h:raise ValueError('ORIGINAL_SOURCE_FILE_CHANGED')
    return m,digest,files


def build(dataset,output):
    from .snapshot_clock import source_clock
    output=Path(output);m,digest,files=source_files(dataset)
    output.mkdir(parents=True,exist_ok=False)
    db=sqlite3.connect(output/'replay.sqlite')
    try:
        db.execute('PRAGMA temp_store=FILE');db.execute('PRAGMA cache_size=-16384')
        db.execute('CREATE TABLE market(i INTEGER PRIMARY KEY,at INTEGER,venue TEXT,kind TEXT,ex TEXT,market TEXT,source TEXT,world_at INTEGER,body TEXT)')
        db.execute('CREATE TABLE flow(i INTEGER PRIMARY KEY,at INTEGER,executed INTEGER,ex TEXT,tid TEXT,match TEXT,body TEXT)')
        db.execute('CREATE TABLE identities(tid TEXT,ex TEXT,match TEXT,facts TEXT,PRIMARY KEY(tid,ex,match))')
        counts={};verifier=Flow()
        for name,(path,expected)in zip(('market','flow'),files):
            previous=-1;count=0
            with gzip.open(path,'rt',encoding='utf-8')as f:
                for line in f:
                    row=json.loads(line);at=row['available_at_ms']
                    if type(at)is not int or at<0 or at<previous:raise ValueError('ORIGINAL_TAPE_CLOCK')
                    previous=at
                    if '_disk_record'in row:raise ValueError('RESERVED_DISK_RECORD_FIELD')
                    if name=='market':
                        for value in(row['venue'],row['kind']):
                            if type(value)is not str:raise ValueError('CACHE_SOURCE_IDENTIFIER_TYPES')
                        for value in(row.get('exchange_id'),row.get('market_id'),row.get('source')):
                            if value is not None and type(value)is not str:raise ValueError('CACHE_SOURCE_IDENTIFIER_TYPES')
                        if row['kind']=='book'and any(type(row.get(k))is not str or not row[k]for k in('exchange_id','market_id')):
                            raise ValueError('CACHE_BOOK_SCOPE_REQUIRED')
                        snapshot=row.get('snapshot_origin')=='original_official_sig_realtime_frame'or'native_event_received_at_ms'in row
                        world=source_clock(row)if snapshot and row['venue']=='SIG'and row['kind']in('book','gap')else None
                        db.execute('INSERT INTO market VALUES(?,?,?,?,?,?,?,?,?)',(count,at,row['venue'],row['kind'],row.get('exchange_id'),row.get('market_id'),row.get('source',row['venue']),world,line))
                    else:
                        if any(type(row.get(k))is not str or not row[k]for k in('tournament_id','exchange_id','market_id','match_id')):
                            raise ValueError('CACHE_TRADE_SCOPE_REQUIRED')
                        # Reuse exact validation; global duplicate facts live in
                        # SQLite instead of an unbounded Python seen dictionary.
                        verifier.accept(at,row);verifier.seen.clear();verifier.rows.clear()
                        key=(row['tournament_id'],row['exchange_id'],row['match_id'])
                        facts=json.dumps([row[k]for k in('market_id','executed_at_ms','yes_price_micros','quantity')],separators=(',',':'))
                        prior=db.execute('SELECT facts FROM identities WHERE tid=? AND ex=? AND match=?',key).fetchone()
                        if prior and prior[0]!=facts:raise ValueError('ORIGINAL_MATCH_FACTS_CHANGED')
                        if not prior:db.execute('INSERT INTO identities VALUES(?,?,?,?)',(*key,facts))
                        db.execute('INSERT INTO flow VALUES(?,?,?,?,?,?,?)',(count,at,row['executed_at_ms'],row['exchange_id'],row['tournament_id'],row['match_id'],line))
                    count+=1
                    if count%10000==0:db.commit()
            if sha(path)!=expected:raise ValueError('SOURCE_CHANGED_DURING_CACHE_BUILD')
            counts[name]=count;db.commit()
        if not counts['market']:raise ValueError('NONEMPTY_MARKET_TAPE_REQUIRED')
        db.executescript('''CREATE INDEX market_time ON market(at,i);
          CREATE INDEX market_state ON market(venue,kind,ex,market,source,at);
          CREATE INDEX flow_scope ON flow(ex,at,executed,match);
          CREATE TABLE execution(i INTEGER PRIMARY KEY,at INTEGER,record INTEGER);
          INSERT INTO execution SELECT ROW_NUMBER() OVER(ORDER BY executed,at,match,i)-1,executed,i FROM flow;
          CREATE TABLE world(i INTEGER PRIMARY KEY,at INTEGER,record INTEGER);
          INSERT INTO world SELECT ROW_NUMBER() OVER(ORDER BY world_at,i)-1,world_at,i FROM market WHERE world_at IS NOT NULL;''')
        counts['world']=db.execute('SELECT COUNT(*) FROM world').fetchone()[0]
        counts['execution']=counts['flow'];db.commit()
    finally:db.close()
    # Recheck both manifests as well as compressed source bytes before COMPLETE.
    _,final_digest,final_files=source_files(dataset)
    if final_digest!=digest or final_files!=files:raise ValueError('CACHE_SOURCE_IDENTITY_CHANGED')
    manifest={'format':FORMAT,'sourceManifestSha256':digest,'sourceManifest':m,
        'databaseSha256':sha(output/'replay.sqlite'),'databaseBytes':(output/'replay.sqlite').stat().st_size,
        'counts':counts,'productionAuthority':False,'newMarketData':False}
    raw=(json.dumps(manifest,sort_keys=True,indent=2)+'\n').encode()
    (output/'manifest.json').write_bytes(raw)
    (output/'COMPLETE.json').write_text(json.dumps({'manifestSha256':hashlib.sha256(raw).hexdigest()})+'\n')
    return hashlib.sha256(raw).hexdigest()


class Series(Sequence):
    def __init__(self,store,table,count,*,bodies=True):
        self.store=store;self.table=table;self.count=count;self.bodies=bodies
    def __len__(self):return self.count
    def __iter__(self):
        if self.bodies:return self.records(0,self.count)
        return (r[0]for r in self.store.query(f'SELECT at FROM {self.table} ORDER BY i'))
    def __getitem__(self,index):
        if isinstance(index,slice):return Slice(self,range(*index.indices(len(self))))
        if index<0:index+=len(self)
        if not 0<=index<len(self):raise IndexError(index)
        if not self.bodies:
            return self.store.db.execute(f'SELECT at FROM {self.table} WHERE i=?',(index,)).fetchone()[0]
        return next(self.records(index,index+1))
    def records(self,start,stop):
        if self.table in('market','flow'):
            query=f'SELECT i,body FROM {self.table} WHERE i>=? AND i<? ORDER BY i';base=self.table
        else:
            base='flow'if self.table=='execution'else'market'
            query=f'SELECT b.i,b.body FROM {self.table} s JOIN {base} b ON b.i=s.record WHERE s.i>=? AND s.i<? ORDER BY s.i'
        for i,body in self.store.query(query,(start,stop)):
            row=json.loads(body);row['_disk_record']=(base,i)
            self.store.decoded_rows+=1
            yield row


class Slice(Sequence):
    def __init__(self,series,indices):self.series=series;self.indices=indices
    def __len__(self):return len(self.indices)
    def __getitem__(self,index):
        chosen=self.indices[index]
        return Slice(self.series,chosen)if isinstance(chosen,range)else self.series[chosen]
    def __iter__(self):
        if self.series.bodies and self.indices.step==1:
            return self.series.records(self.indices.start,self.indices.stop)
        return (self.series[i]for i in self.indices)


def record_key(row):return row.get('_disk_record',id(row))


class Books:
    def __init__(self,store,world=False,capacity=512):
        self.store=store;self.world=world;self.capacity=capacity;self.cache=OrderedDict()
    def __getitem__(self,key):
        from moloch.learning_v2.liquidity import Book
        if key not in self.cache:
            table,i=key
            if table!='market':raise ValueError('MARKET_BOOK_RECORD_REQUIRED')
            row=self.store.rows[i]
            if row['kind']!='book':raise ValueError('BOOK_RECORD_REQUIRED')
            if self.world:
                from .snapshot_clock import source_clock
                at=source_clock(row)
            else:at=row['available_at_ms']
            self.cache[key]=Book(row['exchange_id'],at,row.get('source_at_ms'),
                tuple(map(tuple,row['bids'])),tuple(map(tuple,row['asks'])),row['original_sha256'])
            if len(self.cache)>self.capacity:self.cache.popitem(last=False)
        self.cache.move_to_end(key);return self.cache[key]


class Store:
    def __init__(self,path,expected_manifest_sha256,source_dataset):
        path=Path(path).resolve();raw=(path/'manifest.json').read_bytes();digest=hashlib.sha256(raw).hexdigest()
        if digest!=expected_manifest_sha256 or json.loads((path/'COMPLETE.json').read_text())['manifestSha256']!=digest:
            raise ValueError('FROZEN_REPLAY_CACHE_MANIFEST_CHANGED')
        self.manifest=json.loads(raw);self.cache_manifest_sha256=digest
        if self.manifest['format']!=FORMAT or self.manifest['productionAuthority']is not False:
            raise ValueError('EXPLICIT_REPLAY_CACHE_FORMAT')
        database=(path/'replay.sqlite').resolve()
        if not database.is_relative_to(path)or sha(database)!=self.manifest['databaseSha256']:
            raise ValueError('FROZEN_REPLAY_CACHE_DATABASE_CHANGED')
        source_manifest,source_sha,_=source_files(source_dataset)
        if source_sha!=self.manifest['sourceManifestSha256']or source_manifest!=self.manifest['sourceManifest']:
            raise ValueError('CACHE_IS_NOT_CONFIGURED_SOURCE_DATASET')
        self.db=sqlite3.connect(database.as_uri()+'?mode=ro',uri=True)
        self.db.execute('PRAGMA cache_size=-16384');self.db.execute('PRAGMA temp_store=FILE')
        self.decoded_rows=0;self.views={};self.cursors=set();self.closed=False
        self.world_groups={};self.information_groups=None
        counts=self.manifest['counts']
        for table in('market','flow','world','execution'):
            count,first,last=self.db.execute(f'SELECT COUNT(*),MIN(i),MAX(i) FROM {table}').fetchone()
            if count!=counts[table]or(count and(first!=0 or last!=count-1)):
                self.close();raise ValueError('CONTIGUOUS_REPLAY_INDEX_REQUIRED')
        self.rows=Series(self,'market',counts['market']);self.times=Series(self,'market',counts['market'],bodies=False)
        self.flow_rows=Series(self,'flow',counts['flow']);self.flow_times=Series(self,'flow',counts['flow'],bodies=False)
        self.execution_rows=Series(self,'execution',counts['execution']);self.execution_times=Series(self,'execution',counts['execution'],bodies=False)
        atexit.register(self.close)

    def world_view(self,bounds):
        if bounds not in self.views:
            lo,hi=bounds;name='role_world_'+str(len(self.views))
            # The venue index orders by contract before time. On a multi-day
            # corpus it scatters full-row reads across every SIG snapshot just
            # to validate one role. Keep the identical guard in source-time
            # order using the cache's mandatory chronological index.
            missing=self.db.execute("SELECT COUNT(*) FROM market INDEXED BY market_time WHERE at>=? AND at<? AND venue='SIG' AND kind IN('book','gap') AND world_at IS NULL",(lo,hi)).fetchone()[0]
            if missing:raise ValueError('SNAPSHOT_WORLD_CLOCK_METADATA_REQUIRED')
            self.db.execute(f'CREATE TEMP TABLE {name}(i INTEGER PRIMARY KEY,at INTEGER,record INTEGER,kind TEXT,ex TEXT,market TEXT,books INTEGER)')
            self.db.execute(f'''INSERT INTO {name} SELECT ROW_NUMBER() OVER(ORDER BY w.i)-1,w.at,w.record,m.kind,m.ex,m.market,
                SUM(m.kind='book') OVER(ORDER BY w.i ROWS UNBOUNDED PRECEDING)
                FROM world w JOIN market m ON m.i=w.record WHERE w.at>=? AND w.at<? AND m.at>=? AND m.at<?''',(lo,hi,lo,hi))
            self.db.execute(f'CREATE INDEX {name}_state ON {name}(kind,ex,market,at,i)')
            count=self.db.execute(f'SELECT COUNT(*) FROM {name}').fetchone()[0]
            self.views[bounds]=(name,Series(self,name,count),Series(self,name,count,bodies=False))
            self.world_groups[bounds]=list(self.db.execute(f'SELECT DISTINCT kind,ex,market FROM {name}'))
        return self.views[bounds]

    def query(self,sql,parameters=()):
        cursor=self.db.execute(sql,parameters);self.cursors.add(cursor)
        try:
            for row in cursor:yield row
        finally:
            if cursor in self.cursors:
                try:cursor.close()
                except sqlite3.ProgrammingError:
                    if not self.closed:raise
                self.cursors.discard(cursor)

    def prefix(self,bounds,before,*,world=False,first=False):
        lo,hi=bounds
        if world:
            name,_,_=self.world_view(bounds)
            selected=[]
            for kind,ex,market in self.world_groups[bounds]:
                row=self.db.execute(f'SELECT i,record FROM {name} WHERE kind=? AND ex IS ? AND market IS ? AND at<? ORDER BY at DESC,i DESC LIMIT 1',
                                    (kind,ex,market,before)).fetchone()
                if row:selected.append(row)
            for _,record in sorted(selected):yield self.rows[record]
        else:
            if self.information_groups is None:
                self.information_groups=list(self.db.execute("SELECT DISTINCT venue,ex,market,source FROM market WHERE (venue='SIG' AND kind='book') OR (venue IN('Kalshi','Polymarket') AND kind IN('reference','reference_gap'))"))
            selected=[];order='ASC'if first else'DESC'
            for venue,ex,market,source in self.information_groups:
                candidates=[]
                for kind in(('book',)if venue=='SIG'else('reference','reference_gap')):
                    extra=' AND world_at>=? AND world_at<?'if venue=='SIG'else''
                    args=(venue,kind,ex,market,source,lo,before)+((lo,hi)if venue=='SIG'else())
                    row=self.db.execute(f'''SELECT i FROM market WHERE venue=? AND kind=? AND ex IS ? AND market IS ?
                        AND source IS ? AND at>=? AND at<?{extra} ORDER BY at {order},i {order} LIMIT 1''',args).fetchone()
                    if row:candidates.append(row[0])
                if candidates:selected.append(min(candidates)if first else max(candidates))
            for record in sorted(selected):yield self.rows[record]

    def history_page(self,bounds,exchange,opened,lower,upper,offset,limit,*,through=None):
        lo,hi=bounds
        query='''SELECT body FROM flow WHERE i IN(SELECT MIN(i) FROM flow WHERE ex=? AND at>=? AND at<?
           AND at<=? AND executed>=? AND executed<? AND (? IS NULL OR i<=?) GROUP BY tid,ex,match)
           ORDER BY executed DESC,match LIMIT ? OFFSET ?'''
        return [json.loads(r[0])for r in self.db.execute(query,(exchange,lo,hi,opened,lower,upper,through,through,limit,offset))]

    def coverage(self,split,maximum_gap_ms):
        result={}
        for role in('train','evolution','test'):
            lo,hi=split.bounds(role);venues={}
            for venue in('SIG','Kalshi','Polymarket'):
                kind='book'if venue=='SIG'else'reference'
                count,markets=self.db.execute('SELECT COUNT(*),COUNT(DISTINCT market)+COALESCE(MAX(market IS NULL),0) FROM market WHERE at>=? AND at<? AND venue=? AND kind=?',(lo,hi,venue,kind)).fetchone()
                previous=lo;maximum=0;first=None;last=None
                for(at,)in self.db.execute('SELECT DISTINCT at FROM market WHERE at>=? AND at<? AND venue=? AND kind=? ORDER BY at',(lo,hi,venue,kind)):
                    if first is None:first=at
                    maximum=max(maximum,at-previous);previous=at;last=at
                maximum=max(maximum,hi-previous)
                venues[venue]={'observations':count,'markets':markets,'first_ms':first,'last_ms':last,
                    'maximum_gap_ms':maximum,'coverage_pass':first is not None and maximum<=maximum_gap_ms}
            result[role]={'bounds_ms':[lo,hi],'venues':venues,'coverage_pass':all(v['coverage_pass']for v in venues.values())}
        return result

    def close(self):
        if self.closed:return
        self.closed=True
        for cursor in tuple(self.cursors):cursor.close()
        self.cursors.clear();self.db.close()
        atexit.unregister(self.close)


class DiskFlowTape(FlowTape):
    def __init__(self,cache,expected_sha256,source_dataset):
        self.disk_store=Store(cache,expected_sha256,source_dataset);s=self.disk_store
        self.manifest=s.manifest['sourceManifest'];self.manifest_sha256=s.manifest['sourceManifestSha256']
        self.rows,self.times,self.flow_rows,self.flow_times=s.rows,s.times,s.flow_rows,s.flow_times
        self.exchanges=('__GLOBAL_WAIT__',)+tuple(r[0]for r in s.db.execute("SELECT DISTINCT ex FROM market WHERE kind='book' ORDER BY ex"))
        self.index={ex:i for i,ex in enumerate(self.exchanges)}


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--dataset',type=Path,required=True);p.add_argument('--output',type=Path,required=True)
    a=p.parse_args();print(json.dumps({'cacheManifestSha256':build(a.dataset,a.output),'newMarketData':False,'productionChanged':False}))
