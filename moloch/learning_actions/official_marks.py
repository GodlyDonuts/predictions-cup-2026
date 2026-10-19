"""Causal tournament-position valuation receipts, never executable quotes.

This adapter does not change the active midpoint experiment. Missing official
marks make valuation unavailable; they do not reject orders or shrink the legal
trading universe. The recorded account's cash/lots are never adopted here.
"""
import bisect,hashlib,json,re
from types import MappingProxyType
from decimal import Decimal,InvalidOperation
from .economics import Wealth,GrowthLedger
from .contract import UNIT
from moloch.learning_v2.reference_adapters import millis


def exact(value,scale=1):
    try:d=Decimal(str(value))*scale
    except InvalidOperation:raise ValueError('EXACT_OFFICIAL_MARK_NUMBER')from None
    if not d.is_finite()or d!=d.to_integral_value():raise ValueError('EXACT_OFFICIAL_MARK_NUMBER')
    return int(d)


def normalize_positions(raw,original_sha256,*,endpoint,tournament,bounds):
    if hashlib.sha256(raw).hexdigest()!=original_sha256:raise ValueError('ORIGINAL_POSITION_RECEIPT_CHANGED')
    if not re.fullmatch(r'/tournaments/[A-Za-z0-9_-]+/portfolio/positions',endpoint)or not tournament:
        raise ValueError('EXPLICIT_OFFICIAL_TOURNAMENT_SCOPE')
    receipt=json.loads(raw);at=millis(receipt['receivedAt']);start=millis(receipt['requestStartedAt'])
    if receipt['endpoint']!=endpoint or receipt['status']!=200:raise ValueError('OFFICIAL_MARK_ENDPOINT_OR_STATUS')
    if not bounds[0]<=at<bounds[1]or start>at:raise ValueError('OFFICIAL_MARK_AUTHORIZED_INFORMATION_CLOCK')
    seen=set();rows=[]
    for position in receipt['data']['positions']:
        ex=str(position['exchangeId']);market=str(position['marketId']);q=exact(position['quantity'])
        if not ex.isdigit()or not market.isdigit()or ex in seen:raise ValueError('OFFICIAL_MARK_UNIQUE_SCOPE')
        seen.add(ex)
        if not q:continue  # No price inference from a zero-position row.
        row=dict(exchange_id=ex,market_id=market,tournament_id=tournament,available_at_ms=at,
            original_sha256=original_sha256,source_clock='original_positions_receipt_receivedAt_publisher_clock_unknown')
        if position['currentPrice']is None:
            rows.append({**row,'kind':'official_mark_missing'});continue
        price=exact(position['currentPrice'],UNIT)
        if not 0<=price<=UNIT:raise ValueError('OFFICIAL_MARK_PRICE_BOUNDS')
        # Position prices are interpreted as YES-normalized, validated against
        # signed quantity and the reported position value. The actual receipts
        # round marketValue to cents, so allow at most half a cent per row.
        reported=Decimal(str(position['marketValue']))
        expected=Decimal(abs(q))*Decimal(price if q>0 else UNIT-price)/UNIT
        if not reported.is_finite()or reported*100!=(reported*100).to_integral_value()or abs(reported-expected)>Decimal('.005'):
            raise ValueError('OFFICIAL_MARK_SIGNED_VALUE_ORIENTATION_OR_CENT_ROUNDING')
        rows.append({**row,'kind':'official_mark','yes_price_micros':price,
            'orientation_basis':'YES_normalized_verified_against_signed_quantity_marketValue_at_cent_precision'})
    return rows


