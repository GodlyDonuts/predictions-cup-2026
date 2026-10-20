"""Post-hoc paper wealth attribution; never actor information or a reward.

Fill-time midpoint changes are not executable profits. The residual includes
mark movement and any other non-fill economic changes, not proven alpha.
"""
from copy import deepcopy
from collections import Counter
from .economics import snapshot


class Attribution:
    def __init__(self,env,*,historical_last_mark=False,maximum_mark_age_ms=120000):
        if env.role not in('train','evolution'):raise ValueError('NO_INDEPENDENT_TEST_DIAGNOSTIC')
        self.env=env;self.options={'historical_last_mark':historical_last_mark,'maximum_mark_age_ms':maximum_mark_age_ms}
        self.records=[];self.start_index=len(env.api_fills);self.start_cash=env.cash;self.start_debt=self.debt()
        self.start_fees=env.fees;self.start,self.start_error=self.mark();self.start_at=env.now
        self.original_fill=env._apply_fill;self.original_snapshot=env._snapshot_finance;self.original_restore=env._restore_finance
        env._apply_fill=self.fill;env._snapshot_finance=self.save;env._restore_finance=self.restore

    def debt(self):
        ledger=getattr(self.env,'component_ledger',None)
        return 0 if ledger is None else sum(ledger.debt.values())

    def mark(self):
        try:return snapshot(self.env,**self.options),None
        except ValueError as error:return None,str(error)

    def save(self):return self.original_snapshot(),len(self.records)

    def restore(self,state):
        financial,count=state;self.original_restore(financial);del self.records[count:]

    def fill(self,order,price,quantity,*,origin='crossing_book'):
        before,error_before=self.mark();index=len(self.env.api_fills)
        known=self.env.actor_books.get(order.spec.exchange)
        book_state='absent'if known is None else'invalid'if known.invalidated else'valid'
        refs=self.env.reference_cache.for_market(self.env.mapping.get(order.spec.exchange))
        venues=sorted({r['venue']for _,r in refs})
        result=self.original_fill(order,price,quantity,origin=origin)
        after,error_after=self.mark()
        if len(self.env.api_fills)!=index+1:raise ValueError('ATTRIBUTION_REQUIRES_SINGLE_APPENDED_FILL')
        row=deepcopy(self.env.api_fills[-1]);impact=None if before is None or after is None else after.twice_micros-before.twice_micros
        fee=2*row['fee'];adverse=(2*quantity*self.env.mechanics.adverse_execution_micros if origin=='crossing_book'else 0)
        self.records.append({'fill':row,'origin':origin,'impactTwiceMicros':impact,'feesTwiceMicros':fee,
            'deliveredBookStateAtFill':book_state,'knownReferenceVenuesAtFill':venues,
            'modeledAdverseCostTwiceMicros':adverse,
            'grossMarkImpactBeforeFeesAndAdverseTwiceMicros':None if impact is None else impact+fee+adverse,
            'unavailableMarkReasons':[v for v in(error_before,error_after)if v],
            'staleMarkExchanges':sorted(set(()if before is None else before.stale_exchanges)|set(()if after is None else after.stale_exchanges))})
        return result

    def summary(self):
        fills=self.env.api_fills[self.start_index:]
        if len(fills)!=len(self.records)or any(f!=r['fill']for f,r in zip(fills,self.records)):
            raise ValueError('ATTRIBUTION_FILL_JOURNAL_DIVERGED')
        end,error=self.mark();complete=self.start is not None and end is not None and all(r['impactTwiceMicros']is not None for r in self.records)
        pnl=None if self.start is None or end is None else end.twice_micros-self.start.twice_micros
        impact=sum(r['impactTwiceMicros']or 0 for r in self.records)
        cash_delta=2*((self.env.cash-self.debt())-(self.start_cash-self.start_debt))
        fill_cash=sum(2*(f['cash_delta_micros']-f.get('collateral_debt_delta_micros',0))for f in fills)
        rows={}
        information={}
        for r in self.records:
            key=r['deliveredBookStateAtFill'];item=information.setdefault(key,{'fills':0,'turnoverMicros':0,'attributedImpactTwiceMicros':0,'unattributedFills':0})
            item['fills']+=1;item['turnoverMicros']+=r['fill']['quantity']*r['fill']['price']
            item['attributedImpactTwiceMicros']+=r['impactTwiceMicros']or 0
            item['unattributedFills']+=r['impactTwiceMicros']is None
        for ex in set(r['fill']['exchange']for r in self.records)|set(()if self.start is None else self.start.position_values_twice)|set(()if end is None else end.position_values_twice):
            selected=[r for r in self.records if r['fill']['exchange']==ex]
            marked=(None if self.start is None or end is None else
                end.position_values_twice.get(ex,0)-self.start.position_values_twice.get(ex,0)+
                sum(2*(r['fill']['cash_delta_micros']-r['fill'].get('collateral_debt_delta_micros',0))for r in selected))
            covered=all(r['impactTwiceMicros']is not None for r in selected)
            cost=sum(r['impactTwiceMicros']or 0 for r in selected)
            rows[ex]={'fills':len(selected),'markedPnlTwiceMicros':marked,'fillMarkImpactTwiceMicros':cost if covered else None,
                'nonFillMarkResidualTwiceMicros':marked-cost if covered and marked is not None else None}
        if pnl is not None and sum(r['markedPnlTwiceMicros']for r in rows.values())+cash_delta-fill_cash!=pnl:
            raise ValueError('ATTRIBUTION_CONSERVATION_FAILED')
        return {'classification':'posthoc_inspected_paper_midpoint_attribution_not_executable_roi_or_alpha',
            'startMs':self.start_at,'endMs':self.env.now,'wholeEpisodeCompleted':self.env.now==self.env.end,
            'completeAttribution':complete,'markedPnlTwiceMicros':pnl,
            'fillMarkImpactTwiceMicros':impact if complete else None,
            'nonFillWealthResidualTwiceMicros':pnl-impact if complete else None,
            'feesTwiceMicros':2*(self.env.fees-self.start_fees),
            'modeledAdverseCostTwiceMicros':sum(r['modeledAdverseCostTwiceMicros']for r in self.records),
            'grossFillMarkImpactBeforeFeesAndAdverseTwiceMicros':sum(r['grossMarkImpactBeforeFeesAndAdverseTwiceMicros']or 0 for r in self.records)if complete else None,
            'nonFillNetCashChangeTwiceMicros':cash_delta-fill_cash,
            'fillCount':len(fills),'turnoverMicros':sum(f['quantity']*f['price']for f in fills),
            'origins':dict(Counter(r['origin']for r in self.records)),
            'deliveredBookStateAtFill':information,
            'knownReferenceVenuesAtFill':dict(Counter('+'.join(r['knownReferenceVenuesAtFill'])or'none'for r in self.records)),
            'informationTiming':'fill_time_cached_observations_not_proof_of_decision_time_information_or_use',
            'staleMarkExchanges':sorted(set(ex for r in self.records for ex in r['staleMarkExchanges'])|
                set(()if end is None else end.stale_exchanges)|set(()if self.start is None else self.start.stale_exchanges)),
            'unavailableBoundaryMarks':[v for v in(self.start_error,error)if v],
            'byExchange':rows,'productionChanged':False,'independentTestUsed':False,'roiEstablished':False}

    def close(self):
        self.env._apply_fill=self.original_fill;self.env._snapshot_finance=self.original_snapshot;self.env._restore_finance=self.original_restore

    def __enter__(self):return self
    def __exit__(self,*args):self.close()
