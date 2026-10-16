"""Offline sealed original-input replay; fixed reviewed sampler only, no network."""
import argparse
import base64
import gzip
import hashlib
import types
import json
import os
from pathlib import Path
import re
import stat

MAX_COMPRESSED=2*1024**3
MAX_DECODED=32*1024**3
MAX_LINE=32*1024**2
MAX_RECORDS=10_000_000
MAX_BODY=16*1024**2
HISTORICAL_LF='200abfd0701f56f43183b1b7b60cd1cdf11525f0fb9daea72e9bf6f08d90bc40'
HISTORICAL_CRLF='1f63cf06d05b68e46d2febe57607125d9eb95d6e87db3f4d29b401d39d8e700b'
BOOK_VERSION_LF='2fa9b142b672f4a9076e185ac91be0b288945b0e1bba6afb0797594ed6678529'
# Capture producer hashes select a fixed reviewed version, NEVER a code path.
PRODUCER_VERSIONS={HISTORICAL_LF:'historical_200ab',HISTORICAL_CRLF:'historical_200ab',BOOK_VERSION_LF:'book_version_2fa9'}
KNOWN_PRODUCERS=set(PRODUCER_VERSIONS)
SAMPLERS={
    'historical_200ab':(
        'research/moloch-trade-trigger-20261016/witness/known-samplers/crossvenue_observer_200ab.py',
        frozenset({HISTORICAL_LF,HISTORICAL_CRLF})),
    'book_version_2fa9':(
        'moloch/external-data/crossvenue_observer.py',
        frozenset({BOOK_VERSION_LF,'4a2d585d9b2ce81add31cc2a6a8ba99fb6df6bcd019dbf167535e4e1910f3471'})),
}

class Rejected(ValueError): pass

def need(ok,code):
    if not ok:raise Rejected(code)

def compact(value):
    return json.dumps(value,sort_keys=True,separators=(',',':'),ensure_ascii=False,allow_nan=False)

def sha(raw):return hashlib.sha256(raw).hexdigest()

def integer(value):return type(value) is int and 0<=value<=9007199254740991

def loads(raw):
    def pairs(items):
        out={}
        for k,v in items:
            need(k not in out,'DUPLICATE_JSON_KEY');out[k]=v
        return out
    return json.loads(raw,object_pairs_hook=pairs,parse_constant=lambda _:need(False,'NONFINITE_JSON'))

def digest_file(path):
    path=Path(path); before=path.lstat()
    need(stat.S_ISREG(before.st_mode) and before.st_nlink==1,'INPUT_NOT_SINGLE_REGULAR_FILE')
    result=hashlib.sha256()
    with path.open('rb') as stream:
        info=os.fstat(stream.fileno())
        need((info.st_dev,info.st_ino)==(before.st_dev,before.st_ino),'INPUT_CHANGED')
        for chunk in iter(lambda:stream.read(1024**2),b''):result.update(chunk)
        after=os.fstat(stream.fileno())
    identity=lambda i:(i.st_dev,i.st_ino,i.st_size,i.st_mtime_ns)
    need(identity(before)==identity(after)==identity(path.lstat()),'INPUT_CHANGED')
    return result.hexdigest(),before.st_size,identity(before)

class Pinned:
    def __init__(self,ref):
        need(isinstance(ref,dict) and isinstance(ref.get('path'),str) and Path(ref['path']).is_absolute(),'ABSOLUTE_INPUT_REQUIRED')
        need(isinstance(ref.get('sha256'),str) and re.fullmatch('[0-9a-f]{64}',ref['sha256']) and integer(ref.get('bytes')),'INPUT_PIN')
        self.path=Path(ref['path']);self.sha,self.bytes,self.identity=digest_file(self.path)
        need((self.sha,self.bytes)==(ref['sha256'],ref['bytes']),'INPUT_HASH_OR_LENGTH')
    def recheck(self):
        need(digest_file(self.path)==(self.sha,self.bytes,self.identity),'INPUT_CHANGED')
    def small(self,maximum):
        need(self.bytes<=maximum,'SMALL_INPUT_LIMIT');return self.path.read_bytes()


