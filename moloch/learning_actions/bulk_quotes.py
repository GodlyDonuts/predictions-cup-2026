"""Paid price-only quotes, separate from delivered full-depth observations.

This is a scoped counterfactual snapshot adapter. Original-history gaps remain
unknown. Latest recorded prints do not establish complete engine-price parity.
"""
from copy import deepcopy
import re
import numpy as np
from .component_execution import ComponentEpisode,ComponentTape,ComponentPolicy,append_rows
from .full_depth import FEATURES,_identity
from .feature_cache import identity
from .order_pages import clock_feature
from .contract import UNIT
from .observation_protocol import BULK_QUOTES


def initialize(env):
    env.quote_selection=[];env.known_quotes={};env.last_quote_response=None
    env.quote_keys=set(env.keys['SIG']);env.quote_request_sequence=0
    add_targets(env)


def add_targets(env):
    keys=[('bulk_prices',)]+[('quote',ex)for ex in env.exchanges[1:]]
    known=env.exchanges[1:]if getattr(env,'named_api_targets',False)else[k[1]for k in env.keys['SIG']if k[0]=='book']
    keys.extend(('bulk_prices','group',tuple(known[i:i+100]))for i in range(0,len(known),100))
    for key in keys:
        if key not in env.quote_keys:env.keys['SIG'].append(key);env.quote_keys.add(key)


def snapshot(env,exchange):
    world=env.books.get(exchange)
    if world is None or world.invalidated or exchange not in env.mapping:
        raise ValueError('REPLAY_QUOTE_SOURCE_UNAVAILABLE')
    bids=[p for p,n in world.book.bids if n-world.depleted.get(('bids',p),0)>0]
    asks=[p for p,n in world.book.asks if n-world.depleted.get(('asks',p),0)>0]
    for o in env.api_orders.values():
        s=o.spec
        if s.exchange!=exchange or o.remaining<=0 or s.market or(s.expiration_ms is not None and s.expiration_ms<=env.now):continue
        price=s.price if s.side=='YES'else UNIT-s.price
        (bids if(s.action=='BUY')==(s.side=='YES')else asks).append(price)
    bid=max(bids)if bids else None;ask=min(asks)if asks else None
    original=env.trade_history.latest(exchange,env.now)
    latest=None if original is None else original['yes_price_micros']
    at=-1 if original is None else original['executed_at_ms']
    for f in reversed(env.api_fills):
        if f['exchange']==exchange and f['filled_at_ms']>=at:
            latest=f['price']if f['side']=='YES'else UNIT-f['price'];break
    return {'exchangeId':exchange,'marketId':env.mapping[exchange],'option':None,
        'latestPrice':latest,'bestBid':bid,'bestAsk':ask,'spread':None if bid is None or ask is None else ask-bid,
        '_latest_recorded_or_counterfactual_only':True,'_option_source_available':False}


def request(env,key,ids=None):
    if key not in env.keys['SIG']:return {'status':'SOURCE_NOT_AVAILABLE'}
    if env.budget.active is not None:return {'status':'BUSY','ready_ms':env.busy_until}
    ready=env.budget.ready_at(env.now,False)
    if ready>env.now:return {'status':'RATE_WAIT','ready_ms':ready}
    if key[0]=='quote':ids=[key[1]]
    elif ids is None:ids=key[2]if len(key)==3 else env.quote_selection
    if isinstance(ids,str):ids=ids.split(',')
    env.feed_sequence+=1;rid=f'quote-read:{env.feed_sequence}'
    assert env.budget.admit(rid,env.now,False)
    env.quote_request_sequence+=1
    captured={'_request_at_ms':env.now,'_request_sequence':env.quote_request_sequence,'data':[],'missingIds':[],
        '_projection_complete':False,'_latest_price_tie_break':'execution_time_original_match_id_then_counterfactual'}
    try:
        if type(ids)not in(list,tuple)or not 1<=len(ids)<=100:raise ValueError('QUOTE_IDS_1_TO_100')
        if any(type(ex)is not str or not ex or(ex not in env.exchanges and re.fullmatch(r'\d+',ex)is None)for ex in ids):
            raise ValueError('QUOTE_IDS_FORMAT')
        captured['requestedIds']=tuple(ids)
        for ex in ids:
            if ex not in env.exchanges[1:]:captured['missingIds'].append(ex)
            else:captured['data'].append(snapshot(env,ex))
        captured['http_status']=200
    except ValueError as error:
        captured.update(data=[],missingIds=[],error=str(error),
            http_status=503 if str(error)=='REPLAY_QUOTE_SOURCE_UNAVAILABLE'else 400)
    env.busy_until=env.now+env.mechanics.read_latency_ms
    env.feed_requests.append({'venue':'SIG','at_ms':env.now,'key':key,'request_id':rid});env.feed_request_counts['SIG']+=1
    env.clock.schedule(env.busy_until,30,'api_quote_receipt',(rid,key,captured))
    return {'status':'IN_FLIGHT'}


def publish(env,response):
    r=deepcopy(response);r['_received_at_ms']=env.now
    version=(r['_request_at_ms'],r['_request_sequence'])
    last=env.last_quote_response
    if last is None or version>=(last['_request_at_ms'],last['_request_sequence']):env.last_quote_response=r
    if r['http_status']==200:
        for q in r['data']:
            old=env.known_quotes.get(q['exchangeId'])
            if old is None or version>=old['_version']:
                env.known_quotes[q['exchangeId']]={**q,'_version':version,'_request_at_ms':r['_request_at_ms'],
                    '_received_at_ms':env.now}
        for ex in r['missingIds']:
            old=env.known_quotes.get(ex)
            if old is not None and version>=old['_version']:env.known_quotes.pop(ex)
    env.observation_revision+=1


