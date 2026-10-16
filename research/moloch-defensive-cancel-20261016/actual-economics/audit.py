"""Finite original-capture auditor. Private directory input, aggregate output only."""
import argparse
from collections import Counter
from datetime import datetime
from decimal import Decimal
import hashlib
import json
from pathlib import Path

LAUNCH='2026-10-16T17:07:56Z'
def sha(raw):return hashlib.sha256(raw).hexdigest()
def ms(text):return int(datetime.fromisoformat(text.replace('Z','+00:00')).timestamp()*1000)
def need(ok,why):
    if not ok:raise ValueError(why)
def chain(raw,suffix=False):
    lines=raw.split(b'\n')[1:-1] if suffix else raw.splitlines()
    previous=None;out=[]
    for line in lines:
        row=json.loads(line);field=b',"hash":"'+row['hash'].encode()+b'"';at=line.rfind(field)
        need(at>=0 and sha(line[:at]+line[at+len(field):])==row['hash'],'ORIGINAL_ROW_HASH')
        if previous:need(row['previous']==previous['hash'] and row['sequence']==previous['sequence']+1,'SOURCE_CHAIN')
        elif not suffix:need(row['sequence']==1 and row['previous']=='0'*64,'DISPATCH_GENESIS')
        previous=row;out.append(row)
    return out

def sealed(raw):
    raw=raw.strip();value=json.loads(raw);field=b',"hash":"'+value['hash'].encode()+b'"';at=raw.find(field)
    need(at>=0 and sha(raw[:at]+raw[at+len(field):])==value['hash'],'BANK_REPORT_SEAL')
    return value

def receipt(raw,expected):
    # Receipt is a Rust struct: field order differs from its stored Value. Keep
    # original data numeric bytes, rather than re-encoding floats through Python.
    text=raw.decode();value=json.loads(text);at=text.index('"data":')+7;_,end=json.JSONDecoder().raw_decode(text,at)
    parts=[json.dumps(k)+':'+json.dumps(value[k],separators=(',',':'),ensure_ascii=False) for k in ['endpoint','requestStartedAt','receivedAt','status']]
    parts.append('"data":'+text[at:end]);need(sha(('{'+','.join(parts)+'}').encode())==expected,'TYPED_RECEIPT_HASH')
    need(value['status']==200,'OFFICIAL_RECEIPT_STATUS');return value