def lines(pin):
    decoded=0
    with pin.path.open('rb') as raw,gzip.GzipFile(fileobj=raw,mode='rb') as stream:
        for count in range(MAX_RECORDS+1):
            line=stream.readline(MAX_LINE+1)
            if not line:break
            decoded+=len(line)
            need(count<MAX_RECORDS and len(line)<=MAX_LINE and decoded<=MAX_DECODED,'DECOMPRESS_BOUND')
            need(line.endswith(b'\n'),'INCOMPLETE_JSON_LINE')
            yield loads(line)


def known_sampler(producer_sha=HISTORICAL_LF):
    need(producer_sha in PRODUCER_VERSIONS,'UNREVIEWED_CAPTURE_PRODUCER')
    relative,expected=SAMPLERS[PRODUCER_VERSIONS[producer_sha]]
    path=Path(__file__).resolve().parents[3]/relative
    before=path.lstat()
    need(stat.S_ISREG(before.st_mode) and before.st_nlink==1 and before.st_size<=1024**2,'LOCAL_PRODUCER_BOUND')
    raw=path.read_bytes()
    after=path.lstat()
    identity=lambda m:(m.st_dev,m.st_ino,m.st_size,m.st_mtime_ns)
    need(identity(before)==identity(after) and len(raw)==before.st_size,'LOCAL_PRODUCER_CHANGED')
    # Only these exact reviewed LF/CRLF byte variants are accepted. No general
    # normalization, manifest-selected module or unrelated cached .pyc is used.
    need(sha(raw) in expected,'UNREVIEWED_LOCAL_PRODUCER')
    # Compile ONLY the exact checked fixed-repository bytes, never a manifest
    # path or a potentially unrelated cached .pyc file.
    module=types.ModuleType('_reviewed_trace_sampler');module.__file__=str(path)
    exec(compile(raw,str(path),'exec'),module.__dict__)
    return module

