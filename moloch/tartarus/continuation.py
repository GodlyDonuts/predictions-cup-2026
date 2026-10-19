"""Research-only residual tranches from original actual fills and current FIFO.

No cash credit, order submission, retirement or approval. The caller must still
authenticate the original worker/dispatch closure, compile a fresh complete
account, re-audit all scenarios/funding and obtain the next physical review.
"""
from copy import deepcopy
import hashlib
import json
from decimal import Decimal
from .sequence_audit import amount,need,UNIT


def digest(value):
    return hashlib.sha256(json.dumps(value,sort_keys=True,separators=(',',':'),ensure_ascii=False,allow_nan=False).encode()).hexdigest()


def residual(nomination_raw,expected_raw_sha256,legs,current_fills,original_fill_count,quote,official_order_id,*,physical_terminal):
    need(hashlib.sha256(nomination_raw).hexdigest()==expected_raw_sha256,'CONTINUATION_NOMINATION_BYTES')
    nom=json.loads(nomination_raw);need(bool(legs),'CONTINUATION_EMPTY_PLAN');first=legs[0];action=nom['action']
    need(physical_terminal is True and quote['retired']is True,'CONTINUATION_POSITIVE_TERMINAL_REQUIRED')
    need(quote['reviewedRankCampaign']['campaignSha256']==digest(nom),'CONTINUATION_CAMPAIGN_BINDING')
    n=first['quantity'];need(type(n)is int and n>0,'CONTINUATION_QUANTITY')
    need(action['exchangeId']==quote['exchangeId']==first['ex']and action['side']==quote['side']==first['side']
         and action['action']==first['action']and quote['action']==first['action'].lower()
         and action['quantity']==quote['quantity']==n and amount(action['limitPrice'])==amount(quote['price'])==first['price'],
         'CONTINUATION_ACTION_CHANGED')
    need(type(original_fill_count)is int and 0<=original_fill_count<=len(current_fills)
         and digest(current_fills[:original_fill_count])==nom['account']['actualFillsSha256'],'CONTINUATION_ORIGINAL_FILL_PREFIX')
    need(isinstance(official_order_id,str)and bool(official_order_id),'CONTINUATION_ORIGINAL_ORDER_ID')
    matched=[];seen=set();filled=0
    for fill in current_fills[original_fill_count:]:
        need(fill['sourceId']not in seen,'CONTINUATION_DUPLICATE_ACTUAL_FILL');seen.add(fill['sourceId'])
        if fill['orderId']!=quote['id']:continue
        need(fill['counterfactual']is False and fill['actualOrderId']==official_order_id
             and fill['exchangeId']==first['ex']and fill['side']==first['side']and fill['action']==quote['action'],
             'CONTINUATION_ORIGINAL_ACTUAL_FILL_REQUIRED')
        quantity=fill['quantity'];need(type(quantity)is int and quantity>0,'CONTINUATION_ACTUAL_QUANTITY')
        price=amount(fill['price']);need(price<=first['price']if first['action']=='BUY'else price>=first['price'],'CONTINUATION_ACTUAL_LIMIT')
        filled+=quantity;matched.append(fill['sourceId'])
    need(type(quote['filled'])is int and filled==quote['filled']and 0<=filled<=n,'CONTINUATION_ACTUAL_FILL_TOTAL')
    remaining=deepcopy(legs)
    if filled==n:remaining.pop(0)
    else:
        remaining[0]['quantity']=remaining[0]['n']=n-filled;remaining[0].pop('basis',None)
    return {'classification':'actual_fill_residual_requires_current_reaudit','remainingLegs':remaining,
            'actualFirstLegFilledQuantity':filled,'firstLegRemainingQuantity':n-filled,'actualFillSourceIds':matched,
            'currentFinancialReauditRequired':True,'actualSaleProceedsCreditedByThisFunction':False,
            'terminalAssertionAuthenticatedByThisFunction':False,'executionAuthority':False}


def rebase_fifo(current_positions,legs):
    """Cost hypothetical future SELL pieces from actual current original lots.

    Hypothetical BUYs affect only this temporary path; nothing returns a live
    wallet or spendable cash. sequence_audit must independently check funding.
    """
    positions=deepcopy(current_positions);out=[]
    for old in legs:
        ex,side,action=old['ex'],old['side'],old['action'];n,price=old['quantity'],old['price']
        need(side in ['YES','NO']and action in ['BUY','SELL']and type(n)is int and 0<n<=10000
             and type(price)is int and 0<price<UNIT,'CONTINUATION_HYPOTHETICAL_ACTION')
        sides=positions.setdefault(ex,{'YES':[],'NO':[]});opposite='NO'if side=='YES'else'YES'
        need(not sides.get(opposite,[]),'CONTINUATION_HYPOTHETICAL_NETTING_UNSUPPORTED')
        lots=sides.setdefault(side,[]);basis=0
        if action=='BUY':lots.append({'quantity':n,'price':format(Decimal(price)/UNIT,'.6f')})
        else:
            left=n
            while left:
                need(bool(lots),'CONTINUATION_UNCOVERED_HYPOTHETICAL_SELL');lot=lots[0]
                need(type(lot['quantity'])is int and lot['quantity']>0,'CONTINUATION_ORIGINAL_LOT_QUANTITY')
                take=min(left,lot['quantity']);basis+=take*amount(lot['price']);left-=take;lot['quantity']-=take
                if not lot['quantity']:lots.pop(0)
        out.append({'ex':ex,'side':side,'action':action,'quantity':n,'n':n,'price':price,'basis':basis})
    return out
