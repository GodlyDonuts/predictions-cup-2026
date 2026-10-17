"""Frozen five-minute original-book clock comparison; offline and read-only."""
import argparse
from collections import Counter
from decimal import Decimal
import hashlib
import importlib.util
import io
import json
import math
from pathlib import Path, PurePosixPath
import re
import sys
import zipfile

HERE=Path(__file__).resolve().parent
ROOT=HERE.parents[1]
NS_PER_MS=1000000
WINDOW_NS=300000*NS_PER_MS
AGE_NS=10000*NS_PER_MS
PROTOCOL_LF_SHA='b6a8db505e27ae0bbbff2e18d281d8f8ff8d06877cb497d8f5bdddd76c5d3bfd'

def need(ok,code):
    if not ok:raise ValueError('BOOK_CLOCK_'+code)
def sha(raw):return hashlib.sha256(raw).hexdigest()
def module(path,expected,name):
    raw=path.read_bytes();need(sha(raw.replace(b'\r\n',b'\n'))==expected,'CHECKED_READER_CHANGED')
    spec=importlib.util.spec_from_file_location(name,path);out=importlib.util.module_from_spec(spec);sys.modules[name]=out
    exec(compile(raw,str(path),'exec'),out.__dict__);return out
V=module(ROOT/'research/moloch-profit-exit-timing-20261017/verify-original-evidence.py','9c9524c43350a471d31cf515f55b0deb5f9169bc8e525b3c769780ac34539c44','_clock_raw_seals')
N=module(ROOT/'research/moloch-owned-fill-markouts-20261015/analyze.py','f1db3b56bf701a0ded0c84486c04f805122deb0342fe196af17113e566e4ba39','_clock_native_book')

def file_sha(path):
    h=hashlib.sha256()
    with path.open('rb') as f:
        for part in iter(lambda:f.read(1024*1024),b''):h.update(part)
    return h.hexdigest()
class Packet:
    def __init__(self,path,expected):
        self.path=Path(path).absolute();need(self.path.is_file() and not self.path.is_symlink(),'PACKET_FILE')
        self.expected=expected;need(re.fullmatch('[0-9a-f]{64}',expected) is not None and self.path.stat().st_size<=2*1024**3,'PACKET_BOUND')
        need(file_sha(self.path)==expected,'ZIP_PIN');self.z=zipfile.ZipFile(self.path);infos=self.z.infolist()
        need(0<len(infos)<=128 and len({x.filename for x in infos})==len(infos),'ZIP_MEMBERS')
        self.names={i.filename:i for i in infos if not i.is_dir()}
        for i in infos:
            name=PurePosixPath(i.filename);need(not name.is_absolute() and '..' not in name.parts and '\\' not in i.filename and not i.flag_bits&1,'ZIP_MEMBER_PATH')
        need(sum(i.file_size for i in infos)<=4*1024**3,'ZIP_DECODED_BOUND')
        manifests=[x for x in self.names if PurePosixPath(x).name in ('packet-manifest.json','manifest.json')];need(len(manifests)==1,'PACKET_MANIFEST')
        self.manifest_name=manifests[0];raw=self.small(self.manifest_name,1024**2);value=V.load(raw);self.manifest_sha=sha(raw);pins=value.get('files',value)
        need(isinstance(pins,dict) and pins,'MEMBER_PINS');self.pins=pins
        need(set(self.names)==set(pins)|{self.manifest_name},'UNPINNED_MEMBER')
        for name,pin in pins.items():
            need(isinstance(pin,dict) and type(pin.get('bytes')) is int and 0<=pin['bytes']<=2*1024**3 and re.fullmatch('[0-9a-f]{64}',pin.get('sha256','')) is not None,'MEMBER_PIN_FORMAT')
            need(name in self.names and self.names[name].file_size==pin['bytes'],'MEMBER_SIZE');h=hashlib.sha256();n=0
            with self.z.open(name) as f:
                for block in iter(lambda:f.read(1024*1024),b''):h.update(block);n+=len(block)
            need(n==pin['bytes'] and h.hexdigest()==pin['sha256'],'MEMBER_RAW_HASH')
    def name(self,base):
        names=[n for n in self.names if PurePosixPath(n).name==base];need(len(names)==1,'MISSING_OR_AMBIGUOUS_'+base);return names[0]
    def small(self,name,maximum=64*1024**2):
        need(self.names[name].file_size<=maximum,'SMALL_MEMBER_BOUND');return self.z.read(name)
    def json(self,base):return V.load(self.small(self.name(base)))
    def sealed(self,base):return V.sealed(self.small(self.name(base)))[0]
    def original_pin(self,base):
        name=self.name(base);return {'member':name,**self.pins[name]}
    def finish(self):need(file_sha(self.path)==self.expected,'ZIP_CHANGED');self.z.close()

