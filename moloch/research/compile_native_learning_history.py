"""Extract development books from an exact pinned original native journal.

Held-out prices never enter this output. This verifies original bytes/chain,
not receiver-side economic replay or execution-model calibration. Financial
events are excluded from model observations; original files remain private.
"""
import argparse
from collections import Counter
from datetime import datetime
from decimal import Decimal
import gzip
import hashlib
import io
import json
from pathlib import Path
from .verify_learning_transport import source_hash


def sha(raw):return hashlib.sha256(raw).hexdigest()


def clock(value):
    dt=datetime.fromisoformat(value.replace('Z','+00:00'))
    if dt.utcoffset() is None:raise ValueError('EXPLICIT_SOURCE_CLOCK_REQUIRED')
    return int(dt.timestamp()*1000)


def amount(value):
    d=Decimal(str(value))*1_000_000
    if not d.is_finite()or d!=d.to_integral_value()or not 0<=d<=1_000_000:raise ValueError('EXACT_PROBABILITY_REQUIRED')
    return int(d)


def levels(rows,bids):
    out=[]
    for item in rows:
        n=Decimal(str(item['quantity']))
        if not n.is_finite()or n<=0 or n!=n.to_integral_value():raise ValueError('INTEGER_POSITIVE_NATIVE_DEPTH')
        out.append([amount(item['price']),int(n)])
    prices=[p for p,n in out]
    if prices!=sorted(set(prices),reverse=bids):raise ValueError('NATIVE_DEPTH_ORDER_OR_DUPLICATE')
    return out


def role_at(at,roles):
    found=[name for name,(lo,hi)in roles.items()if lo<=at<hi]
    if len(found)>1:raise ValueError('OVERLAPPING_CALENDAR_ROLES')
    return found[0]if found else None


def extract(event,row_hash,at):
    kind=event['type']
    if kind=='gap':return dict(kind='gap',venue='SIG',available_at_ms=at,market_id=event.get('marketId'),exchange_id=event.get('exchangeId'),original_sha256=row_hash)
    if kind!='book':return None
    b=event['book'];received=clock(b['receivedAt']);source=clock(b['sourceAt'])
    if received>at:raise ValueError('BOOK_AVAILABLE_BEFORE_ORIGINAL_RECEIPT')
    return dict(kind='book',venue='SIG',available_at_ms=at,received_at_ms=received,source_at_ms=source,
                source_clock_semantics='original_native_sourceAt',publisher_at_ms=None,market_id=str(b['marketId']),
                exchange_id=str(b['exchangeId']),sequence=b['sequence'],original_sha256=row_hash,
                bids=levels(b['bids'],True),asks=levels(b['asks'],False))


def compile_stream(stream,pin,protocol,emit):
    roles={name:tuple(map(clock,bounds))for name,bounds in protocol['calendarRolesUtc'].items()}
    digest=hashlib.sha256();size=0;sequence=0;previous='';last=-1;counts=Counter();types=Counter();markets=set();first=None
    while True:
        raw=stream.readline(16*1024*1024+1)
        if not raw:break
        if len(raw)>16*1024*1024 or not raw.endswith(b'\n'):raise ValueError('ORIGINAL_NATIVE_ROW_BOUND_OR_PARTIAL')
        size+=len(raw)
        if size>pin['bytes']:raise ValueError('ORIGINAL_PREFIX_GREW')
        digest.update(raw);row=json.loads(raw);source_hash(raw,row)
        if row['sequence']!=sequence+1 or row['previous']!=previous:raise ValueError('ORIGINAL_NATIVE_CHAIN')
        sequence=row['sequence'];previous=row['hash'];e=row['event'];received=clock(e['receivedAt']);at=clock(e.get('processedAt',e['receivedAt']))
        if at<received or at<last:raise ValueError('ORIGINAL_NATIVE_CLOCK_REGRESSION')
        first=at if first is None else first;last=at;role=role_at(at,roles);counts[role or 'gap_calendar']+=1;types[e['type']]+=1
        # Do not interpret or emit held-out book prices. Whole-row hash integrity
        # and timestamp/coverage checks above do not select or fit a model.
        if role not in ('fit','search'):continue
        observation=extract(e,row['hash'],at)
        if observation is not None:
            observation['calendar_role']=role;emit(observation)
            if observation['kind']=='book':markets.add(observation['exchange_id'])
    if size!=pin['bytes']or digest.hexdigest()!=pin['sha256']or sequence!=pin['terminalSequence']:raise ValueError('ORIGINAL_NATIVE_PREFIX_PIN')
    return dict(originalBytes=size,originalSha256=digest.hexdigest(),terminalSequence=sequence,terminalHash=previous,
                firstAvailableAtMs=first,lastAvailableAtMs=last,calendarRowCounts=dict(counts),nativeTypeCounts=dict(types),developmentExchanges=len(markets))


def compile_file(source,pin_path,protocol_path,output):
    pinraw=pin_path.read_bytes();pin=json.loads(pinraw);protocolraw=protocol_path.read_bytes();protocol=json.loads(protocolraw)
    if protocol['originalNativePrefixSha256']!=pin['sha256']or protocol['originalNativePrefixBytes']!=pin['bytes']:raise ValueError('PROTOCOL_SOURCE_PIN')
    output.mkdir(parents=True,exist_ok=False);counts=Counter();observations=output/'observations.jsonl.gz'
    import zstandard
    try:
        with source.open('rb')as compressed,zstandard.ZstdDecompressor().stream_reader(compressed)as decoder,io.BufferedReader(decoder)as reader,observations.open('xb')as outer,gzip.GzipFile(fileobj=outer,mode='wb',mtime=0,compresslevel=1)as out:
            def emit(row):
                counts[row['kind']]+=1;out.write((json.dumps(row,separators=(',',':'),allow_nan=False)+'\n').encode())
            verified=compile_stream(reader,pin,protocol,emit)
        manifest={'schemaVersion':1,'classification':'inspected_native_development_history_only','venuesPresent':['SIG'],
                  'originalPrefix':verified,'sourcePinSha256':sha(pinraw),'protocolSha256':sha(protocolraw),
                  'compilerSha256':sha(Path(__file__).read_bytes()),'observationsSha256':sha(observations.read_bytes()),
                  'observationCounts':dict(counts),'heldOutPricesIncluded':False,'lockedTestEligible':False,
                  'prefixSemanticReplayOnReceiver':False,'executionCalibrationAvailable':False,'productionChanged':False,'executionAuthority':False}
        raw=(json.dumps(manifest,indent=2)+'\n').encode();(output/'dataset-manifest.json').write_bytes(raw)
        (output/'COMPLETE.json').write_text(json.dumps({'manifestSha256':sha(raw)})+'\n',encoding='utf-8');return manifest
    except BaseException:
        (output/'FAILED').write_text('No COMPLETE or usable training dataset; preserve failed extraction evidence.\n',encoding='utf-8');raise


if __name__=='__main__':
    p=argparse.ArgumentParser()
    for flag in ['source','pin','protocol','output']:p.add_argument('--'+flag,type=Path,required=True)
    a=p.parse_args();print(json.dumps(compile_file(a.source,a.pin,a.protocol,a.output)),flush=True)
