"""Pin existing three-venue files and emit only frozen fit/search observations.

No network, credentials, account client, synthetic fills or held-out features.
Individual response bytes are checked even for a bounded active gzip prefix.
"""
import argparse
import base64
from collections import Counter,defaultdict
import gzip
import hashlib
import json
import os
from pathlib import Path
import stat
import time
from .capture_native_stream import NativeVerifier
from .reference_adapters import millis,kalshi_references,polymarket_references
from moloch.research.compile_native_learning_history import extract,role_at


def sha(raw):return hashlib.sha256(raw).hexdigest()


def pin_prefix(source,destination):
    before=source.lstat()
    if not stat.S_ISREG(before.st_mode)or source.is_symlink()or before.st_nlink!=1:raise ValueError('REGULAR_ORIGINAL_SOURCE_REQUIRED')
    size=before.st_size;digest=hashlib.sha256()
    with source.open('rb')as src,destination.open('xb')as out:
        opened=os.fstat(src.fileno())
        if (before.st_dev,before.st_ino)!=(opened.st_dev,opened.st_ino):raise ValueError('ORIGINAL_SOURCE_REPLACED')
        left=size
        while left:
            raw=src.read(min(left,1024*1024))
            if not raw:raise ValueError('ORIGINAL_PREFIX_TRUNCATED')
            out.write(raw);digest.update(raw);left-=len(raw)
        after=source.lstat()
        if (before.st_dev,before.st_ino)!=(after.st_dev,after.st_ino)or after.st_size<size:raise ValueError('ORIGINAL_SOURCE_CHANGED')
        src.seek(0);again=hashlib.sha256();left=size
        while left:
            raw=src.read(min(left,1024*1024))
            if not raw:raise ValueError('ORIGINAL_PREFIX_TRUNCATED')
            again.update(raw);left-=len(raw)
        if again.digest()!=digest.digest():raise ValueError('ORIGINAL_PREFIX_BYTES_CHANGED')
    return {'source':str(source),'copiedPrefixBytes':size,'sha256':digest.hexdigest(),'device':before.st_dev,'inode':before.st_ino}


def native_rows(path,roles,emit):
    verifier=NativeVerifier();previous='0'*64;sequence=0;last=-1;tail=0
    with path.open('rb')as stream:
        while raw:=stream.readline(24*1024*1024+1):
            if len(raw)>24*1024*1024:raise ValueError('NATIVE_RECEIVER_ROW_BOUND')
            if not raw.endswith(b'\n'):tail=len(raw);break
            row=json.loads(raw);claimed=row.pop('hash')
            if sha(json.dumps(row,sort_keys=True,separators=(',',':')).encode())!=claimed or row['sequence']!=sequence+1 or row['previous']!=previous:raise ValueError('NATIVE_RECEIVER_CHAIN')
            sequence=row['sequence'];previous=claimed;at=row['observed_at_ms']
            if at<last:raise ValueError('NATIVE_RECEIVER_CLOCK_REGRESSION')
            last=at;event=row['event'];verifier.accept(event)
            if event['kind']!='native_row':continue
            original=json.loads(base64.b64decode(event['raw_base64'],validate=True));e=original['event']
            available=max(at,millis(e.get('processedAt',e['receivedAt'])))
            role=role_at(available,roles)
            if role not in ('fit','search'):continue
            observation=extract(e,original['hash'],available)
            if observation is not None:
                if observation.get('source_at_ms')is not None and observation['source_at_ms']>available:raise ValueError('NATIVE_FUTURE_SOURCE_CLOCK')
                observation.update(calendar_role=role,receiver_observed_at_ms=at,receiver_row_hash=claimed)
                emit(observation)
    if verifier.anchor is None:raise ValueError('NATIVE_PRODUCER_ANCHOR_MISSING')
    return {'receiverRowsVerified':sequence,'nativeRowsVerified':verifier.rows,'anchor':verifier.anchor,
            'terminalReceiverHash':previous,'excludedIncompleteTailBytes':tail,'prefixSemanticReplayOnReceiver':False}