class OfficialMarkTape:
    def __init__(self,rows,*,tournament,bounds):
        self.tournament=tournament;self.bounds=tuple(bounds);self.rows={};self.times={}
        bounds=self.bounds
        if any(type(t)is not int for t in bounds)or len(bounds)!=2 or bounds[0]>=bounds[1]:raise ValueError('OFFICIAL_MARK_ROLE_BOUNDS')
        for row in sorted(rows,key=lambda r:r['available_at_ms']):
            at=row['available_at_ms'];ex=row['exchange_id']
            if type(at)is not int or row['tournament_id']!=tournament or not bounds[0]<=at<bounds[1]:raise ValueError('OFFICIAL_MARK_ROLE_OR_TOURNAMENT')
            pin=row.get('original_sha256','')
            if not re.fullmatch(r'[0-9a-f]{64}',pin):raise ValueError('OFFICIAL_MARK_ORIGINAL_HASH_REQUIRED')
            if row['kind']not in('official_mark','official_mark_missing'):raise ValueError('OFFICIAL_MARK_KIND')
            if row['kind']=='official_mark'and(type(row['yes_price_micros'])is not int or not 0<=row['yes_price_micros']<=UNIT):
                raise ValueError('OFFICIAL_MARK_PRICE_BOUNDS')
            history=self.rows.setdefault(ex,[])
            if history and history[-1]['market_id']!=row['market_id']:raise ValueError('OFFICIAL_MARK_EXCHANGE_MARKET_CHANGED')
            if history and history[-1]['available_at_ms']==at and any(history[-1].get(k)!=row.get(k)for k in('kind','yes_price_micros','market_id')):
                raise ValueError('CONFLICTING_SAME_RECEIPT_TIME_OFFICIAL_MARK')
            history.append(dict(row))
        self.times=MappingProxyType({ex:tuple(row['available_at_ms']for row in history)for ex,history in self.rows.items()})
        self.rows=MappingProxyType({ex:tuple(MappingProxyType(row)for row in history)for ex,history in self.rows.items()})

    def get(self,exchange,at):
        if type(at)is not int or not self.bounds[0]<=at<=self.bounds[1]:raise ValueError('OFFICIAL_MARK_QUERY_OUTSIDE_ROLE')
        times=self.times.get(exchange,());index=bisect.bisect_right(times,at)-1
        if index<0:return None
        row=self.rows[exchange][index]
        return None if row['kind']=='official_mark_missing'else row


def snapshot_official(env,tape,*,maximum_age_ms=120000,historical_last_mark=False):
    if type(maximum_age_ms)is not int or maximum_age_ms<=0:raise ValueError('POSITIVE_OFFICIAL_MARK_AGE')
    values={};stale=set()
    for (exchange,side),lots in env.lots.items():
        quantity=sum(n for n,_ in lots)
        if not quantity:continue
        row=tape.get(exchange,env.now)
        if row is None:raise ValueError('OFFICIAL_TOURNAMENT_MARK_UNAVAILABLE:'+exchange)
        if getattr(env,'mapping',{}).get(exchange,row['market_id'])!=row['market_id']:raise ValueError('OFFICIAL_MARK_POSITION_MARKET_SCOPE')
        if env.now-row['available_at_ms']>maximum_age_ms:
            if not historical_last_mark:raise ValueError('OFFICIAL_TOURNAMENT_MARK_STALE:'+exchange)
            stale.add(exchange)
        if side not in('YES','NO'):raise ValueError('OFFICIAL_MARK_POSITION_SIDE')
        price=row['yes_price_micros']if side=='YES'else UNIT-row['yes_price_micros']
        values[exchange]=values.get(exchange,0)+2*quantity*price
    convention=('last_observed_official_tournament_receipt_prices_with_explicit_staleness'if historical_last_mark else
        'causal_official_tournament_position_receipt_prices_not_executable_liquidation')
    return Wealth(env.now,2*env.cash+sum(values.values()),values,convention,tuple(sorted(stale)))


class OfficialGrowthLedger(GrowthLedger):
    """The existing human reward and daily boundaries, with explicit mark tape."""
    def __init__(self,env,marks,**kwargs):
        self.official_marks=marks
        if not marks.bounds[0]<=env.now<=env.end<=marks.bounds[1]:raise ValueError('WHOLE_OFFICIAL_VALUATION_EPISODE_IN_ROLE')
        super().__init__(env,**kwargs)

    def _value(self,env):
        value=snapshot_official(env,self.official_marks,maximum_age_ms=self.maximum_mark_age_ms,
            historical_last_mark=self.historical_last_mark)
        self.stale_exchanges.update(value.stale_exchanges);self.stale_observations+=bool(value.stale_exchanges)
        return value
