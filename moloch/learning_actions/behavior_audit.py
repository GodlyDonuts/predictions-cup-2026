"""Observe policy sizing and timing without changing the simulated trajectory."""
from collections import Counter, deque
import math

from .contract import MAX_QUANTITY


class BehaviorAudit:
    def __init__(self, env):
        self.env=env;self.counters=Counter();self.statuses=Counter();self.samples=deque(maxlen=128)
        self.alarms=[];self.quantities=[];self._observed=None;self._finance=None
        self.original_observe=env.observe;self.original_action=env.act_raw;self.original_canonical=env._canonical
        env.observe=self.observe;env.act_raw=self.action;env._canonical=self.canonical

    def observe(self):
        observation=self.original_observe()
        # Copy already-delivered finance at the actual decision observation.
        # The diagnostic never returns this dictionary to the model.
        self._observed=observation
        self._finance=(self.env.known_cash,{key:sum(n for n,_ in lots)for key,lots in self.env.known_lots.items()})
        self.counters['observations']+=1
        contracts=[i for i,(kind,_)in enumerate(observation['entities'])if kind=='contract']
        self.counters['observed_contract_rows']+=len(contracts)
        for i in contracts:
            row=observation['contracts'][i]
            self.counters['delivered_SIG_book_rows']+=int(bool(row[7]))
            for venue,start in [('Kalshi',8),('Polymarket',11)]:
                self.counters['delivered_'+venue+'_reference_rows']+=int(bool(row[start]or row[start+1]))
        return observation

    def action(self, action, observation):
        if observation is not self._observed:raise ValueError('AUDIT_REQUIRES_EXACT_DECISION_OBSERVATION')
        family=int(action['family']);kind,key=observation['entities'][int(action['market'])]
        record={'at_ms':self.env.now,'family':family,'kind':kind}
        if family in (1,2,5,6):
            fraction=float(action['target_fraction'])
            quantity=min(MAX_QUANTITY,1+int(math.expm1(fraction*math.log(MAX_QUANTITY))))
            side=('YES','NO')[int(action['side'])];cash,lots=self._finance;held=lots.get((key,side),0)
            record.update(exchange=key,side=side,quantity=quantity,delivered_held=held,delivered_cash_micros=cash)
            self.quantities.append(quantity);self.counters['order_action_attempts']+=1
            if family in (2,6):
                self.counters['sell_action_attempts']+=1
                self.counters['sell_exceeds_delivered_holding']+=int(quantity>held)
                self.counters['sell_with_positive_delivered_holding']+=int(held>0)
                self.counters['sell_exact_delivered_holding']+=int(held>0 and quantity==held)
        result=self.original_action(action,observation)
        record['immediate_status']=str(result['status'])
        self.statuses[f'{family}:{result["status"]}']+=1
        if result['status']=='ALARM':
            delay=max(0,result['ready_ms']-self.env.now)
            self.alarms.append(delay)
            self.counters['uninterruptible_alarms']+=int(not result.get('wake_on_observation',True))
            self.counters['alarms_past_episode_end']+=int(result['ready_ms']>=self.env.end)
        self.samples.append(record)
        return result

    def canonical(self, spec):
        result=self.original_canonical(spec)
        self.counters['canonicalization_calls']+=1
        if spec.action=='SELL':
            self.counters['canonical_sell_calls']+=1
            self.counters['sell_calls_converted_to_complement_buy']+=int(result.action=='BUY')
        return result

    def report(self):
        def distribution(values):
            if not values:return {'count':0}
            a=sorted(values)
            return dict(count=len(a),minimum=a[0],median=a[len(a)//2],p95=a[min(len(a)-1,math.ceil(.95*len(a))-1)],maximum=a[-1])
        fills=Counter(f'{f["action"]}_{f["side"]}'for f in self.env.api_fills)
        return dict(counters=dict(self.counters),immediateStatuses=dict(self.statuses),
            requestedQuantities=distribution(self.quantities),requestedAlarmDelayMs=distribution(self.alarms),
            actualFillActions=dict(fills),recentActions=list(self.samples),
            canonicalizationIsAttemptCountNotCommittedOrderCount=True,
            diagnosticTruthReturnedToPolicy=False,actionsModified=False,optimizerRun=False,
            independentTestUsed=False,productionChanged=False)

    def close(self):
        self.env.observe=self.original_observe;self.env.act_raw=self.original_action
        self.env._canonical=self.original_canonical