def gzip_rows(path,allow_incomplete):
    try:
        with gzip.open(path,'rb')as src:
            while raw:=src.readline(16*1024*1024+1):
                if len(raw)>16*1024*1024:raise ValueError('EXTERNAL_ORIGINAL_ROW_BOUND')
                if not raw.endswith(b'\n'):
                    if allow_incomplete:return
                    raise ValueError('EXTERNAL_PARTIAL_CLOSED_ROW')
                yield raw
    except EOFError:
        if not allow_incomplete:raise


def compile_development(native,kalshi,polys,protocol_path,source_pin_path,output):
    protocol_raw=protocol_path.read_bytes();protocol=json.loads(protocol_raw);source_pin_raw=source_pin_path.read_bytes();source_pin=json.loads(source_pin_raw)
    if source_pin['protocolSha256']!=sha(protocol_raw):raise ValueError('FROZEN_PROTOCOL_SOURCE_PIN')
    roles={k:tuple(map(millis,v))for k,v in protocol['calendarRolesUtc'].items()}
    if time.time_ns()//1000000<roles['search'][1]:raise ValueError('SEARCH_CAPTURE_NOT_COMPLETE_YET')
    ku=(kalshi/'universe.json').read_bytes();pu=Path(source_pin['polymarketUniversePath']).read_bytes()
    if sha(ku)!=source_pin['kalshiUniverseSha256']or sha(pu)!=source_pin['polymarketUniverseSha256']:raise ValueError('FROZEN_REFERENCE_MAPPING_CHANGED')
    kuniverse=json.loads(ku);km={m['ticker']:m for m in kuniverse['markets']};analogs={m['ticker']for m in kuniverse.get('candidateAnalogMarkets',[])}
    pm={m['assetId']:m for m in json.loads(pu)['outcomes']}
    output.mkdir(parents=True,exist_ok=False);originals=output/'originals';originals.mkdir();observations=[];pins=[];counts=Counter();exchanges=set();markets=defaultdict(set)
    for name,raw in [('protocol.original.json',protocol_raw),('reference-source-pin.original.json',source_pin_raw),('kalshi-universe.original.json',ku),('polymarket-universe.original.json',pu)]:
        (originals/name).write_bytes(raw)
    def emit(row):
        observations.append(row);counts[(row['calendar_role'],row['venue'],row['kind'])]+=1
        if row['calendar_role']=='fit':
            if row['kind']=='book':exchanges.add(row['exchange_id'])
            elif row['kind']=='reference':markets[row['venue']].add(row['market_id'])
    try:
        n=originals/'native.original.jsonl';pins.append(pin_prefix(native,n));nv=native_rows(n,roles,emit)
        anchor=nv['anchor'];expected=protocol['nativeProducer']
        if anchor['producer_pid']!=expected['pid']or anchor['producer_start_ticks']!=expected['startTicks']:raise ValueError('EXACT_NATIVE_PRODUCER_REQUIRED')
        first_hour=protocol['calendarRolesUtc']['fit'][0][:13].replace('-','').replace(':','');last_hour=protocol['calendarRolesUtc']['search'][1][:13].replace('-','').replace(':','')
        groups=[('Kalshi',sorted(p for p in(kalshi/'journal').glob('*.ndjson.gz')if first_hour<=p.name[:11]<=last_hour))]
        for root in polys:
            if not root.exists():continue
            groups.append(('Polymarket',sorted((p for p in(root/'journal').glob('*.ndjson.gz')if p.stat().st_mtime_ns//1_000_000>=roles['fit'][0]),key=lambda p:(p.stat().st_mtime_ns,p.name))))
        for venue,files in groups:
            for index,p in enumerate(files):
                dest=originals/f'{len(pins):04d}-{p.name}';pin=pin_prefix(p,dest);pin.update(venue=venue,activeGzipPrefixAllowed=index==len(files)-1);pins.append(pin)
                for raw in gzip_rows(dest,index==len(files)-1):
                    row=json.loads(raw);r=row.get('receipt',row)
                    if not isinstance(r,dict)or 'receivedAt'not in r or 'bodyText'not in r:continue
                    available=max(millis(r['receivedAt']),millis(row['firstAvailableAt'])if 'firstAvailableAt'in row else 0);role=role_at(available,roles)
                    if role not in ('fit','search'):continue
                    if venue=='Kalshi':
                        mapping=km.get(row.get('ticker'))
                        if mapping is None or not mapping.get('sigMappings')or row.get('ticker')in analogs:continue
                        rows=kalshi_references(raw,mapping['sigMappings'],available,mapping_kind='direct_party_reference')
                    else:
                        identity=row.get('identity')
                        if not identity:continue
                        if pm.get(identity.get('assetId'))!=identity:raise ValueError('ORIGINAL_POLYMARKET_MAPPING_IDENTITY_CHANGED')
                        rows=polymarket_references(raw,available,invalid_market_data='gap')
                    for item in rows:item['calendar_role']=role;emit(item)
        (output/'coverage-counts.json').write_text(json.dumps({'observationCounts':{'/'.join(k):v for k,v in counts.items()},
            'fitSigExchanges':len(exchanges),'fitReferenceMarkets':{v:len(m)for v,m in markets.items()},
            'allCoverageChecksPassed':False,'classification':'counts_before_temporal_coverage_validation_not_completed_dataset'},indent=2)+'\n')
        if len(exchanges)<protocol['minimumFitObservedSigExchanges']or any(len(markets[v])<protocol['minimumFitMappedReferenceMarketsPerExternalVenue']for v in ['Kalshi','Polymarket']):raise ValueError('REQUIRED_FORWARD_SOURCE_COVERAGE_FAILED')
        gap=source_pin['maximumBoundaryOrObservationGapMs']
        if type(gap)is not int or gap<=0:raise ValueError('EXPLICIT_POSITIVE_COVERAGE_GAP')
        for role in ('fit','search'):
            lo,hi=roles[role]
            for venue in ('SIG','Kalshi','Polymarket'):
                times=sorted(r['available_at_ms']for r in observations if r['calendar_role']==role and r['venue']==venue and r['kind']in ('book','reference'))
                if not times or max(b-a for a,b in zip([lo]+times,times+[hi]))>gap:raise ValueError('FORWARD_CALENDAR_COVERAGE_GAP:'+role+':'+venue)
        observations.sort(key=lambda r:r['available_at_ms']);target=output/'observations.jsonl.gz'
        with target.open('xb')as f,gzip.GzipFile(fileobj=f,mode='wb',mtime=0,compresslevel=1)as out:
            for row in observations:out.write((json.dumps(row,separators=(',',':'),allow_nan=False)+'\n').encode())
        manifest={'classification':'frozen_three_venue_development_prefix_not_locked_test','protocolSha256':sha(protocol_raw),
                  'referenceSourcePinSha256':sha(source_pin_raw),'kalshiUniverseSha256':sha(ku),'polymarketUniverseSha256':sha(pu),
                  'observationsSha256':sha(target.read_bytes()),'observationCounts':{'/'.join(k):v for k,v in counts.items()},
                  'fitSigExchanges':len(exchanges),'fitReferenceMarkets':{v:len(m)for v,m in markets.items()},'originalPrefixes':pins,
                  'nativeTransportVerification':nv,'heldoutFeaturesIncluded':False,'referenceContractEquivalenceVerified':False,
                  'nativeProducerBinaryIndependentlyVerifiedByCompiler':False,'developmentReceiptBodyHashesVerified':True,
                  'executionCalibrated':False,'accountRequests':0,'productionAuthority':False}
        raw=(json.dumps(manifest,indent=2)+'\n').encode();(output/'dataset-manifest.json').write_bytes(raw);(output/'COMPLETE.json').write_text(json.dumps({'manifestSha256':sha(raw)})+'\n');return manifest
    except BaseException:
        (output/'FAILED').write_text('No completed dataset; preserve pinned originals and diagnostics.\n');raise


if __name__=='__main__':
    p=argparse.ArgumentParser()
    for name in ['native','kalshi','protocol','source-pin','output']:p.add_argument('--'+name,type=Path,required=True)
    p.add_argument('--polymarket',type=Path,action='append',required=True);a=p.parse_args()
    result=compile_development(a.native,a.kalshi,a.polymarket,a.protocol,a.source_pin,a.output)
    print(json.dumps({k:result[k]for k in ['classification','fitSigExchanges','fitReferenceMarkets','heldoutFeaturesIncluded','accountRequests']}))
