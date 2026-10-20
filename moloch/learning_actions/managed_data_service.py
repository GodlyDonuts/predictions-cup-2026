"""Optional actor-pausable data collection on the existing simulated queues.

Every payload still uses request_observation and its original paid receipt.
Only known book/live-reference targets are collected; missing history is not
invented. A waiting foreground SIG operation gets the next actor turn before
the background service competes for that slot.
"""
import math

PROFILE='model_pausable_book_reference_service_v1'
EVENT='managed_market_data_poll'


def profile(value):
    if value not in(None,PROFILE)or type(value)not in(type(None),str):
        raise ValueError('EXPLICIT_MANAGED_DATA_PROFILE')
    return value


def specification(value):
    if value is None:return None
    if(not isinstance(value,dict)or not {'profile','initially_enabled'}<=set(value)
            or set(value)-{'profile','initially_enabled','finance_period_ms'}
            or profile(value['profile'])!=PROFILE or type(value['initially_enabled'])is not bool):
        raise ValueError('EXPLICIT_MANAGED_DATA_CONFIGURATION')
    period=value.get('finance_period_ms')
    if period is not None and(type(period)is not int or period<1):raise ValueError('EXPLICIT_POSITIVE_FINANCE_READ_PERIOD')
    return value


def advance_alarm(env,before,result):
    if result.get('wake_on_observation',True):
        target=min(env.end,max(before+1,env.decision_times(result.get('ready_ms'))))
        env.advance_until_observable(target)
    else:
        target=result.get('ready_ms')
        if type(target)is not int or target<env.now:raise ValueError('EXACT_FUTURE_UNINTERRUPTED_ALARM_REQUIRED')
        env.advance(min(env.end,max(before+1,target)))