class Sources:
    def __init__(self,entries):
        need(isinstance(entries,list) and len(entries)<=1024,'SIG_SOURCE_BOUND')
        self.refs={};self.pins=[];self.current=None;self.offset=None;self.previous=None;self.sequence=None
        self.unread=[];self.consumed=0;self.visited=set()
        for entry in entries:
            need(isinstance(entry.get('recordedPath'),str) and entry['recordedPath'] not in self.refs,'SIG_SOURCE_CONFLICT')
            pin=Pinned(entry);self.pins.append(pin);self.refs[entry['recordedPath']]=(entry,pin)
    def bookmark(self,b):
        need(isinstance(b,dict) and b.get('historyPrefixVerified') is False and b.get('path') in self.refs,'SIG_BOOKMARK_SOURCE')
        entry,pin=self.refs[b['path']]
        need(b.get('device')==entry.get('recordedDevice') and b.get('inode')==entry.get('recordedInode'),'SIG_BOOKMARK_IDENTITY')
        need(all(integer(b.get(k)) for k in ['fileBytesAtObservation','readOffset','bufferedBytes']),'SIG_BOOKMARK_CLOCK_OR_OFFSET')
        need(b['readOffset']<=b['fileBytesAtObservation']<=pin.bytes and b['bufferedBytes']<=b['readOffset'],'SIG_BOOKMARK_BOUND')
        need(type(b.get('skipPartialLine')) is bool,'SIG_BOOKMARK_SKIP')
        return pin
    def start(self,b):
        self.bookmark(b)
        need(b['bufferedBytes']==0 and b.get('lastSequence') is None and b.get('lastHash') is None and b.get('lastConsumedEndOffset') is None,'SIG_INITIAL_BOOKMARK')
        # Tail pins its initial EOF at open, then bookmark fstats again. Genuine
        # appends between those observations do not move the original cursor.
        need(b['readOffset']<=b['fileBytesAtObservation'],'SIG_INITIAL_OFFSET')
        pin=self.refs[b['path']][1]
        with pin.path.open('rb') as stream:
            if b['readOffset']:
                stream.seek(b['readOffset']-1); need(b['skipPartialLine'] is (stream.read(1)!=b'\n'),'SIG_INITIAL_PARTIAL_FLAG')
            else:need(b['skipPartialLine'] is False,'SIG_INITIAL_PARTIAL_FLAG')
        self.visited.add(b['path'])
        self.current=b;self.offset=b['readOffset'];self.previous=self.sequence=None
        self.skip_partial=b['skipPartialLine']
    def check_end(self,b):
        self.bookmark(b)
        need(self.current is not None and b['path']==self.current['path'],'SIG_SEGMENT_ORDER')
        need(b['readOffset']>=max(self.offset,self.current['readOffset']) and b['fileBytesAtObservation']>=self.current['fileBytesAtObservation'] and b['bufferedBytes']<=12*1024**2,'SIG_BOOKMARK_REGRESSION')
        need(b.get('lastHash')==self.previous and b.get('lastSequence')==self.sequence,'SIG_BOOKMARK_FRONTIER')
        expected=self.offset if self.sequence is not None else None
        need(b.get('lastConsumedEndOffset')==expected,'SIG_BOOKMARK_FRONTIER')
        if b['fileBytesAtObservation']>self.offset:
            self.unread.append({'sourceSha256':self.refs[b['path']][1].sha,'unconsumedBytesAtObservation':b['fileBytesAtObservation']-self.offset})
    def rotate(self,event):
        self.check_end(event['previousBookmark']);b=event['nextBookmark'];self.bookmark(b)
        need(b['path'] not in self.visited and b['readOffset']==0 and b['bufferedBytes']==0 and b['skipPartialLine'] is False and b.get('lastHash') is None and b.get('lastSequence') is None and b.get('lastConsumedEndOffset') is None,'SIG_ROTATION_BOUNDARY')
        need(event.get('crossSegmentContinuityVerified') is False,'SIG_ROTATION_AUTHORITY')
        self.visited.add(b['path']);self.current=b;self.offset=0;self.previous=self.sequence=None;self.skip_partial=False
    def consume(self,ref):
        b=ref['bookmark'];pin=self.bookmark(b)
        need(self.current is not None and b['path']==self.current['path'],'SIG_SEGMENT_ORDER')
        need(b['readOffset']>=self.current['readOffset'] and b['fileBytesAtObservation']>=self.current['fileBytesAtObservation'] and b['skipPartialLine'] is False and b['bufferedBytes']<=12*1024**2,'SIG_BOOKMARK_REGRESSION')
        if self.skip_partial:
            with pin.path.open('rb') as stream:
                stream.seek(self.offset); skipped=stream.readline(8*1024**2+1)
            need(skipped.endswith(b'\n') and len(skipped)<=8*1024**2,'SIG_INITIAL_PARTIAL_UNAVAILABLE')
            self.offset+=len(skipped);self.skip_partial=False
        need(integer(ref.get('offset')) and integer(ref.get('bytes')) and 0<ref['bytes']<=8*1024**2+1,'SIG_LINE_BOUND')
        need(ref['offset']==self.offset and ref['offset']+ref['bytes']<=b['readOffset'],'SIG_CONSUMPTION_GAP')
        with pin.path.open('rb') as stream:
            stream.seek(ref['offset']);raw=stream.read(ref['bytes'])
        need(len(raw)==ref['bytes'] and raw.endswith(b'\n') and raw.count(b'\n')==1 and sha(raw)==ref.get('rawLineSha256'),'SIG_RAW_MEMBERSHIP')
        row=loads(raw)
        need(list(row)==['event','hash','previous','sequence'] and re.fullmatch('[0-9a-f]{64}',row.get('hash','')),'SIG_ROW_FIELDS')
        field=b',"hash":"'+row['hash'].encode()+b'"';position=raw.rfind(field)
        need(position>=0 and sha(raw[:position]+raw[position+len(field):-1])==row['hash'],'SIG_ROW_HASH')
        need(integer(row['sequence']) and row['sequence']>=1 and isinstance(row['previous'],str) and (row['previous']=='' or re.fullmatch('[0-9a-f]{64}',row['previous'])),'SIG_ROW_FIELDS')
        need(integer(ref.get('sequence')),'SIG_REFERENCE_SEQUENCE')
        need(all(row[k]==ref.get(k) for k in ['sequence','hash','previous']) and row['event'].get('type')==ref.get('originalEventType'),'SIG_REFERENCE_CONFLICT')
        if self.previous is not None:need(row['previous']==self.previous and row['sequence']==self.sequence+1,'SIG_CHAIN_GAP')
        need(b.get('lastHash')==row['hash'] and b.get('lastSequence')==row['sequence'] and b.get('lastConsumedEndOffset')==self.offset+len(raw),'SIG_BOOKMARK_FRONTIER')
        self.offset+=len(raw);self.previous=row['hash'];self.sequence=row['sequence'];self.current=b;self.consumed+=1
        return row


