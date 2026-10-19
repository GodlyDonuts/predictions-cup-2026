"""Observed conditional intervals and funded-capital sizing for offline learning.

Unknown absolute portfolio wealth stays unknown. The original funded cash is
only the explicit denominator for actions/features; it creates no cash, margin,
reference marks or executable liquidity. External settlement equivalence is
not established by same-party mapping.
"""
import numpy as np
from .account import UNIT
from .episodes import Episode, Tape

CONTRACT_FEATURES=32
GLOBAL_FEATURES=16
FEATURE_SCHEMA='observed_conditional_intervals_funded_sizing_32_16_v4'


class IntervalEpisode(Episode):
    def __init__(self,*args,**kwargs):
        super().__init__(*args,**kwargs)
        self.sizing_capital_micros=self.initial_wealth
        if type(self.sizing_capital_micros)is not int or self.sizing_capital_micros<=0:
            raise ValueError('ORIGINAL_POSITIVE_FUNDED_CAPITAL')

    def venue_interval(self,ex,venue):
        rows=[r for (market,_),r in self.reference_cache.for_market(self.mapping.get(ex))
              if r.get('venue')==venue
              and r.get('mapping_kind')=='direct_party_reference'
              and r.get('mapping_semantics')=='same_party_conditional_not_contract_equivalence']
        if not rows:return None
        r=max(rows,key=lambda r:r['available_at_ms'])
        for key in ['available_at_ms','received_at_ms']:
            if type(r.get(key))is not int or not 0<=self.now-r[key]<=self.mechanics.maximum_reference_age_ms:return None
        source=r.get('source_at_ms')
        if source is not None and (type(source)is not int or not 0<=self.now-source<=self.mechanics.maximum_reference_age_ms):return None
        bid,ask=r.get('bid_yes_micros'),r.get('ask_yes_micros')
        if type(bid)is not int or type(ask)is not int or not 0<=bid<=ask<=UNIT:return None
        return bid,ask

    def _reference(self,ex,venue):
        ref,valid=super()._reference(ex,venue)
        return ref,valid and self.venue_interval(ex,venue)is not None

    def marks(self):
        result={}
        for ex in self.exchanges[1:]:
            interval=self.venue_interval(ex,self.mechanics.valuation_source)
            if interval is not None:
                midpoint=sum(interval)//2
                result[(ex,'YES')]=midpoint;result[(ex,'NO')]=UNIT-midpoint
        return result

    def yes_intervals(self):
        result={}
        for ex in self.exchanges[1:]:
            ranges=[x for v in ['Kalshi','Polymarket'] if (x:=self.venue_interval(ex,v))is not None]
            # Union preserves venue disagreement. Intersecting intervals could
            # falsely imply more precision or yield an inverted interval.
            if ranges:result[ex]=(min(x[0]for x in ranges),max(x[1]for x in ranges))
        return result

    def observe(self):
        observation=super().observe();n=len(self.exchanges);extra=np.zeros((n,8),np.float32)
        reads=self.mechanics.preflight_reads
        read_span=reads*self.mechanics.read_latency_ms+max(0,reads-1)*self.mechanics.budget.read_completion_gap_ms
        if self.budget.active is not None or self.budget.unknown or self.budget.poisoned:
            minimum_delay=60000
        else:
            read_wait=max(0,self.budget.ready_at(self.now,False)-self.now)if reads else 0
            minimum_delay=max(read_wait+read_span,max(0,self.budget.ready_at(self.now,True)-self.now))
        feasible=np.array([patience>minimum_delay for patience in [1000,10000,60000]],bool)
        masks=observation['masks']
        # Patience is measured from decision, including all preflight reads.
        # Mask only choices that cannot reach admission even at the modeled
        # physical lower bound. Never reward abandoned drafts as actual trades.
        for family in [1,2,4]:
            masks['patience'][:,family,:,:]&=feasible
            if not feasible.any():masks['side'][:,family,:]=False
            masks['family'][:,family]=masks['side'][:,family,:].any(-1)
        masks['market'][1:]=masks['family'][1:,1:].any(-1)
        for i,ex in enumerate(self.exchanges[1:],1):
            for j,venue in enumerate(['Kalshi','Polymarket']):
                interval=self.venue_interval(ex,venue)
                if interval is not None:
                    bid,ask=interval;extra[i,j*4:j*4+4]=[bid/UNIT,ask/UNIT,(ask-bid)/UNIT,1.]
        observation['contracts']=np.concatenate([observation['contracts'],extra],axis=-1)
        observation['global_state']=np.concatenate([observation['global_state'],np.array([
            self.sizing_capital_micros/self.initial_wealth,float(observation['wealth']is None)],np.float32)])
        observation['sizing_capital_micros']=self.sizing_capital_micros
        observation['minimum_modeled_admission_delay_ms']=minimum_delay
        observation['valuation_classification']='conditional_external_intervals_not_executable_or_realized_roi'
        return observation


class IntervalTape(Tape):
    episode_type=IntervalEpisode
