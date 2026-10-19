"""Original external books -> conditional reference features, never SIG fills.

Party orientation is explicit. Bid/ask uncertainty is retained alongside the
midpoint; neither contract equivalence nor executable portfolio value follows.
"""
import hashlib
import json
from datetime import datetime
from decimal import Decimal

UNIT=1_000_000


def millis(text):
    d=datetime.fromisoformat(text.replace('Z','+00:00'))
    if d.utcoffset()is None:raise ValueError('EXPLICIT_REFERENCE_CLOCK_REQUIRED')
    return int(d.timestamp()*1000)


def probability(value):
    d=Decimal(str(value))*UNIT
    if not d.is_finite()or d!=d.to_integral_value()or not 0<=d<=UNIT:raise ValueError('EXACT_REFERENCE_PROBABILITY')
    return int(d)


def receipt(row,available):
    if type(available)is not int or available<0:raise ValueError('EXACT_REFERENCE_AVAILABILITY_CLOCK')
    r=row.get('receipt',row)
    if r['status']!=200:return None
    if hashlib.sha256(r['bodyText'].encode()).hexdigest()!=r['bodySha256']:raise ValueError('REFERENCE_BODY_BYTES_CHANGED')
    body=json.loads(r['bodyText'])
    if 'body'in r and body!=r['body']:raise ValueError('REFERENCE_DERIVED_BODY_CHANGED')
    start=millis(r['requestStartedAt']);end=millis(r['receivedAt'])
    if not start<=end<=available:raise ValueError('REFERENCE_AVAILABILITY_CLOCK')
    return r,body,end


def top(levels,bids):
    prices=[]
    for row in levels:
        price,size=(row['price'],row['size'])if isinstance(row,dict)else row
        q=Decimal(str(size))
        if not q.is_finite()or q<0:raise ValueError('REFERENCE_DEPTH')
        if q>0:prices.append(probability(price))
    if not prices:raise ValueError('TWO_SIDED_REFERENCE_REQUIRED')
    return max(prices)if bids else min(prices)


def feature(raw,venue,market,source,bid,ask,side,available,received,source_at):
    if side not in ('YES','NO'):raise ValueError('EXPLICIT_REFERENCE_ORIENTATION')
    if not 0<=bid<=ask<=UNIT or source_at is not None and source_at>available:raise ValueError('REFERENCE_INTERVAL_OR_SOURCE_CLOCK')
    if side=='NO':bid,ask=UNIT-ask,UNIT-bid
    return {'kind':'reference','venue':venue,'market_id':str(market),'source':source,
            'available_at_ms':available,'received_at_ms':received,'source_at_ms':source_at,
            'price_yes_micros':(bid+ask)//2,'bid_yes_micros':bid,'ask_yes_micros':ask,
            'midpoint_rounding':'floor_to_integer_millionth','original_sha256':hashlib.sha256(raw).hexdigest(),
            'mapping_kind':'direct_party_reference','mapping_semantics':'same_party_conditional_not_contract_equivalence',
            'pricing_authority':False,'resolution_equivalence_verified':False}


def gap(raw,venue,market,source,available):
    return {'kind':'reference_gap','venue':venue,'market_id':str(market),'source':source,
            'available_at_ms':available,'original_sha256':hashlib.sha256(raw).hexdigest(),
            'reason':'TWO_SIDED_REFERENCE_UNAVAILABLE','pricing_authority':False}


def kalshi_references(raw,mappings,available_at_ms,*,mapping_kind=None):
    row=json.loads(raw)
    if row.get('mappingKind',mapping_kind)!='direct_party_reference':return []
    checked=receipt(row,available_at_ms)
    if checked is None:return []
    _,body,received=checked;book=body.get('orderbook_fp')
    if book is None:return []
    allowed=set(map(str,row.get('sigMarketIds',[m['sigMarketId']for m in mappings])))
    selected=[m for m in mappings if str(m['sigMarketId'])in allowed]
    try:bid=top(book['yes_dollars'],True);ask=UNIT-top(book['no_dollars'],True)
    except ValueError as error:
        if str(error)!='TWO_SIDED_REFERENCE_REQUIRED':raise
        return [gap(raw,'Kalshi',m['sigMarketId'],row['ticker'],available_at_ms)for m in selected]
    return [feature(raw,'Kalshi',m['sigMarketId'],row['ticker'],bid,ask,m['side'],available_at_ms,received,None)
            for m in selected]


def polymarket_references(raw,available_at_ms):
    row=json.loads(raw)
    if 'identity'not in row or 'receipt'not in row or row.get('invalid'):return []
    identity=row['identity'];checked=receipt(row,available_at_ms)
    if checked is None:return []
    _,body,received=checked
    if isinstance(body,list):
        index=row.get('bodyIndex')
        if type(index)is not int or not 0<=index<len(body):raise ValueError('REFERENCE_BATCH_INDEX')
        body=body[index]
    if body['asset_id']!=identity['assetId']or body['market']!=identity['conditionId']:raise ValueError('REFERENCE_TOKEN_IDENTITY')
    selected=[m for m in identity['sigReferences']if m.get('relationship')=='same_party_conditional'and m.get('sigSide')in ('YES','NO')]
    try:bid=top(body['bids'],True);ask=top(body['asks'],False)
    except ValueError as error:
        if str(error)!='TWO_SIDED_REFERENCE_REQUIRED':raise
        return [gap(raw,'Polymarket',m['sigMarketId'],identity['conditionId'],available_at_ms)for m in selected]
    if identity['outcome']=='No':bid,ask=UNIT-ask,UNIT-bid
    elif identity['outcome']!='Yes':raise ValueError('BINARY_REFERENCE_OUTCOME_REQUIRED')
    stamp=body['timestamp']
    if not(type(stamp)is int or isinstance(stamp,str)and stamp.isdigit()):raise ValueError('EXACT_REFERENCE_SOURCE_CLOCK')
    source_at=int(stamp)
    if source_at<0:raise ValueError('REFERENCE_SOURCE_CLOCK')
    return [feature(raw,'Polymarket',m['sigMarketId'],identity['conditionId'],bid,ask,m['sigSide'],available_at_ms,received,source_at)
            for m in selected]