class BulkQuoteEpisode(ComponentEpisode):
    observation_protocol=BULK_QUOTES

    def __init__(self,*args,**kwargs):super().__init__(*args,**kwargs);initialize(self)

    def request_observation(self,venue,key,*,quote_ids=None,**kwargs):
        if venue=='SIG'and key[0]in('quote','bulk_prices'):return request(self,key,quote_ids)
        return super().request_observation(venue,key,**kwargs)

    def _event(self,at,kind,value):
        if kind=='api_quote_receipt':
            rid,key,response=value;self.budget.complete(rid,at,status=response['http_status']);publish(self,response)
            self.feed_receipts.append({'venue':'SIG','at_ms':at,'key':key});self.feed_receipt_counts['SIG']+=1;return
        return super()._event(at,kind,value)

    def observe(self):
        add_targets(self);base=super().observe();rows=[];entities=[];operations=[]
        # Dedicated contract slots preserve the older depth/ref/position/flow
        # features. A quote does not insert or renew anything in actor_books.
        for i,(kind,ex)in enumerate(base['entities']):
            q=self.known_quotes.get(ex)if kind=='contract'else None
            if q is not None:
                base['contracts'][i,25:33]=[(q[k]or 0)/UNIT for k in('bestBid','bestAsk','latestPrice','spread')]+[
                    clock_feature(self.now-q['_request_at_ms']),float(q['bestBid']is not None),
                    float(q['bestAsk']is not None),float(q['latestPrice']is not None)]
        for ex in self.exchanges[1:]:
            row=np.zeros(FEATURES,np.float32);row[0]=self.quote_selection.count(ex);row[24]=16.
            row[33:40]=identity('contract',ex);rows.append(row);entities.append(('quote_add',ex));operations.append((5,))
        for index,ex in enumerate(self.quote_selection):
            row=np.zeros(FEATURES,np.float32);row[0]=index/100.;row[24]=17.
            row[33:40]=identity('contract',ex);rows.append(row);entities.append(('quote_remove',index));operations.append((6,))
        row=np.zeros(FEATURES,np.float32);row[0]=len(self.quote_selection)/100.;row[24]=18.
        rows.append(row);entities.append(('quote_clear',None));operations.append((6,))
        if self.last_quote_response is not None:
            r=self.last_quote_response;row=np.zeros(FEATURES,np.float32)
            row[:5]=[r['http_status']/1000.,len(r['data'])/100.,len(r['missingIds'])/100.,
                clock_feature(self.now-r['_request_at_ms']),clock_feature(self.now-r['_received_at_ms'])]
            row[8:15]=identity('quote_read_error',r.get('error'));row[24]=19.
            rows.append(row);entities.append(('quote_response',None));operations.append(())
        offset=len(base['entities']);out=append_rows(base,rows,entities)
        for j,families in enumerate(operations):
            if families==(5,)and len(self.quote_selection)>=100:continue  # Actual endpoint packet limit.
            for f in families:out['masks']['family'][offset+j,f]=True
            out['masks']['market'][offset+j]=bool(families)
        return out

    def act_raw(self,action,observation):
        f=int(action['family']);i=int(action['market']);kind,key=observation['entities'][i]
        if kind in('quote_add','quote_remove','quote_clear'):
            if not observation['masks']['family'][i,f]:raise ValueError('ACTION_ENTITY_MISMATCH')
            if any(not 0<=float(action[k])<=1 for k in('target_fraction','child_fraction')):raise ValueError('RAW_LATENT_RANGE')
            if kind=='quote_add':
                if len(self.quote_selection)>=100:result={'status':'QUOTE_PACKET_MAX_100'}
                else:self.quote_selection.append(key);result={'status':'QUOTE_SELECTION_UPDATED'}
            elif kind=='quote_remove':self.quote_selection.pop(key);result={'status':'QUOTE_SELECTION_UPDATED'}
            else:self.quote_selection.clear();result={'status':'QUOTE_SELECTION_CLEARED'}
            self._last_local_action={'action':dict(action),'at_ms':self.now,'target_identity':_identity(kind,key),
                'result_status':result['status']}
            return result
        return super().act_raw(action,observation)


class BulkQuoteTape(ComponentTape):episode_type=BulkQuoteEpisode


class BulkQuotePolicy(ComponentPolicy):
    architecture='tartarus-raw-api-bulk-quotes-gru-v8'
    observation_protocol=BULK_QUOTES


def migrate_bulk_quotes(parent):
    if type(parent)is not ComponentPolicy:raise ValueError('EXACT_COMPONENT_PARENT_REQUIRED_FOR_QUOTE_MIGRATION')
    child=BulkQuotePolicy(parent.hidden_size).to(next(parent.parameters()).device);child.load_state_dict(parent.state_dict())
    child.global_state_encoding=getattr(parent,'global_state_encoding','raw');child.train(parent.training)
    return child,{'operator':'explicit_paid_bulk_quote_reads_and_local_subset_planning',
        'old_architecture':parent.architecture,'new_architecture':child.architecture,'all_weights_preserved_exactly':True,
        'optimizer_reset_required':True,'old_trajectories_must_not_be_reused':True,'old_scores_must_not_be_reused':True,
        'independent_test_qualification':False}
