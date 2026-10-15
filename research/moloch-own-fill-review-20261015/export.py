"""Offline-only streaming export. Never opens a network client or changes source files.

Run nice -n 10 python3 export.py RUN_ROOT NEW_PRIVATE_OUTPUT_DIRECTORY.
The output is private; publish only aggregates from the separate audit.
"""
from pathlib import Path
import sys,json,hashlib,os,collections

def compact(value):
    return json.dumps(value,sort_keys=True,separators=(',',':'),ensure_ascii=False).encode()
def digest(raw):return hashlib.sha256(raw).hexdigest()
def write_new(path,raw):
    fd=os.open(path,os.O_WRONLY|os.O_CREAT|os.O_EXCL,0o600)
    with os.fdopen(fd,'wb')as f:f.write(raw);f.flush();os.fsync(f.fileno())
def export(root,out):
    root=Path(root);out=Path(out)
    assert root.is_absolute() and out.is_absolute() and not out.exists()
    assert not out.is_relative_to(root),'OUTPUT_MUST_BE_OUTSIDE_LIVE_ROOT'
    out.mkdir(mode=0o700,parents=False)
    bank=root/'bank-all-markets-v1'
    report_raw=(bank/'report.json').read_bytes();report=json.loads(report_raw)
    manifest_raw=(bank/'manifest.json').read_bytes();manifest=json.loads(manifest_raw)
    for sealed in [report,manifest]:assert digest(compact({k:v for k,v in sealed.items()if k!='hash'}))==sealed['hash'],'SEALED_DOCUMENT_HASH'
    assert report['manifestHash']==manifest['hash'],'REPORT_MANIFEST_SCOPE'
    fills=report['fills'];entries=[f for f in fills if f['action']=='buy' and not f.get('counterfactual',True)]
    exchanges={f['exchangeId']for f in entries};assert exchanges
    orders=[o for o in report['orders']if o['exchangeId']in exchanges]
    selected_fills=[f for f in fills if f['exchangeId']in exchanges]
    dispatch_raw=(root/'dispatch.ndjson').read_bytes();assert dispatch_raw.endswith(b'\n')
    dispatch=[json.loads(line)for line in dispatch_raw.splitlines()]
    selected_dispatch=[]
    for row in dispatch:
        e=row['event'];qid=e.get('qid')
        if any(o['id']==qid for o in orders):
            e={k:v for k,v in e.items()if k in ['kind','at','qid','actualOrderId','orderId','receiptHash','fillReceiptHash','orderReceiptHash','quote','code','status']}
            selected_dispatch.append({'sourceSequence':row['sequence'],'sourceHash':row['hash'],'event':e})
    prefix=hashlib.sha256();previous='';count=0;prefix_bytes=0;types=collections.Counter();retained=0;inherited=[];event_hash=hashlib.sha256()
    fd=os.open(out/'events.ndjson',os.O_WRONLY|os.O_CREAT|os.O_EXCL,0o600)
    with os.fdopen(fd,'wb')as dest,(bank/'events.jsonl').open('rb')as source:
        while count<report['sequence']:
            raw=source.readline(32*1024*1024+1)
            assert raw and raw.endswith(b'\n') and len(raw)<=32*1024*1024,'MISSING_OR_OVERSIZED_PREFIX_ROW'
            row=json.loads(raw);body={k:v for k,v in row.items()if k!='hash'}
            assert set(body)=={'event','previous','sequence'}
            assert row['sequence']==count+1 and row['previous']==previous,'SOURCE_CHAIN'
            assert digest(compact(body))==row['hash'],'SOURCE_HASH'
            prefix.update(raw);prefix_bytes+=len(raw);count+=1;previous=row['hash'];e=row['event'];types[e['type']]+=1
            if e['type']=='actual_position_adoption':inherited=[p['exchangeId']for p in e['positions']]
            exchange=e.get('exchangeId',e.get('book',{}).get('exchangeId'))
            if exchange in exchanges and (e['type']in ['book','gap'] or e['type'].startswith('actual_')):
                normalized={'sourceSequence':count,'sourceHash':previous,'event':e}
                line=compact(normalized)+b'\n';dest.write(line);event_hash.update(line);retained+=1
        dest.flush();os.fsync(dest.fileno())
    assert previous==report['previous'],'REPORT_ANCHOR_MISMATCH'
    # Preserve only chosen new-entry scope. No whole-account positions/profile is
    # copied. Source-derived inherited aggregate remains explicitly separate.
    private={'schemaVersion':1,'scopeExchanges':sorted(exchanges),'reportAt':report['report']['at'],
        'policy':manifest['policy'],'fills':selected_fills,'orders':orders,'dispatch':selected_dispatch,
        'initiallyInheritedExchangesInScope':sorted(exchanges.intersection(inherited)),
        'reportedNewTradingRealized':report['report'].get('newTradingRealized'),
        'reportedInheritedRealizedGross':report['report'].get('inheritedRealizedGross'),
        'source':{'bankPrefixBytes':prefix_bytes,'bankPrefixSha256':prefix.hexdigest(),'bankSequence':count,'bankTailHash':previous,
            'reportSha256':digest(report_raw),'manifestSha256':digest(manifest_raw),'dispatchPrefixBytes':len(dispatch_raw),'dispatchPrefixSha256':digest(dispatch_raw),
            'retainedEvents':retained,'retainedEventsSha256':event_hash.hexdigest(),'sourceTypeCounts':dict(types)}}
    write_new(out/'private.json',compact(private)+b'\n')
    print(json.dumps({'exportDirectory':str(out),'reportAt':private['reportAt'],'newEntryExchanges':len(exchanges),'fills':len(selected_fills),'retainedEvents':retained,'bankSequence':count,'bankPrefixSha256':prefix.hexdigest()}))
if __name__=='__main__':export(*sys.argv[1:])