def analyze(root):
    pins=[];files={};captures=[]
    for name in ['capture-index.json','second-capture-index.json','final-capture-index.json']:
        index=json.loads((root/name).read_bytes());captures.append({'indexArtifact':name,'capturedAtMs':index['capturedAtMs'],'indexSha256':sha((root/name).read_bytes())})
        for ref in index['files']:
            path=Path(ref['localPath']);raw=path.read_bytes()
            need(len(raw)==ref['bytes'] and sha(raw)==ref['sha256'],'PRIVATE_CAPTURE_PIN')
            files[path.name]=raw;pins.append({'artifact':path.name,'bytes':len(raw),'sha256':ref['sha256'],
                'originalStartOffset':ref['startOffset'],'originalBytesAtOpen':ref['sourceBytesAtOpen']})
    dispatch=chain(files['final-dispatch.ndjson']);events=[row['event'] for row in dispatch]
    bank=sealed(files['bank-report.json']);final=sealed(files['final-bank-report.json'])
    journal=chain(files['bank-events.suffix.jsonl'],True);breadth=chain(files['breadth.suffix.jsonl'],True)
    by_sequence={r['sequence']:r for r in journal};need(by_sequence[bank['sequence']]['hash']==bank['previous'],'REPORT_SOURCE_MEMBERSHIP')
    fills=bank['fills'];keys={(f['actualOrderId'],f['sourceId']) for f in fills};need(len(keys)==len(fills),'DUPLICATE_ACCOUNTED_FILL')
    original=[f for f in fills if f['exchangeId']=='894'];need(len(original)==6,'UNEXPECTED_FINITE_FILL_COHORT')
    buys=[f for f in original if f['action']=='buy'];sells=[f for f in original if f['action']=='sell']
    yes=sum(f['quantity'] for f in buys if f['side']=='YES');no=sum(f['quantity'] for f in buys if f['side']=='NO')
    need(len(buys)==4 and len(sells)==2 and yes==2479 and no==1291,'FILL_ORIGINS')
    need('894' not in bank['report']['inheritedInventory']['adoptedExchangeIds'],'INHERITED_MISATTRIBUTION')
    for f in original:
        h=f['receiptHash'];candidates=[raw for name,raw in files.items() if name.endswith(h+'.json')];need(candidates,'ORIGINAL_FILL_RECEIPT_MISSING')
        r=receipt(candidates[0],h);need(str(r['data']['orderId'])==f['actualOrderId'] and r['data']['exchangeId']==f['exchangeId'],'FILL_RECEIPT_SCOPE')
        matched=[x for x in r['data']['data'] if str(x['id'])==f['sourceId']]
        need(len(matched)==1 and abs(Decimal(str(matched[0]['quantity'])))==f['quantity'] and Decimal(str(matched[0]['price']))==Decimal(f['price']),'FILL_RECEIPT_MEMBERSHIP')
    pairs=min(yes,no);pair_gain=Decimal(pairs)*(1-Decimal('.335')-Decimal('.660'))
    residual=yes-pairs;exit_qty=sum(f['quantity'] for f in sells);need(exit_qty==residual==1188,'RESIDUAL_RECONCILIATION')
    exit_cash=sum(Decimal(f['price'])*f['quantity'] for f in sells);exit_fee=sum(Decimal(f['fee']) for f in sells)
    exit_gain=exit_cash-Decimal('.335')*residual-exit_fee
    intents={e['qid']:e for e in events if e.get('kind')=='intent'}
    acks=[e for e in events if e.get('kind')=='ack' and ms(e['receipt']['receivedAt'])>=ms(LAUNCH)]
    cancels=[e for e in events if e.get('kind')=='cancel_attempt' and ms(e.get('at',LAUNCH))>=ms(LAUNCH)]
    ack_map={e['qid']:e for e in events if e.get('kind')=='cancel_ack'};closed={e['qid']:e for e in events if e.get('kind')=='closed'}
    cancel_summary=[]
    for e in cancels:
        qid=e['qid'];q=intents[qid]['quote'];item={'qid':qid,'issuedAt':e['at'],'optionalDefensive':e.get('defensive') is not None,'ackAt':ack_map.get(qid,{}).get('receipt',{}).get('receivedAt'),'terminalConfirmed':qid in closed}
        if qid in closed:
            proof=closed[qid];receipts={}
            for key in ['orderReceiptHash','fillReceiptHash']:
                h=proof[key];raw=next(raw for name,raw in files.items() if name.endswith(h+'.json'));receipts[key]=receipt(raw,h)
            order=receipts['orderReceiptHash'];tape=receipts['fillReceiptHash']
            original_ack=next(x['receipt'] for x in events if x.get('kind')=='ack' and x.get('qid')==qid)
            actual=str(original_ack['data']['orderId'])
            need(str(order['data']['id'])==str(tape['data']['orderId'])==actual and order['data']['exchangeId']==tape['data']['exchangeId']==q['exchangeId'] and order['data']['action']==q['action'] and order['data']['side']==q['side'].lower(),'CANCEL_RECEIPT_SCOPE')
            need(order['data']['open'] is False and order['data']['quantityFilled']==0 and tape['data']['totalQuantityFilled']==0 and tape['data']['data']==[] and tape['data']['coverage']['complete'] is True and tape['data']['pagination']['hasMore'] is False,'CANCEL_TERMINAL_PROOF')
            item.update(terminalOrderObservedAt=order['receivedAt'],completeTapeObservedAt=tape['receivedAt'],filledQuantity=0,reason=order['data']['terminalReasonCode'])
            native=[r for r in journal if r['event'].get('type')=='actual_order_terminal' and r['event'].get('orderId')==qid]
            item['nativeTerminalAppliedAt']=native[-1]['event']['processedAt'] if native else None
            native_q=next(x for x in final['orders'] if x['id']==qid)
            item['nativeReportRetired']=native_q['retired'];item['nativeReportFilled']=native_q['filled']
            need(native_q['retired'] is True and native_q['filled']==0,'NATIVE_TERMINAL_REPORT')
        cancel_summary.append(item)
    placement=[]
    source_end=max(ms(r['event']['processedAt']) for r in journal if r['event'].get('processedAt'))
    public_trades={}
    for row in journal:
        e=row['event']
        if e.get('type')=='trade':
            key=(e['idNamespace'],e['exchangeId'],e['id'])
            need(key not in public_trades or all(public_trades[key][k]==e[k] for k in ['yesPrice','quantity','executedAt']),'PUBLIC_TRADE_CONFLICT')
            public_trades[key]=e
    for ack in acks:
        q=intents[ack['qid']]['quote'];book_candidates=[r['event'] for r in journal if r['event'].get('type')=='book' and r['event']['book']['exchangeId']==q['exchangeId'] and r['event'].get('processedAt','')<=ack['receipt']['requestStartedAt']]
        b=book_candidates[-1]['book'] if book_candidates else None
        item={'qid':ack['qid'],'exchangeId':q['exchangeId'],'side':q['side'],'action':q['action'],'quantity':q['quantity'],'limit':q['price'],'ackAt':ack['receipt']['receivedAt'],'draftToAckMs':ms(ack['receipt']['receivedAt'])-q['createdAt']}
        if b and ms(ack['receipt']['receivedAt'])-ms(b['receivedAt'])<=10000:
            same=b['bids'][0] if q['side']=='YES' else b['asks'][0];opposite=b['asks'][0] if q['side']=='YES' else b['bids'][0]
            item.update(originalFreshBookReceivedAt=b['receivedAt'],sameSideDisplayedTopQuantity=same['quantity'],oppositeSideDisplayedTopQuantity=opposite['quantity'])
        else:item['originalFreshPlacementBook']='not_in_frozen_suffix'
        at=ms(ack['receipt']['receivedAt'])
        after=[t for t in public_trades.values() if t['exchangeId']==q['exchangeId'] and ms(t['executedAt'])>=at]
        threshold=Decimal(q['price']) if q['side']=='YES' else 1-Decimal(q['price'])
        lower=(q['side']=='YES' and q['action']=='buy') or (q['side']=='NO' and q['action']=='sell')
        price_eligible=[t for t in after if (Decimal(t['yesPrice'])<=threshold if lower else Decimal(t['yesPrice'])>=threshold)]
        item.update(publicObservedPrintsAfterAck=len(after) if at<source_end else None,publicPriceCompatiblePrintsAfterAck=len(price_eligible) if at<source_end else None,publicTapeComplete=False,publicObservationWindowEndMs=source_end)
        placement.append(item)
    trace=json.loads(files['final-decision-trace-latest.json'])
    return {'schemaVersion':1,'launchAt':LAUNCH,'auditBoundaryAt':final['report']['at'],
        'captureClocks':captures,'sourcePins':pins,'sourceChecks':{'dispatchRows':len(dispatch),'dispatchLastSequence':dispatch[-1]['sequence'],'dispatchLastHash':dispatch[-1]['hash'],'bankSuffixRows':len(journal),'bankSuffixSequence':[journal[0]['sequence'],journal[-1]['sequence']],'breadthSuffixRows':len(breadth),'initialBankReportSequence':bank['sequence'],'initialReportMembershipVerified':True,'finalBankReportSequence':final['sequence'],'finalReportBeyondFrozenSuffix':final['sequence']>journal[-1]['sequence'],'fullBankReplayPerformed':False,'crossFileAtomicSnapshotClaimed':False},
        'economics':{'priorOwnerBuyFillRows':len(buys),'priorOwnerUniqueBuyOrders':len({f['actualOrderId'] for f in buys}),'pairedUnits':pairs,'pairedGainExact':str(pair_gain),'carriedYesUnits':residual,'carriedBasisExact':str(Decimal('.335')*residual),'newOwnerSellFillRows':len(sells),'newOwnerUniqueSellOrders':len({f['actualOrderId'] for f in sells}),'exitProceedsExact':str(exit_cash),'exitFeesExact':str(exit_fee),'exitGainExact':str(exit_gain),'completeLifecycleGainExact':str(pair_gain+exit_gain),'newBuyActualFillRows':sum(ms(f['executedAt'])>=ms(LAUNCH) and f['action']=='buy' for f in final['fills']),'finalOrdinaryRealized':final['report']['newTradingRealized'],'finalInheritedGross':final['report']['inheritedRealizedGross'],'finalCombinedRealized':final['report']['realized']},
        'newVersionAcknowledgments':placement,'cancellations':cancel_summary,'optionalDefensiveAttemptObserved':any(x['optionalDefensive'] for x in cancel_summary),'latestProgress':json.loads(files['final-progress.json']),'latestDecisionTrace':{'qid':trace['trace']['nativeQid'],'draftToAckMs':trace['trace']['ackReceivedAtMs']-trace['trace']['candidateCreatedAtMs'],**trace['summary']},
        'interpretation':{'actualProfitabilityEstablished':False,'publicTradeCompletenessProved':False,'queuePriorityProved':False,'financialAuthority':False}}

if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--private-input',type=Path,required=True);parser.add_argument('--output',type=Path,required=True);args=parser.parse_args()
    result=analyze(args.private_input)
    with args.output.open('x',encoding='utf-8',newline='\n') as f:json.dump(result,f,indent=2);f.write('\n')
