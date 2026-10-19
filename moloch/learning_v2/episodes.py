"""Predecoded causal development episodes and mechanics-only policy actions."""
import bisect
import gzip
import hashlib
import json
import math
from pathlib import Path
import numpy as np
from .account import UNIT, Target, project, wealth
from .liquidity import Book
from .reference_env import ReferenceEnv
from .policy import FAMILIES, HEAD_SIZES

CONTRACT_FEATURES = 24
GLOBAL_FEATURES = 14
FEATURE_SCHEMA = 'causal_capacity_24_14_v2'


class Tape:
    def __init__(self, directory: Path):
        raw = (directory/'dataset-manifest.json').read_bytes()
        complete = json.loads((directory/'COMPLETE.json').read_text())
        if hashlib.sha256(raw).hexdigest() != complete['manifestSha256']:
            raise ValueError('DATASET_MANIFEST_CHANGED')
        self.manifest = json.loads(raw); self.manifest_sha256 = hashlib.sha256(raw).hexdigest()
        path = directory/'observations.jsonl.gz'
        if hashlib.sha256(path.read_bytes()).hexdigest() != self.manifest['observationsSha256']:
            raise ValueError('COMPILED_OBSERVATIONS_CHANGED')
        with gzip.open(path,'rt',encoding='utf-8') as src: self.rows = tuple(json.loads(line) for line in src)
        self.times = tuple(row['available_at_ms'] for row in self.rows)
        if not self.times or any(a>b for a,b in zip(self.times,self.times[1:])):
            raise ValueError('TAPE_INFORMATION_CLOCK')
        self.exchanges = ('__GLOBAL_WAIT__',) + tuple(sorted({r['exchange_id'] for r in self.rows if r['kind']=='book'}))
        self.index = {ex:i for i,ex in enumerate(self.exchanges)}

    def episode(self, mechanics, start_ms, end_ms, *, initial_cash_micros, target_wealth_micros, burnin_ms=60_000, advance_to_start=True):
        if start_ms-burnin_ms < self.times[0] or end_ms > self.times[-1] or start_ms>=end_ms:
            raise ValueError('EPISODE_OUTSIDE_AVAILABLE_CHRONOLOGY')
        env = Episode(mechanics,start_ms-burnin_ms,initial_cash_micros,self.exchanges,target_wealth_micros,start_ms,end_ms)
        lo = bisect.bisect_left(self.times,start_ms-burnin_ms)
        hi = bisect.bisect_right(self.times,end_ms)
        for row in self.rows[lo:hi]: env.clock.schedule(row['available_at_ms'],10,'tape',row)
        if advance_to_start: env.advance(start_ms)
        return env


