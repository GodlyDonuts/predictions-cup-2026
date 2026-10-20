"""Bounded original capture compilation for explicit pre-test development.

No HTTP, policy inference, financial writer or independent-test admission.
Canonical transport must come from the separately pinned original Node audit.
"""
import argparse,gzip,hashlib,heapq,json
from collections import Counter
from pathlib import Path
from .prospective_source import ProspectiveDecoder,identifier
from moloch.learning_flow.features import Flow
from moloch.learning_v2.reference_adapters import millis
from moloch.research.verify_learning_transport import source_hash

FORMAT='original_completed_SIG_capture_development_v1'
LIMIT=16*1024*1024


def sha(path):
    with Path(path).open('rb')as f:return hashlib.file_digest(f,'sha256').hexdigest()


def pinned_json(path,pin):
    path=Path(path)
    if not path.is_file()or path.is_symlink()or path.stat().st_size>1024*1024 or sha(path)!=pin:
        raise ValueError('PINNED_CAPTURE_METADATA_REQUIRED')
    return json.loads(path.read_bytes())


def lines(path):
    with Path(path).open('rb')as f:
        while raw:=f.readline(LIMIT+1):
            if len(raw)>LIMIT or not raw.endswith(b'\n'):raise ValueError('BOUNDED_COMPLETE_ORIGINAL_RECORD_REQUIRED')
            yield raw


