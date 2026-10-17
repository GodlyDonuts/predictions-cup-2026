"""Finite original R27 incident verification; no Session/account/network calls."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path

ROOT=Path(__file__).resolve().parents[3]
SHA='7ee0a95b75500d732e2568a1962eb1491cd525125284a495f39bc14d38666af4'
NODE_SHA='19e964fb2168bee447600a23e14cdb83a1dd5ead24be04a0c43cd0f824ba0774'
ANCHOR_SHA='8c09fd1aaa589dd0e4e55da09188b5b5aa7f19c9ea3378c779155da534dc02d3'
H=lambda b:hashlib.sha256(b).hexdigest()
path=ROOT/'research/moloch-source-latency-20261017/compare_book_clocks.py'
assert H(path.read_bytes().replace(b'\r\n',b'\n'))=='1fbc599e16417778d4aa48545eb2c7af9c3073875fd4ffb26f72c0333443a34b'
spec=importlib.util.spec_from_file_location('_r27_incident_checked',path)
V=importlib.util.module_from_spec(spec);spec.loader.exec_module(V)

def chain(packet,name,sequence,previous):
    rows=[]
    for raw in packet.small(packet.name(name),64*1024**2).splitlines(keepends=True):
        row,_=V.V.sealed(raw)
        assert row['sequence']==sequence+1 and row['previous']==previous
        sequence=row['sequence'];previous=row['hash'];rows.append(row)
    return rows,sequence,previous

def run(incident,node,anchor_packet):
    packets=[]
    try:
        p=V.Packet(incident,SHA);packets.append(p)
        n=V.Packet(node,NODE_SHA);packets.append(n)
        a=V.Packet(anchor_packet,ANCHOR_SHA);packets.append(a)
        assert Path(incident).stat().st_size==6054486 and len(p.pins)==18
        report,tokens=V.V.sealed(p.small(p.name('report.original.json')))
        before,prior_tokens=V.V.sealed(n.small(n.name('report.original.json')))
        anchor=p.sealed('anchor-head.original.json');head=p.sealed('head.original.json');bank=p.sealed('bank-manifest.original.json')
        assert p.small(p.name('anchor-head.original.json'))==a.small(a.name('head.original.json'))
        assert anchor['sequence']==489338
        assert report['manifestHash']==head['manifestHash']==anchor['manifestHash']==bank['hash']
        native,seq,last=chain(p,'native.suffix.jsonl',anchor['sequence'],anchor['previous'])
        assert seq==head['sequence']==report['sequence']==492481 and last==head['previous']==report['previous']
        assert V.V.array_tokens(tokens['fills'])==V.V.array_tokens(prior_tokens['fills']) and len(report['fills'])==382
        fundamental=p.small(p.name('fundamental.original.jsonl'))
        assert fundamental.startswith(n.small(n.name('fundamental.original.jsonl')))
        fpol,_,_=chain(p,'fundamental.original.jsonl',0,'0'*64)
        gens=[r for r in fpol if r['event']['kind']=='review_generation'];assert len(gens)==6
        dispatch,_,_=chain(p,'dispatch.original.ndjson',0,'0'*64)
        q=next(q for q in report['orders'] if q['id']==15612)
        assert (q['exchangeId'],q['side'],q['action'],q['quantity'],q['price'],q['filled'],q['retired'])==('851','NO','buy',680,'0.875000',0,False)
        assert max(q['id'] for q in before['orders'])<15612
        assert not any(r['event'].get('qid')==15612 for r in dispatch)
        assert not any(r['event'].get('orderId')==15612 and r['event']['type'].startswith('actual_') for r in native)
        stop=p.json('stop.original.json');status=p.json('status.original.json');queue=p.json('queue.original.json')
        assert stop['at']=='2026-10-17T14:21:28.344Z' and stop['reason']==status['reason']=='FUNDAMENTAL_NOT_ELIGIBLE'
        assert status['mode']=='stopped' and status['pid']==654706
        assert queue['globalUnknownCount']==0 and queue['hasUnknown'] is False and queue['state']['active'] is None and queue['state']['manual_review'] is False
        event=next(r['event'] for r in reversed(native) if r['event']['type']=='book' and r['event']['book']['exchangeId']=='851')
        book=event['book'];at=V.N.instant(stop['at'])
        ages={k:V.ms(at-V.N.instant(book[k])) for k in ['sourceAt','requestStartedAt','receivedAt']}
        assert all(at-V.N.instant(book[k])>10000*1000000 for k in ages)
        result={'schemaVersion':1,'verifierSourceRawSha256':H(Path(__file__).read_bytes()),'verifierSourceLfSha256':H(Path(__file__).read_bytes().replace(b'\r\n',b'\n')),'result':'PASS_ORIGINAL_R27_PREINTENT_BOOK_EXPIRY_INCIDENT','incidentZipSha256':SHA,'incidentZipBytes':6054486,
            'originalArtifactsRawPinsVerified':18,'packetManifestSha256':p.manifest_sha,'nodeContinuityZipSha256':NODE_SHA,'anchorIncidentZipSha256':ANCHOR_SHA,
            'nativeSuffix':{'anchorSequence':489338,'firstSequence':489339,'finalSequence':seq,'rows':len(native),'rawPin':p.original_pin('native.suffix.jsonl'),'exactRawSealsAndTailVerified':True},
            'prior382RawFillTokensUnchanged':True,'wholeOriginalSixGenerationPrefixPreserved':True,'generationHashes':[r['event']['generation']['generationSha256'] for r in gens],
            'originalFundamentalPin':p.original_pin('fundamental.original.jsonl'),'dispatchRows':len(dispatch),'originalDispatchPin':p.original_pin('dispatch.original.ndjson'),
            'quote':{'qid':15612,'exchangeId':'851','side':'NO','action':'buy','quantity':680,'limit':'0.875000','filled':0,'retired':False},
            'noOriginalDispatchOrActualNativeOperationForQid':True,'stoppedAt':stop['at'],'reason':stop['reason'],'pid':654706,'originalStopPin':p.original_pin('stop.original.json'),
            'queue':{'globalUnknownCount':0,'active':None,'manualReview':False},'final851Book':{k:book.get(k) for k in ['marketId','exchangeId','transport','sequence','sourceAt','requestStartedAt','receivedAt','invalid']},
            'bookAgeAtStopMs':ages,'bookSourceReference':event.get('sourceRef'),'wholeNativePrefixReportedByCapture':p.json('capture.json')['wholeNativePrefix'],
            'wholeNativePrefixIndependentlyRehashedHere':False,'sourceBodyMembershipIndependentlyVerified':False,'financialReplayPerformed':False,'newAccountRequests':0,'executionAuthority':False,'profitabilityEstablished':False}
        for packet in packets:packet.finish()
        return result
    finally:
        for packet in packets:packet.z.close()

def main():
    ap=argparse.ArgumentParser();ap.add_argument('--incident',required=True);ap.add_argument('--node-proof',required=True);ap.add_argument('--anchor-incident',required=True);ap.add_argument('--output',required=True);a=ap.parse_args()
    out=Path(a.output);assert not out.exists();result=run(a.incident,a.node_proof,a.anchor_incident)
    with out.open('x',encoding='utf-8',newline='\n') as f:json.dump(result,f,indent=2);f.write('\n')
if __name__=='__main__':main()