class Episode(ReferenceEnv):
    def __init__(self, mechanics, clock_start, cash, exchanges, target, start, end):
        super().__init__(mechanics,clock_start,cash)
        self.exchanges = exchanges; self.target_wealth = target; self.initial_wealth = cash
        self.start = start; self.end = end
        self.scope = {}; self.reference_cache = {}; self.mapping = {}
        self.action_projections = []; self._depth_totals = {}

    def _displayed_totals(self, ex, book):
        cached=self._depth_totals.get(ex)
        if cached is not None and cached[0] is book:return cached[1:]
        totals=(sum(p*q for p,q in book.bids),sum(p*q for p,q in book.asks))
        # The compiled Tape constructs frozen Books with tuple/int levels.
        # Mutable caller-owned level lists retain their original recomputation.
        if type(book.bids)is tuple and type(book.asks)is tuple and all(type(x)is tuple for x in (*book.bids,*book.asks)):
            self._depth_totals[ex]=(book,*totals)
        else:self._depth_totals.pop(ex,None)
        return totals

    def _refresh_reference(self, market):
        points = [(k,r) for k,r in self.reference_cache.items() if k[0]==market and r['mapping_kind']=='direct_party_reference']
        for ex in self.scope.get(market,[]):
            for venue in ['Kalshi','Polymarket']:
                candidates = [r for k,r in points if r['venue']==venue]
                if not candidates:
                    self.add_reference_gap(ex,self.now,venue); continue
                r = max(candidates,key=lambda r:r['available_at_ms'])
                self.add_reference(ex,r['price_yes_micros'],self.now,r['original_sha256'],
                                   received_at_ms=r['received_at_ms'],source_at_ms=r.get('source_at_ms'),source_kind=venue)

    def _event(self, at, kind, value):
        if kind != 'tape': return super()._event(at,kind,value)
        r = value; market = r.get('market_id')
        if r['kind']=='book':
            ex = r['exchange_id']; self.scope.setdefault(market,set()).add(ex); self.mapping[ex] = market
            b = Book(ex,at,r.get('source_at_ms'),tuple(map(tuple,r['bids'])),tuple(map(tuple,r['asks'])),r['original_sha256'])
            super()._event(at,'book',b); self._refresh_reference(market)
        elif r['kind']=='gap':
            selected = [r['exchange_id']] if r.get('exchange_id') else list(self.scope.get(market,[])) if market else list(self.books)
            for ex in selected:
                if ex in self.books: self.books[ex].invalidate()
        elif r['kind']=='reference':
            self.reference_cache[(market,r.get('source',r['venue']))] = r
            self._refresh_reference(market)
        elif r['kind']=='reference_gap':
            self.reference_cache.pop((market,r.get('source',r['venue'])),None)
            self._refresh_reference(market)
        else: raise ValueError('UNKNOWN_TAPE_OBSERVATION')

    def _reference(self, ex, venue):
        ref = self.references.get((ex,venue))
        valid = (ref is not None and 0<=self.now-ref.received_at_ms<=self.mechanics.maximum_reference_age_ms
                 and (ref.source_at_ms is None or 0<=self.now-ref.source_at_ms<=self.mechanics.maximum_reference_age_ms))
        return ref, valid

    def observe(self):
        account = self.account(); marks = self.marks(); w = wealth(account,marks)
        n = len(self.exchanges); features = np.zeros((n,CONTRACT_FEATURES),np.float32)
        present = np.zeros(n,bool); present[0] = True
        masks = {'market':np.zeros(n,bool),'family':np.zeros((n,len(FAMILIES)),bool),
                 'side':np.ones((n,len(FAMILIES),2),bool),
                'target_scale':np.ones((n,len(FAMILIES),2),np.float32),
                 **{k:np.ones((n,len(FAMILIES),2,HEAD_SIZES[k]),bool)for k in ['price','patience']}}
        masks['market'][0] = True; masks['family'][:,0] = True
        masks['side'][:,[1,2,4],:] = False
        busy = self.budget.active is not None or any(o.phase=='preflight' for o in self.orders.values())
        for i,ex in enumerate(self.exchanges[1:],1):
            q = self.books.get(ex)
            if q is None: continue
            present[i] = True; masks['market'][i] = True
            b = q.book; bid = b.bids[0][0] if b.bids else 0; ask = b.asks[0][0] if b.asks else UNIT
            valid_book = q.visible(self.now,self.mechanics.maximum_book_age_ms)
            k,kv = self._reference(ex,'Kalshi'); p,pv = self._reference(ex,'Polymarket')
            def fraction(amount): return amount/w if w is not None else 0.
            cost = account.market_cost.get(ex,0)
            buy_pending = account.pending_market_cost.get(ex,0)
            sell_pending = sum(v for (e,s),v in account.pending_sells.items()if e==ex)
            pending_yes=account.pending_buys.get((ex,'YES'),0)-account.pending_sells.get((ex,'YES'),0)
            pending_no=account.pending_buys.get((ex,'NO'),0)-account.pending_sells.get((ex,'NO'),0)
            bid_total,ask_total=self._displayed_totals(ex,b)
            features[i] = [bid/UNIT,ask/UNIT,(ask-bid)/UNIT,
                           (self.now-(b.source_at_ms if b.source_at_ms is not None else b.available_at_ms))/10000,
                           fraction(bid_total),fraction(ask_total),
                           k.price_yes/UNIT if k else 0.,float(kv),(self.now-k.received_at_ms)/20000 if k else 0.,
                           p.price_yes/UNIT if p else 0.,float(pv),(self.now-p.received_at_ms)/20000 if p else 0.,
                           fraction(account.quantities.get((ex,'YES'),0)*UNIT),fraction(account.quantities.get((ex,'NO'),0)*UNIT),
                           fraction(cost),fraction(buy_pending),fraction(sell_pending*UNIT),float(valid_book),0.,
                           abs(k.price_yes-p.price_yes)/UNIT if kv and pv else 0.,
                           fraction(max(0,self.mechanics.limits.maximum_market_cost-cost-buy_pending)),
                           fraction(self.mechanics.limits.maximum_order_quantity*UNIT),
                           fraction(pending_yes*UNIT),fraction(pending_no*UNIT)]
            for side_id,side in enumerate(['YES','NO']):
                opposite='NO'if side=='YES'else'YES'
                net=(account.quantities.get((ex,side),0)-account.quantities.get((ex,opposite),0)
                     +account.pending_buys.get((ex,side),0)-account.pending_buys.get((ex,opposite),0)
                     -account.pending_sells.get((ex,side),0)+account.pending_sells.get((ex,opposite),0))
                mark=marks.get((ex,side),0)
                masks['target_scale'][i,2,side_id]=min(1.,max(1e-9,net*mark/w))if w else 1.
            # The initial marked-capital experiment requires a current mark
            # for a newly acquired position. This is an observability mask,
            # not a signal or a synthetic zero-return valuation.
            mark_available = kv if self.mechanics.valuation_source=='Kalshi' else pv
            # A known own resting order can be cancelled even when its public
            # book/reference is stale. Match ReferenceEnv.cancel's state gate.
            masks['family'][i,3] = self.budget.active is None and any(
                o.request.exchange==ex and o.phase=='resting' and not o.cancel_requested
                for o in self.orders.values())
            if busy or not valid_book or w is None or not mark_available: continue
            for side_id,side in enumerate(['YES','NO']):
                buys = ask if side=='YES' else UNIT-bid
                sells = bid if side=='YES' else UNIT-ask
                projected={}
                for family,action,price in [(1,'BUY',buys),(2,'SELL',sells),(4,'SELL',sells)]:
                    if not 0<price<UNIT: continue
                    if action not in projected:
                        t = Target(ex,side,UNIT if action=='BUY' else 0,UNIT,price,action)
                        projected[action] = project(account,marks,t,self.mechanics.limits).admitted_quantity>0
                    allowed = projected[action]
                    masks['side'][i,family,side_id] = allowed
                    for offset in range(5):
                        masks['price'][i,family,side_id,offset] = 0<price+(offset-2)*5000<UNIT
                # CANCEL targets any known own outstanding order, not anonymous quotes.
            for family in [1,2,4]: masks['family'][i,family] = masks['side'][i,family].any()
        # Keep every observed contract in the information set, but give WAIT
        # exactly one action identity. Hundreds of stale-market WAIT aliases
        # otherwise dominate exploration and its entropy bonus.
        masks['market'][1:] = masks['family'][1:,1:].any(-1)
        masks['family'][1:,0] = False
        def fraction(amount): return amount/w if w is not None else 0.
        active = {ex for (ex,side),qty in account.quantities.items()if qty}
        active |= {ex for (ex,side),qty in account.pending_buys.items()if qty}
        cap = self.mechanics.limits.maximum_fresh_markets
        remaining_slots = max(0,cap-len(active-self.mechanics.limits.inherited_exchanges)) if cap is not None else 0
        global_state = np.array([fraction(account.cash),fraction(account.reserved_cash),fraction(account.spendable),fraction(account.liabilities),
                                 math.log(w/self.initial_wealth)if w else 0.,self.target_wealth/w if w else 0.,
                                 max(0,self.end-self.now)/(self.end-self.start),fraction(account.owned_cost),
                                 max(0,self.budget.next_at-self.now)/60000,float(busy),float(remaining_slots),float(w is not None)],np.float32)
        global_state=np.concatenate([global_state,np.array([
            fraction(max(0,self.mechanics.limits.maximum_committed-account.owned_cost-account.reserved_commitment)),
            fraction(self.mechanics.limits.maximum_committed)],np.float32)])
        return {'contracts':features,'global_state':global_state,'present':present,'masks':masks,'wealth':w}

    def act(self, action):
        family = FAMILIES[int(action['family'])]
        ex = self.exchanges[int(action['market'])]
        if family=='WAIT': return None
        if family=='CANCEL':
            orders = [o.id for o in self.orders.values()if o.request.exchange==ex and o.phase=='resting' and not o.cancel_requested]
            return self.cancel(min(orders)) if orders else False
        side = ['YES','NO'][int(action['side'])]
        q = self.books.get(ex)
        if q is None: return None
        book = q.book; action_kind = 'BUY' if family=='BUY' else 'SELL'
        if action_kind=='BUY':
            base = book.asks[0][0] if side=='YES' else UNIT-book.bids[0][0]
        else: base = book.bids[0][0] if side=='YES' else UNIT-book.asks[0][0]
        limit = base+(int(action['price'])-2)*5000
        target = 0 if family=='EXIT' else int(float(action['target_fraction'])*UNIT)
        child = int(float(action['child_fraction'])*UNIT)
        t = Target(ex,side,target,child,limit,action_kind)
        projection = self.submit(t,[1000,10000,60000][int(action['patience'])])
        self.action_projections.append((self.now,t,projection))
        return projection
