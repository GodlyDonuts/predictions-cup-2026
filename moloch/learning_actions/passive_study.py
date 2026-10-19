"""Causal quoting reference on inspected data; not a learned or qualified bot.

The reference reads actual actor-delivered books and improves each side by one
tick when the spread permits it. It uses the available bank, shares real API
admissions, and never chooses quotes from future print prices. Comparing a
counterfactual flow hypothesis with no-flow does not establish live profit.
"""
import argparse,json
from pathlib import Path
from .experiment import Experiment
from .passive import DisplayedQueueFlow
from .realtime import DerivedStream
from .contract import OrderSpec,UNIT,TICK,MAX_QUANTITY
from .economics import GrowthLedger
from .selection import frozen


def reference(experiment,passive):
    lo,hi=experiment.train_bounds;start=lo+120000;end=min(hi,start+480000)
    e=experiment.world('train',start,end)
    if not passive:e.passive=None
    ledger=GrowthLedger(e,development=True,historical_last_mark=True)
    cursor=0;requests=0
    def wait():
        while e.budget.active is not None and e.now<e.end:
            e.advance_until_observable(min(e.end,e.busy_until))
    while e.now<e.end:
        wait()
        if e.now>=e.end:break
        keys=[k for k in e.keys['SIG']if k[0]=='book']
        if not keys:e.advance_until_observable(min(e.end,e.now+100));continue
        key=keys[cursor%len(keys)]
        ready=e.budget.ready_at(e.now,False)
        if ready>e.now:e.advance_until_observable(min(e.end,ready));continue
        e.request_observation('SIG',key);wait()
        if e.now>=e.end:break
        cursor+=1;world=e.actor_books.get(key[1])
        if world is None or world.invalidated:continue
        book=world.book
        if not book.bids or not book.asks:continue
        improve=TICK if experiment.config.get('reference_quote_mode','join')=='improve'else 0
        bid,ask=book.bids[0][0]+improve,book.asks[0][0]-improve
        if not TICK<=bid<ask<=UNIT-TICK:continue
        quantity=min(MAX_QUANTITY,e.buying_power()//(bid+UNIT-ask+2*e.mechanics.limits.fee_per_share))
        if quantity<=0:continue
        ready=e.budget.ready_at(e.now,True)
        if ready>e.now:e.advance_until_observable(min(e.end,ready));continue
        requests+=1
        e.dispatch('MULTI',[OrderSpec(key[1],'YES','BUY',quantity,bid),
                           OrderSpec(key[1],'NO','BUY',quantity,UNIT-ask)],
                   idempotency_key='causal-reference-'+str(requests))
    ledger.transition();out=ledger.finish()
    return {**out,'passive_enabled':passive,'simulated_api_requests':len(e.budget.request_ids),
        'quoted_packets':requests,'token_mints':e.realtime.mints if e.realtime else 0,
        'source_print_volume':e.passive.source_volume if e.passive else None,
        'intercepted_volume':e.passive.intercepted_volume if e.passive else 0,
        'passive_fills':sum(f.get('execution_model')=='passive_strict_priority'for f in e.api_fills),
        'counterparty_owner_exclusion_complete':False,'execution_calibrated':False}


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--config',type=Path,required=True)
    p.add_argument('--output',type=Path,required=True);args=p.parse_args()
    config=json.loads(args.config.read_text(encoding='utf-8-sig'))
    config.update(sig_realtime_mode=DerivedStream.mode,initial_sig_realtime='request_token',
                  passive_flow_mode=DisplayedQueueFlow.mode,device='cpu',
                  private_fill_notifications='requires_active_sig_stream')
    experiment=Experiment(config,args.output)
    results=[reference(experiment,enabled)for enabled in(False,True)]
    out={'classification':'causal_reference_driver_flow_hypothesis_study_not_learned_roi',
        'dataset_manifest_sha256':experiment.dataset_sha,'quote_prices_from_causal_delivered_books_only':True,
        'reference_uses_available_bank_not_fixed_tiny_orders':True,'results':results,
        'independent_test_used':False,'http_requests':0,'production_changed':False,
        'profitable_generalization_established':False}
    frozen(args.output/'RESULT.json',out)
    print(json.dumps(out,allow_nan=False),flush=True)


if __name__=='__main__':main()