class ManagedDataService:
    def __init__(self,env,config):
        specification(config);self.env=env;self.enabled=config['initially_enabled'];self.closed=False
        self.sequence=0;self.pending={v:None for v in('SIG','Kalshi','Polymarket')}
        self.cursors={v:0 for v in self.pending};self.catalog={v:None for v in self.pending}
        self.requests={v:0 for v in self.pending};self.waits=0;self.actor_ready=None;self.waiting_actor=False
        self.target_requests={};self.finance_period=config.get('finance_period_ms');self.finance_due=env.now;self.finance_cursor=0
        self.in_background=False
        self.original_request=env.request_observation;self.original_dispatch=env.dispatch
        self.original_action=env.act_raw;self.original_remember=env._remember
        env.request_observation=self.request;env.dispatch=self.dispatch
        env.act_raw=self.action;env._remember=self.remember
        env.managed_data_service=self;env.managed_data_profile=PROFILE
        if self.enabled:
            for venue in self.pending:self.queue(venue,env.now)

    def queue(self,venue,at):
        if not self.enabled or self.closed or at>=self.env.end:return
        old=self.pending[venue]
        if old is not None and old[0]<=at:return
        self.sequence+=1;self.pending[venue]=(at,self.sequence)
        self.env.clock.schedule(at,25,EVENT,(venue,self.sequence))

    def remember(self,venue,key,row):
        new=key not in self.env.latest;result=self.original_remember(venue,key,row)
        if new:
            self.catalog[venue]=None
            if self.allowed(venue,key):self.queue(venue,self.env.now)
        return result

    @staticmethod
    def allowed(venue,key):return key[0]==('book'if venue=='SIG'else'reference')

    def foreground(self,result):
        if self.in_background:return
        if result['status']in('RATE_WAIT','BUSY'):
            self.actor_ready=max(self.env.now+1,result['ready_ms'])
        elif result['status']=='IN_FLIGHT':self.actor_ready=None

    def request(self,venue,key,**kwargs):
        result=self.original_request(venue,key,**kwargs)
        if venue=='SIG':self.foreground(result)
        return result

    def dispatch(self,*args,**kwargs):
        result=self.original_dispatch(*args,**kwargs);self.foreground(result);return result

    def action(self,action,observation):
        result=self.original_action(action,observation)
        family=int(action['family']);kind,key=observation['entities'][int(action['market'])]
        sig_attempt=(family in(1,2,3,4,7,8,9)or family==10 and kind=='read'and key[0]=='SIG')
        if not sig_attempt or result['status']not in('RATE_WAIT','BUSY'):self.actor_ready=None
        if self.waiting_actor and self.actor_ready is None:
            self.actor_ready=None;self.waiting_actor=False;self.queue('SIG',self.env.now)
        return result

    def toggle(self):
        self.enabled=not self.enabled;self.actor_ready=None;self.waiting_actor=False
        self.pending={v:None for v in self.pending};self.env.observation_revision+=1
        if self.enabled:
            for venue in self.pending:self.queue(venue,self.env.now)
        return {'status':'DATA_SERVICE_STARTED'if self.enabled else'DATA_SERVICE_PAUSED'}

    def features(self):
        return [float(self.enabled),float(self.waiting_actor),
                *[math.log1p(self.requests[v])for v in('SIG','Kalshi','Polymarket')]]

    def event(self,at,value):
        venue,sequence=value
        if self.pending[venue]!=(at,sequence):return
        self.pending[venue]=None
        if not self.enabled or self.closed:return
        env=self.env
        if venue=='SIG':
            if env.budget.unknown:self.waits+=1;self.queue(venue,at+1000);return
            if env.budget.active is not None:self.waits+=1;self.queue(venue,max(at+1,env.busy_until));return
            ready=env.budget.ready_at(at,False)
            if ready>at:self.waits+=1;self.queue(venue,ready);return
            if self.actor_ready is not None:
                if at<self.actor_ready:self.queue(venue,self.actor_ready);return
                self.waiting_actor=True;env.observation_revision+=1;return
        finance=venue=='SIG'and self.finance_period is not None and at>=self.finance_due
        if finance:key=(('account',),('positions',),('owned_orders',))[self.finance_cursor]
        else:
            keys=self.catalog[venue]
            if keys is None:
                keys=tuple(k for k in env.keys[venue]if self.allowed(venue,k)and k in env.latest)
                self.catalog[venue]=keys
            if not keys:
                if venue=='SIG'and self.finance_period is not None:self.queue(venue,self.finance_due)
                return  # A genuine later catalog receipt wakes this venue.
            key=keys[self.cursors[venue]%len(keys)]
        self.in_background=True
        kwargs={'order_limit':200,'continue_order_page':True}if finance and key[0]=='owned_orders'else{}
        try:result=self.original_request(venue,key,**kwargs)
        finally:self.in_background=False
        status=result['status']
        if status=='IN_FLIGHT':
            self.requests[venue]+=1
            name=venue+'/'+key[0];self.target_requests[name]=self.target_requests.get(name,0)+1
            if finance:
                self.finance_cursor=(self.finance_cursor+1)%3
                if self.finance_cursor==0:self.finance_due=at+self.finance_period
            else:self.cursors[venue]+=1
            env.feed_requests[-1]['request_owner']='managed_data_service'
            self.queue(venue,env.busy_until if venue=='SIG'else max(at+1,env.external_next[venue]))
        elif status in('RATE_WAIT','BUSY'):
            self.waits+=1;self.queue(venue,max(at+1,result['ready_ms']))
        elif status=='UNRESOLVED_EXECUTION':self.waits+=1;self.queue(venue,at+1000)
        else:raise ValueError('MANAGED_DATA_REQUEST_FAILED:'+status)

    def close(self):
        if self.closed:return
        self.closed=True;self.enabled=False;self.pending={v:None for v in self.pending}
        env=self.env;env.request_observation=self.original_request;env.dispatch=self.original_dispatch
        env.act_raw=self.original_action;env._remember=self.original_remember;env.managed_data_service=None;env.managed_data_profile=None