def verify(manifest_path,manifest_sha256):
    need(re.fullmatch('[0-9a-f]{64}',manifest_sha256 or '') is not None,'MANIFEST_HASH_REQUIRED')
    manifest_pin=Pinned({'path':str(Path(manifest_path).resolve()),'sha256':manifest_sha256,'bytes':Path(manifest_path).stat().st_size})
    manifest=loads(manifest_pin.small(1024**2));need(type(manifest.get('version')) is int and manifest['version']==1,'MANIFEST_VERSION')
    pins={name:Pinned(manifest[name]) for name in ['seal','trace','samples','producer','mapping']}
    need(pins['producer'].sha in KNOWN_PRODUCERS,'UNREVIEWED_CAPTURE_PRODUCER')
    need(pins['seal'].bytes+pins['trace'].bytes+pins['samples'].bytes<=MAX_COMPRESSED,'COMPRESSED_BOUND')
    seal=loads(pins['seal'].small(65536))
    need(seal.get('kind')=='input_trace_terminal_seal' and type(seal.get('version')) is int and seal['version']==1 and seal.get('complete') is True and seal.get('reason')=='normal_stop','INCOMPLETE_SEAL')
    need(integer(seal.get('outputRows')) and integer(seal.get('traceSequence')) and seal['traceSequence']>=1,'SEAL_COUNTS')
    need(type(seal.get('pendingInputs')) is int and seal['pendingInputs']==0 and seal.get('storedBytesMatchWrittenMembers') is True and seal.get('pinnedReferencesUnchanged') is True,'UNSEALED_INPUTS')
    need(seal.get('historyPrefixVerified') is False and seal.get('wholeWitnessVerified') is False and seal.get('executionAuthority') is False,'SEAL_AUTHORITY')
    file_refs=seal.get('files');need(isinstance(file_refs,list) and len(file_refs)==2,'SEAL_FILES')
    for name,filename in [('trace','input-trace-v1.jsonl.gz'),('samples','trace-samples-v1.jsonl.gz')]:
        refs=[r for r in file_refs if str(r.get('path','')).replace('\\','/').rsplit('/',1)[-1]==filename]
        need(len(refs)==1 and (refs[0].get('sha256'),refs[0].get('bytes'))==(pins[name].sha,pins[name].bytes),'SEAL_FILE_PIN')
    for name,key in [('producer','producer'),('mapping','mappingFileReference')]:
        need((seal[key].get('sha256'),seal[key].get('bytes'))==(pins[name].sha,pins[name].bytes),'SEAL_REFERENCE_PIN')
    module=known_sampler(pins['producer'].sha);sources=Sources(manifest.get('sigSources'))
    expected=iter(lines(pins['samples']));outputs=0;horizon_labels=0;counts={'polls':0,'failedPolls':0,'cacheApplies':0,'sigApplies':0,'rotations':0}
    def emit(value):
        nonlocal outputs,horizon_labels
        sentinel=object();original=next(expected,sentinel)
        need(original is not sentinel and compact(original)==compact(value),'EMITTED_ROW_MISMATCH');outputs+=1
        if value.get('kind')=='horizon_label':horizon_labels+=1
    previous='';sequence=0;study=None;poll=None;apply=None;next_route='/v1/quotes';last_now=None;run=None;capture_clock=None
    for record in lines(pins['trace']):
        need(set(record)=={'sequence','previous','event','hash'} and integer(record.get('sequence')) and record['sequence']==sequence+1 and record.get('previous')==previous,'TRACE_CHAIN')
        need(record['hash']==sha(compact({k:v for k,v in record.items() if k!='hash'}).encode()),'TRACE_HASH')
        sequence=record['sequence'];previous=record['hash'];e=record['event'];kind=e.get('kind')
        clock_key={'run_start':'traceStartedAtMs','poll_begin':'requestStartedAtMs','poll_body':'readCompletedAtMs',
            'apply_begin':'recordedAtMs','apply_complete':'completedAtMs','poll_complete':'completedAtMs',
            'poll_failed':'completedAtMs','sig_rotation':'observedAtMs'}.get(kind)
        need(clock_key is not None and integer(e.get(clock_key)) and (capture_clock is None or e[clock_key]>=capture_clock),'CAPTURE_CLOCK_OR_EVENT')
        prior_capture=capture_clock;capture_clock=e[clock_key]
        if kind=='run_start':
            need(sequence==1 and study is None,'RUN_START_ORDER');run=e;config=e['effectiveSamplerConfig']
            need(e.get('protocol')==module.INPUT_TRACE_PROTOCOL and e.get('historyPrefixVerified') is False and e.get('executionAuthority') is False,'RUN_PROTOCOL')
            need(e.get('producer')==seal['producer'] and e.get('mappingFileReference')==seal['mappingFileReference'],'RUN_REFERENCE_PIN')
            need(e.get('effectiveCacheMappingsWitnessedBy')=='original_cache_response_bodies','MAPPING_WITNESS')
            need(sha(compact(config).encode())==e.get('effectiveSamplerConfigSha256'),'CONFIG_HASH')
            need(set(config)=={'samplingMode','format','journal','durationSeconds','horizonsMs','sigMaximumAgeMs','externalMaximumAgeMs','labelSlackMs'} and isinstance(config.get('journal'),str) and integer(config.get('durationSeconds')),'CONFIG_FIELDS')
            need(config.get('samplingMode') in ('book','trade') and config.get('format') in ('native','breadth'),'CONFIG_MODE')
            need(config.get('horizonsMs')==[1000,3000,10000,30000,120000] and config.get('sigMaximumAgeMs')==3000 and config.get('externalMaximumAgeMs')==20000 and config.get('labelSlackMs')==3000,'CONFIG_BOUNDS')
            need(e.get('samplingPolicy')==(module.TRADE_SAMPLING_POLICY if config['samplingMode']=='trade' else None) and e.get('samplingPolicySha256')==(module.TRADE_SAMPLING_POLICY_SHA256 if config['samplingMode']=='trade' else None),'SAMPLING_POLICY')
            sources.start(e['initialBookmark']);study=module.Study(emit,sampling_mode=config['samplingMode'])
        elif kind=='poll_begin':
            need(study is not None and poll is None and apply is None and e.get('route')==next_route and integer(e.get('requestStartedAtMs')),'POLL_ORDER')
            poll={'sequence':sequence,'route':e['route'],'request':e['requestStartedAtMs'],'body':None,'applied':False};counts['polls']+=1
        elif kind=='poll_body':
            need(poll is not None and apply is None and poll['body'] is None and not poll['applied'] and e.get('beginSequence')==poll['sequence'] and e.get('route')==poll['route'],'POLL_BODY_ORDER')
            need(e.get('bodyComplete') is True and integer(e.get('bodyBytes')) and e['bodyBytes']<=MAX_BODY and e.get('readBytes')==e['bodyBytes'],'CACHE_BODY_INCOMPLETE')
            need(isinstance(e.get('bodyBytesBase64'),str) and len(e['bodyBytesBase64'])<=(MAX_BODY+2)//3*4,'CACHE_BODY_BOUND')
            raw=base64.b64decode(e['bodyBytesBase64'],validate=True)
            need(len(raw)==e['bodyBytes'] and sha(raw)==e.get('bodySha256'),'CACHE_BODY_HASH')
            need(integer(e.get('readCompletedAtMs')) and e['readCompletedAtMs']>=poll['request'] and e.get('status') in (200,None),'CACHE_READ_CLOCK_OR_STATUS')
            poll['body']=(sequence,raw,e['readCompletedAtMs']);del raw
        elif kind=='apply_begin':
            need(study is not None and apply is None and integer(e.get('samplerNowMs')) and integer(e.get('recordedAtMs')) and e['recordedAtMs']>=e['samplerNowMs'],'APPLY_CLOCK_OR_ORDER')
            now=e['samplerNowMs'];need(prior_capture is None or now>=prior_capture,'SAMPLER_PRECEDES_CAPTURE');need(last_now is None or now>=last_now,'SAMPLER_CLOCK_REVERSED');last_now=now
            before=outputs;ref=e['input']
            if e.get('inputKind')=='cache_body':
                need(poll is not None and poll['body'] is not None and not poll['applied'] and ref=={'bodySequence':poll['body'][0],'route':poll['route']} and now>=poll['body'][2],'CACHE_APPLY_BINDING')
                study.external_update(loads(poll['body'][1]),now);poll['body']=None;poll['applied']=True;counts['cacheApplies']+=1
            elif e.get('inputKind')=='sig_row':
                need(poll is None and next_route=='/v1/quotes','SIG_POLL_INTERLEAVE')
                row=sources.consume(ref);(study.breadth if run['effectiveSamplerConfig']['format']=='breadth' else study.accept)(row,now);counts['sigApplies']+=1
            else:raise Rejected('UNKNOWN_INPUT_KIND')
            need(not study.trigger_disabled,'SAMPLER_TRIGGER_DISABLED')
            apply={'sequence':sequence,'before':before,'after':outputs,'now':now}
        elif kind=='apply_complete':
            need(apply is not None and e.get('beginSequence')==apply['sequence'] and e.get('emittedRowsBefore')==apply['before'] and e.get('emittedRowsAfter')==apply['after'] and integer(e.get('completedAtMs')) and e['completedAtMs']>=apply['now'],'APPLY_COMPLETION')
            apply=None
        elif kind=='poll_complete':
            need(poll is not None and apply is None and poll['applied'] and e.get('beginSequence')==poll['sequence'] and integer(e.get('completedAtMs')) and e['completedAtMs']>=last_now,'POLL_COMPLETION')
            next_route='/v1/ws/quotes' if poll['route']=='/v1/quotes' else '/v1/quotes';poll=None
        elif kind=='poll_failed':
            need(poll is not None and apply is None and poll['body'] is None and not poll['applied'] and e.get('beginSequence')==poll['sequence'] and e.get('failureKind')=='transport' and e.get('bodyAvailable') is False and e.get('errorTextRetained') is False,'FAILED_POLL_PROOF')
            need(integer(e.get('completedAtMs')) and e['completedAtMs']>=poll['request'],'FAILED_POLL_CLOCK');counts['failedPolls']+=1;poll=None;next_route='/v1/quotes'
        elif kind=='sig_rotation':
            need(study is not None and poll is None and apply is None and next_route=='/v1/quotes' and run['effectiveSamplerConfig']['format']=='breadth','ROTATION_ORDER')
            sources.rotate(e);counts['rotations']+=1
        else:raise Rejected('UNKNOWN_OR_FAILED_TRACE_EVENT')
    need(study is not None and poll is None and apply is None and next_route=='/v1/quotes','PENDING_TRACE_INPUT')
    need((sequence,previous,outputs)==(seal.get('traceSequence'),seal.get('traceHead'),seal.get('outputRows')),'SEAL_FRONTIER')
    sentinel=object();need(next(expected,sentinel) is sentinel,'EXTRA_SEALED_OUTPUT')
    need(integer(seal.get('sealedAtMs')) and seal['sealedAtMs']>=capture_clock,'SEAL_CLOCK')
    sources.check_end(seal['finalBookmark'])
    for pin in [manifest_pin,*pins.values(),*sources.pins]:pin.recheck()
    return {'version':1,'samplerInputReplayVerified':True,'originalSigMembershipVerified':sources.consumed>0,
        'firstConsumedBookReplayVerified':horizon_labels>0,
        'firstConsumedBookProof':'verified' if horizon_labels else 'unavailable_no_horizon_label',
        'horizonLabels':horizon_labels,'originalCacheBodyMembershipVerified':counts['cacheApplies']>0,
        'wholeWitnessVerified':False,'eligibleForModelValidation':False,'executionAuthority':False,
        'historyPrefixVerified':False,'providerTradeTapeComplete':False,
        'collectorProcessingEpochProof':'unavailable','tournamentTopicBindingProof':'unavailable',
        'resolutionEquivalenceVerified':False,'crossSegmentContinuityVerified':False,
        'unconsumedSourceObservations':sources.unread,'traceRecords':sequence,'outputRows':outputs,
        'samplingMode':run['effectiveSamplerConfig']['samplingMode'],'counts':counts,
        'manifestSha256':manifest_sha256,'producerSha256':pins['producer'].sha}


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('manifest',type=Path);parser.add_argument('--manifest-sha256',required=True)
    args=parser.parse_args()
    print(compact(verify(args.manifest,args.manifest_sha256)))

if __name__=='__main__':main()
