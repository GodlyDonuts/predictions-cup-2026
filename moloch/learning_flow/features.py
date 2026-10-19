"""Causally available official prints, never inferred aggressor-side labels."""
from collections import defaultdict,deque
from decimal import Decimal
import math
from moloch.learning_v2.reference_adapters import millis

FEATURE_NAMES=('log_volume_1s','log_volume_10s','log_volume_60s','log_prints_60s',
               'last_yes_price','recent_print_available','vwap_yes_60s','yes_price_change_10s')


def trade(event,original_sha,available_at_ms):
    if event['type']!='trade':return None
    if type(available_at_ms)is not int or available_at_ms<0:raise ValueError('TRADE_INFORMATION_CLOCK')
    # Reporting-tape identifiers cannot safely be added to engine-match volume:
    # they may describe overlapping economic prints in a different namespace.
    if event.get('idNamespace')=='sig_reporting_tape_only':return None
    if event.get('idNamespace')!='sig_engine_match':raise ValueError('EXPLICIT_OFFICIAL_MATCH_NAMESPACE')
    executed=millis(event['executedAt']);received=millis(event['receivedAt'])
    if not 0<=executed<=received<=available_at_ms:raise ValueError('TRADE_INFORMATION_CLOCK')
    quantity=event['quantity']
    if type(quantity)is not int or not 0<quantity<=2147483647:raise ValueError('EXACT_TRADE_QUANTITY')
    if not isinstance(event['yesPrice'],str):raise ValueError('EXACT_NORMALIZED_YES_PRICE')
    p=Decimal(event['yesPrice'])*1000000
    if not p.is_finite()or p!=p.to_integral_value()or not 0<=p<=1000000:raise ValueError('EXACT_NORMALIZED_YES_PRICE')
    labels={k:event[k]for k in ['tournamentId','exchangeId','marketId','id']}
    if any(not isinstance(x,str)or not x or len(x)>4096 for x in labels.values()):raise ValueError('EXPLICIT_TRADE_SCOPE')
    if not isinstance(original_sha,str)or len(original_sha)!=64 or any(c not in '0123456789abcdef'for c in original_sha):raise ValueError('ORIGINAL_TRADE_HASH')
    return {'kind':'trade_flow','venue':'SIG','available_at_ms':available_at_ms,'received_at_ms':received,
            'executed_at_ms':executed,'exchange_id':labels['exchangeId'],'market_id':labels['marketId'],
            'tournament_id':labels['tournamentId'],'match_id':labels['id'],'quantity':quantity,
            'yes_price_micros':int(p),'original_sha256':original_sha,'aggressor_side_known':False}


class Flow:
    def __init__(self):self.rows=defaultdict(deque);self.seen={};self.last_available=-1;self.last_observed=-1

    def accept(self,at,row):
        if type(at)is not int or at<max(self.last_available,self.last_observed) or row['available_at_ms']!=at or any(type(row[k])is not int for k in ['executed_at_ms','received_at_ms','quantity','yes_price_micros']) or not 0<=row['executed_at_ms']<=row['received_at_ms']<=at:
            raise ValueError('FLOW_AVAILABILITY_CLOCK')
        if not 0<row['quantity']<=2147483647 or not 0<=row['yes_price_micros']<=1000000:raise ValueError('FLOW_EXACT_UNITS')
        self.last_available=at;key=(row['tournament_id'],row['exchange_id'],row['match_id'])
        signature=(row['market_id'],row['executed_at_ms'],row['yes_price_micros'],row['quantity'])
        if key in self.seen:
            if self.seen[key]!=signature:raise ValueError('ORIGINAL_MATCH_FACTS_CHANGED')
            return False
        if len(self.seen)>=1000000:raise ValueError('FINITE_EPISODE_PRINT_BOUND')
        self.seen[key]=signature
        # A newly recovered old print is information, not newly executed flow.
        if at-row['executed_at_ms']<=60000:self.rows[row['exchange_id']].append(dict(row))
        return True

    def observe(self,exchange,at):
        if type(at)is not int or at<max(self.last_available,self.last_observed):raise ValueError('FLOW_OBSERVATION_CLOCK_REGRESSION')
        self.last_observed=at
        q=self.rows.get(exchange)
        if not q:return (0.,)*len(FEATURE_NAMES)
        while q and q[0]['available_at_ms']<at-60000:q.popleft()
        rows=[r for r in q if 0<=at-r['executed_at_ms']<=60000]
        if not rows:return (0.,)*len(FEATURE_NAMES)
        volume=lambda window:sum(r['quantity']for r in rows if at-r['executed_at_ms']<=window)
        ordered=sorted(rows,key=lambda r:(r['executed_at_ms'],r['available_at_ms'],r['match_id']))
        latest=ordered[-1];total=volume(60000);vwap=sum(r['quantity']*r['yes_price_micros']for r in rows)/total
        recent=[r for r in ordered if at-r['executed_at_ms']<=10000]
        change=recent[-1]['yes_price_micros']-recent[0]['yes_price_micros']if len(recent)>1 else 0
        return (*(math.log1p(volume(w))for w in [1000,10000,60000]),math.log1p(len(rows)),
                latest['yes_price_micros']/1000000,1.,vwap/1000000,change/1000000)
