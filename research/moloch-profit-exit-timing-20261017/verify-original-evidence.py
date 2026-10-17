"""Read-only exact-byte chain and finite FIFO verification; no account access."""
from collections import Counter, defaultdict
from copy import deepcopy
from datetime import datetime, timezone
from decimal import Decimal
import hashlib
import json
from pathlib import Path

HERE=Path(__file__).resolve().parent
ARCHIVE_SHA='11167216c88850ff5414f16415f57e00e4924e255dce2174518c1c9f2b40432f'
MANIFEST_SHA='7f5a83e317d49abf3b58f4e8fee73a9b6729a7cb767156613cc695074b943bfe'
TARGET=14627


def need(ok,code):
    if not ok: raise ValueError(code)


def unique(items):
    out={}
    for k,v in items:
        need(k not in out,'DUPLICATE_KEY');out[k]=v
    return out


DECODER=json.JSONDecoder(parse_float=Decimal,object_pairs_hook=unique)
def load(raw): return DECODER.decode(raw.decode() if isinstance(raw,bytes) else raw)
def sha(raw): return hashlib.sha256(raw).hexdigest()
def fixed(n): return format(n,'.6f')
def clock(s):
    t=datetime.fromisoformat(s.replace('Z','+00:00'));need(t.tzinfo is not None,'CLOCK_ZONE')
    d=t.astimezone(timezone.utc)-datetime(1970,1,1,tzinfo=timezone.utc)
    return d.days*86400000+d.seconds*1000+d.microseconds//1000


def fields(raw):
    """Locate outer raw value tokens. Never reserialize arbitrary-precision JSON."""
    s=raw.decode('utf-8').rstrip('\n');need(s.startswith('{') and s.endswith('}'),'OBJECT_LAYOUT')
    at=1;values={};tokens={};spans={}
    while at<len(s)-1:
        pair_start=at;k,end=DECODER.raw_decode(s,at)
        need(isinstance(k,str) and k not in values and s[end]==':','OUTER_FIELD')
        start=end+1;v,at=DECODER.raw_decode(s,start)
        values[k]=v;tokens[k]=s[start:at];spans[k]=(pair_start,at)
        if at==len(s)-1: break
        need(s[at]==',','FIELD_SEPARATOR');at+=1
    need(at==len(s)-1,'TRAILING_BYTES')
    return s,values,tokens,spans


def sealed(raw):
    need(raw.endswith(b'\n') and not raw.endswith(b'\r\n'),'ORIGINAL_LF_REQUIRED')
    s,v,t,sp=fields(raw);need('hash' in v and len(v['hash'])==64,'HASH_FIELD')
    start,end=sp['hash']
    if start>1: body=s[:start-1]+s[end:]
    else: body=s[:start]+s[end+1:]
    need(sha(body.encode())==v['hash'],'ORIGINAL_RAW_BODY_HASH')
    return v,t


def array_tokens(text):
    need(text.startswith('[') and text.endswith(']'),'ARRAY_LAYOUT')
    out=[];at=1
    while at<len(text)-1:
        start=at;_,at=DECODER.raw_decode(text,at);out.append(text[start:at])
        if at==len(text)-1: break
        need(text[at]==',','ARRAY_SEPARATOR');at+=1
    return out


def verify_chain(path,genesis=None,first_sequence=None):
    previous=genesis;last=None;rows=[]
    for raw in path.open('rb'):
        need(len(raw)<=16*1024**2,'ROW_BOUND')
        r,_=sealed(raw);need(set(r)=={'event','hash','previous','sequence'},'ROW_KEYS')
        if last is None:
            need(first_sequence is None or r['sequence']==first_sequence,'FIRST_SEQUENCE')
            anchor=r['previous']
        else: need(r['sequence']==last+1,'CHAIN_SEQUENCE')
        if previous is not None: need(r['previous']==previous,'CHAIN_PREVIOUS')
        previous,last=r['hash'],r['sequence'];rows.append(r)
    need(rows,'EMPTY_CHAIN')
    return rows,dict(rows=len(rows),firstSequence=rows[0]['sequence'],lastSequence=last,
                     initialPrevious=anchor,lastHash=previous,
                     genesisIndependentlyChecked=genesis is not None,rawSha256=sha(path.read_bytes()))