def progress(packet,base,pid):
    raw=packet.json(base);need(raw.get('pid')==pid and raw.get('phase')=='trading' and raw.get('realtimeHealthy') is True,'FIRST_FRESH_PROCESS')
    start=N.instant(raw['at']);return start,raw['at']
def ms(value):return format(Decimal(value)/NS_PER_MS,'f')
def distribution(values):
    if not values:return {'count':0,'medianMs':None,'p90Ms':None,'maxMs':None}
    v=sorted(values);n=len(v);middle=Decimal(v[n//2]) if n%2 else Decimal(v[n//2-1]+v[n//2])/2
    return {'count':n,'medianMs':ms(middle),'p90Ms':ms(v[math.ceil(n*.9)-1]),'maxMs':ms(v[-1])}
class Metrics:
    def __init__(self):
        self.counts=Counter({k:0 for k in ('bookEvents','missingOrUnparseableBookFields','missingOrInvalidBook','explicitNativeInvalidBook','invalidTemporalClockBooks','originalClockMismatch','receivedAfterProcessed','sourceAfterReceipt','sourceAfterProcessed','requestAfterReceipt','staleAnyOriginalClockAtProcessing','staleNativeClockAtProcessing','staleDespiteSourceFreshAtReceipt','staleWithReceiptToProcessingOver10000ms','receiptToProcessingOver10000ms')});self.receipt_processing=[];self.source_receipt=[];self.source_processing=[]
    def observe(self,event,at):
        self.counts['bookEvents']+=1
        try:
            raw=event['book'];source=N.instant(raw['sourceAt']);request=N.instant(raw['requestStartedAt']);received=N.instant(raw['receivedAt']);outer=N.instant(event['receivedAt'])
            self.counts['explicitNativeInvalidBook']+=raw.get('invalid') is True
            self.counts['originalClockMismatch']+=received!=outer
            if received<=at:self.receipt_processing.append(at-received)
            else:self.counts['receivedAfterProcessed']+=1
            if source<=received:self.source_receipt.append(received-source)
            else:self.counts['sourceAfterReceipt']+=1
            if source<=at:self.source_processing.append(at-source)
            else:self.counts['sourceAfterProcessed']+=1
            if request>received:self.counts['requestAfterReceipt']+=1
            valid=received==outer and request<=received<=at and source<=received
            if not valid:self.counts['invalidTemporalClockBooks']+=1
            stale=max(at-source,at-request,at-received)>AGE_NS
            native_clock=source if raw.get('transport')=='official_sig_realtime' else request
            self.counts['staleAnyOriginalClockAtProcessing']+=stale
            self.counts['staleNativeClockAtProcessing']+=(at-native_clock>AGE_NS)
            self.counts['staleDespiteSourceFreshAtReceipt']+=stale and 0<=received-source<=AGE_NS
            self.counts['staleWithReceiptToProcessingOver10000ms']+=stale and at-received>AGE_NS
            self.counts['receiptToProcessingOver10000ms']+=at-received>AGE_NS
            b=N.read_book(event,at,10**40)
            self.counts['parserStatus_'+b.status]+=1
            if b.status!='valid':self.counts['missingOrInvalidBook']+=1
        except (KeyError,TypeError,ValueError,OverflowError) as error:
            self.counts['missingOrUnparseableBookFields']+=1
    def result(self):return {'counts':dict(self.counts),'receiptToProcessing':distribution(self.receipt_processing),'sourceToReceipt':distribution(self.source_receipt),'sourceToProcessing':distribution(self.source_processing)}

def measure(packet,start,start_raw,pid,first_progress_pin):
    anchor=packet.sealed('anchor-head.original.json');head=packet.sealed('head.original.json');report=packet.sealed('report.original.json');bank=packet.sealed('bank-manifest.original.json')
    need(anchor['manifestHash']==head['manifestHash']==report['manifestHash']==bank['hash'],'BANK_IDENTITY')
    need(head['sequence']==report['sequence'] and head['previous']==report['previous'],'FINAL_HEAD_REPORT')
    cutoff=N.instant(report['report']['at']);end=start+WINDOW_NS;need(cutoff>=end,'WINDOW_INCOMPLETE')
    previous=anchor['previous'];sequence=anchor['sequence'];first_at=None;last_at=None;rows=0;window_rows=0;gaps=0;metrics={'all':Metrics()};kind_counts=Counter()
    with packet.z.open(packet.name('native.suffix.jsonl')) as stream:
        while True:
            raw=stream.readline(16*1024**2+1)
            if not raw:break
            need(len(raw)<=16*1024**2 and rows<1000000,'ROW_BOUND');row,_=V.sealed(raw);need(set(row)=={'event','hash','previous','sequence'},'NATIVE_ROW_FIELDS')
            need(row['sequence']==sequence+1 and row['previous']==previous,'ORIGINAL_SUFFIX_CHAIN');event=row['event'];at=N.instant(event['processedAt'])
            need((last_at is None or at>=last_at) and at<=cutoff,'NATIVE_PROCESSED_CLOCK');first_at=at if first_at is None else first_at;last_at=at;previous=row['hash'];sequence=row['sequence'];rows+=1
            if not start<=at<end:continue
            window_rows+=1;kind_counts[event['type']]+=1
            if event['type']=='gap':gaps+=1
            if event['type']!='book':continue
            rawbook=event.get('book');transport=rawbook.get('transport') if isinstance(rawbook,dict) else 'missing_book'
            key='official_sig_api_v1' if transport is None else transport if transport in ('official_sig_realtime','official_sig_api_v1') else 'unrecognized_or_missing'
            metric=metrics.setdefault(key,Metrics());metrics['all'].observe(event,at);metric.observe(event,at)
    need(rows>0 and first_at<=start and last_at>=end,'FULL_FIVE_MINUTE_WINDOW_NOT_COVERED')
    need(sequence==report['sequence'] and previous==report['previous'] and last_at==cutoff,'EXACT_FINAL_NATIVE_PREFIX')
    ref=event.get('sourceRef');need(isinstance(ref,dict) and report['sourceCursors'].get(ref['id'])==ref['sequence'],'FINAL_SOURCE_CURSOR')
    return {'pid':pid,'firstTradingProgressAt':start_raw,'windowNs':WINDOW_NS,'windowStartNs':str(start),'windowEndExclusiveNs':str(end),'fullFiveMinuteWindowCovered':True,'firstProgressOriginal':first_progress_pin,
        'packetZipSha256':packet.expected,'packetMemberManifestSha256':packet.manifest_sha,'allOriginalZipMembersAuthenticated':True,'memberPins':packet.pins,
        'nativeSuffix':{'firstSequence':anchor['sequence']+1,'lastSequence':sequence,'rows':rows,'firstProcessedNs':str(first_at),'lastProcessedNs':str(last_at),'exactRawSealsAndFinalTailVerified':True},
        'windowNativeEvents':window_rows,'windowEventTypes':dict(kind_counts),'windowGapEvents':gaps,'bookMetrics':{k:v.result() for k,v in metrics.items()},'sourceBodyMembershipIndependentlyVerified':False,'financialReplayPerformed':False,'executionAuthority':False}

def run(old_zip,old_sha,old_first_zip,old_first_sha,protocol,protocol_sha,new_zip=None,new_sha=None):
    raw=Path(protocol).read_bytes();need(sha(raw)==protocol_sha and sha(raw.replace(b'\r\n',b'\n'))==PROTOCOL_LF_SHA,'FROZEN_PROTOCOL')
    declaration=V.load(raw);need(declaration['windowMs']==300000 and declaration['policyMaximumBookAgeMs']==10000 and declaration['processes']==['R25/649399','R26/651324'],'PROTOCOL_FIELDS')
    packets=[]
    try:
        old=Packet(old_zip,old_sha);packets.append(old);first=Packet(old_first_zip,old_first_sha);packets.append(first)
        start,text=progress(first,'progress.original.json',649399)
        # The later current capture's progress must NEVER choose the old start.
        need(old.json('progress.original.json').get('pid')==649399,'OLD_CAPTURE_PROCESS')
        result={'protocolRawSha256':protocol_sha,'protocolLfSha256':PROTOCOL_LF_SHA,'sourceCodeSha256':sha(Path(__file__).read_bytes()),'old':measure(old,start,text,649399,{'zipSha256':first.expected,**first.original_pin('progress.original.json')}),
            'new':None,'comparisonPerformed':False,'executionAuthority':False,'profitabilityEstablished':False,'causalBenefitEstablished':False,'originalFourGenerationStabilityIndependentlyVerified':False,'metricDefinitions':{'p90':'nearest rank ceil(0.90*n)','median':'middle value or mean of two middle nanosecond values','staleAnyClock':'max(processed-source,processed-request,processed-receipt)>10000ms, independently of invalid flags','staleNativeClock':'source clock for realtime; request clock for normalized REST','distributionDenominators':'each valid nonnegative clock difference separately; missing/invalid and source-ahead counts explicit'},
            'limitations':declaration['limitations']+['Original admitted native records/anchored seals do not independently authenticate raw receipt bodies.','Five-minute windows include all native book events, not market silence, continuous public-source completeness or financial execution eligibility.']}
        if new_zip is not None:
            need(new_sha is not None,'NEW_ZIP_PIN');new=Packet(new_zip,new_sha);packets.append(new);start,text=progress(new,'first-trading-progress.original.json',651324)
            result['new']=measure(new,start,text,651324,{'zipSha256':new.expected,**new.original_pin('first-trading-progress.original.json')});result['comparisonPerformed']=True
        for p in packets:p.finish()
        need(sha(Path(protocol).read_bytes())==protocol_sha,'PROTOCOL_CHANGED');return result
    finally:
        for p in packets:p.z.close()

def main():
    p=argparse.ArgumentParser();p.add_argument('--old-packet',required=True);p.add_argument('--old-sha256',required=True);p.add_argument('--old-first-progress-packet',required=True);p.add_argument('--old-first-progress-sha256',required=True);p.add_argument('--new-packet');p.add_argument('--new-sha256');p.add_argument('--old-only',action='store_true');p.add_argument('--protocol',required=True);p.add_argument('--protocol-sha256',required=True);p.add_argument('--output',required=True);a=p.parse_args()
    need((a.old_only and a.new_packet is None and a.new_sha256 is None) or (not a.old_only and a.new_packet is not None and a.new_sha256 is not None),'EXPLICIT_OLD_ONLY_OR_TWO_PACKETS')
    output=Path(a.output).absolute();need(not output.exists(),'OUTPUT_EXISTS')
    result=run(a.old_packet,a.old_sha256,a.old_first_progress_packet,a.old_first_progress_sha256,a.protocol,a.protocol_sha256,a.new_packet,a.new_sha256)
    with output.open('x',encoding='utf-8',newline='\n') as f:json.dump(result,f,indent=2,allow_nan=False);f.write('\n')
if __name__=='__main__':main()
