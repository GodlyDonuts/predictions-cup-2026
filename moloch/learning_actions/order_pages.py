"""Paid scoped order pages and their read-only actor representation."""
from copy import deepcopy
import numpy as np
from .full_depth import FullDepthEpisode,FullDepthTape,FEATURES
from .order_history import OrderHistory,STATUSES
from .observation_protocol import ORDER_PAGES
from .feature_cache import identity,log_quantity


def clock_feature(delta):return np.sign(delta)*np.log1p(abs(delta))/32.


def enable(env):
    if env.api_id or env.api_orders or env.api_fills:raise ValueError('ORDER_HISTORY_REQUIRES_NEW_EMPTY_PAPER_ORDER_ACCOUNT')
    env.order_history=OrderHistory('new-counterfactual-paper-account');env.known_order_pages={};env.order_page_scans={}
    env.order_page_issued_cursors={}
    env.order_page_read_keys=set(env.keys['SIG'])


def add_targets(env):
    keys=[('owned_orders','exchange',ex)for ex in env.exchanges[1:]]
    keys.extend(('owned_orders','market',m)for m in env.scope)
    for key in keys:
        if key not in env.order_page_read_keys:env.keys['SIG'].append(key);env.order_page_read_keys.add(key)


def scope(key):
    if key==('owned_orders',):return None,None
    if len(key)!=3 or key[0]!='owned_orders'or key[1]not in('exchange','market'):raise ValueError('ORDER_READ_TARGET')
    return(key[2],None)if key[1]=='exchange'else(None,key[2])


def request(env,key,*,status='open',limit=50,continuation=False,cursor=None,exchange=None,market=None):
    if key not in env.keys['SIG']:return {'status':'SOURCE_NOT_AVAILABLE'}
    if env.budget.active is not None:return {'status':'BUSY','ready_ms':env.busy_until}
    ready=env.budget.ready_at(env.now,False)
    if ready>env.now:return {'status':'RATE_WAIT','ready_ms':ready}
    if len(key)==3 and key[1]=='cursor':
        status,exchange,market=env.order_page_issued_cursors[key[2]];cursor=key[2]
    else:
        target_exchange,target_market=scope(key)
        exchange=target_exchange if exchange is None else exchange;market=target_market if market is None else market
    query_scope=(status,exchange,market)
    if cursor is None and continuation:
        cursor=env.known_order_pages.get(query_scope,{}).get('pagination',{}).get('nextCursor')
    env.feed_sequence+=1;rid=f'owned-order-read:{env.feed_sequence}'
    assert env.budget.admit(rid,env.now,False)
    try:
        page=env.order_history.page(env.now,status=status,limit=limit,exchange=exchange,market=market,
            mapping=env.mapping,cursor=cursor,revision=env.finance_revision)
    except ValueError as error:
        page={'error':str(error),'data':[],'pagination':{},'coverage':{'complete':False},'_scope':query_scope}
    env.busy_until=env.now+env.mechanics.read_latency_ms
    env.feed_requests.append({'venue':'SIG','at_ms':env.now,'key':key,'request_id':rid});env.feed_request_counts['SIG']+=1
    env.clock.schedule(env.busy_until,30,'api_order_page_receipt',(rid,key,page))
    return {'status':'IN_FLIGHT'}


def publish(env,page):
    from .execution_receipts import orders
    page=deepcopy(page);page['_received_at_ms']=env.now;env.known_order_pages[page['_scope']]=page
    if 'error'in page:env.observation_revision+=1;return
    token=page['pagination']['nextCursor']
    if token is not None:
        env.order_page_issued_cursors[token]=page['_scope'];key=('owned_orders','cursor',token)
        if key not in env.order_page_read_keys:env.keys['SIG'].append(key);env.order_page_read_keys.add(key)
    revision=page['_revision'];rows=page['data'];status,exchange,market=page['_scope']
    orders(env,{r.order.id:r.order if r.open else None for r in rows},revision)
    scans=env.order_page_scans;requested=page['_requested_cursor']
    # A delivered cursor retains its immutable observed prefix. Revisiting it
    # may branch the traversal; another branch must not overwrite that proof.
    parent=scans.get(requested)if requested is not None else None
    chain=requested is None or parent is not None
    node=(parent,tuple(r.order.id for r in rows),page['_opened'],page['_first_revision'])if chain else None
    if token is not None and node is not None:scans[token]=node
    if chain and not page['pagination']['hasMore']and page['coverage']['complete']and status in('open','all'):
        seen=set();part=node
        while part is not None:
            seen.update(part[1]);part=part[0]
        absent={}
        for q,o in env.known_orders.items():
            matches=(exchange is None or o.spec.exchange==exchange)and(market is None or env.mapping.get(o.spec.exchange)==market)
            if matches and q not in seen and o.placed_at_ms<=node[2]:absent[q]=None
        orders(env,absent,node[3])
        # A multi-page keyset traversal is not one atomic account baseline.
        if requested is None and exchange is None and market is None:
            env.known_resource_revisions['orders']=max(env.known_resource_revisions['orders'],revision)
    env.finance_observation_count+=1;env.observation_revision+=1


def extend(env,base):
    rows=[];entities=[]
    for key,page in sorted(env.known_order_pages.items(),key=lambda item:repr(item[0])):
        status,exchange,market=key;header=np.zeros(FEATURES,np.float32)
        header[:6]=[STATUSES.index(status)/3 if status in STATUSES else-1,len(page['data'])/200.,
            bool(page['pagination'].get('hasMore')),bool(page['pagination'].get('nextCursor')),
            float('error'in page),clock_feature(env.now-page['_received_at_ms'])]
        header[8:15]=identity('order_page_status',page.get('error','ok'));header[24]=12.
        header[33:40]=identity('order_query',key);rows.append(header);entities.append(('order_page',key))
        for record in page['data']:
            o=record.order;row=np.zeros(FEATURES,np.float32)
            # Documented list quantity is executable remaining while open;
            # closed quantity includes remaining plus total fills.
            quantity=o.remaining if record.open else o.remaining+record.filled
            row[:8]=[log_quantity(quantity)/16.,float(record.open),log_quantity(record.filled)/16. if not record.open else 0,
                (o.spec.price or 0)/1000000.,float(o.spec.side=='YES'),float(o.spec.action=='BUY'),
                clock_feature(env.now-o.placed_at_ms),float(o.spec.expiration_ms is not None)]
            row[8:15]=identity('order_terminal_reason',record.terminal_reason)
            row[15]=clock_feature(o.spec.expiration_ms-env.now)if o.spec.expiration_ms is not None else 0.
            row[24]=13.;row[25:32]=identity('order_id',o.id);row[33:40]=identity('contract',o.spec.exchange)
            rows.append(row);entities.append(('order_page_row',(key,o.id)))
    if not rows:return base
    n=len(rows)
    return {**base,'contracts':np.concatenate((base['contracts'],np.stack(rows))),
        'present':np.concatenate((base['present'],np.ones(n,bool))),
        'masks':{k:np.concatenate((v,np.zeros((n,*v.shape[1:]),dtype=v.dtype)))for k,v in base['masks'].items()},
        'entities':[*base['entities'],*entities]}


class OrderPageEpisode(FullDepthEpisode):
    observation_protocol=ORDER_PAGES
    def __init__(self,*args,**kwargs):super().__init__(*args,**kwargs);enable(self)
    def observe(self):add_targets(self);return extend(self,super().observe())


class OrderPageTape(FullDepthTape):episode_type=OrderPageEpisode