def compile_capture(root,manifest_pin,canonical,audit_path,audit_pin,session_path,output,*,bounds,role,heldout_start):
    root=Path(root).resolve();canonical=Path(canonical);audit_path=Path(audit_path);session_path=Path(session_path)
    if (role not in('fit','search')or len(bounds)!=2 or any(type(v)is not int or v<0 for v in bounds)
            or type(heldout_start)is not int or not bounds[0]<bounds[1]<=heldout_start):
        raise ValueError('EXPLICIT_PRE_TEST_DEVELOPMENT_BOUNDS_REQUIRED')
    manifest_path=root/'complete.json';m=pinned_json(manifest_path,manifest_pin);audit=pinned_json(audit_path,audit_pin)
    if (m.get('format')!='original_completed_sig_forward_fixture_v1'or m.get('trainingAdmitted')is not False
            or m.get('independentTestUsed')is not False or m.get('productionChanged')is not False):
        raise ValueError('ORIGINAL_PUBLIC_DEVELOPMENT_CAPTURE_REQUIRED')
    session=pinned_json(session_path,m['sourceStreamManifestSha256'])
    if session.get('tokenPersistence')is not False or session.get('contestOrders')!=0 or session.get('paperOrders')!=0:
        raise ValueError('PUBLIC_READ_CAPTURE_SESSION_REQUIRED')
    tournament=session['tournamentId'];scope={};markets=set()
    for pair in session['pairs']:
        mid=identifier(pair['marketId']);ex=identifier(pair['exchangeId'])
        if ex in scope and scope[ex]!=mid:raise ValueError('UNIQUE_CAPTURE_EXCHANGE_SCOPE')
        scope[ex]=mid;markets.add(mid)
    if not scope:raise ValueError('EXPLICIT_CAPTURE_CONTRACT_UNIVERSE')
    original=root/'originals/events.jsonl';receipts=root/'originals/receipts.jsonl';archive=root/'originals.tar.gz'
    pins={manifest_path:manifest_pin,audit_path:audit_pin,session_path:m['sourceStreamManifestSha256'],
        original:m['files']['events.jsonl']['sha256'],receipts:m['files']['receipts.jsonl']['sha256'],
        archive:m['archiveSha256'],canonical:audit['canonicalTransportSha256']}
    if (audit['sourceOriginalSha256']!=pins[original]or audit['originalArchiveSha256']!=pins[archive]
            or audit.get('producerHashPreserved')is not True):raise ValueError('ORIGINAL_CANONICAL_TRANSPORT_BINDING')
    def recheck():
        for path,pin in pins.items():
            if not path.is_file()or path.is_symlink()or sha(path)!=pin:raise ValueError('ORIGINAL_CAPTURE_INPUT_CHANGED')
    recheck();output=Path(output).resolve()
    if output==root or output.is_relative_to(root):raise ValueError('NORMALIZATION_OUTSIDE_ORIGINAL_CAPTURE')
    output.mkdir(parents=True,exist_ok=False)
    d=ProspectiveDecoder(tournament,bounds,role,subscription_binding={
        'tournamentId':tournament,'manifestSha256':m['sourceStreamManifestSha256']})
    counts={'transport':Counter(),'REST':Counter()};last_hash='';sequence=0
    def at(event):
        value=millis(event['receivedAt'])
        if not bounds[0]<=value<bounds[1]:raise ValueError('ORIGINAL_RECORD_OUTSIDE_EXPLICIT_DEVELOPMENT_CALENDAR')
        return value
    def stream_rows():
        nonlocal last_hash,sequence
        prior=-1
        for raw in lines(canonical):
            row=json.loads(raw);source_hash(raw,row);sequence+=1
            if row['sequence']!=sequence or row['previous']!=last_hash:raise ValueError('ORIGINAL_PRODUCER_CHAIN')
            last_hash=row['hash'];event=row['event'];kind=event['kind'];counts['transport'][kind]+=1;clock=at(event)
            if kind=='rest_reference':continue  # Append metadata can be older than the public-stream clock.
            if kind not in('subscribed','broadcast')or clock<prior:raise ValueError('PUBLIC_STREAM_KIND_OR_CLOCK')
            if identifier(event['marketId'])not in markets:raise ValueError('CAPTURE_MARKET_OUTSIDE_SESSION')
            prior=clock;yield clock,1,sequence,event,row['hash']
    def rest_rows():
        prior=-1
        for i,raw in enumerate(lines(receipts)):
            event=json.loads(raw);kind=event['kind'];counts['REST'][kind]+=1;clock=at(event)
            if kind not in('market','book')or clock<prior:raise ValueError('ORIGINAL_REST_KIND_OR_CLOCK')
            prior=clock
            if identifier(event['marketId'])not in markets:raise ValueError('CAPTURE_MARKET_OUTSIDE_SESSION')
            if kind=='book':yield clock,0,i,event,hashlib.sha256(raw).hexdigest()
    observed=Counter();exchanges=set();flow=Flow();row_digest=hashlib.sha256();print_digest=hashlib.sha256()
    obs=output/'observations.jsonl.gz';trades=output/'trades.jsonl.gz'
    with gzip.open(obs,'wt',encoding='utf-8',newline='\n')as f,gzip.open(trades,'wt',encoding='utf-8',newline='\n')as g:
        for clock,channel,_,event,pin in heapq.merge(stream_rows(),rest_rows(),key=lambda value:value[:3]):
            rows,prints=d.rest_book(event,pin)if channel==0 else d.websocket(event,pin)
            for row in rows:
                ex=row.get('exchange_id')
                if ex is not None and scope.get(ex)!=row['market_id']:raise ValueError('BOOK_EXCHANGE_OUTSIDE_CAPTURE_SESSION')
                if row['kind']=='book':exchanges.add(ex)
                observed[row['snapshot_origin']+'/'+row['kind']]+=1
                line=json.dumps(row,separators=(',',':'))+'\n';f.write(line);row_digest.update(line.encode())
            for row in prints:
                if scope.get(row['exchange_id'])!=row['market_id']:raise ValueError('TRADE_EXCHANGE_OUTSIDE_CAPTURE_SESSION')
                if flow.accept(clock,row):
                    line=json.dumps(row,separators=(',',':'))+'\n';g.write(line);print_digest.update(line.encode())
    if (sequence!=audit['rows']or last_hash!=audit['terminalProducerHash']
            or dict(counts['transport'])!=audit['counts']or dict(counts['transport'])!=m['counts']['events.jsonl']
            or dict(counts['REST'])!=m['counts']['receipts.jsonl']):
        raise ValueError('COMPLETE_ORIGINAL_CAPTURE_COUNTS_AND_TERMINAL_HASH')
    recheck()
    result=dict(format=FORMAT,classification='bounded_original_capture_development_not_independent_qualification',
        originalManifestSha256=manifest_pin,transportAuditSha256=audit_pin,sessionManifestSha256=m['sourceStreamManifestSha256'],
        originalTransportRows=sequence,receiptBoundsMs=list(bounds),calendarRole=role,heldoutStartMs=heldout_start,
        sameMillisecondOrderingAssumption='REST_then_WS_original_channel_order_not_submillisecond_truth',
        normalizedCounts=dict(observed),decoderCounts=dict(d.decoder.counts),exchanges=len(exchanges),
        observationsSha256=sha(obs),tradeRowsSha256=sha(trades),observationBodySha256=row_digest.hexdigest(),
        tradeBodySha256=print_digest.hexdigest(),reportedTrades=len(flow.seen),
        inputCounts={key:dict(value)for key,value in counts.items()},allInputsRehashedAfterRead=True,
        trainingAdmitted=False,independentTestEligible=False,independentTestUsed=False,productionChanged=False,accountRequests=0)
    with(output/'complete.json').open('x')as f:json.dump(result,f,indent=2)
    return result


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    for key in('root','canonical','audit','session','output'):p.add_argument('--'+key,type=Path,required=True)
    for key in('manifest-pin','audit-pin'):p.add_argument('--'+key,required=True)
    p.add_argument('--bounds',type=int,nargs=2,required=True);p.add_argument('--heldout-start',type=int,required=True)
    p.add_argument('--role',choices=('fit','search'),required=True);a=p.parse_args()
    value=compile_capture(a.root,a.manifest_pin,a.canonical,a.audit,a.audit_pin,a.session,a.output,
        bounds=a.bounds,role=a.role,heldout_start=a.heldout_start)
    print(json.dumps({'complete':True,'exchanges':value['exchanges'],'trainingAdmitted':False,'independentTestUsed':False}))