def verify():
    need(sha((HERE/'evidence-manifest.json').read_bytes())==MANIFEST_SHA,'MANIFEST_PIN')
    manifest=load((HERE/'evidence-manifest.json').read_bytes())
    for name,pin in manifest.items():
        p=HERE/name;need(p.resolve().is_relative_to(HERE.resolve()) and p.is_file(),'PIN_PATH')
        raw=p.read_bytes();need(len(raw)==pin['bytes'] and sha(raw)==pin['sha256'],'FILE_PIN')
    d=HERE/'actual-exit-audit-v4'
    before,bt=sealed((d/'baseline373.original.json').read_bytes())
    after,at=sealed((d/'current.original.json').read_bytes())
    old_fill_tokens,new_fill_tokens=array_tokens(bt['fills']),array_tokens(at['fills'])
    need(len(before['fills'])==373 and len(after['fills'])==380 and
         old_fill_tokens==new_fill_tokens[:373],'ALL_PRIOR_FILL_BYTES_UNCHANGED')
    need(before['sequence']==440934 and after['sequence']==452287 and before['manifestHash']==after['manifestHash'],
         'REPORT_BOUNDARIES')
    dispatch,dispatch_proof=verify_chain(d/'dispatch-full-original.jsonl','0'*64,1)
    need(len(dispatch)==20883,'DISPATCH_ROW_COUNT')
    native,native_proof=verify_chain(d/'native-through-snapshot.original.jsonl',first_sequence=439479)
    need(len(native)==12809 and native[-1]['sequence']==after['sequence'],'NATIVE_SUFFIX_BOUNDARY')
    by_sequence={r['sequence']:r for r in native}
    for report in [before,after]:
        r=by_sequence[report['sequence']]
        # Session's snapshot `previous` is Chain.previous: the current tail row
        # hash, not that row's own predecessor field (session.rs snapshot writer).
        need(r['hash']==report['previous'],'REPORT_CHAIN_TAIL_ANCHOR')
        ref=r['event']['sourceRef']
        need(report['sourceCursors'][ref['id']]==ref['sequence'],'REPORT_SOURCE_CURSOR_ANCHOR')
    kinds=defaultdict(list)
    for row in dispatch:
        e=row['event']
        if e.get('qid')==TARGET: kinds[e['kind']].append(e)
    need(all(len(kinds[k])==1 for k in ['intent','attempt','ack','closed']),'TARGET_LIFECYCLE')
    intent,attempt,ack,closed=(kinds[k][0] for k in ['intent','attempt','ack','closed'])
    q=next(q for q in after['orders'] if q['id']==TARGET);r=ack['receipt'];actual=str(r['data']['orderId'])
    need(clock(intent['at'])<=clock(attempt['at'])<=clock(r['requestStartedAt'])<=clock(r['receivedAt']),
         'ORIGINAL_ATTEMPT_ACK_CLOCKS')
    need(r['status']==200 and r['endpoint']=='/orders' and q['action']=='sell' and q['side']=='NO' and
         q['exchangeId']=='948' and q['marketId']=='259' and q['quantity']==1382 and q['filled']==1382,
         'NATIVE_TARGET_TERMS')
    body=load(intent['spec']['bodyJson']);need(body==intent['spec']['body'],'ORIGINAL_BODY_JSON')
    for key in ['exchangeId','action','side','quantity']:
        need(str(body[key]).lower()==str(r['data'][key]).lower()==str(q[key]).lower(),'ACK_BODY_TERMS')
    need(Decimal(str(body['price']))==Decimal(str(r['data']['price']))==Decimal(q['price']),'ACK_LIMIT')
    order=load((d/'terminal-order.original.json').read_bytes())
    tape=load((d/'complete-fill-tape.original.json').read_bytes())
    need(order['status']==tape['status']==200 and order['endpoint']==f'/orders/{actual}' and
         tape['endpoint']==f'/orders/{actual}/fills?limit=200' and
         clock(order['receivedAt'])<=clock(tape['requestStartedAt'])<=clock(tape['receivedAt']), 'TERMINAL_PAIR')
    od,td=order['data'],tape['data']
    need(str(od['id'])==str(td['orderId'])==actual and od['open'] is False and
         od['quantity']==od['quantityFilled']==1382 and od['action']=='sell' and od['side']=='no' and
         Decimal(str(od['priceLimit']))==Decimal(q['price']) and
         str(od['exchangeId'])==str(td['exchangeId'])=='948' and
         od['tournamentId']==td['tournamentId']==body['tournamentId'] and
         td['coverage']['complete'] is True and td['pagination']['hasMore'] is False and
         td['pagination']['nextCursor'] is None,'COMPLETE_SCOPED_TERMINAL')
    need(len(td['data'])==5 and td['totalQuantityFilled']==-1382,'SIGNED_NO_TOTAL')
    tape_fills={str(f['id']):(abs(f['quantity']),Decimal(str(f['price'])),f['filledAt']) for f in td['data']}
    need(len(tape_fills)==5 and all(f['quantity']<0 and f['side']=='no' for f in td['data']) and
         sum(v[0] for v in tape_fills.values())==1382,'SIGNED_NO_ROWS')
    new=after['fills'][373:];need(len(new)==7 and all(f['counterfactual'] is False for f in new),'NEW_ACTUAL_FILLS')
    sells=[f for f in new if f['orderId']==TARGET];buys=[f for f in new if f['action']=='buy']
    need(len(sells)==5 and len(buys)==2 and len(sells)+len(buys)==len(new),'NEW_FILL_PARTITION')
    need({str(f['sourceId']):(f['quantity'],Decimal(f['price']),f['executedAt']) for f in sells}==tape_fills and
         all(f['actualOrderId']==actual for f in sells),'TAPE_REPORT_EXACT_MATCH')
    observed_native=[];native_ack=[];native_terminal=[]
    report_keys={(f['actualOrderId'],str(f['sourceId'])):f for f in new}
    all_report_keys={(f['actualOrderId'],str(f['sourceId'])) for f in after['fills']}
    for row in native:
        if row['sequence']<=before['sequence']:continue
        e=row['event']
        if e['type']=='actual_fill':
            key=e['actualOrderId'],str(e['fillId'])
            need(key in all_report_keys,'UNACCOUNTED_NATIVE_ACTUAL_FILL')
            if key in report_keys:
                f=report_keys[key]
                need(all(e[k]==f[k] for k in ['orderId','actualOrderId','exchangeId','marketId','side','action','quantity','price','fee','executedAt','receiptHash']) and
                     e['processedAt']==f['at'],'NATIVE_FILL_EXACT')
                observed_native.append(key)
        if e['type']=='actual_order_ack' and e['orderId']==TARGET and e['receivedAt']==r['receivedAt']:native_ack.append(e)
        if e['type']=='actual_order_terminal' and e['orderId']==TARGET:native_terminal.append(e)
    need(observed_native==list(report_keys) and len(observed_native)==7,'NATIVE_NEW_FILL_COMMIT_ORDER')
    need(native_ack and all(e['actualOrderId']==actual and e['quantity']==1382 and
         all(e[k]==q[k]for k in ['exchangeId','marketId','side','action','price']) for e in native_ack),'NATIVE_ACK_MEMBERSHIP')
    need(len(native_terminal)==1 and native_terminal[0]['fillsComplete'] is True and
         native_terminal[0]['filledQuantity']==1382 and native_terminal[0]['actualOrderId']==actual and
         all(native_terminal[0][k]==q[k]for k in ['exchangeId','marketId','side','action']) and
         native_terminal[0]['orderReceiptHash']==closed['orderReceiptHash'] and
         native_terminal[0]['receiptHash']==closed['fillReceiptHash'],'NATIVE_TERMINAL_CLOSED_REFS')
    positions=deepcopy(before['report']['positions']);basis=Decimal(0);proceeds=Decimal(0);purchase=Decimal(0);fees=Decimal(0)
    for f in new:  # native report append/commit order, not execution-time resorting
        n=f['quantity'];price=Decimal(f['price']);fees+=Decimal(f['fee']);lots=positions[f['exchangeId']][f['side']]
        if f['action']=='buy':
            opposite=positions[f['exchangeId']]['YES' if f['side']=='NO' else 'NO']
            need(not opposite,'UNSUPPORTED_OPPOSITE_NETTING_IN_THIS_FINITE_AUDIT')
            purchase+=price*n;lots.append(dict(price=f['price'],quantity=n,executedAt=f['executedAt']))
        else:
            need(f['orderId']==TARGET,'UNEXPECTED_SELL');proceeds+=price*n
            left=n
            while left:
                need(lots,'SELL_WITHOUT_INVENTORY');take=min(left,lots[0]['quantity']);basis+=Decimal(lots[0]['price'])*take
                lots[0]['quantity']-=take;left-=take
                if not lots[0]['quantity']:lots.pop(0)
    need(positions==after['report']['positions'],'ALL_POSITIONS_EXACT_FIFO_PARITY')
    gain=proceeds-basis-fees
    need((sum(f['quantity'] for f in sells),proceeds,basis,gain,purchase,fees)==
         (1382,Decimal('1244.425'),Decimal('1236.890'),Decimal('7.535'),Decimal('548.600'),Decimal(0)), 'EXACT_FINANCIAL_EXPECTATIONS')
    a,b=before['report'],after['report']
    need(Decimal(a['cash'])+proceeds-purchase-fees==Decimal(b['cash']) and
         Decimal(a['ownedCost'])+purchase-basis==Decimal(b['ownedCost']) and
         Decimal(a['realized'])+gain==Decimal(b['realized']) and
         Decimal(a['inheritedRealizedGross'])+gain==Decimal(b['inheritedRealizedGross']) and
         a['newTradingRealized']==b['newTradingRealized']=='-184.970000','REPORT_LEDGER_PARITY')
    old=next(q for q in after['orders']if q['id']==14579)
    timing=load((d/'timing-diagnosis.json').read_bytes())
    need(old==timing['oldQuote'],'TIMING_QUOTE_ORIGINAL_REPORT')
    headroom=old['expiresAt']-clock(timing['knownOwnedRefreshCompletedAt'])
    need(headroom==2911 and headroom<5000 and
         not any(row['event'].get('qid')==14579 and row['event']['kind'] in ['intent','attempt','ack','recovered_ack']for row in dispatch),
         'TIMING_ARITHMETIC_AND_UNATTEMPTED')
    return dict(pass_=True,archiveSha256=ARCHIVE_SHA,evidenceManifestSha256=MANIFEST_SHA,allManifestFilesVerified=len(manifest),
        verifierSha256=sha(Path(__file__).read_bytes()),fullOriginalDispatch=dispatch_proof,nativeSuffix=native_proof,
        baselineSequence=before['sequence'],currentSequence=after['sequence'],unchangedPriorFillRawTokens=373,
        newFillRows=7,otherBuyRows=2,otherBuyPrincipal=fixed(purchase),exitNativeQid=TARGET,exitFillRows=5,
        exitShares=1382,exitProceeds=fixed(proceeds),fifoBasisReleased=fixed(basis),recordedFees=fixed(fees),exitRealizedGain=fixed(gain),
        currentEconomics={k:b[k]for k in ['cash','ownedCost','newTradingRealized','inheritedRealizedGross','realized']},
        timing=dict(oldNativeQid=14579,expiryHeadroomArithmeticMs=headroom,physicalPreflightMinimumExclusiveMs=5000,
                    noOriginalIntentAttemptAck=True,completionClockIndependentOriginalWitnessPresent=False,
                    completionClockSource='derived timing-diagnosis.json only in this ZIP'),
        qualifications=dict(fullNativePrefixReplayed=False,typedRustReceiptDigestsRecreated=False,
                            noMissedFillClaim=True,noCausalPerformanceClaim=True,noCandidateDeploymentAttribution=True))


if __name__=='__main__':
    result=verify();out=HERE/'independent-result-final.json'
    encoded=(json.dumps(result,sort_keys=True,indent=2)+'\n').encode()
    with out.open('xb')as f:f.write(encoded)
    print(json.dumps(dict(passed=True,resultSha256=sha(encoded),resultPath=str(out),exitGain=result['exitRealizedGain'])))
